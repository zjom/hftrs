//! Skeleton showing how to drive the order book from an ITCH event stream.

use itch5::messages::*;
use memmap2::Mmap;
use orderbook::{Order, OrderBook, Side};
use std::env;
use std::fs::File;
use std::ops::ControlFlow;
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
}

#[derive(Default)]
struct Handler {
    book: OrderBook,
}
impl itch5::MessageHandler for Handler {
    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        self.book
            .add(Order {
                id: msg.order_reference_number(),
                side: match msg.buy_sell_indicator() {
                    BuySellIndicator::Buy => Side::Bid,
                    BuySellIndicator::Sell => Side::Ask,
                    BuySellIndicator::Unknown(unknown) => {
                        eprintln!("unknown buy sell indicator in itch msg: {unknown}");
                        return ControlFlow::Break(());
                    }
                },
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp(),
            })
            .unwrap();
        ControlFlow::Continue(())
    }

    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        self.book
            .add(Order {
                id: msg.order_reference_number(),
                side: match msg.buy_sell_indicator() {
                    BuySellIndicator::Buy => Side::Bid,
                    BuySellIndicator::Sell => Side::Ask,
                    BuySellIndicator::Unknown(unknown) => {
                        eprintln!("unknown buy sell indicator in itch msg: {unknown}");
                        return ControlFlow::Break(());
                    }
                },
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp(),
            })
            .unwrap();
        ControlFlow::Continue(())
    }

    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        self.book
            .execute(
                msg.order_reference_number(),
                msg.executed_shares() as u64,
                msg.timestamp(),
            )
            .unwrap();

        ControlFlow::Continue(())
    }

    fn on_order_executed_with_price(&mut self, msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        self.book
            .execute(
                msg.order_reference_number(),
                msg.executed_shares() as u64,
                msg.timestamp(),
            )
            .unwrap();

        ControlFlow::Continue(())
    }

    fn on_order_cancel(&mut self, msg: &OrderCancel) -> ControlFlow<()> {
        self.book
            .cancel(msg.order_reference_number(), msg.cancelled_shares() as u64)
            .unwrap();
        ControlFlow::Continue(())
    }

    fn on_order_delete(&mut self, msg: &OrderDelete) -> ControlFlow<()> {
        self.book.delete(msg.order_reference_number()).unwrap();
        ControlFlow::Continue(())
    }

    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        self.book
            .replace(
                msg.original_order_reference_number(),
                msg.new_order_reference_number(),
                msg.price().into_i64(),
                msg.shares() as u64,
                msg.timestamp(),
            )
            .unwrap();

        ControlFlow::Continue(())
    }
}
