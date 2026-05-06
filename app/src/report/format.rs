//! Pluggable output formats for [`Snapshot`].
//!
//! Each format is a unit struct implementing [`Formatter`]. The
//! [`ReportFormat`] enum is the CLI-facing dispatcher: it selects a
//! formatter at runtime and is the single place to register new formats.

use super::snapshot::{BookSummary, Depth, Snapshot};
use orderbook::{Price, Quantity};
use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;

/// Strategy for serializing a [`Snapshot`] to a writer.
pub trait Formatter {
    fn write(&self, snapshot: &Snapshot, writer: &mut dyn Write) -> io::Result<()>;
}

/// Tab-separated table, one row per book. Compact and `awk`-friendly.
pub struct TsvFormatter;

/// JSON document with `stats` and `books` fields. Human-readable and easy
/// to consume from downstream tools.
pub struct JsonFormatter;

impl Formatter for TsvFormatter {
    fn write(&self, snapshot: &Snapshot, writer: &mut dyn Write) -> io::Result<()> {
        writeln!(
            writer,
            "symb\tbest_ask\tbest_bid\tspread\tmid\tdepth({})",
            snapshot.depth_levels
        )?;
        for b in &snapshot.books {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}",
                b.symbol.as_str().trim_end(),
                fmt_level(b.best_ask),
                fmt_level(b.best_bid),
                fmt_opt(b.spread),
                fmt_opt(b.mid),
                fmt_depth(&b.depth),
            )?;
        }
        Ok(())
    }
}

impl Formatter for JsonFormatter {
    fn write(&self, snapshot: &Snapshot, writer: &mut dyn Write) -> io::Result<()> {
        write!(writer, "{{\"stats\":")?;
        write_stats_json(writer, &snapshot.stats)?;
        write!(
            writer,
            ",\"depth_levels\":{},\"books\":[",
            snapshot.depth_levels
        )?;
        for (i, b) in snapshot.books.iter().enumerate() {
            if i > 0 {
                writer.write_all(b",")?;
            }
            write_book_json(writer, b)?;
        }
        writeln!(writer, "]}}")?;
        Ok(())
    }
}

/// CLI-selectable formats. Add a variant + match arm here to register a
/// new format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportFormat {
    #[default]
    Tsv,
    Json,
}

impl ReportFormat {
    pub fn write(self, snapshot: &Snapshot, writer: impl Write) -> io::Result<()> {
        let mut writer = writer;
        match self {
            ReportFormat::Tsv => TsvFormatter.write(snapshot, &mut writer),
            ReportFormat::Json => JsonFormatter.write(snapshot, &mut writer),
        }
    }
}

impl fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ReportFormat::Tsv => "tsv",
            ReportFormat::Json => "json",
        })
    }
}

impl FromStr for ReportFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "tsv" => Ok(ReportFormat::Tsv),
            "json" => Ok(ReportFormat::Json),
            other => Err(format!(
                "unknown report format '{other}' (expected tsv|json)"
            )),
        }
    }
}

fn fmt_opt<T: fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| "-".to_string(), |x| x.to_string())
}

fn fmt_level(v: Option<(Price, Quantity)>) -> String {
    v.map_or_else(|| "-".to_string(), |(p, q)| format!("{p}@{q}"))
}

fn fmt_depth(d: &Depth) -> String {
    let join = |levels: &[(Price, Quantity)]| {
        levels
            .iter()
            .map(|(p, q)| format!("{p}@{q}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("bids=[{}];asks=[{}]", join(&d.0), join(&d.1))
}

fn write_stats_json(w: &mut dyn Write, stats: &crate::handler::HandlerStats) -> io::Result<()> {
    write!(
        w,
        "{{\"orders_added\":{},\"orders_executed\":{},\"orders_cancelled\":{},\
         \"orders_deleted\":{},\"orders_replaced\":{},\"stock_directory_msgs\":{},\
         \"skipped_locate\":{}}}",
        stats.orders_added,
        stats.orders_executed,
        stats.orders_cancelled,
        stats.orders_deleted,
        stats.orders_replaced,
        stats.stock_directory_msgs,
        stats.skipped_locate,
    )
}

fn write_book_json(w: &mut dyn Write, b: &BookSummary) -> io::Result<()> {
    write!(
        w,
        "{{\"locate\":{},\"symbol\":\"{}\",\"best_bid\":",
        b.locate,
        b.symbol.as_str().trim_end(),
    )?;
    write_level_json(w, b.best_bid)?;
    write!(w, ",\"best_ask\":")?;
    write_level_json(w, b.best_ask)?;
    write!(
        w,
        ",\"spread\":{},\"mid\":{}",
        json_opt(b.spread),
        json_opt(b.mid)
    )?;
    write!(w, ",\"depth\":{{\"bids\":")?;
    write_levels_json(w, &b.depth.0)?;
    write!(w, ",\"asks\":")?;
    write_levels_json(w, &b.depth.1)?;
    write!(w, "}}}}")?;
    Ok(())
}

fn write_level_json(w: &mut dyn Write, lvl: Option<(Price, Quantity)>) -> io::Result<()> {
    match lvl {
        None => write!(w, "null"),
        Some((p, q)) => write!(w, "{{\"price\":{p},\"qty\":{q}}}"),
    }
}

fn write_levels_json(w: &mut dyn Write, levels: &[(Price, Quantity)]) -> io::Result<()> {
    write!(w, "[")?;
    for (i, (p, q)) in levels.iter().enumerate() {
        if i > 0 {
            w.write_all(b",")?;
        }
        write!(w, "{{\"price\":{p},\"qty\":{q}}}")?;
    }
    write!(w, "]")
}

fn json_opt<T: fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| "null".to_string(), |x| x.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::HandlerStats;
    use itch5::messages::Symbol;
    use std::str::FromStr;

    fn fixture() -> Snapshot {
        Snapshot {
            stats: HandlerStats {
                orders_added: 3,
                orders_executed: 1,
                orders_cancelled: 0,
                orders_deleted: 0,
                orders_replaced: 0,
                stock_directory_msgs: 1,
                skipped_locate: 0,
            },
            depth_levels: 2,
            books: vec![
                BookSummary {
                    locate: 7,
                    symbol: Symbol::from_str("AAPL").unwrap(),
                    best_bid: Some((1000, 100)),
                    best_ask: Some((1010, 50)),
                    spread: Some(10),
                    mid: Some(1005),
                    depth: (vec![(1000, 100), (995, 200)], vec![(1010, 50), (1015, 75)]),
                },
                BookSummary {
                    locate: 9,
                    symbol: Symbol::from_str("MSFT").unwrap(),
                    best_bid: None,
                    best_ask: None,
                    spread: None,
                    mid: None,
                    depth: (vec![], vec![]),
                },
            ],
        }
    }

    #[test]
    fn tsv_emits_header_and_one_row_per_book() {
        let mut buf = Vec::new();
        TsvFormatter.write(&fixture(), &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let mut lines = out.lines();
        assert_eq!(
            lines.next().unwrap(),
            "symb\tbest_ask\tbest_bid\tspread\tmid\tdepth(2)",
        );
        assert_eq!(
            lines.next().unwrap(),
            "AAPL\t1010@50\t1000@100\t10\t1005\tbids=[1000@100,995@200];asks=[1010@50,1015@75]",
        );
        assert_eq!(lines.next().unwrap(), "MSFT\t-\t-\t-\t-\tbids=[];asks=[]",);
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn json_renders_missing_levels_as_null() {
        let mut buf = Vec::new();
        JsonFormatter.write(&fixture(), &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\"orders_added\":3"));
        assert!(out.contains("\"depth_levels\":2"));
        assert!(out.contains("\"symbol\":\"AAPL\""));
        assert!(out.contains("\"best_bid\":{\"price\":1000,\"qty\":100}"));
        assert!(out.contains("\"symbol\":\"MSFT\""));
        assert!(out.contains("\"best_bid\":null"));
        assert!(out.contains("\"spread\":null"));
    }

    #[test]
    fn report_format_round_trips_via_str() {
        assert_eq!(ReportFormat::from_str("tsv").unwrap(), ReportFormat::Tsv);
        assert_eq!(ReportFormat::from_str("JSON").unwrap(), ReportFormat::Json);
        assert!(ReportFormat::from_str("xml").is_err());
        assert_eq!(ReportFormat::Tsv.to_string(), "tsv");
        assert_eq!(ReportFormat::Json.to_string(), "json");
    }

    #[test]
    fn report_format_dispatch_matches_direct_call() {
        let snap = fixture();
        let mut via_enum = Vec::new();
        ReportFormat::Tsv.write(&snap, &mut via_enum).unwrap();
        let mut via_struct = Vec::new();
        TsvFormatter.write(&snap, &mut via_struct).unwrap();
        assert_eq!(via_enum, via_struct);
    }
}
