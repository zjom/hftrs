# hftrs

An end-to-end Nasdaq market-data pipeline in Rust: from a UDP socket on the
wire through the [MoldUDP64] transport, through the
[Nasdaq TotalView-ITCH 5.0] message protocol, into a per-symbol limit order
book that you can query for top-of-book, spread, mid, and depth.

The repo is a [Cargo workspace] of four crates — three reusable libraries
and one binary that wires them together:

| Crate | Role |
|---|---|
| [`moldudp`](./moldudp) | MoldUDP64 client (gap detection + transparent retransmission) and a reference test server. |
| [`itch5`](./itch5) | Zero-copy parser for all 23 ITCH 5.0 message types. |
| [`orderbook`](./orderbook) | Per-symbol limit order book with arena-allocated, intrusively linked price levels. |
| [`app`](./app) | A binary that replays a recorded ITCH file over a real multicast group, runs the client against it, and prints top-of-book snapshots. |

Each subcrate has its own README with deeper rationale; this document gives
the workspace-level picture and a sense of *why* each piece looks the way it
does.

## Why these three pieces

A real exchange feed is layered. Nasdaq publishes order-book activity as a
sequence of fixed-width binary records (ITCH 5.0). Those records are
delivered as message blocks inside framed packets over a UDP multicast
transport (MoldUDP64) that carries its own session identifier, sequence
numbers, and a sidecar unicast channel for retransmissions. To consume the
feed you need three independent pieces of software:

1. A **transport client** that joins the multicast group, reassembles a
   gapless ordered byte stream from a lossy network, and surfaces packets
   without copying.
2. A **wire-format parser** that takes those packet payloads and turns the
   fixed-width binary records into typed Rust structs.
3. A **stateful book** that consumes the typed events and maintains the
   live order book per symbol.

Splitting the workspace this way makes each layer independently testable
and replaceable. The book has no idea what ITCH is; the ITCH parser has no
idea what a UDP socket is; the MoldUDP64 client has no idea what's inside
the message bytes. The `app` crate is the one place that knows about all
three.

## End-to-end data flow

```
   recorded                                                    per-symbol
   ITCH file                                                   order books
       │                                                            ▲
       ▼                                                            │
 ┌────────────┐ multicast  ┌───────────────┐  bytes  ┌────────────┐ │
 │ MoldUDP64  │ ─────────▶ │   MoldUDP64   │ ──────▶ │ ITCH 5.0   │ │
 │ test server│  (UDP +    │     client    │         │  parser    │ │
 │            │  rerequest)│ (gap detect + │         │ (zero-copy │ │
 │            │ ◀───retx── │  retransmit)  │         │  visitor)  │ │
 └────────────┘            └───────────────┘         └────────────┘ │
                                                            │       │
                                                            ▼       │
                                                      MessageHandler┤
                                                      dispatches to ┤
                                                      add/exec/cxl/ ┤
                                                      replace ──────┘
```

The `app` binary runs both ends in the same process: one thread streams a
captured ITCH file out as MoldUDP64 packets to a multicast group while
randomly varying batch sizes (and occasionally dropping packets through the
test server's `send_dropped` API to exercise gap detection), and another
thread runs the real client against that group, parses the ITCH messages
out, and feeds them into per-symbol order books. The two halves only share
configuration and a shutdown flag — they communicate over the network
exactly the way a production deployment would.

## Design themes

A few principles run through the whole workspace:

### Zero allocation on the steady-state hot path

This is the design constraint that drives most of the structural choices.

- **MoldUDP64 receives** land in pre-allocated 512 KiB buffers from a
  lock-free pool of 1024 slots; buffers return to the pool when their
  `Datagram` is dropped. The client never calls into the allocator while
  reading packets.
- **ITCH parsing** is a bounds-checked pointer cast: every message struct
  is `#[repr(C, packed)]` and implements [`zerocopy::FromBytes`], so a
  parsed `&AddOrderNoMPIDAttribution` is just a typed view over the
  packet's bytes — no copy, no `Vec<u8>`, no per-message allocation.
- **The order book** uses a slab arena (see
  [`orderbook/src/pool.rs`](orderbook/src/pool.rs)) for `Order` nodes with
  an intrusive free list, so insert/cancel/replace touches the allocator
  only when the arena needs to grow.

The reason for the obsession is that allocator pressure is one of the
classic ways a market-data consumer falls behind during a busy open. If
you have to call `malloc` every time a new order arrives at 100k+
events/second, you will eventually meet a slow path you didn't budget for.

### Intrusive doubly-linked lists at each price level

Inside the order book, every price level is a doubly-linked list of orders
in time-priority order. The `prev`/`next` pointers (slab indices, actually
— see below) live *inside* each `Node` rather than in a separate
`LinkedList` allocation. Cancelling or fully-executing an order is two
pointer writes (splice out of the list) plus a free-list push. No
scanning, no hash lookup at a price level, and the level header
(`head`/`tail`/`total_qty`) stays correct without walking the list.

### `u32` slab indices instead of pointers

Slot addresses in the arena are `u32`, not `*mut Node`. Two effects:

- The `OrderId → Index` `HashMap` has 4-byte values instead of 8, so the
  table is roughly half the size and friendlier to L1.
- Linked-list pointers inside each node are `u32`, so a node is smaller
  and more nodes fit per cache line.

The cost is a single `u32` × `usize` widening per dereference, which the
compiler folds into the load.

### `BTreeMap` price ladder by default, with a clear escape hatch

The book uses `BTreeMap<Price, Level>` for the price ladder on each side.
For a single security with a known tick band you would prefer a
vector-indexed ladder for O(1) level access, and the trait surface in
[`orderbook/src/level.rs`](orderbook/src/level.rs) is narrow enough that
swapping in an array-backed implementation is a one-file change. The
`BTreeMap` is the conservative default: works for thousands of symbols at
arbitrary price ranges and gives you ordered iteration for depth
calculations for free.

### Visitor-style parser API

The ITCH parser is a `Parser` plus a `MessageHandler` trait with a default
no-op method per message type. You implement only the handlers you care
about; the parser dispatches statically and inlines through the trait
methods at release optimization. Returning `ControlFlow::Break(())` from
any handler stops the parse early, which makes the same API usable for
"count every trade in this file" *and* "stream forever from a socket and
fill an order book". No closures, no boxed callbacks.

### Realistic transport reliability

MoldUDP64 is interesting because it gives you a clean view of what
"reliable UDP" looks like in practice: a multicast downstream for live
packets, a unicast sidecar for retransmissions, a session identifier so
you can tell a fresh sequence space from the old one, and an explicit
end-of-session marker. The client implementation in `moldudp` mirrors all
of this:

- A multicast receiver thread that detects sequence gaps as soon as a
  packet's sequence number jumps and enqueues a `RetransmissionRequest`
  for the missing range.
- A pool of `N` re-request sender threads (one per configured server)
  that compete on a shared MPMC channel — slow servers are bypassed
  automatically by faster ones, no head-of-line blocking, no per-server
  coordination.
- A re-request *receive* thread that merges retransmitted packets into
  the same data channel as live packets.

Live and retransmitted packets land in *receive order* on the consumer
channel, not in sequence order. The consumer is the one with semantic
context (symbol state, partial fills) and is in the best position to
reorder, so the library refuses to make that decision on its behalf.

### Failure-mode reasoning

Parsing returns `Result`, not panics. The `BookError` enum
(in [`orderbook/src/error.rs`](orderbook/src/error.rs)) names the specific
invariants that can be violated by a malformed or out-of-order feed —
duplicate add, unknown order, over-execute, over-cancel — so a downstream
consumer can decide whether to log and continue, request a session
restart, or trip a circuit breaker. The `app` integration logs each of
these counters at the end of a run, which makes regressions in the
parser/book interaction easy to catch when re-running against the same
recorded file.

## Things explicitly *not* in this repo

These would all be reasonable next steps. They're called out because
omitting them was a choice, not an oversight:

- **A matching engine.** This is a market-data consumer, not an exchange.
  It reconstructs the book from the public feed; it does not match orders
  or generate executions.
- **SoupBinTCP framing.** Production deployments often run MoldUDP64 for
  the live downstream and SoupBinTCP (or Glimpse) for guaranteed delivery
  and snapshot recovery. Out of scope here; the test server in `moldudp`
  is sufficient to exercise the client.

## Performance notes

The workspace ships a [Criterion] benchmark suite that exercises each
component independently and the integrated pipeline as a whole. The
shared release profile uses `lto = "fat"`, a single codegen unit, and
`panic = "abort"`; the bench profile inherits that and adds debug
symbols so `perf` and flamegraphs work without rebuilding. For stable
numbers, pin to an isolated core:

```sh
taskset -c 3 cargo bench --workspace
```

The benchmarks are split into four suites, each layered on the next:

### Component microbenchmarks

| Suite | What it measures |
|---|---|
| `cargo bench -p itch5` | wire-format framing in isolation; per-message-type dispatch (`A`/`F`/`E`/`X`/`D`/`U`/`P`); a no-op-handler floor and a counting-handler ceiling against a 1M-message recorded sample; zero-copy field accessor cost (`Price4`, `Timestamp`, `Symbol`). |
| `cargo bench -p orderbook` | nanosecond-resolution single-op latency for `add` / `delete` / `cancel` / `execute_partial` / `execute_full` / `replace` against a steady-state book; top-of-book and depth(10) queries scaled across book sizes 1k → 1M; bulk inserts; fragmented arena refills that exercise the slab free list. |
| `cargo bench -p moldudp` | packet header decode and message-iter cost varying message density (1 / 10 / 100 / 1000 per packet); end-to-end client receive throughput for several traffic shapes including ITCH-sized (38 B) batched packets; gap detection + retransmission round-trip. |

### Integrated pipeline

`cargo bench -p app --bench end_to_end` wires the full stack together —
`MoldUDP64Server` → kernel UDP → `MoldUDP64` client → ITCH parser →
per-symbol order books — and replays a real ITCH 5.0 file through it.
This is the closest thing here to "what does the running system do
under load". By contrast, the orderbook suite's `itch_replay`
benchmark drives the parser and book directly without any UDP transit,
so the gap between the two numbers is exactly the cost of the network
layer (kernel UDP send/recv, gap detection, channel handoff) on the
host the benchmark ran on.

### Methodology

- **Real data.** A 1M-message recorded ITCH 5.0 feed lives at
  `data/itch_1000_000` and is the input for every realistic-workload
  benchmark. Override the path by setting `ITCH5_BENCH_FILE`. If the
  file is missing, the data-driven benches print a notice and skip,
  so synthetic-only benches still run.
- **Throughput in two units.** Where it makes sense, benches report
  both `Throughput::Elements` (events/sec) and `Throughput::Bytes`
  (bytes/sec). Bytes are the right unit when comparing against NIC
  line rate; elements are the right unit when comparing against
  exchange message rate.
- **Single-op latency via `iter_custom`.** Hot-path microbenchmarks
  in the orderbook suite use Criterion's `iter_custom` to time
  hundreds of ops per measurement window, then divide; this reduces
  per-iteration overhead to noise and gives nanosecond-resolution
  per-op numbers.
- **Steady state.** Every benchmark that touches the order book runs
  against a pre-warmed, steady-state book (default: 50k resting
  orders) rather than an empty one — the cold-start path is not
  representative of what an HFT consumer will see.
- **Pre-faulting.** mmap'd inputs are XOR-walked once before the
  measurement loop so the first iteration doesn't pay for major
  page faults.
- **Backpressure.** The end-to-end pipeline benchmark sends in
  bounded chunks and drains each chunk before queueing the next, so
  the kernel UDP buffer can't overflow and silently drop packets.
  This trades peak throughput for honesty about delivered rate.

### Caveats

These benchmarks measure the system on the host they ran on. They do
*not* characterise:

- Production NIC behavior — there is no kernel-bypass path here, so
  the end-to-end suite is bounded by per-syscall UDP cost on
  loopback. A real deployment behind DPDK / `AF_XDP` / Solarflare
  EFVI will see materially different numbers.
- Real multicast contention. The test server delivers over loopback
  unicast for portability; you will see different jitter against an
  IGMP-joined group on a switched network.
- Long-tail latency under live exchange data. Criterion gives mean
  and confidence intervals; for production hardening, follow up with
  a histogram / HDR-style measurement against a recorded session.

### Concrete numbers

Filled in alongside the platform they were taken on. Run `cargo bench
--workspace` locally and write the relevant `taskset` invocation,
kernel version, and CPU model next to the numbers — they only mean
something with that context.

## Building and running

The workspace builds on stable Rust (edition 2024). To replay a recorded
ITCH file end-to-end through the local multicast loopback:

```sh
cargo run --release -p app -- \
  --file /path/to/01302019.NASDAQ_ITCH50 \
  --watch AAPL --watch MSFT \
  --out /tmp/snapshot.tsv
```

The output is a tab-separated table of `symbol`, `best_ask`, `best_bid`,
`spread`, `mid`, and top-10 depth on each side, taken at the moment the
feed ends. The full set of CLI flags (multicast group, re-request bind
address, session identifier, batch size cap) is in
[`app/src/main.rs`](app/src/main.rs).

For unit and integration tests across the workspace:

```sh
cargo test --workspace
```

## License

Every crate in this workspace is released into the public domain under
[The Unlicense](https://unlicense.org/).

[MoldUDP64]: https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/moldudp64.pdf
[Nasdaq TotalView-ITCH 5.0]: https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/NQTVITCHspecification.pdf
[Cargo workspace]: https://doc.rust-lang.org/cargo/reference/workspaces.html
[Criterion]: https://github.com/bheisler/criterion.rs
