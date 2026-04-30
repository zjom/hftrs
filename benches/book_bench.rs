//! Hot-path microbenchmarks.
//!
//! Run with `cargo bench`. For stable numbers, pin to an isolated core and
//! disable turbo / hyperthreading on that core:
//!
//! ```sh
//! taskset -c 3 cargo bench
//! ```
//!
//! `bench` profile inherits `release` settings (LTO, single codegen unit) but
//! keeps debug symbols so we can attach `perf` / generate flamegraphs.

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use orderbook::{Order, OrderBook, Side};

const N: u64 = 10_000;

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

fn bench_add(c: &mut Criterion) {
    let mut g = c.benchmark_group("add");
    g.throughput(Throughput::Elements(N));
    g.bench_function("10k_fresh_orders", |b| {
        b.iter_batched(
            OrderBook::new,
            |mut book| {
                fill_book(&mut book, N);
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });
    g.finish();
}

fn bench_top_of_book(c: &mut Criterion) {
    let mut book = OrderBook::new();
    fill_book(&mut book, N);
    c.bench_function("best_bid_and_ask", |b| {
        b.iter(|| {
            black_box(book.best_bid());
            black_box(book.best_ask());
        });
    });
}

fn bench_cancel_random(c: &mut Criterion) {
    c.bench_function("cancel_then_readd_10k", |b| {
        b.iter_batched(
            || {
                let mut book = OrderBook::new();
                fill_book(&mut book, N);
                book
            },
            |mut book| {
                // Delete and re-add every order — exercises unlink + slab reuse.
                for i in 0..N {
                    book.delete(i).unwrap();
                }
                fill_book(&mut book, N);
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_execute_top(c: &mut Criterion) {
    c.bench_function("execute_partial_at_top", |b| {
        b.iter_batched(
            || {
                let mut book = OrderBook::new();
                fill_book(&mut book, N);
                book
            },
            |mut book| {
                // Drain the top by partial fills; never exhausts an order.
                for i in 0..1_000u64 {
                    let _ = book.execute(i * 2, 1, i); // even ids are bids
                }
                black_box(book);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_add,
    bench_top_of_book,
    bench_cancel_random,
    bench_execute_top
);
criterion_main!(benches);
