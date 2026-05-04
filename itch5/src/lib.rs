//! Zero-copy parser for the [Nasdaq TotalView-ITCH 5.0] binary protocol.
//!
//! ITCH 5.0 is Nasdaq's real-time market data feed. It encodes every order
//! book event — adds, cancels, executions, replacements — plus market-wide
//! events and cross-trade notifications as compact fixed-length binary
//! messages, each prefixed with a 2-byte big-endian length.
//!
//! # Quick start
//!
//! Implement [`MessageHandler`] for the message types you care about, then
//! hand your bytes to [`Parser`]:
//!
//! ```no_run
//! use std::ops::ControlFlow;
//! use itch5::{MessageHandler, Parser, messages::TradeMessage};
//!
//! #[derive(Default)]
//! struct TradeCounter { count: u64 }
//!
//! impl MessageHandler for TradeCounter {
//!     fn on_trade_message(&mut self, _msg: &TradeMessage) -> ControlFlow<()> {
//!         self.count += 1;
//!         ControlFlow::Continue(())
//!     }
//! }
//!
//! let data: &[u8] = &[]; // replace with real feed bytes
//! let mut handler = TradeCounter::default();
//! Parser::new(data).parse_stream(&mut handler).unwrap();
//! ```
//!
//! Return `ControlFlow::Break(())` from any handler method to stop early.
//!
//! # Wire format
//!
//! ```text
//! ┌──────────────────────┬───────────────────────────┐
//! │  2 B (big-endian)    │  N bytes (message body)   │
//! │  message length      │  tag + fixed-width fields  │
//! └──────────────────────┴───────────────────────────┘
//! ```
//!
//! All message structs are `#[repr(C, packed)]` and implement
//! [`zerocopy::FromBytes`], so parsing is a bounds-checked pointer cast —
//! no heap allocation, no copying.
//!
//! [Nasdaq TotalView-ITCH 5.0]: https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/NQTVITCHspecification.pdf

pub mod messages;
mod parse;

pub use parse::{MessageHandler, ParseError, Parser, parse_one};
