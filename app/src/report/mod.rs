//! Report generation: format-agnostic snapshot + pluggable formatters.
//!
//! The reporting pipeline is split into two stages:
//!
//! 1. [`snapshot::Snapshot::capture`] walks the registry once and produces
//!    a pure-data view of every book's metrics (best bid/ask, spread, mid,
//!    depth ladder) plus the run's [`HandlerStats`].
//! 2. A [`Formatter`] consumes the snapshot and writes it to a sink.
//!
//! ## Adding a new format
//! Implement [`Formatter`] for a new type, add a variant to [`ReportFormat`],
//! and wire the variant in [`ReportFormat::write`] and the `--format` CLI.
//!
//! ## Adding a new metric
//! Extend [`snapshot::BookSummary`] and capture it in
//! [`snapshot::Snapshot::capture`]. Existing formatters keep working;
//! formatters that should surface the new field are updated independently.
//!
//! [`HandlerStats`]: crate::handler::HandlerStats

pub mod format;
pub mod snapshot;

pub use format::{Formatter, JsonFormatter, ReportFormat, TsvFormatter};
pub use snapshot::{BookSummary, Snapshot};

use crate::handler::MessageHandler;
use orderbook::registry::Registry;
use std::io;

/// Capture a snapshot of `handler` and write it via `format`.
pub fn write<R: Registry, W: io::Write>(
    handler: &MessageHandler<R>,
    format: ReportFormat,
    depth: usize,
    writer: W,
) -> io::Result<()> {
    let snapshot = Snapshot::capture(handler, depth);
    tracing::info!(
        "writing report: {} book(s), depth={depth}, format={format}",
        snapshot.books.len(),
    );
    format.write(&snapshot, writer)
}
