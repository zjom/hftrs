//! Pure render pass over [`App`] state. Holds a borrow of the registry to
//! resolve per-row best bid/ask only for rows actually inside the symbol
//! list's viewport — invisible rows pay nothing per frame.

use super::app::{App, DepthLadder, Mode};
use itch5::messages::Symbol;
use orderbook::registry::Registry;
use orderbook::{OrderBook, Price, Quantity};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table,
};

pub fn draw<R: Registry>(f: &mut Frame, app: &mut App, registry: &R) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // body
            Constraint::Length(3), // footer (filter + help)
        ])
        .split(f.area());

    draw_header(f, outer[0], app);
    draw_body(f, outer[1], app, registry);
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

fn draw_body<R: Registry>(f: &mut Frame, area: Rect, app: &mut App, registry: &R) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    draw_symbol_list(f, cols[0], app, registry);
    draw_depth(f, cols[1], app);
}

fn draw_symbol_list<R: Registry>(f: &mut Frame, area: Rect, app: &mut App, registry: &R) {
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
    let inner = block.inner(area);
    let inner_height = inner.height as usize;
    let total = app.filtered.len();

    // Adjust scroll offset so the selection stays in view, mirroring ratatui's
    // own logic but applied before we materialise items so we can clip.
    let selected = app.list_state.selected();
    let mut offset = app.list_state.offset();
    if let Some(sel) = selected {
        if sel < offset {
            offset = sel;
        } else if inner_height > 0 && sel >= offset + inner_height {
            offset = sel + 1 - inner_height;
        }
    }
    let max_offset = total.saturating_sub(inner_height);
    offset = offset.min(max_offset);
    *app.list_state.offset_mut() = offset;

    let visible_count = total.saturating_sub(offset).min(inner_height);
    let items: Vec<ListItem> = app.filtered[offset..offset + visible_count]
        .iter()
        .map(|&i| {
            let (locate, symbol) = app.symbols[i];
            ListItem::new(symbol_row_line(&symbol, registry.get(locate)))
        })
        .collect();

    // Drive the List with a local state whose offset is 0 — we already
    // pre-clipped, so the widget should render every item we passed in.
    let mut local_state = ListState::default()
        .with_offset(0)
        .with_selected(selected.and_then(|s| s.checked_sub(offset)));
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

fn symbol_row_line(symbol: &Symbol, book: Option<&OrderBook>) -> Line<'static> {
    let (bid, ask, orders) = match book {
        Some(book) => {
            let bid = book
                .best_bid()
                .map_or_else(|| "—".to_string(), |(p, q)| format!("{} x{}", price(p), q));
            let ask = book
                .best_ask()
                .map_or_else(|| "—".to_string(), |(p, q)| format!("{} x{}", price(p), q));
            (bid, ask, book.len())
        }
        None => ("—".to_string(), "—".to_string(), 0),
    };
    Line::from(vec![
        Span::styled(
            format!("{:<8}", symbol.as_str().trim_end()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("{:<14}", bid), Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("{:<14}", ask), Style::default().fg(Color::Red)),
        Span::raw(" "),
        Span::styled(
            format!("orders={}", orders),
            Style::default().fg(Color::DarkGray),
        ),
    ])
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

    draw_ladder_side(f, columns[0], &app.depth, true);
    draw_ladder_side(f, columns[1], &app.depth, false);
}

fn draw_ladder_side(f: &mut Frame, area: Rect, depth: &DepthLadder, is_bid: bool) {
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

    let rows: Vec<Row> = if levels.is_empty() {
        let empty_cells: Vec<Cell> = vec!["—", "—", "—"]
            .into_iter()
            .map(|s| Cell::from(Line::from(s).alignment(alignment)))
            .collect();
        vec![Row::new(empty_cells)]
    } else {
        levels
            .iter()
            .map(|(p, q)| {
                let pct = if total == 0 {
                    0.0
                } else {
                    (*q as f64) / (total as f64) * 100.0
                };

                let mut row_data = vec![price(*p), q.to_string(), format!("{:>5.1}%", pct)];

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
