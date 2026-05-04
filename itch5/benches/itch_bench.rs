use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use memmap2::Mmap;
use std::fs::File;
use std::{env, hint};

#[derive(Default)]
struct Handler {
    trades: u64,
}

impl itch5::MessageHandler for Handler {
    fn on_trade(&mut self, _msg: &itch5::messages::Trade) -> std::ops::ControlFlow<()> {
        self.trades += 1;
        std::ops::ControlFlow::Continue(())
    }
}

fn bench_parse_one_million_msgs(c: &mut Criterion) {
    let path = env::var("ITCH5_BENCH_1M_FILE").expect("ITCH5_BENCH_1M_FILE env var not set");
    let file = File::open(&path).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };

    let mut group = c.benchmark_group("itch5");
    group.throughput(Throughput::ElementsAndBytes {
        bytes: mmap.len() as u64,
        elements: 1_000_000,
    });

    group.bench_function("parse_1M_msgs", |b| {
        b.iter(|| {
            let mut visitor = Handler::default();
            itch5::Parser::new(&mmap)
                .parse_stream(&mut visitor)
                .unwrap();
            hint::black_box(visitor.trades);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse_one_million_msgs);
criterion_main!(benches);
