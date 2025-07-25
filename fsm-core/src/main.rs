//! src/main.rs
//! ============================================================================
//! # File Manager TUI Application Entry Point
//!
//! A modern, async terminal user interface file manager built with ratatui and tokio.
//! Features include directory navigation, file operations, search functionality,
//! and a command palette for power users.

use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::Arc,
    time::Instant,
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
use tracing::{info, warn};

use fsm_core::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        actions::Action,
        event_loop::{EventLoop, TaskResult},
    },
    model::{
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
    view::ui::View,
};

type AppTerminal = Terminal<Backend<Stdout>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
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
    controller: EventLoop,
    state: Arc<Mutex<AppState>>,
    shutdown: Arc<Notify>,
    last_memory_check: Instant,
}

impl App {
    /// Initialize the application with all necessary components
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting File Manager TUI");

        let terminal: AppTerminal = setup_terminal().context("Failed to initialize terminal")?;

        // Concurrently load configuration and determine the current directory to improve startup time.
        let config_handle = tokio::spawn(Config::load());
        let dir_handle = tokio::spawn(tokio::fs::canonicalize("."));

        let config: Arc<Config> = Arc::new(config_handle.await?.unwrap_or_else(|e| {
            info!("Failed to load config, using defaults: {}", e);
            Config::default()
        }));

        let cache: Arc<ObjectInfoCache> =
            Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
        let fs_state: FSState = FSState::default();
        let ui_state: UIState = UIState::default();

        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(
            config,
            cache,
            fs_state,
            ui_state,
            task_tx.clone(),
            action_tx,
        )));

        let controller: EventLoop = EventLoop::new(app_state.clone(), task_rx, action_rx);
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        let current_dir: PathBuf = dir_handle
            .await?
            .context("Failed to get current directory")?;

        {
            let mut state: MutexGuard<'_, AppState> = app_state.lock().await;
            state.enter_directory(current_dir).await;
            state.ui.request_redraw(RedrawFlag::All); // Use UI state for redraw management
        }

        info!("Application initialization complete");

        Ok(Self {
            terminal,
            controller,
            state: app_state,
            shutdown,
            last_memory_check: Instant::now(),
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

            // Check memory usage periodically (every 5 seconds)
            self.check_memory_usage();

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

    /// Render the UI if a redraw is needed with performance monitoring
    async fn render(&mut self) -> Result<()> {
        let mut state: MutexGuard<'_, AppState> = self.state.lock().await;

        if state.ui.needs_redraw() {
            let start: Instant = Instant::now();

            self.terminal
                .draw(|frame: &mut Frame<'_>| {
                    View::redraw(frame, &mut state);
                })
                .context("Failed to draw terminal")?;

            state.ui.clear_redraw();

            // Monitor render performance - log slow renders that could impact UX
            let duration = start.elapsed();
            if duration.as_millis() > 16 {
                // > 16ms = < 60fps
                info!(
                    "Slow render detected: {}ms (target: <16ms for 60fps)",
                    duration.as_millis()
                );
            } else if duration.as_millis() > 8 {
                // Log renders that are getting close to the threshold
                tracing::debug!("Render time: {}ms", duration.as_millis());
            }
        }

        Ok(())
    }

    /// Check memory usage and log warnings if memory is getting low
    fn check_memory_usage(&mut self) {
        let now: Instant = Instant::now();

        // Check memory every 5 seconds to avoid performance impact
        if now.duration_since(self.last_memory_check).as_secs() >= 5 {
            self.last_memory_check = now;

            match sys_info::mem_info() {
                Ok(mem_info) => {
                    let available_mb: u64 = mem_info.avail / 1024; // Convert KB to MB
                    let total_mb: u64 = mem_info.total / 1024;
                    let used_percent: f64 =
                        ((total_mb - available_mb) as f64 / total_mb as f64) * 100.0;

                    // Log memory warnings based on available memory
                    if available_mb < 100 {
                        // Less than 100MB available
                        warn!(
                            "Critical memory usage: Only {}MB available ({}% used)",
                            available_mb, used_percent as u32
                        );
                    } else if available_mb < 500 {
                        // Less than 500MB available
                        info!(
                            "High memory usage: {}MB available ({}% used)",
                            available_mb, used_percent as u32
                        );
                    } else if used_percent > 80.0 {
                        tracing::debug!(
                            "Memory usage: {}MB available ({}% used)",
                            available_mb,
                            used_percent as u32
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to get memory info: {}", e);
                }
            }
        }
    }

    /// Setup signal handlers for graceful shutdown
    async fn setup_shutdown_handler(&self) {
        let shutdown: Arc<Notify> = self.shutdown.clone();

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};

                let mut sigterm =
                    signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");

                tokio::select! {
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM signal");
                    }
                    _ = sigint.recv() => {
                        info!("Received SIGINT signal");
                    }
                    _ = signal::ctrl_c() => {
                        info!("Received Ctrl+C signal");
                    }
                }
            }

            #[cfg(not(unix))]
            {
                if let Err(e) = signal::ctrl_c().await {
                    warn!("Failed to listen for Ctrl+C: {}", e);
                    return;
                }
                info!("Received Ctrl+C signal");
            }

            shutdown.notify_one();
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            warn!("Failed to cleanup terminal: {}", e);
        }
    }
}

/// Initialize terminal in raw mode with alternate screen
fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode().context("Failed to enable raw mode")?;

    let mut stdout: Stdout = io::stdout();
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
    let original_hook: Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static> =
        std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // Try to restore terminal on panic
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        warn!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
