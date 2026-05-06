//! TUI application state.
//!
//! [`App`] caches a sorted `(locate, symbol)` table for the symbol list and
//! UI-only state (selection, filter, scroll, depth ladder for the focused
//! symbol). The symbol cache is rebuilt only when new stock-directory
//! messages arrive — in practice the boot-time burst — so steady-state
//! refreshes touch the registry only to read the focused symbol's depth.
//! Per-row best bid/ask are computed lazily during render for visible rows
//! only; see [`crate::tui::ui`].

use crate::handler::{HandlerStats, MessageHandler};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use itch5::messages::Symbol;
use orderbook::registry::Registry;
use orderbook::{Price, Quantity};
use ratatui::widgets::ListState;

/// Levels per side captured for the focused symbol. The ladder widget never
/// shows more than this many rows.
pub const DEPTH_LEVELS: usize = 15;

/// Depth ladder for the currently selected symbol.
#[derive(Default)]
pub struct DepthLadder {
    pub bids: Vec<(Price, Quantity)>,
    pub asks: Vec<(Price, Quantity)>,
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
            last_directory_msgs: 0,
            last_filter: None,
        }
    }

    /// Pull a fresh view of the registry and handler stats. Skips the symbol
    /// cache rebuild when no new stock-directory messages have arrived, and
    /// the filter index rebuild when neither input changed.
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

        // Build depth ladder for the selected symbol only.
        self.depth = match self
            .selected_locate
            .and_then(|loc| handler.registry().get(loc))
        {
            Some(book) => {
                let (bids, asks) = book.depth(DEPTH_LEVELS);
                DepthLadder { bids, asks }
            }
            None => DepthLadder::default(),
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
