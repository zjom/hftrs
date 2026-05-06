//! TUI application state.
//!
//! [`App`] caches a sorted `(locate, symbol)` table for the symbol list and
//! UI-only state (selection, filter, scroll, depth ladder for the focused
//! symbol). The symbol cache is rebuilt only when new stock-directory
//! messages arrive — in practice the boot-time burst — so steady-state
//! refreshes touch the registry only to read the focused symbol's depth.
//!
//! Per-frame: callers stamp the layout-derived [`Viewport`] with
//! [`set_viewport`] *before* taking the handler lock, then [`refresh`]
//! materialises every visible cell into [`visible_rows`] and [`depth`] under
//! the lock. The renderer (see [`crate::tui::ui`]) consumes only these
//! caches, so the lock can be dropped before drawing.
//!
//! [`set_viewport`]: App::set_viewport
//! [`refresh`]: App::refresh
//! [`visible_rows`]: App::visible_rows
//! [`depth`]: App::depth

use crate::handler::{HandlerStats, MessageHandler};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use itch5::messages::Symbol;
use orderbook::registry::Registry;
use orderbook::{Price, Quantity};
use ratatui::widgets::ListState;

/// Depth ladder for the currently selected symbol.
#[derive(Default)]
pub struct DepthLadder {
    pub bids: Vec<(Price, Quantity)>,
    pub asks: Vec<(Price, Quantity)>,
}

/// Layout-derived row counts for the next render. Stamped by the event loop
/// from terminal size (cheap, lock-free) so [`App::refresh`] knows exactly
/// how much data the renderer will consume — and the renderer can run
/// without re-deriving anything from the registry.
#[derive(Default, Clone, Copy)]
pub struct Viewport {
    /// Visible row count for the symbol list (block borders subtracted).
    pub list_height: usize,
    /// Visible data rows for one ladder side, after block borders, the
    /// inter-side divider, and the table header.
    pub ladder_rows: usize,
}

/// One materialised symbol-list row. Resolved under the handler lock during
/// [`App::refresh`] so the render pass needs no registry access.
#[derive(Clone, Copy)]
pub struct VisibleRow {
    pub symbol: Symbol,
    pub best_bid: Option<(Price, Quantity)>,
    pub best_ask: Option<(Price, Quantity)>,
    pub orders: usize,
}

/// Input mode: normal navigation vs. live filter editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
}

pub struct App {
    /// Sorted cache of `(locate, symbol)` for every registered symbol. Stable
    /// for the rest of the run once stock-directory messages stop arriving.
    pub symbols: Vec<(u16, Symbol)>,
    /// Indices into [`Self::symbols`] matching the active filter, in display
    /// order. Rebuilt only when `symbols` or `filter` changes.
    pub filtered: Vec<usize>,
    pub stats: HandlerStats,
    pub depth: DepthLadder,
    pub list_state: ListState,
    pub filter: String,
    pub mode: Mode,
    /// Locate of the symbol currently selected, used to keep the same symbol
    /// highlighted across snapshots even if the filtered list re-orders.
    pub selected_locate: Option<u16>,
    /// Layout sizing for the next render, captured before locking.
    pub viewport: Viewport,
    /// Pre-clipped slice of the symbol list. The first entry corresponds to
    /// `filtered[list_state.offset()]`.
    pub visible_rows: Vec<VisibleRow>,
    /// Last observed `stats.stock_directory_msgs`; a change implies a possible
    /// new registry entry, so the symbol cache must be rebuilt.
    last_directory_msgs: u64,
    /// `filter` value used the last time `filtered` was rebuilt.
    last_filter: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            filtered: Vec::new(),
            stats: HandlerStats::default(),
            depth: DepthLadder::default(),
            list_state: ListState::default(),
            filter: String::new(),
            mode: Mode::Normal,
            selected_locate: None,
            viewport: Viewport::default(),
            visible_rows: Vec::new(),
            last_directory_msgs: 0,
            last_filter: None,
        }
    }

    /// Stamp the layout sizing for the upcoming refresh. Must be called
    /// before [`refresh`] (and before locking the handler) so the lock
    /// window covers exactly the work the renderer will display.
    ///
    /// [`refresh`]: Self::refresh
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    /// Pull a fresh view of the registry and handler stats. Skips the symbol
    /// cache rebuild when no new stock-directory messages have arrived, and
    /// the filter index rebuild when neither input changed. Materialises
    /// every visible cell into [`visible_rows`] and [`depth`] so the render
    /// pass can run lock-free.
    ///
    /// [`visible_rows`]: Self::visible_rows
    /// [`depth`]: Self::depth
    pub fn refresh<R: Registry>(&mut self, handler: &MessageHandler<R>) {
        self.stats = *handler.stats();

        let mut symbols_changed = false;
        if self.stats.stock_directory_msgs != self.last_directory_msgs {
            self.symbols.clear();
            for (locate, symbol, _) in handler.registry().iter() {
                self.symbols.push((locate, *symbol));
            }
            self.symbols.sort_by_key(|(_, s)| s.to_u64());
            self.last_directory_msgs = self.stats.stock_directory_msgs;
            symbols_changed = true;
        }

        if symbols_changed || self.last_filter.as_deref() != Some(self.filter.as_str()) {
            self.rebuild_filtered();
        }

        // Reconcile selection with the (possibly resized) filtered set.
        let selected = self
            .selected_locate
            .and_then(|loc| {
                self.filtered
                    .iter()
                    .position(|&i| self.symbols[i].0 == loc)
            })
            .or(if self.filtered.is_empty() {
                None
            } else {
                Some(0)
            });
        self.list_state.select(selected);
        self.selected_locate = selected.and_then(|i| {
            self.filtered
                .get(i)
                .map(|&j| self.symbols[j].0)
        });

        // Pre-clip the symbol list to the viewport and resolve each visible
        // row's best bid/ask. Mirrors ratatui's own scroll-into-view logic so
        // the render pass can hand the pre-clipped slice straight to `List`
        // with offset 0.
        let height = self.viewport.list_height;
        let total = self.filtered.len();
        let mut offset = self.list_state.offset();
        if let Some(sel) = self.list_state.selected() {
            if sel < offset {
                offset = sel;
            } else if height > 0 && sel >= offset + height {
                offset = sel + 1 - height;
            }
        }
        offset = offset.min(total.saturating_sub(height));
        *self.list_state.offset_mut() = offset;

        let visible_count = total.saturating_sub(offset).min(height);
        self.visible_rows.clear();
        self.visible_rows.reserve(visible_count);
        for &i in &self.filtered[offset..offset + visible_count] {
            let (locate, symbol) = self.symbols[i];
            self.visible_rows.push(match handler.registry().get(locate) {
                Some(book) => VisibleRow {
                    symbol,
                    best_bid: book.best_bid(),
                    best_ask: book.best_ask(),
                    orders: book.len(),
                },
                None => VisibleRow {
                    symbol,
                    best_bid: None,
                    best_ask: None,
                    orders: 0,
                },
            });
        }

        // Fetch only as much depth as the ladder can show. We split the row
        // budget across both sides so the merged ladder (with spacers for
        // the spread) fills the available height in the typical case.
        let per_side = self.viewport.ladder_rows.div_ceil(2);
        self.depth = match self
            .selected_locate
            .and_then(|loc| handler.registry().get(loc))
        {
            Some(book) if per_side > 0 => {
                let (bids, asks) = book.depth(per_side);
                DepthLadder { bids, asks }
            }
            _ => DepthLadder::default(),
        };
    }

    fn rebuild_filtered(&mut self) {
        self.filtered.clear();
        let needle = self.filter.as_str();
        for (i, (_, sym)) in self.symbols.iter().enumerate() {
            if needle.is_empty() || sym.as_str().trim_end().starts_with(needle) {
                self.filtered.push(i);
            }
        }
        self.last_filter = Some(self.filter.clone());
    }

    /// Symbol for the row at `filtered_idx`, if any.
    pub fn symbol_at(&self, filtered_idx: usize) -> Option<&(u16, Symbol)> {
        self.filtered
            .get(filtered_idx)
            .and_then(|&i| self.symbols.get(i))
    }

    /// Returns `true` when the user wants to quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        match self.mode {
            Mode::Normal => self.handle_key_normal(key),
            Mode::Filter => {
                self.handle_key_filter(key);
                false
            }
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) -> bool {
        // Ctrl-C also quits, mirroring the rest of the pipeline.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home => self.move_to(0),
            KeyCode::End if !self.filtered.is_empty() => self.move_to(self.filtered.len() - 1),
            KeyCode::Char('g') => self.move_to(0),
            KeyCode::Char('G') if !self.filtered.is_empty() => self.move_to(self.filtered.len() - 1),
            _ => {}
        }
        false
    }

    fn handle_key_filter(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(char) => {
                for c in char.to_uppercase() {
                    self.filter.push(c);
                }
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let max = self.filtered.len() as i32 - 1;
        let new = (cur + delta).clamp(0, max) as usize;
        self.move_to(new);
    }

    fn move_to(&mut self, idx: usize) {
        if let Some(&i) = self.filtered.get(idx) {
            self.list_state.select(Some(idx));
            self.selected_locate = Some(self.symbols[i].0);
        }
    }
}
