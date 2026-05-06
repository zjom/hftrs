//! Terminal UI for live exploration of the registry.
//!
//! Runs on a dedicated thread. To minimise impact on the receive loop, the
//! TUI shares the [`MessageHandler`] via [`SharedHandler`]: it acquires the
//! mutex briefly each frame to build a [`Snapshot`] of registry state, then
//! renders without holding any lock. Redraws are gated to ~10 Hz so contention
//! with the hot path is negligible.
//!
//! [`MessageHandler`]: crate::handler::MessageHandler
//! [`SharedHandler`]: crate::handler::SharedHandler
//! [`Snapshot`]: crate::report::Snapshot

mod app;
mod ui;

use crate::handler::SharedHandler;
use anyhow::{Context, Result};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use orderbook::registry::Registry;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub use app::App;

/// Approximate redraw interval. Chosen to keep mutex contention with the
/// receive loop low while remaining responsive to keystrokes.
const FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// Run the TUI on the calling thread until the user quits or `shutdown` is
/// raised. On exit, terminal state is restored regardless of error.
pub fn run<R: Registry + Send + 'static>(
    handler: SharedHandler<R>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let mut terminal = setup_terminal().context("setting up terminal")?;
    let res = event_loop(&mut terminal, handler, &shutdown);
    if let Err(e) = restore_terminal(&mut terminal) {
        tracing::error!("failed to restore terminal: {e}");
    }
    res
}

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

fn setup_terminal() -> Result<Tui> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("enabling raw mode")?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("entering alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("creating terminal")
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode().context("disabling raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
    )
    .context("leaving alternate screen")?;
    terminal.show_cursor().context("showing cursor")?;
    Ok(())
}

fn event_loop<R: Registry>(
    terminal: &mut Tui,
    handler: SharedHandler<R>,
    shutdown: &Arc<AtomicBool>,
) -> Result<()> {
    let mut app = App::new();
    let mut last_draw = Instant::now() - FRAME_INTERVAL;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let now = Instant::now();
        if now.duration_since(last_draw) >= FRAME_INTERVAL {
            // Briefly lock for the snapshot AND draw: the renderer needs the
            // registry to fetch best bid/ask for visible rows only. Holding
            // the lock for the duration of the draw is fine — it's bounded by
            // the small number of visible rows and the depth ladder.
            let h = handler.lock().expect("handler mutex poisoned");
            app.refresh(&*h);
            let registry = h.registry();
            terminal.draw(|f| ui::draw(f, &mut app, registry))?;
            drop(h);
            last_draw = now;
        }

        // Wait for the next event up to the next frame deadline. `poll`
        // returns immediately if an event is ready.
        let timeout = FRAME_INTERVAL
            .checked_sub(Instant::now().duration_since(last_draw))
            .unwrap_or(Duration::ZERO);
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && app.handle_key(key)
        {
            // User asked to quit the TUI; raise shutdown so the rest
            // of the pipeline winds down too.
            shutdown.store(true, Ordering::Relaxed);
            break;
        }
    }
    Ok(())
}
