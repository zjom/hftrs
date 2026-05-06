//! ITCH 5.0 replay application.
//!
//! Wires the moldudp transport, the itch5 parser, and the orderbook
//! registry into a single process: a server thread streams an mmap'd
//! ITCH file into a multicast group, a client thread consumes the
//! resulting MoldUDP64 packets and rebuilds the books, and on shutdown
//! a per-symbol report is emitted.
//!
//! ## Module map
//! - [`config`]   — CLI surface ([`Config`]).
//! - [`pipeline`] — top-level orchestration ([`pipeline::run`]).
//! - [`replay`]   — server-side replay loop.
//! - [`handler`]  — receive loop + ITCH dispatch.
//! - [`report`]   — pluggable report formats.

pub mod config;
pub mod handler;
pub mod pipeline;
pub mod replay;
pub mod report;
pub use config::Config;
pub use pipeline::run;
