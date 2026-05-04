//! ITCH 5.0 parser microbenchmarks.
//!
//! ## What we measure
//!
//! 1. **Wire-format framing.** `parse_one` is the inner loop of every
//!    consumer. We want to know its per-message cost in isolation, with no
//!    handler dispatch.
//! 2. **Visitor dispatch overhead.** [`Parser::parse_stream`] dispatches one
//!    indirect call per message. Even with default no-op handlers, we want
//!    to confirm the overhead is amortized into nanoseconds.
//! 3. **Per-message-type cost.** The hot order-book messages —
//!    `A`/`F`/`E`/`X`/`D`/`U`/`P` — dominate real feeds. We bench each in
//!    isolation against a stream of homogeneous messages so a regression
//!    in one type is visible.
//! 4. **Realistic mixed workload.** A 1M-message recorded sample driven
//!    through both a no-op handler (parser-only baseline) and a counting
//!    handler (touches every accessor on the hot types). Reports
//!    throughput in messages/sec and bytes/sec.
//! 5. **Field-decode cost.** The zero-copy big-endian accessors
//!    (`Price4::into_i64`, `Timestamp::to_u64`, `Symbol::to_u64`) are the
//!    leaves of the call tree; we confirm they fold to single loads.
//!
//! ## Running
//!
//! ```sh
//! # From the workspace root, pinned to an isolated core:
//! taskset -c 3 cargo bench -p itch5
//! ```
//!
//! The realistic-workload benches need a recorded ITCH 5.0 file. By
//! default they use `../data/itch_1000_000` relative to this crate (the
//! sample committed to the repo). Override via:
//!
//! ```sh
//! ITCH5_BENCH_FILE=/path/to/file.itch cargo bench -p itch5
//! ```
//!
//! If the file is missing, those benches print a notice and skip rather
//! than fail the run.

use std::fs::File;
use std::hint::black_box;
use std::ops::ControlFlow;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use memmap2::Mmap;
use zerocopy::FromBytes;

use itch5::messages::*;
use itch5::{MessageHandler, Parser, parse_one};

// ─── Fixtures ──────────────────────────────────────────────────────────────

/// Build a buffer of `count` framed messages, each `body_len` bytes long
/// with `tag` as the first body byte and zero-filled fields.
///
/// Zero is a valid bit pattern for every field type in ITCH 5.0 (all fields
/// are integers, fixed-width arrays, or single bytes), so `zerocopy::FromBytes`
/// will succeed against bodies built this way.
fn synthetic_stream(tag: u8, body_len: usize, count: usize) -> Vec<u8> {
    let frame_len = 2 + body_len;
    let mut buf = Vec::with_capacity(count * frame_len);
    let len_be = (body_len as u16).to_be_bytes();
    let mut body = vec![0u8; body_len];
    body[0] = tag;
    for _ in 0..count {
        buf.extend_from_slice(&len_be);
        buf.extend_from_slice(&body);
    }
    buf
}

fn try_load_sample() -> Option<Mmap> {
    let path: PathBuf = std::env::var_os("ITCH5_BENCH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../data/itch_1000_000"));
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "skip: ITCH5 sample at {} unavailable ({e}). Set ITCH5_BENCH_FILE to override.",
                path.display()
            );
            return None;
        }
    };
    let mmap = unsafe { Mmap::map(&file).ok()? };
    // Pre-fault every page so the first iteration doesn't pay the cost.
    black_box(mmap.iter().fold(0u8, |a, b| a ^ b));
    Some(mmap)
}

// ─── Handlers ──────────────────────────────────────────────────────────────

/// No-op visitor — measures pure parse + dispatch with the inliner free to
/// remove the body of every handler.
#[derive(Default)]
struct NoopHandler;
impl MessageHandler for NoopHandler {}

/// Visitor that touches the full set of accessors on the hot-path message
/// types so the parser cannot dead-code-eliminate field decode.
#[derive(Default)]
struct CountingHandler {
    adds: u64,
    execs: u64,
    cancels: u64,
    deletes: u64,
    replaces: u64,
    trades: u64,
    locate_xor: u64,
    ts_xor: u64,
    price_xor: i64,
    qty_xor: u64,
    sym_xor: u64,
}

impl MessageHandler for CountingHandler {
    fn on_add_order_no_mpid_attribution(
        &mut self,
        m: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        self.adds += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        self.ts_xor ^= m.timestamp().to_u64();
        self.price_xor ^= m.price().into_i64();
        self.qty_xor ^= m.shares() as u64;
        self.sym_xor ^= m.stock().to_u64();
        ControlFlow::Continue(())
    }
    fn on_add_order_with_mpid_attribution(
        &mut self,
        m: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        self.adds += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        self.ts_xor ^= m.timestamp().to_u64();
        self.price_xor ^= m.price().into_i64();
        self.qty_xor ^= m.shares() as u64;
        self.sym_xor ^= m.stock().to_u64();
        ControlFlow::Continue(())
    }
    fn on_order_executed(&mut self, m: &OrderExecuted) -> ControlFlow<()> {
        self.execs += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        self.ts_xor ^= m.timestamp().to_u64();
        ControlFlow::Continue(())
    }
    fn on_order_executed_with_price(&mut self, m: &OrderExecutedWithPrice) -> ControlFlow<()> {
        self.execs += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        self.ts_xor ^= m.timestamp().to_u64();
        ControlFlow::Continue(())
    }
    fn on_order_cancel(&mut self, m: &OrderCancel) -> ControlFlow<()> {
        self.cancels += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        ControlFlow::Continue(())
    }
    fn on_order_delete(&mut self, m: &OrderDelete) -> ControlFlow<()> {
        self.deletes += 1;
        self.locate_xor ^= m.stock_locate() as u64;
        ControlFlow::Continue(())
    }
    fn on_order_replace(&mut self, m: &OrderReplace) -> ControlFlow<()> {
        self.replaces += 1;
        self.price_xor ^= m.price().into_i64();
        ControlFlow::Continue(())
    }
    fn on_trade(&mut self, m: &Trade) -> ControlFlow<()> {
        self.trades += 1;
        self.price_xor ^= m.price().into_i64();
        self.sym_xor ^= m.stock().to_u64();
        ControlFlow::Continue(())
    }
}

// ─── Benches ───────────────────────────────────────────────────────────────

/// Pure framing: walk a buffer with [`parse_one`] and only inspect the tag
/// byte. No dispatch, no `cast`. This is the floor — the cost of the length
/// prefix decode and slice splits.
fn bench_framing_only(c: &mut Criterion) {
    const COUNT: usize = 100_000;
    // Use AddOrderNoMPIDAttribution as a representative size (36 B body).
    let buf = synthetic_stream(b'A', AddOrderNoMPIDAttribution::LEN, COUNT);

    let mut g = c.benchmark_group("itch5/framing");
    g.throughput(Throughput::Elements(COUNT as u64));
    g.bench_function("parse_one_only_100k", |b| {
        b.iter(|| {
            let mut rest: &[u8] = &buf;
            let mut tag_xor = 0u8;
            while !rest.is_empty() {
                let (body, next) = parse_one(rest).unwrap();
                tag_xor ^= body[0];
                rest = next;
            }
            black_box(tag_xor);
        });
    });
    g.finish();
}

/// Full `parse_stream` over a homogeneous synthetic stream of each hot
/// message type, dispatched into a no-op handler. Compares dispatch+cast
/// overhead per type.
fn bench_per_msg_type(c: &mut Criterion) {
    const COUNT: usize = 100_000;

    let cases: &[(&str, u8, usize)] = &[
        ("add_no_mpid", b'A', AddOrderNoMPIDAttribution::LEN),
        ("add_with_mpid", b'F', AddOrderWithMPIDAttribution::LEN),
        ("order_executed", b'E', OrderExecuted::LEN),
        ("order_cancel", b'X', OrderCancel::LEN),
        ("order_delete", b'D', OrderDelete::LEN),
        ("order_replace", b'U', OrderReplace::LEN),
        ("trade", b'P', Trade::LEN),
    ];

    let mut g = c.benchmark_group("itch5/per_msg_type");
    for &(name, tag, body_len) in cases {
        let buf = synthetic_stream(tag, body_len, COUNT);
        g.throughput(Throughput::ElementsAndBytes {
            elements: COUNT as u64,
            bytes: buf.len() as u64,
        });
        g.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                let mut h = NoopHandler;
                Parser::new(&buf).parse_stream(&mut h).unwrap();
            });
        });
    }
    g.finish();
}

/// Realistic mixed workload: parse the recorded sample with the same
/// distribution of message types you'd see live.
///
/// - `noop` reports the parser's intrinsic ceiling (pure dispatch + cast).
/// - `counting` reports the parser plus minimal field decode on each hot
///   message, which is what an order-book consumer actually pays.
fn bench_full_file(c: &mut Criterion) {
    let Some(mmap) = try_load_sample() else {
        return;
    };

    let mut g = c.benchmark_group("itch5/full_file");
    g.throughput(Throughput::Bytes(mmap.len() as u64));
    g.sample_size(20);

    g.bench_function("noop_handler", |b| {
        b.iter(|| {
            let mut h = NoopHandler;
            Parser::new(&mmap).parse_stream(&mut h).unwrap();
        });
    });

    g.bench_function("counting_handler", |b| {
        b.iter(|| {
            let mut h = CountingHandler::default();
            Parser::new(&mmap).parse_stream(&mut h).unwrap();
            // Force the compiler to keep the field decodes.
            black_box((
                h.adds,
                h.execs,
                h.cancels,
                h.deletes,
                h.replaces,
                h.trades,
                h.locate_xor,
                h.ts_xor,
                h.price_xor,
                h.qty_xor,
                h.sym_xor,
            ));
        });
    });
    g.finish();
}

/// Tight loop over the zero-copy big-endian field accessors. Confirms each
/// folds to a single load (or load + bswap) at release optimization.
fn bench_field_accessors(c: &mut Criterion) {
    const N: usize = 1_000_000;

    let mut g = c.benchmark_group("itch5/field_accessors");
    g.throughput(Throughput::Elements(N as u64));

    // Build one synthetic Add-order message we can re-decode N times.
    let buf = synthetic_stream(b'A', AddOrderNoMPIDAttribution::LEN, 1);
    let (body, _) = parse_one(&buf).unwrap();
    let msg = AddOrderNoMPIDAttribution::ref_from_bytes(body).unwrap();

    g.bench_function("price4_into_i64", |b| {
        b.iter(|| {
            let mut acc = 0i64;
            for _ in 0..N {
                acc ^= black_box(msg).price().into_i64();
            }
            black_box(acc);
        });
    });
    g.bench_function("timestamp_to_u64", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..N {
                acc ^= black_box(msg).timestamp().to_u64();
            }
            black_box(acc);
        });
    });
    g.bench_function("symbol_to_u64", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for _ in 0..N {
                acc ^= black_box(msg).stock().to_u64();
            }
            black_box(acc);
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_framing_only,
    bench_per_msg_type,
    bench_full_file,
    bench_field_accessors,
);
criterion_main!(benches);
