//! TUI application state.
//!
//! [`App`] holds a [`SymbolRow`] vector refreshed each frame plus UI-only
//! state (selection, filter, scroll, depth ladder for the focused symbol).
//! All registry interaction happens in [`App::refresh`], which is called
//! while the handler mutex is held; everything else operates on owned data.

use crate::handler::{HandlerStats, MessageHandler};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use itch5::messages::Symbol;
use orderbook::registry::Registry;
use orderbook::{Price, Quantity};
use ratatui::widgets::{ListState, TableState};

/// Levels per side captured for the focused symbol. The ladder widget never
/// shows more than this many rows.
pub const DEPTH_LEVELS: usize = 15;

/// One row in the symbol list. Captured during the per-frame snapshot so the
/// renderer never touches a book directly.
pub struct SymbolRow {
    pub locate: u16,
    pub symbol: Symbol,
    pub best_bid: Option<(Price, Quantity)>,
    pub best_ask: Option<(Price, Quantity)>,
    pub order_count: usize,
}

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
    pub rows: Vec<SymbolRow>,
    pub stats: HandlerStats,
    pub registered: usize,
    pub depth: DepthLadder,
    pub list_state: ListState,
    pub depth_state: TableState,
    pub filter: String,
    pub mode: Mode,
    /// Locate of the symbol currently selected, used to keep the same symbol
    /// highlighted across snapshots even if the filtered list re-orders.
    pub selected_locate: Option<u16>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            stats: HandlerStats::default(),
            registered: 0,
            depth: DepthLadder::default(),
            list_state: ListState::default(),
            depth_state: TableState::default(),
            filter: String::new(),
            mode: Mode::Normal,
            selected_locate: None,
        }
    }

    /// Pull a fresh view of the registry and handler stats
    pub fn refresh<R: Registry>(&mut self, handler: &MessageHandler<R>) {
        self.stats = *handler.stats();
        self.registered = handler.registry().len();

        // Filter/collect into rows. We reuse initially allocated Vec;
        self.rows.clear();
        let needle = (!self.filter.is_empty()).then(|| self.filter.as_str());
        for (locate, symbol, book) in handler.registry().iter() {
            if let Some(n) = &needle
                && !symbol.as_str().trim_end().starts_with(n)
            {
                continue;
            }
            self.rows.push(SymbolRow {
                locate,
                symbol: *symbol,
                best_bid: book.best_bid(),
                best_ask: book.best_ask(),
                order_count: book.len(),
            });
        }
        self.rows.sort_by_key(|r| r.symbol.to_u64());

        // Reconcile selection with the (possibly resized) row set.
        let selected = self
            .selected_locate
            .and_then(|loc| self.rows.iter().position(|r| r.locate == loc))
            .or(if self.rows.is_empty() { None } else { Some(0) });
        self.list_state.select(selected);
        self.selected_locate = selected.and_then(|i| self.rows.get(i).map(|r| r.locate));

        // Build depth ladder for the selected symbol.
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
            KeyCode::End if !self.rows.is_empty() => self.move_to(self.rows.len() - 1),
            KeyCode::Char('g') => self.move_to(0),
            KeyCode::Char('G') if !self.rows.is_empty() => self.move_to(self.rows.len() - 1),
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
        if self.rows.is_empty() {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let max = self.rows.len() as i32 - 1;
        let new = (cur + delta).clamp(0, max) as usize;
        self.move_to(new);
    }

    fn move_to(&mut self, idx: usize) {
        if idx < self.rows.len() {
            self.list_state.select(Some(idx));
            self.selected_locate = Some(self.rows[idx].locate);
        }
    }
}
