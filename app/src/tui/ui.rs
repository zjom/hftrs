//! Pure render pass over [`App`] state. No I/O, no locks.

use super::app::{App, DepthLadder, Mode, SymbolRow};
use orderbook::{Price, Quantity};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};

pub fn draw(f: &mut Frame, app: &App) {
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
        Span::raw(format!(" {} symbols", app.registered)),
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

fn draw_body(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    draw_symbol_list(f, cols[0], app);
    draw_depth(f, cols[1], app);
}

fn draw_symbol_list(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| ListItem::new(symbol_row_line(row)))
        .collect();
    let title = if app.filter.is_empty() {
        format!(" symbols [{}] ", app.rows.len())
    } else {
        format!(" symbols [{} match \"{}\"] ", app.rows.len(), app.filter)
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let mut state = app.list_state;
    f.render_stateful_widget(list, area, &mut state);
}

fn symbol_row_line(row: &SymbolRow) -> Line<'static> {
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
            format!("orders={}", row.order_count),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn draw_depth(f: &mut Frame, area: Rect, app: &App) {
    let title = match app
        .list_state
        .selected()
        .and_then(|i| app.rows.get(i))
        .map(|r| r.symbol.as_str().trim_end().to_string())
    {
        Some(s) => format!(" depth · {} ", s),
        None => " depth ".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    draw_ladder_side(f, columns[0], &app.depth, true);
    draw_ladder_side(f, columns[1], &app.depth, false);
}

fn draw_ladder_side(f: &mut Frame, area: Rect, depth: &DepthLadder, is_bid: bool) {
    let levels = if is_bid { &depth.bids } else { &depth.asks };
    let total: Quantity = levels.iter().map(|(_, q)| *q).sum();
    let header_color = if is_bid { Color::Green } else { Color::Red };
    let header_label = if is_bid { "BIDS" } else { "ASKS" };

    let rows: Vec<Row> = if levels.is_empty() {
        vec![Row::new(vec![
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
        ])]
    } else {
        let rows = levels.iter().map(|(p, q)| {
            let pct = if total == 0 {
                0.0
            } else {
                (*q as f64) / (total as f64) * 100.0
            };
            Row::new(vec![price(*p), q.to_string(), format!("{:>5.1}%", pct)])
        });
        if is_bid {
            rows.collect()
        } else {
            rows.rev().collect()
        }
    };

    let header = Row::new(vec!["price", "qty", "share"]).style(
        Style::default()
            .fg(header_color)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::TOP).title(Span::styled(
        format!(
            " {} ({} lvls, total={}) ",
            header_label,
            levels.len(),
            total
        ),
        Style::default().fg(header_color),
    )));
    f.render_widget(table, area);
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
