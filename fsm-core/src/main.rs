//! ``src/main.rs``
//! ============================================================================
//! # File Manager TUI Application Entry Point
//!
//! A modern, async terminal user interface file manager built with ratatui and tokio.
//! Features include directory navigation, file operations, search functionality,
//! and a command palette for power users.
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cargo_common_metadata)]
#![allow(clippy::wildcard_dependencies)]
#![allow(clippy::multiple_crate_versions)]

use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    result::Result as StdResult,
    sync::Arc,
    time::{Duration, Instant},
};

use fsm_core::logging::{ProfilingData, init_logging_with_level};

use anyhow::{Context, Error, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{
    signal,
    sync::{Notify, mpsc},
    task::JoinHandle,
};

use fsm_core::{
    config::Config,
    controller::{
        actions::Action,
        event_loop::{EventLoop, TaskResult},
    },
    logging::shutdown_logging,
    model::metadata_manager::MetadataManager,
    model::{
        fs_state::FSState,
        shared_state::SharedState,
        ui_state::{Component, UIState},
    },
    printer::finalize_logs,
    view::ui::View,
};
use tracing::{self as Tracer, instrument};
use tracing_appender::non_blocking::WorkerGuard;

type AppTerminal = Terminal<Backend<Stdout>>;

const MIN_FRAME_TIME: Duration = Duration::from_millis(16); // 60 FPS max
const FORCE_RENDER_INTERVAL: Duration = Duration::from_millis(100); // Force refresh every 100ms

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    setup_panic_handler();

    let app: App = App::new()
        .await
        .context("Failed to initialize application")?;

    app.run().await.context("Application runtime error")?;

    println!("Application exited cleanly");

    let _ = finalize_logs();

    Ok(())
}

/// Application runtime configuration and state
struct App {
    terminal: AppTerminal,
    controller: EventLoop,
    state: Arc<SharedState>,
    shutdown: Arc<Notify>,
    last_memory_check: Instant,
    _tracer_guard: WorkerGuard,
}

impl App {
    /// Initialize the application with all necessary components
    async fn new() -> Result<Self> {
        let tracer_guard: WorkerGuard = init_logging_with_level("TRACE").await?;

        Tracer::info!("Starting File Manager TUI");

        let terminal: AppTerminal = setup_terminal().context("Failed to initialize terminal")?;

        // Concurrently load configuration and determine the current directory to improve startup time.
        let config_handle: JoinHandle<StdResult<Config, Error>> = tokio::spawn(Config::load());
        let dir_handle: JoinHandle<StdResult<PathBuf, io::Error>> =
            tokio::spawn(tokio::fs::canonicalize("."));

        let config: Arc<Config> = Arc::new(config_handle.await?.unwrap_or_else(|e| -> Config {
            Tracer::info!("Failed to load config, using defaults: {e}");
            Config::default()
        }));

        // UNIFIED METADATA MANAGER: Single source of truth for all metadata
        let metadata_manager: Arc<MetadataManager> = Arc::new(MetadataManager::new());

        let fs_state: FSState = FSState::default();
        let ui_state: UIState = UIState::default();

        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();

        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        // SHARED STATE: Fine-grained locking architecture
        let shared_state = Arc::new(SharedState::new(
            config,
            metadata_manager,
            ui_state,
            fs_state,
            task_tx.clone(),
            action_tx,
        ));

        let controller: EventLoop = EventLoop::new(shared_state.clone(), task_rx, action_rx);
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        let current_dir: PathBuf = dir_handle
            .await?
            .context("Failed to get current directory")?;

        // Initialize current directory in FS state
        {
            let mut fs_guard = shared_state.lock_fs();
            fs_guard.active_pane_mut().cwd = current_dir.clone();
        }

        // Mark UI for initial render
        {
            let mut ui_guard = shared_state.lock_ui();
            ui_guard.mark_dirty(Component::All);
        }

        // Trigger initial directory scan
        let shared_state_clone = shared_state.clone();
        tokio::spawn(async move {
            if let Err(e) = shared_state_clone
                .scan_directory_and_update_entries(&current_dir)
                .await
            {
                tracing::error!("Failed to scan initial directory: {}", e);
            }
        });

        Tracer::info!("Application initialization complete");

        Ok(Self {
            terminal,
            controller,
            state: shared_state,
            shutdown,
            last_memory_check: Instant::now(),
            _tracer_guard: tracer_guard,
        })
    }

    #[allow(clippy::future_not_send)]
    async fn run(mut self) -> Result<()> {
        self.setup_shutdown_handler().await;

        tracing::info!(
            marker = "APP_START",
            operation_type = "application",
            "Starting main event loop"
        );

        // Smart rendering with frame pacing for UI responsiveness
        let mut last_render: Instant = Instant::now();

        loop {
            self.check_memory_usage();

            // Smart redraw: only render when needed or forced refresh
            let now: Instant = Instant::now();
            let should_render: bool = {
                // Try to lock UI state non-blocking - concurrent architecture allows this
                self.state.try_lock_ui().is_some_and(|ui_guard| {
                    ui_guard.is_dirty()
                        || (now.duration_since(last_render) > FORCE_RENDER_INTERVAL)
                        || (now.duration_since(last_render) > MIN_FRAME_TIME
                            && ui_guard.has_pending_changes())
                })
            };

            if should_render {
                self.render().await?;
                last_render = now;
            }

            let action: Action = tokio::select! {
                () = self.shutdown.notified() => {
                    tracing::info!(
                        marker = "APP_EXIT_CLEAN",
                        operation_type = "application",
                        "Shutdown signal received"
                    );

                    break;
                }

                maybe_action = self.controller.next_action() => {
                    if let Some(action) = maybe_action { action } else {
                        tracing::info!(
                            marker = "APP_EXIT_CLEAN",
                            operation_type = "application",
                            "Controller stream ended"
                        );

                        break;
                    }
                }
            };

            if matches!(action, Action::Quit) {
                tracing::info!(
                    marker = "QUIT_ACTION_PROCESSED",
                    operation_type = "application",
                    "Quit action received"
                );

                break;
            }

            tracing::debug!(
                marker = "ACTION_DISPATCH_START",
                operation_type = "action_handling",
                action = ?action,
                "Dispatching action to controller"
            );

            self.controller.dispatch_action(action).await;
        }

        tracing::info!(
            marker = "APP_EXIT_CLEAN",
            operation_type = "application",
            "Main event loop ended"
        );

        self.shutdown().await?;

        Ok(())
    }

    // Optimized non-blocking render function with fine-grained locking
    async fn render(&mut self) -> Result<()> {
        // Check if UI needs rendering without blocking
        let needs_render = {
            if let Some(ui_guard) = self.state.try_lock_ui() {
                ui_guard.is_dirty()
            } else {
                // UI state is busy - yield and skip this frame
                tokio::task::yield_now().await;
                return Ok(());
            }
        };

        if needs_render {
            let start: Instant = Instant::now();

            // Render using fine-grained locking - View::redraw handles its own locking
            self.terminal
                .draw(|frame: &mut Frame<'_>| {
                    View::redraw(frame, &self.state);
                })
                .context("Failed to draw terminal")?;

            // Clear dirty flag after successful render
            if let Some(mut ui_guard) = self.state.try_lock_ui() {
                ui_guard.clear_dirty();
            }

            let duration: Duration = start.elapsed();

            #[allow(clippy::cast_possible_truncation)]
            let duration_us: u64 = duration.as_micros() as u64;

            // Collect profiling data for render operations
            let profiling_data: ProfilingData =
                ProfilingData::collect_profiling_data(None, duration);

            if duration.as_millis() > 16 {
                tracing::warn!(
                    marker = "PERF_FRAME_RENDER",
                    operation_type = "render",
                    duration_us = duration_us,
                    duration_ns = profiling_data.operation_duration_ns.unwrap_or(0),
                    "Slow render detected: {}ms (target: <16ms for 60fps)",
                    duration.as_millis()
                );
            } else {
                tracing::debug!(
                    marker = "UI_RENDER_COMPLETE",
                    operation_type = "render",
                    duration_us = duration_us,
                    duration_ns = profiling_data.operation_duration_ns.unwrap_or(0),
                    "Render completed in {}ms",
                    duration.as_millis()
                );
            }
        }

        Ok(())
    }

    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_possible_truncation)]
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
                        Tracer::warn!(
                            "Critical memory usage: Only {}MB available ({}% used)",
                            available_mb,
                            used_percent as u32
                        );
                    } else if available_mb < 500 {
                        // Less than 500MB available
                        Tracer::info!(
                            "High memory usage: {}MB available ({}% used)",
                            available_mb,
                            used_percent as u32
                        );
                    } else if used_percent > 80.0 {
                        Tracer::debug!(
                            "Memory usage: {}MB available ({}% used)",
                            available_mb,
                            used_percent as u32
                        );
                    }
                }

                Err(e) => {
                    Tracer::debug!("Failed to get memory info: {}", e);
                }
            }
        }
    }

    #[allow(clippy::unused_async)]
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
                       Tracer::info!("Received SIGTERM signal");
                    }

                    _ = sigint.recv() => {
                       Tracer::info!("Received SIGINT signal");
                    }

                    _ = signal::ctrl_c() => {
                       Tracer::info!("Received Ctrl+C signal");
                    }
                }
            }

            #[cfg(not(unix))]
            {
                if let Err(e) = signal::ctrl_c().await {
                    warn!("Failed to listen for Ctrl+C: {}", e);
                    return;
                }
                Tracer::info!("Received Ctrl+C signal");
            }

            shutdown.notify_one();
        });
    }

    pub async fn shutdown(mut self) -> Result<()> {
        Tracer::info!("Application shutting down gracefully");

        // Shutdown logging system first (this is async-safe)
        shutdown_logging().await?;

        // Then cleanup terminal
        cleanup_terminal(&mut self.terminal)?;

        Ok(())
    }
}

impl Drop for App {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            eprintln!("Failed to cleanup terminal: {e}");
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

    Tracer::info!("Terminal setup complete");
    Ok(terminal)
}

/// Restore terminal to normal mode
fn cleanup_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;

    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;

    terminal.show_cursor().context("Failed to show cursor")?;

    Tracer::info!("Terminal cleanup complete");
    Ok(())
}

/// Setup panic handler for graceful terminal restoration
fn setup_panic_handler() {
    use std::panic as StdPanicker;

    let original_hook: Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static> =
        StdPanicker::take_hook();

    StdPanicker::set_hook(Box::new(move |panic_info: &PanicHookInfo<'_>| {
        // Try to restore terminal on panic
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        eprintln!("Application panicked: {panic_info}");

        original_hook(panic_info);
    }));
}
