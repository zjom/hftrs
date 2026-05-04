# orderbook

A limit order book for Nasdaq TotalView-ITCH 5.0 market data, written in Rust. Companion piece to my MoldUDP64 client and ITCH parser.

## Design

The book keeps three structures in sync:

- **Two `BTreeMap<Price, Level>`**, one per side. Best bid is the rightmost key in `bids`, best ask is the leftmost key in `asks`. O(log n) on insert/remove, top-of-book is essentially free.
- **`HashMap<OrderId, Index>`** for O(1) lookup from exchange order id to slab index — this is what cancel and execute messages address.
- **`Pool` (slab arena)** of `Node { Order, prev, next }`. Orders live in the arena, not in individual `Box` allocations. The `prev`/`next` fields turn each price level into an *intrusive* doubly linked list, so unlinking a fully-filled or deleted order is two pointer writes — no scan, no hash lookup.

All ITCH event handlers (`add`, `execute`, `execute_at`, `cancel`, `delete`, `replace`) map onto these structures with no allocation in the steady state once the arena is warm.

### Why an arena instead of `Box<Node>`

Allocator pressure during a market open is the kiss of death. Pre-sizing an arena with `with_capacity` amortizes allocation up front and reuses slots through an intrusive free list. Slab indices are also `u32`, half the size of a pointer, which keeps the `OrderId → Index` hash table denser and more cache-friendly.

### Why `BTreeMap` instead of a vector-indexed price ladder

A flat array indexed by `(price - min) / tick` would give O(1) access at every level and is the right answer when the tick range is bounded and known up front (e.g. a single NMS issue with a known price band). For a general book covering thousands of symbols at variable price ranges, `BTreeMap` is the safer default. The trait surface here is narrow enough that swapping in an array-backed implementation is a one-file change — see `level.rs` for the type.

### Type widths

ITCH 5.0 wire prices are `u32` with four implicit decimals; quantities are `u32`. Both are widened to `i64` and `u64` respectively to keep aggregation and spread / mid arithmetic out of overflow territory and to allow signed deltas. Newtype wrappers around `Price` / `Quantity` / `OrderId` would buy extra type safety; left as a follow-up so the scaffold stays readable.

## Usage

```rust
use orderbook::{Order, OrderBook, Side};

let mut book = OrderBook::with_capacity(1 << 18);

book.add(Order { id: 1, side: Side::Bid, price: 100_0000, qty: 100, ts: 0 })?;
book.add(Order { id: 2, side: Side::Ask, price: 101_0000, qty: 50,  ts: 1 })?;

assert_eq!(book.best_bid(), Some((100_0000, 100)));
assert_eq!(book.spread(),   Some(1_0000));

let trade = book.execute(1, 30, 2)?;
assert_eq!(trade.qty, 30);
```

## Wiring into the ITCH parser

`examples/replay_itch.rs` sketches how each ITCH message maps to a book operation:

| ITCH message | Book method |
|---|---|
| `A`, `F` (Add Order) | `add` |
| `E` (Order Executed) | `execute` |
| `C` (Order Executed With Price) | `execute_at` |
| `X` (Order Cancel) | `cancel` |
| `D` (Order Delete) | `delete` |
| `U` (Order Replace) | `replace` |

Trade-only messages (`P`), system events (`S`), and the various status messages don't touch the book.

## Benchmarks

```sh
taskset -c 3 cargo bench -p orderbook
```

The bench profile inherits release optimizations (`lto = "fat"`, single codegen unit) but keeps debug symbols so you can attach `perf`. The suite ([`benches/book_bench.rs`](benches/book_bench.rs)) is divided into four groups:

- **`orderbook/single_op_latency/{add,delete,execute_partial,
  execute_full,cancel_partial,replace}`** — nanosecond-resolution
  per-op latency against a pre-warmed 50k-order book. Uses
  `iter_custom` to amortize Criterion's per-iteration overhead across
  1000 ops per measurement window. This is the number that matters
  on the hot path.
- **`orderbook/query_scaling/{best_bid,best_ask,spread,mid,depth_10}`
  × {1k,10k,100k,1M}** — top-of-book and depth(10) queries against
  pre-built books at four orders of magnitude. Confirms that query
  cost is bounded by the BTreeMap traversal and doesn't grow
  linearly with order count.
- **`orderbook/bulk/{fresh_inserts_10k,delete_then_readd_10k,
  fragmented_refill_5k}`** — bulk workloads. The fragmented variant
  pre-deletes every other order before timing the refill so the slab
  free list is exercised under real churn rather than fresh growth.
- **`orderbook/itch_replay/registry_replay_full_file`** — drives a
  registry of order books off the recorded ITCH sample
  (`../data/itch_1000_000` by default; override with
  `ITCH5_BENCH_FILE=...`). Throughput is reported in *book events*
  (add + execute + cancel + delete + replace), not parsed messages,
  so the number isn't inflated by trades and status records.

## Tests

```sh
cargo test
```

Integration tests cover top-of-book queries, FIFO within a price level, partial / full cancels, delete clearing empty levels, replace breaking time priority, over-execute rejection, and `execute_at` print pricing.

## Roadmap

- [ ] **Property tests with `proptest`.** The invariant "level `total_qty` equals the sum of resting order qtys at that price" should hold after any sequence of valid events. Same for `order_count` and the `OrderId → Index` map being injective onto live slots.
- [x] **End-to-end replay.** Pipe a recorded TotalView session through `MoldUDP64 → ITCH → book` and diff top-of-book snapshots against a Python reference (e.g. `nasdaq_protocols`).
  - See [../app/](../app/).
- [ ] **Vector-indexed price ladder variant** for symbols with a known tick band, with benchmarks against the `BTreeMap` baseline.
- [ ] **`no_std` support.** The hot path doesn't touch the heap once the arena is sized; it's mostly a matter of swapping `HashMap` for `hashbrown` and dropping the `Display` impl on `BookError`.
- [ ] **Self-trade prevention / crossed-book detection hooks.** Strictly a matching-engine concern, but the book should at least flag when a new order would lock or cross.
- [ ] **`FxHashMap`** in place of the default hasher — measurable win once the order map is big.

## License

[Unlicense](https://unlicense.org/)
