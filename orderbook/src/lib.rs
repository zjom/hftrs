//! A limit order book for Nasdaq TotalView-ITCH 5.0 market data.
//!
//! See the `README.md` for design rationale. The hot-path entry points are
//! [`OrderBook::add`], [`OrderBook::execute`], [`OrderBook::cancel`],
//! [`OrderBook::delete`], and [`OrderBook::replace`].

pub mod book;
pub mod error;
pub mod level;
pub mod pool;
pub mod registry;
pub mod types;

pub use book::OrderBook;
pub use error::{BookError, Result};
pub use types::{Order, OrderId, Price, Quantity, Side, Timestamp, Trade};
