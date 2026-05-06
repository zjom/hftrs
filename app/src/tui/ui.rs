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

/// One vertical slot in the unified price ladder.
///
/// `price = None` is a spacer row, used to make the visual gap between two
/// adjacent levels proportional to their tick distance.
struct LadderRow {
    price: Option<Price>,
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
    draw_ladder_side(f, columns[0], &app.depth, &rows, true);
    draw_ladder_side(f, columns[1], &app.depth, &rows, false);
}

/// Merge bids and asks into a single descending price sequence, inserting
/// blank rows wherever adjacent levels are more than one tick apart so the
/// rendered gap reflects the price distance. Output is capped to `max_rows`
/// so it never exceeds the cached viewport height.
fn build_ladder(depth: &DepthLadder, max_rows: usize) -> Vec<LadderRow> {
    use std::collections::BTreeMap;

    let mut levels: BTreeMap<Price, (Option<Quantity>, Option<Quantity>)> = BTreeMap::new();
    for &(p, q) in &depth.bids {
        levels.entry(p).or_insert((None, None)).0 = Some(q);
    }
    for &(p, q) in &depth.asks {
        levels.entry(p).or_insert((None, None)).1 = Some(q);
    }
    if levels.is_empty() {
        return Vec::new();
    }

    // Walk descending: highest price (top of book ask) first, lowest (worst
    // bid) last.
    let sorted: Vec<_> = levels.into_iter().rev().collect();

    // Visual unit = smallest observed gap. Falls back to 1 if all levels
    // collapse onto one price (only possible with a single entry).
    let unit = sorted
        .windows(2)
        .map(|w| w[0].0 - w[1].0)
        .filter(|&d| d > 0)
        .min()
        .unwrap_or(1);

    // Cap inserted spacers per gap so a single far-out level can't push the
    // ladder past the available height.
    const MAX_SPACER_ROWS: i64 = 4;

    let mut rows = Vec::with_capacity(sorted.len() * 2);
    for (i, &(p, (bid, ask))) in sorted.iter().enumerate() {
        if rows.len() >= max_rows {
            break;
        }
        rows.push(LadderRow {
            price: Some(p),
            bid,
            ask,
        });
        if let Some(&(next_p, _)) = sorted.get(i + 1) {
            let spacers = ((p - next_p) / unit - 1).clamp(0, MAX_SPACER_ROWS);
            for _ in 0..spacers {
                if rows.len() >= max_rows {
                    break;
                }
                rows.push(LadderRow {
                    price: None,
                    bid: None,
                    ask: None,
                });
            }
        }
    }
    rows
}

fn draw_ladder_side(
    f: &mut Frame,
    area: Rect,
    depth: &DepthLadder,
    ladder: &[LadderRow],
    is_bid: bool,
) {
    let (levels, label, color, is_reversed, alignment, borders, flex) = if is_bid {
        (
            &depth.bids,
            "BIDS",
            Color::Green,
            true,
            Alignment::Right,
            Borders::TOP | Borders::RIGHT,
            Flex::End,
        )
    } else {
        (
            &depth.asks,
            "ASKS",
            Color::Red,
            false,
            Alignment::Left,
            Borders::TOP,
            Flex::Start,
        )
    };

    let total: Quantity = levels.iter().map(|(_, q)| *q).sum();

    let mut column_defs = vec![
        ("price", Constraint::Length(12)),
        ("qty", Constraint::Length(12)),
        ("share", Constraint::Length(8)),
    ];

    if is_reversed {
        column_defs.reverse();
    }

    let header_cells: Vec<Cell> = column_defs
        .iter()
        .map(|(h, _)| Cell::from(Line::from(*h).alignment(alignment)))
        .collect();
    let constraints: Vec<Constraint> = column_defs.iter().map(|(_, c)| *c).collect();

    let header =
        Row::new(header_cells).style(Style::default().fg(color).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = if ladder.is_empty() {
        let empty_cells: Vec<Cell> = vec!["—", "—", "—"]
            .into_iter()
            .map(|s| Cell::from(Line::from(s).alignment(alignment)))
            .collect();
        vec![Row::new(empty_cells)]
    } else {
        ladder
            .iter()
            .map(|row| {
                let mut row_data = match (row.price, side_qty(row, is_bid)) {
                    (Some(p), Some(q)) => {
                        let pct = if total == 0 {
                            0.0
                        } else {
                            (q as f64) / (total as f64) * 100.0
                        };
                        vec![price(p), q.to_string(), format!("{:>5.1}%", pct)]
                    }
                    // Price exists on the other side only — keep the price
                    // column populated so the row aligns visually, but blank
                    // qty/share to make clear there's no liquidity here.
                    // (Some(p), None) => vec![price(p), String::new(), String::new()],
                    // Spacer row in the merged ladder: leave everything blank
                    // so the negative space conveys the price gap.
                    (_, _) => vec![String::new(), String::new(), String::new()],
                };

                if is_reversed {
                    row_data.reverse();
                }
                let cells: Vec<Cell> = row_data
                    .into_iter()
                    .map(|s| Cell::from(Line::from(s).alignment(alignment)))
                    .collect();

                Row::new(cells)
            })
            .collect()
    };

    let table_title = Line::from(format!(
        " {} ({} lvls, total={}) ",
        label,
        levels.len(),
        total
    ))
    .style(Style::default().fg(color))
    .centered();

    let table = Table::new(rows, constraints)
        .header(header)
        .block(
            Block::default()
                .borders(borders)
                .title(table_title)
                .padding(Padding::horizontal(2)),
        )
        .flex(flex);

    f.render_widget(table, area);
}

fn side_qty(row: &LadderRow, is_bid: bool) -> Option<Quantity> {
    if is_bid { row.bid } else { row.ask }
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
