//! Pure render pass over [`App`] state. Reads only fields populated by
//! [`App::refresh`] (visible-row cache, depth ladder, viewport sizing), so
//! the renderer can run without holding the handler lock.
//!
//! [`App::refresh`]: super::app::App::refresh

use super::app::{App, DepthLadder, Mode, VisibleRow};
use orderbook::{Price, Quantity};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table,
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // body
            Constraint::Length(3), // footer (filter + help)
        ])
        .split(f.area());

    draw_header(f, outer[0], app);
    draw_body(f, outer[1], app);
    draw_footer(f, outer[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let s = app.stats;
    let line = Line::from(vec![
        Span::styled("registry", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {} symbols", app.symbols.len())),
        sep(),
        Span::styled("added", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.orders_added)),
        sep(),
        Span::styled("exec", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.orders_executed)),
        sep(),
        Span::styled("cancel", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.orders_cancelled)),
        sep(),
        Span::styled("delete", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.orders_deleted)),
        sep(),
        Span::styled("replace", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.orders_replaced)),
        sep(),
        Span::styled("skip", Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}", s.skipped_locate)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" hftrs · itch5 replay ");
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_body(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    draw_symbol_list(f, cols[0], app);
    draw_depth(f, cols[1], app);
}

fn draw_symbol_list(f: &mut Frame, area: Rect, app: &mut App) {
    let title = if app.filter.is_empty() {
        format!(" symbols [{}] ", app.filtered.len())
    } else {
        format!(
            " symbols [{} match \"{}\"] ",
            app.filtered.len(),
            app.filter
        )
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    let items: Vec<ListItem> = app
        .visible_rows
        .iter()
        .map(|row| ListItem::new(symbol_row_line(row)))
        .collect();

    // `App::refresh` already pre-clipped to the viewport and wrote the
    // matching offset onto `list_state`. Drive the List with a local state
    // whose offset is 0 so it renders the slice verbatim, translating the
    // absolute selection into a slice-relative index.
    let offset = app.list_state.offset();
    let mut local_state = ListState::default().with_offset(0).with_selected(
        app.list_state
            .selected()
            .and_then(|s| s.checked_sub(offset)),
    );
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut local_state);
}

fn symbol_row_line(row: &VisibleRow) -> Line<'static> {
    let bid = row
        .best_bid
        .map_or_else(|| "—".to_string(), |(p, q)| format!("{} x{}", price(p), q));
    let ask = row
        .best_ask
        .map_or_else(|| "—".to_string(), |(p, q)| format!("{} x{}", price(p), q));
    Line::from(vec![
        Span::styled(
            format!("{:<8}", row.symbol.as_str().trim_end()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("{:<14}", bid), Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("{:<14}", ask), Style::default().fg(Color::Red)),
        Span::raw(" "),
        Span::styled(
            format!("orders={}", row.orders),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// One vertical slot in the unified price ladder. Either side may be
/// `None` if no liquidity rests at that price.
struct LadderRow {
    price: Price,
    bid: Option<Quantity>,
    ask: Option<Quantity>,
}

fn draw_depth(f: &mut Frame, area: Rect, app: &App) {
    let title = app
        .list_state
        .selected()
        .and_then(|i| app.symbol_at(i))
        .map_or_else(
            || " depth ".to_string(),
            |(_, sym)| format!(" depth · {} ", sym.as_str().trim_end()),
        );

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let rows = build_ladder(&app.depth, app.viewport.ladder_rows);
    draw_ladder_side(f, columns[0], &rows, true);
    draw_ladder_side(f, columns[1], &rows, false);
}

/// Merge bids and asks into a single descending price sequence. Output is
/// capped to `max_rows` so it never exceeds the cached viewport height.
fn build_ladder(depth: &DepthLadder, max_rows: usize) -> Vec<LadderRow> {
    use std::collections::BTreeMap;

    let mut levels: BTreeMap<Price, (Option<Quantity>, Option<Quantity>)> = BTreeMap::new();
    for &(p, q) in &depth.bids {
        levels.entry(p).or_insert((None, None)).0 = Some(q);
    }
    for &(p, q) in &depth.asks {
        levels.entry(p).or_insert((None, None)).1 = Some(q);
    }

    // Walk descending: highest price (top of book ask) first, lowest (worst
    // bid) last.
    levels
        .into_iter()
        .rev()
        .take(max_rows)
        .map(|(price, (bid, ask))| LadderRow { price, bid, ask })
        .collect()
}

/// Static rendering config for one side of the ladder. Two factory methods
/// (`bids`, `asks`) own all the per-side knobs, so the renderer only needs
/// to know which one it's drawing.
#[derive(Clone, Copy)]
struct LadderSide {
    is_bid: bool,
    label: &'static str,
    color: Color,
    alignment: Alignment,
    borders: Borders,
    flex: Flex,
    /// `(header, width)` ordered for this side's reading direction — bids
    /// read right-to-left from the spread, so the price column sits last.
    columns: [(&'static str, u16); 3],
    /// Endpoints of the per-row share gradient: rows fade from `dim` at
    /// minimal share toward `peak` at the largest qty on this side.
    dim: (u8, u8, u8),
    peak: (u8, u8, u8),
}

impl LadderSide {
    fn bids() -> Self {
        Self {
            is_bid: true,
            label: "BIDS",
            color: Color::Green,
            alignment: Alignment::Right,
            borders: Borders::TOP | Borders::RIGHT,
            flex: Flex::End,
            columns: [("share", 8), ("qty", 12), ("price", 12)],
            dim: (20, 60, 20),
            peak: (140, 255, 140),
        }
    }

    fn asks() -> Self {
        Self {
            is_bid: false,
            label: "ASKS",
            color: Color::Red,
            alignment: Alignment::Left,
            borders: Borders::TOP,
            flex: Flex::Start,
            columns: [("price", 12), ("qty", 12), ("share", 8)],
            dim: (60, 20, 20),
            peak: (255, 140, 140),
        }
    }

    fn qty_of(&self, row: &LadderRow) -> Option<Quantity> {
        if self.is_bid { row.bid } else { row.ask }
    }

    /// Reorder a canonical `[price, qty, share]` triple into this side's
    /// column order.
    fn order<T>(&self, [price, qty, share]: [T; 3]) -> [T; 3] {
        if self.is_bid {
            [share, qty, price]
        } else {
            [price, qty, share]
        }
    }

    fn cell(&self, s: impl Into<String>) -> Cell<'static> {
        Cell::from(Line::from(s.into()).alignment(self.alignment))
    }

    /// Linear interpolation between `dim` and `peak`. `intensity` is the
    /// row's qty as a fraction of the largest qty on this side.
    fn shade(&self, intensity: f64) -> Color {
        let t = intensity.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Color::Rgb(
            lerp(self.dim.0, self.peak.0),
            lerp(self.dim.1, self.peak.1),
            lerp(self.dim.2, self.peak.2),
        )
    }
}

#[derive(Default, Clone, Copy)]
struct LadderStats {
    total: Quantity,
    max_qty: Quantity,
    n_levels: usize,
}

impl LadderStats {
    fn collect(ladder: &[LadderRow], side: &LadderSide) -> Self {
        ladder
            .iter()
            .filter_map(|r| side.qty_of(r))
            .fold(Self::default(), |mut s, q| {
                s.total += q;
                s.max_qty = s.max_qty.max(q);
                s.n_levels += 1;
                s
            })
    }
}

fn draw_ladder_side(f: &mut Frame, area: Rect, ladder: &[LadderRow], is_bid: bool) {
    let side = if is_bid {
        LadderSide::bids()
    } else {
        LadderSide::asks()
    };
    let stats = LadderStats::collect(ladder, &side);

    let header = Row::new(side.columns.map(|(h, _)| side.cell(h)))
        .style(Style::default().fg(side.color).add_modifier(Modifier::BOLD));
    let constraints = side.columns.map(|(_, w)| Constraint::Length(w));

    let rows: Vec<Row> = if ladder.is_empty() {
        vec![Row::new(side.order(["—"; 3]).map(|s| side.cell(s)))]
    } else {
        ladder
            .iter()
            .map(|row| ladder_data_row(row, &side, &stats))
            .collect()
    };

    let title = Line::from(format!(
        " {} ({} lvls, total={}) ",
        side.label, stats.n_levels, stats.total
    ))
    .style(Style::default().fg(side.color))
    .centered();

    let table = Table::new(rows, constraints)
        .header(header)
        .block(
            Block::default()
                .borders(side.borders)
                .title(title)
                .padding(Padding::horizontal(2)),
        )
        .flex(side.flex);

    f.render_widget(table, area);
}

fn ladder_data_row(row: &LadderRow, side: &LadderSide, stats: &LadderStats) -> Row<'static> {
    let qty = side.qty_of(row);
    let cells: [String; 3] = match qty {
        Some(q) => {
            let pct = if stats.total > 0 {
                (q as f64) / (stats.total as f64) * 100.0
            } else {
                0.0
            };
            [price(row.price), q.to_string(), format!("{:>5.1}%", pct)]
        }
        None => [String::new(), String::new(), String::new()],
    };

    let style = match (qty, stats.max_qty) {
        (Some(q), m) if m > 0 => Style::default().fg(side.shade(q as f64 / m as f64)),
        _ => Style::default(),
    };

    Row::new(side.order(cells).map(|s| side.cell(s))).style(style)
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let line = match app.mode {
        Mode::Filter => Line::from(vec![
            Span::styled(
                "/",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&app.filter),
            Span::styled("█", Style::default().fg(Color::Yellow)),
            Span::styled(
                "   Enter: confirm   Esc: clear",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Mode::Normal => Line::from(vec![
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" move  "),
            Span::styled("g/G", Style::default().fg(Color::Cyan)),
            Span::raw(" top/bot  "),
            Span::styled("/", Style::default().fg(Color::Cyan)),
            Span::raw(" filter  "),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" quit"),
        ]),
    };
    let block = Block::default().borders(Borders::ALL);
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn sep() -> Span<'static> {
    Span::styled("  │  ", Style::default().fg(Color::DarkGray))
}

/// Render an ITCH price (4 implicit decimals) as a human-readable string.
fn price(p: Price) -> String {
    let whole = p / 10_000;
    let frac = (p.abs() % 10_000) as u32;
    format!("{}.{:04}", whole, frac)
}
