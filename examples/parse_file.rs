extern crate itch5;
use memmap2::Mmap;
use std::env;
use std::fs::File;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: cargo run --example parse_file </PATH/TO/ITCH5/FILE>");
        return;
    }

    let input_file_path = args.get(1).unwrap();
    let file = File::open(input_file_path).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };

    let mut visitor = Handler::default();
    itch5::Parser::new(&mmap)
        .parse_stream(&mut visitor)
        .unwrap();

    eprintln!("trades: {}", visitor.trades);
}

#[derive(Default)]
struct Handler {
    trades: u64,
}

impl itch5::MessageHandler for Handler {
    fn on_trade_message(&mut self, _msg: &itch5::TradeMessage) -> std::ops::ControlFlow<()> {
        self.trades += 1;
        std::ops::ControlFlow::Continue(())
    }
}
