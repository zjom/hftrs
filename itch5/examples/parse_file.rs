extern crate itch5;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err("usage: cargo run --example parse_file </PATH/TO/ITCH5/FILE>".into());
    }

    let input_file_path = args.get(1).unwrap();
    let file = File::open(input_file_path)?;
    let file_size = file.metadata()?.len() as usize;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::with_capacity(file_size);
    reader.read_to_end(&mut buf)?;

    let mut visitor = Handler::default();
    itch5::Parser::new(&buf).parse_stream(&mut visitor)?;

    println!("trades: {}", visitor.trades);
    Ok(())
}

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
