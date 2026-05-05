use crate::handler::MessageHandler;
use orderbook::registry::Registry;
use std::io;

pub fn write_report<R: Registry>(
    handler: &MessageHandler<R>,
    mut writer: impl io::Write,
) -> io::Result<()> {
    let book_count = handler.registry().iter().count();
    let stats = handler.stats();
    log::info!(
        "writing report: {} book(s) | stats: stock_dirs={} added={} executed={} \
         cancelled={} deleted={} replaced={} skipped_locate={}",
        book_count,
        stats.stock_directory_msgs,
        stats.orders_added,
        stats.orders_executed,
        stats.orders_cancelled,
        stats.orders_deleted,
        stats.orders_replaced,
        stats.skipped_locate,
    );

    writeln!(writer, "symb\tbest_ask\tbest_bid\tspread\tmid\tdepth(10)")?;
    handler
        .registry()
        .iter()
        .map(|(_locate, symb, book)| {
            let best_ask = book.best_ask().unwrap_or((0, 0));
            let best_bid = book.best_bid().unwrap_or((0, 0));
            let spread = book.spread().unwrap_or(0);
            let mid = book.mid().unwrap_or(0);
            let depth = book.depth(10);

            log::debug!(
                "book {}: best_ask={:?} best_bid={:?} spread={} mid={} depth(10)={:?}",
                symb.as_str(),
                best_ask,
                best_bid,
                spread,
                mid,
                depth,
            );

            writeln!(
                writer,
                "{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
                symb.as_str(),
                best_ask,
                best_bid,
                spread,
                mid,
                depth,
            )
        })
        .collect::<io::Result<Vec<_>>>()?;

    log::info!("report complete");
    Ok(())
}
