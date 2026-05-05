//! Limit-order-book hot-path benchmarks.
//!
//! Two layers of measurement:
//!
//! 1. **Single-op micro.** One `add` / `cancel` / `delete` / `execute` /
//!    `replace` against a pre-warmed steady-state book, timed via
//!    [`Bencher::iter_custom`] so we can amortize criterion's per-iter
//!    overhead across many ops and report nanosecond-resolution latency
//!    per op.
//! 2. **Bulk and replay.** Larger workloads — bulk inserts, fragmented
//!    delete-and-readd cycles to exercise slab reuse, and (when a sample
//!    is available) a real ITCH 5.0 file replayed through the registry,
//!    which is the workload that actually matches what an HFT firm will
//!    run in production.
//!
//! Top-of-book and depth queries are benched on books scaled across four
//! orders of magnitude (1k, 10k, 100k, 1M) so it's clear how cost grows
//! with book size.
//!
//! ## Running
//!
//! ```sh
//! taskset -c 3 cargo bench -p orderbook
//! ```
//!
//! The ITCH replay benchmark looks for `../data/itch_1000_000` by default;
//! override with `ITCH5_BENCH_FILE=...`. Missing file → bench is skipped.

use std::fs::File;
use std::hint::black_box;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use memmap2::Mmap;

use itch5::messages::*;
use orderbook::registry::HashMapRegistry;
use orderbook::{Order, OrderBook, Side};

const HOT_BOOK_SIZE: u64 = 50_000;

// ─── Fixtures ──────────────────────────────────────────────────────────────

/// Deterministic synthetic order generator. Spreads orders across ~100
/// price levels per side around a central reference, giving each level a
/// non-trivial FIFO depth — closer to a real top-of-book picture than the
/// "1 order per level" pathological case.
fn fill_book(book: &mut OrderBook, n: u64) {
    for i in 0..n {
        let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
        let price = if matches!(side, Side::Bid) {
            10_000 - (i as i64 % 100)
        } else {
            10_010 + (i as i64 % 100)
        };
        book.add(Order {
            id: i,
            side,
            price,
            qty: 100,
            ts: i,
        })
        .unwrap();
    }
}

fn fresh_book(n: u64) -> OrderBook {
    let mut b = OrderBook::with_capacity(n.next_power_of_two() as usize);
    fill_book(&mut b, n);
    b
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
    black_box(mmap.iter().fold(0u8, |a, b| a ^ b));
    Some(mmap)
}

// ─── Single-op latency micros ──────────────────────────────────────────────
//
// Pattern: pre-build a steady-state book, run `iter_custom` so we measure
// many ops per criterion iteration. Each closure reports time over `iters
// * BATCH` ops, but criterion divides by `iters` only, so we manually
// scale via `Throughput::Elements(BATCH)` to get per-op numbers.

const PER_ITER_OPS: u64 = 1_000;

fn bench_single_op_latencies(c: &mut Criterion) {
    let mut g = c.benchmark_group("orderbook/single_op_latency");
    g.throughput(Throughput::Elements(PER_ITER_OPS));

    // ── add: into a steady-state book at fresh order IDs ──
    g.bench_function("add", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                let next_id_base = HOT_BOOK_SIZE;
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    let id = next_id_base + k;
                    let side = if k % 2 == 0 { Side::Bid } else { Side::Ask };
                    let price = if matches!(side, Side::Bid) {
                        9_950 - (k as i64 % 50)
                    } else {
                        10_050 + (k as i64 % 50)
                    };
                    book.add(Order {
                        id,
                        side,
                        price,
                        qty: 100,
                        ts: id,
                    })
                    .unwrap();
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    // ── delete: remove well-distributed orders one at a time ──
    g.bench_function("delete", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                // Pick well-distributed ids so we don't only ever hit the head.
                let stride = HOT_BOOK_SIZE / PER_ITER_OPS;
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    book.delete(k * stride).unwrap();
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    // ── execute (partial): partial fills against the head of each level ──
    g.bench_function("execute_partial", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    // Even ids are bids, with qty 100. Take 1 each time.
                    let _ = book.execute(k * 2, 1, k);
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    // ── execute (full): fully consume an order, exercises unlink path ──
    g.bench_function("execute_full", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    let _ = book.execute(k * 2, 100, k); // full fill
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    // ── cancel (partial qty reduction): no unlink ──
    g.bench_function("cancel_partial", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    let _ = book.cancel(k * 2, 1);
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    // ── replace: cancel old + add new (price/qty change) ──
    g.bench_function("replace", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut book = fresh_book(HOT_BOOK_SIZE);
                let new_id_base = HOT_BOOK_SIZE;
                let start = Instant::now();
                for k in 0..PER_ITER_OPS {
                    book.replace(k * 2, new_id_base + k, 9_900, 50, k).unwrap();
                }
                total += start.elapsed();
                black_box(&book);
            }
            total
        });
    });

    g.finish();
}

// ─── Top-of-book / depth queries across book sizes ─────────────────────────

fn bench_query_scaling(c: &mut Criterion) {
    let mut g = c.benchmark_group("orderbook/query_scaling");
    let sizes: &[u64] = &[1_000, 10_000, 100_000, 1_000_000];

    for &size in sizes {
        let book = fresh_book(size);

        g.throughput(Throughput::Elements(1));
        g.bench_with_input(BenchmarkId::new("best_bid", size), &book, |b, book| {
            b.iter(|| black_box(book.best_bid()));
        });
        g.bench_with_input(BenchmarkId::new("best_ask", size), &book, |b, book| {
            b.iter(|| black_box(book.best_ask()));
        });
        g.bench_with_input(BenchmarkId::new("spread", size), &book, |b, book| {
            b.iter(|| black_box(book.spread()));
        });
        g.bench_with_input(BenchmarkId::new("mid", size), &book, |b, book| {
            b.iter(|| black_box(book.mid()));
        });
        g.bench_with_input(BenchmarkId::new("depth_10", size), &book, |b, book| {
            b.iter(|| black_box(book.depth(10)));
        });
    }
    g.finish();
}

// ─── Bulk workloads ────────────────────────────────────────────────────────

fn bench_bulk(c: &mut Criterion) {
    const BULK: u64 = 10_000;

    let mut g = c.benchmark_group("orderbook/bulk");

    g.throughput(Throughput::Elements(BULK));
    g.bench_function("fresh_inserts_10k", |b| {
        b.iter_batched(
            || OrderBook::with_capacity(BULK as usize),
            |mut book| {
                fill_book(&mut book, BULK);
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });

    g.throughput(Throughput::Elements(BULK * 2));
    g.bench_function("delete_then_readd_10k", |b| {
        b.iter_batched(
            || fresh_book(BULK),
            |mut book| {
                for k in 0..BULK {
                    book.delete(k).unwrap();
                }
                fill_book(&mut book, BULK);
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });

    // Fragmented arena: leave half the slots free, then refill — exercises
    // the slab free-list under realistic churn rather than fresh growth.
    g.throughput(Throughput::Elements(BULK / 2));
    g.bench_function("fragmented_refill_5k", |b| {
        b.iter_batched(
            || {
                let mut book = fresh_book(BULK);
                // Delete every other order so the free list is fragmented.
                for k in (0..BULK).step_by(2) {
                    book.delete(k).unwrap();
                }
                book
            },
            |mut book| {
                let next_id_base = BULK;
                for k in 0..BULK / 2 {
                    let side = if k % 2 == 0 { Side::Bid } else { Side::Ask };
                    let price = if matches!(side, Side::Bid) {
                        9_950 - (k as i64 % 50)
                    } else {
                        10_050 + (k as i64 % 50)
                    };
                    book.add(Order {
                        id: next_id_base + k,
                        side,
                        price,
                        qty: 100,
                        ts: k,
                    })
                    .unwrap();
                }
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });

    g.finish();
}

// ─── Realistic ITCH replay through the registry ────────────────────────────

struct ReplayHandler {
    registry: HashMapRegistry,
    adds: u64,
    execs: u64,
    cancels: u64,
    deletes: u64,
    replaces: u64,
    skipped: u64,
    errors: u64,
}

impl ReplayHandler {
    fn new() -> Self {
        Self {
            registry: HashMapRegistry::with_capacity(1 << 13),
            adds: 0,
            execs: 0,
            cancels: 0,
            deletes: 0,
            replaces: 0,
            skipped: 0,
            errors: 0,
        }
    }
}

impl itch5::MessageHandler for ReplayHandler {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        self.registry.register(msg.stock_locate(), msg.stock());
        ControlFlow::Continue(())
    }
    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        let Some(book) = self.registry.get_mut(msg.stock_locate()) else {
            self.skipped += 1;
            return ControlFlow::Continue(());
        };
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(_) => return ControlFlow::Continue(()),
        };
        if book
            .add(Order {
                id: msg.order_reference_number(),
                side,
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp().to_u64(),
            })
            .is_ok()
        {
            self.adds += 1;
        } else {
            self.errors += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        let Some(book) = self.registry.get_mut(msg.stock_locate()) else {
            self.skipped += 1;
            return ControlFlow::Continue(());
        };
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(_) => return ControlFlow::Continue(()),
        };
        if book
            .add(Order {
                id: msg.order_reference_number(),
                side,
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp().to_u64(),
            })
            .is_ok()
        {
            self.adds += 1;
        } else {
            self.errors += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .execute(
                    msg.order_reference_number(),
                    msg.executed_shares() as u64,
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.execs += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_executed_with_price(&mut self, msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .execute_at(
                    msg.order_reference_number(),
                    msg.executed_shares() as u64,
                    msg.execution_price().into_i64(),
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.execs += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_cancel(&mut self, msg: &OrderCancel) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .cancel(msg.order_reference_number(), msg.cancelled_shares() as u64)
                .is_ok()
            {
                self.cancels += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_delete(&mut self, msg: &OrderDelete) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book.delete(msg.order_reference_number()).is_ok() {
                self.deletes += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .replace(
                    msg.original_order_reference_number(),
                    msg.new_order_reference_number(),
                    msg.price().into_i64(),
                    msg.shares() as u64,
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.replaces += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
}

/// End-to-end realistic workload: drive the registry of order books off
/// the recorded ITCH file. Reports both the wall time per replay and the
/// effective book-event rate (add + execute + cancel + delete + replace).
fn bench_itch_replay(c: &mut Criterion) {
    let Some(mmap) = try_load_sample() else {
        return;
    };

    // Pre-count book events so throughput numbers reflect what the book
    // actually did, not what the parser saw.
    let book_events = {
        let mut h = ReplayHandler::new();
        itch5::Parser::new(&mmap).parse_stream(&mut h).unwrap();
        h.adds + h.execs + h.cancels + h.deletes + h.replaces
    };

    let mut g = c.benchmark_group("orderbook/itch_replay");
    g.throughput(Throughput::Elements(book_events));
    g.sample_size(20);

    g.bench_function("registry_replay_full_file", |b| {
        b.iter(|| {
            let mut h = ReplayHandler::new();
            itch5::Parser::new(&mmap).parse_stream(&mut h).unwrap();
            black_box((
                h.adds, h.execs, h.cancels, h.deletes, h.replaces, h.skipped, h.errors,
            ));
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_single_op_latencies,
    bench_query_scaling,
    bench_bulk,
    bench_itch_replay,
);
criterion_main!(benches);
