//! Pure-data view of registry state at a point in time.
//!
//! A [`Snapshot`] is what every formatter consumes; it carries no I/O and
//! no formatting concerns, so it is cheap to construct in tests and easy to
//! extend without touching format code.

use crate::handler::{HandlerStats, MessageHandler};
use itch5::messages::Symbol;
use orderbook::registry::Registry;
use orderbook::{Price, Quantity};

/// One side of a depth ladder: price levels in best-first order.
pub type Levels = Vec<(Price, Quantity)>;

/// Two-sided depth ladder: `(bids, asks)`.
pub type Depth = (Levels, Levels);

/// Per-book metrics captured at snapshot time.
pub struct BookSummary {
    pub locate: u16,
    pub symbol: Symbol,
    pub best_bid: Option<(Price, Quantity)>,
    pub best_ask: Option<(Price, Quantity)>,
    pub spread: Option<Price>,
    pub mid: Option<Price>,
    /// `(bids, asks)`, up to [`Snapshot::depth_levels`] entries per side.
    pub depth: Depth,
}

/// Complete report payload: per-run counters + per-book summaries.
pub struct Snapshot {
    pub stats: HandlerStats,
    pub depth_levels: usize,
    pub books: Vec<BookSummary>,
}

impl Snapshot {
    /// Capture the registry into a pure `Snapshot`.
    pub fn capture<R: Registry>(handler: &MessageHandler<R>, depth_levels: usize) -> Self {
        let books = handler
            .registry()
            .iter()
            .map(|(locate, symbol, book)| BookSummary {
                locate,
                symbol: *symbol,
                best_bid: book.best_bid(),
                best_ask: book.best_ask(),
                spread: book.spread(),
                mid: book.mid(),
                depth: book.depth(depth_levels),
            })
            .collect();
        Self {
            stats: *handler.stats(),
            depth_levels,
            books,
        }
    }
}
