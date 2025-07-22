//! src/main.rs
//! ============================================================================
//! # Minimal async TUI entry for power-user file manager (ratatui v0.24+)
//! - Launches async event loop and renders UI via View orchestrator

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use fsm::config::config::Config;
use fsm::controller::actions::Action;
use fsm::controller::event_loop::{Controller, TaskResult};
use fsm::model::app_state::AppState;
use fsm::model::fs_state::FSState;
use fsm::model::ui_state::UIState;
use fsm::view::ui::View;
use fsm::{Logger, cache::cache_manager::ObjectInfoCache};
use futures::FutureExt;
use ratatui::{Terminal, backend::CrosstermBackend as Backend};
use std::{io::Error, time::Duration};
use std::{io::Stdout, sync::Arc};
use tokio::sync::{Mutex, MutexGuard, Notify, mpsc};
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --- Early: Set up logging/tracing ---
    Logger::init_tracing();
    info!("App start.");

    // --- Terminal and raw mode setup ---
    enable_raw_mode().map_err(|e: Error| {
        error!("Failed to enable raw mode: {}", e);
        e
    })?;
    let mut stdout: Stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend: Backend<Stdout> = Backend::new(stdout);
    let mut terminal: Terminal<Backend<Stdout>> = Terminal::new(backend)?;

    // --- App and controller setup ---
    let config: Arc<Config> = Arc::new(Config::default());
    let cache: Arc<ObjectInfoCache> = Arc::new(ObjectInfoCache::default());
    let fs: FSState = FSState::default();
    let ui: UIState = UIState::default();
    let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
    let app: Arc<Mutex<AppState>> =
        Arc::new(Mutex::new(AppState::new(config, cache, fs, ui, task_tx)));
    let mut controller = Controller::new(app.clone(), task_rx);

    // --- Set redraw = true so UI draws on first iteration ---
    {
        let mut app: MutexGuard<'_, AppState> = app.lock().await;
        app.redraw = true;
    }

    // --- Ctrl+C graceful shutdown ---
    let shutdown: Arc<Notify> = Arc::new(Notify::new());
    let shutdown_clone: Arc<Notify> = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_clone.notify_one();
    });

    // --- Main event/UI loop ---
    loop {
        // 1. Redraw only if needed (immediate mode)
        { // Lock app state for drawing
            let mut app: MutexGuard<'_, AppState> = app.lock().await;
            if app.redraw {
                terminal.draw(|frame| {
                    View::redraw(frame, &app);
                })?;
                app.redraw = false; // Clear after draw!
            }
        }

        // 2. Poll events/tasks and dispatch actions
        if let Some(action) = controller.poll_events().await {
            if action == Action::Quit {
                info!("Quit action received.");
                break;
            }
            controller.dispatch_action(action).await;
        }

        // 3. Graceful shutdown if Ctrl+C pressed
        if shutdown.notified().now_or_never().is_some() {
            info!("Shutdown signal received.");
            break;
        }

        // 4. Small sleep to reduce busy loop (adjust as needed)
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // --- Terminal restore (even on panic) ---
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    info!("App exited cleanly.");
    Ok(())
}
