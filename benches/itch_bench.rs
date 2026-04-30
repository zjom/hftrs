use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use memmap2::Mmap;
use std::fs::File;
use std::hint;

#[derive(Default)]
struct Handler {
    trades: u64,
}

impl itch5::MessageHandler for Handler {
    fn on_trade_message(
        &mut self,
        _msg: &itch5::messages::TradeMessage,
    ) -> std::ops::ControlFlow<()> {
        self.trades += 1;
        std::ops::ControlFlow::Continue(())
    }
}

fn bench_parse_one_million_msgs(c: &mut Criterion) {
    let path = "data/itch_1000_000";
    let file = File::open(&path).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };

    let mut group = c.benchmark_group("itch5");
    group.throughput(Throughput::ElementsAndBytes {
        bytes: mmap.len() as u64,
        elements: 1_000_000,
    });

    group.bench_function("parse_feed", |b| {
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
