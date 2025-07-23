//! src/main.rs
//! ============================================================================
//! # File Manager TUI Application Entry Point
//!
//! A modern, async terminal user interface file manager built with ratatui and tokio.
//! Features include directory navigation, file operations, search functionality,
//! and a command palette for power users.

use std::{
    io::{self, Stdout},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{
    signal,
    sync::{Mutex, MutexGuard, Notify, mpsc},
};
use tracing::{error, info, warn};

use fsm::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::config::Config,
    controller::{
        actions::Action,
        event_loop::{Controller, TaskResult},
    },
    model::{app_state::AppState, fs_state::FSState, ui_state::UIState},
    view::ui::View,
};

type AppTerminal = Terminal<Backend<Stdout>>;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup panic handler early
    setup_panic_handler();

    // Initialize and run the application
    let app: App = App::new()
        .await
        .context("Failed to initialize application")?;

    app.run().await.context("Application runtime error")?;

    info!("Application exited cleanly");
    Ok(())
}

/// Application runtime configuration and state
struct App {
    terminal: AppTerminal,
    controller: Controller,
    state: Arc<Mutex<AppState>>,
    shutdown: Arc<Notify>,
}

impl App {
    /// Initialize the application with all necessary components
    async fn new() -> Result<Self> {
        // Initialize logging first
        Logger::init_tracing();
        info!("Starting File Manager TUI");

        // Setup terminal
        let terminal: AppTerminal = setup_terminal().context("Failed to initialize terminal")?;

        // Load configuration
        let config: Arc<Config> = Arc::new(Config::load().await.unwrap_or_else(|e| {
            warn!("Failed to load config, using defaults: {}", e);
            Config::default()
        }));

        // Initialize core components
        let cache: Arc<ObjectInfoCache> = Arc::new(ObjectInfoCache::default());
        let fs_state: FSState = FSState::default();
        let ui_state: UIState = UIState::default();

        // Create communication channels
        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        // Create application state
        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(
            config, cache, fs_state, ui_state, task_tx, action_tx,
        )));

        // Initialize controller
        let controller: Controller = Controller::new(app_state.clone(), task_rx, action_rx);

        // Setup shutdown notification
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        // Perform initial directory scan
        let current_dir: PathBuf = tokio::fs::canonicalize(".")
            .await
            .context("Failed to get current directory")?;

        {
            let mut state: MutexGuard<'_, AppState> = app_state.lock().await;
            state.enter_directory(current_dir).await;
            state.redraw = true;
        }

        info!("Application initialization complete");

        Ok(Self {
            terminal,
            controller,
            state: app_state,
            shutdown,
        })
    }

    /// Run the main application event loop
    async fn run(mut self) -> Result<()> {
        // Setup graceful shutdown handler
        self.setup_shutdown_handler().await;

        info!("Starting main event loop");

        // Main event loop
        loop {
            // Render UI if needed
            self.render().await?;

            // Wait for next event
            let action: Action = tokio::select! {
                _ = self.shutdown.notified() => {
                    info!("Shutdown signal received");
                    break;
                }

                maybe_action = self.controller.next_action() => {
                    match maybe_action {
                        Some(action) => action,
                        None => {
                            info!("Controller stream ended");
                            break;
                        }
                    }
                }
            };

            // Handle quit action
            if matches!(action, Action::Quit) {
                info!("Quit action received");
                break;
            }

            // Dispatch action to controller
            self.controller.dispatch_action(action).await;
        }

        info!("Main event loop ended");
        Ok(())
    }

    /// Render the UI if a redraw is needed
    async fn render(&mut self) -> Result<()> {
        let mut state: MutexGuard<'_, AppState> = self.state.lock().await;

        if state.redraw {
            self.terminal
                .draw(|frame: &mut Frame<'_>| {
                    View::redraw(frame, &mut state);
                })
                .context("Failed to draw terminal")?;

            state.redraw = false;
        }

        Ok(())
    }

    /// Setup signal handlers for graceful shutdown
    async fn setup_shutdown_handler(&self) {
        let shutdown: Arc<Notify> = self.shutdown.clone();

        tokio::spawn(async move {
            match signal::ctrl_c().await {
                Ok(()) => {
                    info!("Received Ctrl+C signal");
                    shutdown.notify_one();
                }
                Err(e) => {
                    error!("Failed to listen for Ctrl+C: {}", e);
                }
            }
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            error!("Failed to cleanup terminal: {}", e);
        }
    }
}

/// Initialize terminal in raw mode with alternate screen
fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode().context("Failed to enable raw mode")?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;

    let backend: Backend<Stdout> = Backend::new(stdout);
    let terminal: Terminal<Backend<Stdout>> =
        Terminal::new(backend).context("Failed to create terminal")?;

    info!("Terminal setup complete");
    Ok(terminal)
}

/// Restore terminal to normal mode
fn cleanup_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;

    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;

    terminal.show_cursor().context("Failed to show cursor")?;

    info!("Terminal cleanup complete");
    Ok(())
}

/// Setup panic handler for graceful terminal restoration
fn setup_panic_handler() {
    let original_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // Try to restore terminal on panic
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        error!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
