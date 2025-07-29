//! FSM-Core main.rs - Entry point with FileSystemOperator integration

use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{signal, sync::mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Level, debug, error, info, instrument, warn};

use fsm_core::{
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        event_loop::{EventLoop, TaskResult},
        state_coordinator::StateCoordinator,
    },
    error::{AppError, AppResult, TracedResult},
    logging::{Logger, LoggingConfig},
    measure_time,
    model::{
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
    operators::file_system_operator::{ScanMode, spawn_directory_scan},
    trace_fn, trace_operation,
    view::ui::UIRenderer,
};

type AppTerminal = Terminal<Backend<Stdout>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
#[instrument(name = "main")]
async fn main() -> Result<()> {
    let startup_start: Instant = Instant::now();
    setup_panic_handler();

    let config: LoggingConfig = LoggingConfig {
        console_level: Level::TRACE,
        ..Default::default()
    };

    let _logger: Logger = Logger::init_with_config(config)?;

    info!("Starting FSM-Core with integrated FileSystemOperator");

    let app: App = measure_time!("app_initialization", {
        App::new().await.trace_err("app_creation")?
    });

    let startup_time: Duration = startup_start.elapsed();
    info!(
        startup_time_ms = startup_time.as_millis(),
        "FSM-Core startup completed"
    );

    app.run().await.trace_err("app_runtime")?;

    info!("FSM-Core exited cleanly");
    Ok(())
}

struct App {
    terminal: AppTerminal,
    event_loop: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    ui_renderer: UIRenderer,
    cancel_token: CancellationToken,
}

impl App {
    #[instrument(name = "app_new", skip_all)]
    async fn new() -> AppResult<Self> {
        let _span = trace_operation!("app_initialization");

        // Create cancellation token for graceful shutdown
        let cancel_token: CancellationToken = CancellationToken::new();

        // Terminal setup
        let terminal = measure_time!("terminal_setup", {
            setup_terminal().trace_err("terminal_setup")?
        });

        // Config loading
        let config: Arc<Config> = {
            let _span = trace_operation!("config_loading");
            match Config::load().await {
                Ok(config) => {
                    info!("Configuration loaded successfully");
                    Arc::new(config)
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load config, using defaults");
                    Arc::new(Config::default())
                }
            }
        };

        // Cache initialization
        let cache = {
            let _span = trace_operation!("cache_initialization");
            let cache = Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
            info!(
                cache_capacity = config.cache.max_capacity,
                "Object cache initialized"
            );
            cache
        };

        // Directory resolution
        let current_dir = {
            let _span = trace_operation!("directory_resolution");
            tokio::fs::canonicalize(".")
                .await
                .map_err(|e| AppError::Other(format!("Failed to resolve current directory: {}", e)))
                .context("Failed to resolve current directory")?
        };

        info!(directory = %current_dir.display(), "Working directory resolved");

        // State initialization
        let (app_state, fs_state, ui_state) = {
            let _span = trace_operation!("state_initialization");

            let app_state = Arc::new(Mutex::new(AppState::new(config.clone(), cache.clone())));
            let fs_state = Arc::new(Mutex::new(FSState::new(current_dir.clone())));
            let ui_state = Arc::new(RwLock::new(UIState::default()));

            info!("Application states initialized");
            (app_state, fs_state, ui_state)
        };

        // Coordinator and event loop setup with FileSystemOperator integration
        let state_coordinator = {
            let _span = trace_operation!("coordinator_setup");
            Arc::new(StateCoordinator::new(app_state, ui_state, fs_state))
        };

        let (event_loop, task_tx) = {
            let _span = trace_operation!("event_loop_creation");
            EventLoop::new(state_coordinator.clone())
        };

        // Initial directory loading using FileSystemOperator
        Self::load_initial_directory(
            &state_coordinator,
            current_dir,
            task_tx,
            cancel_token.clone(),
        )
        .await
        .trace_err("initial_directory_load")?;

        let ui_renderer = UIRenderer::new();

        info!("Application initialization with FileSystemOperator completed successfully");

        Ok(Self {
            terminal,
            event_loop,
            state_coordinator,
            ui_renderer,
            cancel_token,
        })
    }

    #[instrument(
        name = "load_initial_directory",
        skip(coordinator, task_tx, cancel_token)
    )]
    async fn load_initial_directory(
        coordinator: &StateCoordinator,
        dir: PathBuf,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
    ) -> AppResult<()> {
        let _span = trace_operation!("initial_directory_scan", path = %dir.display());

        info!(
            directory = %dir.display(),
            "Starting initial directory scan with FileSystemOperator"
        );

        // Use FileSystemOperator for initial directory scan
        let operation_id = spawn_directory_scan(
            dir.clone(),
            false,              // Don't show hidden files initially
            ScanMode::TwoPhase, // Fast display + background metadata
            task_tx,
            cancel_token,
        );

        coordinator.request_redraw(RedrawFlag::All);

        info!(
            operation_id = %operation_id,
            directory = %dir.display(),
            "Initial directory scan started successfully"
        );
        Ok(())
    }

    #[instrument(name = "app_run", skip(self))]
    async fn run(mut self) -> AppResult<()> {
        let run_start = Instant::now();

        info!("Starting main application loop with FileSystemOperator");

        // Setup graceful shutdown
        self.setup_shutdown().await;

        // Main event loop
        let loop_result = self.run_main_loop().await;

        let total_runtime = run_start.elapsed();

        match loop_result {
            Ok(_) => {
                info!(
                    runtime_secs = total_runtime.as_secs(),
                    "Application completed successfully"
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    runtime_secs = total_runtime.as_secs(),
                    error = %e,
                    "Application terminated with error"
                );
                Err(e)
            }
        }
    }

    #[instrument(name = "main_loop", skip(self))]
    async fn run_main_loop(&mut self) -> AppResult<()> {
        let mut frame_count = 0u64;
        let mut render_interval = tokio::time::interval(tokio::time::Duration::from_millis(16));

        loop {
            tokio::select! {
                // Event loop processing with FileSystemOperator integration
                result = self.event_loop.run() => {
                    match result {
                        Ok(_) => {
                            info!("Event loop completed normally");
                            break;
                        }
                        Err(e) => {
                            error!(error = %e, "Event loop error");
                            return Err(AppError::Other(format!("Event loop failed: {}", e)));
                        }
                    }
                }

                // Render cycle
                _ = render_interval.tick() => {
                    if let Err(e) = self.render_frame(frame_count).trace_err("frame_render") {
                        warn!(
                            frame = frame_count,
                            error = %e,
                            "Frame render failed"
                        );
                    }
                    frame_count += 1;
                }

                // Handle cancellation
                _ = self.cancel_token.cancelled() => {
                    info!("Received cancellation signal, shutting down gracefully");
                    break;
                }
            }
        }

        Ok(())
    }

    #[instrument(name = "render_frame", skip(self), fields(frame = frame_count))]
    fn render_frame(&mut self, frame_count: u64) -> AppResult<()> {
        if !self.state_coordinator.needs_redraw() {
            return Ok(());
        }

        let render_start = Instant::now();

        let render_result = self.terminal.draw(|frame: &mut Frame| {
            let _span = tracing::info_span!("ui_render", frame = frame_count).entered();
            self.ui_renderer.render(frame, &self.state_coordinator);
        });

        let render_duration = render_start.elapsed();

        match render_result {
            Ok(_) => {
                self.state_coordinator.clear_redraw();

                if frame_count % 60 == 0 {
                    // Log every second at 60fps
                    tracing::debug!(
                        frame = frame_count,
                        render_time_us = render_duration.as_micros(),
                        "Frame rendered"
                    );
                }
                Ok(())
            }
            Err(e) => {
                let error = AppError::Render {
                    component: "main_ui".to_string(),
                    reason: e.to_string(),
                };

                error!(
                    frame = frame_count,
                    render_time_us = render_duration.as_micros(),
                    error = %error,
                    "Frame render failed"
                );

                Err(error)
            }
        }
    }

    #[instrument(name = "setup_shutdown", skip(self))]
    async fn setup_shutdown(&self) {
        let shutdown = self.event_loop.shutdown_handle();
        let cancel_token = self.cancel_token.clone();

        tokio::spawn(
            async move {
                let ctrl_c = async {
                    signal::ctrl_c()
                        .await
                        .expect("Failed to install Ctrl+C handler");
                };

                #[cfg(unix)]
                let terminate = async {
                    signal::unix::signal(signal::unix::SignalKind::terminate())
                        .expect("Failed to install signal handler")
                        .recv()
                        .await;
                };

                #[cfg(not(unix))]
                let terminate = std::future::pending::<()>();

                tokio::select! {
                    _ = ctrl_c => {
                        info!("Received Ctrl+C signal");
                    }
                    _ = terminate => {
                        info!("Received terminate signal");
                    }
                }

                info!("Initiating graceful shutdown");
                cancel_token.cancel();
                shutdown.notify_one();
            }
            .instrument(tracing::info_span!("shutdown_handler")),
        );
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _span = trace_fn!("app_cleanup");

        // Cancel all ongoing operations
        self.cancel_token.cancel();

        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            warn!(error = %e, "Terminal cleanup failed");
        } else {
            debug!("Terminal cleanup completed successfully");
        }

        // Flush logs to ensure all file system operations are recorded
        Logger::flush();
    }
}

#[instrument(name = "setup_terminal")]
fn setup_terminal() -> AppResult<AppTerminal> {
    let _span = trace_operation!("terminal_initialization");

    enable_raw_mode()
        .map_err(|e| AppError::Terminal(format!("Failed to enable raw mode: {}", e)))?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| AppError::Terminal(format!("Failed to enter alternate screen: {}", e)))?;

    let backend = Backend::new(stdout);
    let terminal = Terminal::new(backend)
        .map_err(|e| AppError::Terminal(format!("Failed to create terminal: {}", e)))?;

    info!("Terminal setup completed successfully");
    Ok(terminal)
}

#[instrument(name = "cleanup_terminal")]
fn cleanup_terminal(terminal: &mut AppTerminal) -> AppResult<()> {
    disable_raw_mode()
        .map_err(|e| AppError::Terminal(format!("Failed to disable raw mode: {}", e)))?;

    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| AppError::Terminal(format!("Failed to leave alternate screen: {}", e)))?;

    terminal
        .show_cursor()
        .map_err(|e| AppError::Terminal(format!("Failed to show cursor: {}", e)))?;

    info!("Terminal cleanup completed");
    Ok(())
}

fn setup_panic_handler() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo| {
        // Ensure terminal is cleaned up before panic
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        // Log panic with tracing
        error!(
            panic_info = %info,
            location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            "PANIC: Application panicked"
        );

        // Flush logs including file system operation logs
        Logger::flush();

        original(info);
    }));
}
