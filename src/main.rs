//! src/main.rs
//! ============================================================================
//! # Minimal async TUI entry for power-user file manager (ratatui v0.24+)
//! - Launches async event loop and renders UI via View orchestrator

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fsm::cache::cache_manager::ObjectInfoCache;
use fsm::config::config::Config;
use fsm::controller::actions::Action;
use fsm::controller::event_loop::{Controller, TaskResult};
use fsm::model::app_state::AppState;
use fsm::model::fs_state::FSState;
use fsm::model::ui_state::UIState;
use fsm::view::ui::View;
use fsm::Logger;
use ratatui::{backend::CrosstermBackend as Backend, Terminal};
use std::io::{Error, Stdout};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, MutexGuard, Notify};
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
    info!("Terminal setup complete.");

    // --- App and controller setup ---
    let config = Arc::new(Config::load().await.unwrap_or_default());
    let cache = Arc::new(ObjectInfoCache::default());
    let fs = FSState::default();
    let ui = UIState::default();
    let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();
    let app = Arc::new(Mutex::new(AppState::new(
        config,
        cache,
        fs,
        ui,
        task_tx,
        action_tx.clone(),
    )));
    let mut controller = Controller::new(app.clone(), task_rx, action_rx);
    info!("App and controller setup complete.");

    // Initial directory scan
    let current_dir = tokio::fs::canonicalize(".").await?;
    app.lock().await.enter_directory(current_dir).await;

    // --- Set redraw = true so UI draws on first iteration ---
    {
        let mut app: MutexGuard<'_, AppState> = app.lock().await;
        app.redraw = true;
    }

    // --- Ctrl+C graceful shutdown ---
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_clone.notify_one();
        info!("Ctrl+C signal received.");
    });

    info!("Starting main event loop.");
    'main_loop: loop {
        // 1. Redraw only if needed
        {
            let mut app_lock = app.lock().await;
            if app_lock.redraw {
                terminal.draw(|frame| {
                    View::redraw(frame, &app_lock);
                })?;
                app_lock.redraw = false;
            }
        }

        // 2. Wait for next action or shutdown
        let action = tokio::select! {
            _ = shutdown.notified() => {
                info!("Shutdown signal received, breaking main loop.");
                break 'main_loop;
            },
            maybe_action = controller.next_action() => {
                match maybe_action {
                    Some(action) => action,
                    None => {
                        info!("Action stream finished, exiting.");
                        break 'main_loop;
                    }
                }
            }
        };

        // 3. Dispatch action
        if let Action::Quit = action {
            info!("Quit action received, breaking main loop.");
            break 'main_loop;
        }
        controller.dispatch_action(action).await;
    }

    // --- Terminal restore ---
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    info!("App exited cleanly.");
    Ok(())
}