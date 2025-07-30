//! FSM-Core main.rs - Entry point with FileSystemOperator integration

use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    result::Result as StdResult,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{signal, sync::mpsc, time::Interval};
use tokio::{sync::Notify, time as TokioTime};

use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, span::EnteredSpan, trace, warn};

use fsm_core::{
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        actions::{Action, RenderTrigger},
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
    operators::file_system_operator::FileSystemOperator,
    trace_operation,
    view::ui::UIRenderer,
};

type AppTerminal = Terminal<Backend<Stdout>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
#[instrument(
    level = "info",
    name = "main_application_entry",
    fields(
        marker = "APP_START",
        operation_type = "application_lifecycle",
        message = "FSM-Core application started"
    )
)]
async fn main() -> Result<()> {
    let startup_start: Instant = Instant::now();
    setup_panic_handler();

    let _logger: Logger = Logger::init_with_config(LoggingConfig::development())?;

    info!(
        marker = "APP_START",
        operation_type = "application_lifecycle",
        message = "Starting FSM-Core with integrated FileSystemOperator"
    );

    let app: App = measure_time!("app_initialization", {
        App::new().await.trace_err("app_creation")?
    });

    let startup_time: Duration = startup_start.elapsed();

    info!(
        marker = "APP_STARTUP_COMPLETE",
        operation_type = "application_lifecycle",
        duration_us = startup_time.as_micros(),
        message = "FSM-Core startup completed"
    );

    app.run().await.trace_err("app_runtime")?;

    info!(
        marker = "APP_EXIT_CLEAN",
        operation_type = "application_lifecycle",
        message = "FSM-Core exited cleanly"
    );

    Ok(())
}

struct App {
    terminal: AppTerminal,
    event_loop: Option<EventLoop>,
    state_coordinator: Arc<StateCoordinator>,
    ui_renderer: UIRenderer,
    cancel_token: CancellationToken,
    action_tx: mpsc::UnboundedSender<Action>,
}

impl App {
    #[instrument(
        level = "info",
        name = "app_new",
        skip_all,
        fields(
            marker = "APP_INITIALIZATION_START",
            operation_type = "application_lifecycle",
            message = "Application new instance creation started"
        )
    )]
    async fn new() -> AppResult<Self> {
        let _span: EnteredSpan = trace_operation!("app_initialization");

        // Create cancellation token for graceful shutdown
        let cancel_token: CancellationToken = CancellationToken::new();

        // Terminal setup
        let terminal: Terminal<Backend<Stdout>> = measure_time!("terminal_setup", {
            setup_terminal().trace_err("terminal_setup")?
        });

        // Config loading
        let config: Arc<Config> = {
            let _span: EnteredSpan = trace_operation!("config_loading");

            match Config::load().await {
                Ok(config) => {
                    info!(
                        marker = "CONFIG_LOAD_SUCCESS",
                        operation_type = "configuration_management",
                        message = "Configuration loaded successfully"
                    );

                    Arc::new(config)
                }

                Err(e) => {
                    warn!(
                        marker = "CONFIG_LOAD_FAILED",
                        operation_type = "configuration_management",
                        error = %e,
                        message = "Failed to load config, using defaults"
                    );

                    Arc::new(Config::default())
                }
            }
        };

        // Cache initialization
        let cache: Arc<ObjectInfoCache> = {
            let _span: EnteredSpan = trace_operation!("cache_initialization");

            let cache: Arc<ObjectInfoCache> =
                Arc::new(ObjectInfoCache::with_config(config.cache.clone()));

            info!(
                marker = "CACHE_INIT",
                operation_type = "cache_management",
                cache_capacity = config.cache.max_capacity,
                message = "Object cache initialized"
            );

            cache
        };

        // Directory resolution
        let current_dir: PathBuf = {
            let _span: EnteredSpan = trace_operation!("directory_resolution");

            tokio::fs::canonicalize(".")
                .await
                .map_err(|e| AppError::Other(format!("Failed to resolve current directory: {e}")))
                .context("Failed to resolve current directory")?
        };

        info!(
            marker = "DIRECTORY_RESOLVED",
            operation_type = "file_system",
            current_path = %current_dir.display(),
            message = "Working directory resolved"
        );

        // State initialization
        let (app_state, fs_state, ui_state) = {
            let _span: EnteredSpan = trace_operation!("state_initialization");

            let app_state: Arc<Mutex<AppState>> =
                Arc::new(Mutex::new(AppState::new(config.clone(), cache.clone())));
            let fs_state: Arc<Mutex<FSState>> =
                Arc::new(Mutex::new(FSState::new(current_dir.clone())));
            let ui_state: Arc<RwLock<UIState>> = Arc::new(RwLock::new(UIState::default()));

            info!(
                marker = "APP_STATE_DEFAULT_CREATED",
                operation_type = "state_management",
                message = "Application states initialized"
            );
            (app_state, fs_state, ui_state)
        };

        // Coordinator and event loop setup with FileSystemOperator integration
        let state_coordinator: Arc<StateCoordinator> = {
            let _span: EnteredSpan = trace_operation!("coordinator_setup");
            Arc::new(StateCoordinator::new(app_state, ui_state, fs_state))
        };

        // Create a channel for injecting actions into the event loop
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        let (event_loop, task_tx) = {
            let _span: EnteredSpan = trace_operation!("event_loop_creation");
            EventLoop::new(state_coordinator.clone(), action_rx)
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

        let ui_renderer: UIRenderer = UIRenderer::new();

        info!(
            marker = "APP_INITIALIZATION_COMPLETE",
            operation_type = "application_lifecycle",
            message = "Application initialization with FileSystemOperator completed successfully"
        );

        Ok(Self {
            terminal,
            event_loop: Some(event_loop),
            state_coordinator,
            ui_renderer,
            cancel_token,
            action_tx,
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
        let _span: EnteredSpan = trace_operation!("initial_directory_scan", path = %dir.display());

        info!(
            directory = %dir.display(),
            "Starting initial directory scan with FileSystemOperator"
        );

        // Use FileSystemOperator for initial directory scan
        let _scan_handle = FileSystemOperator::spawn_two_phase_directory_scan(
            0, // task_id
            dir.clone(),
            false, // Don't show hidden files initially
            task_tx,
            cancel_token,
        );

        coordinator.request_redraw(RedrawFlag::All);

        info!(
            directory = %dir.display(),
            "Initial directory scan started successfully"
        );
        Ok(())
    }

    #[instrument(
        level = "info",
        name = "app_run",
        skip(self),
        fields(
            marker = "APP_RUN_START",
            operation_type = "application_lifecycle",
            message = "Starting main application loop"
        )
    )]
    async fn run(mut self) -> AppResult<()> {
        let run_start = Instant::now();

        info!(
            marker = "APP_RUN_START",
            operation_type = "application_lifecycle",
            message = "Starting main application loop with FileSystemOperator"
        );

        // Shutdown is now handled in run_main_loop

        // Main event loop
        let loop_result: StdResult<(), AppError> = self.run_main_loop().await;

        let total_runtime: Duration = run_start.elapsed();

        match loop_result {
            Ok(_) => {
                info!(
                    marker = "APP_EXIT_CLEAN",
                    operation_type = "application_lifecycle",
                    duration_us = total_runtime.as_micros(),
                    message = "Application completed successfully"
                );

                Ok(())
            }

            Err(e) => {
                error!(
                    marker = "APPLICATION_PANIC",
                    operation_type = "application_lifecycle",
                    duration_us = total_runtime.as_micros(),
                    error = %e,
                    message = "Application terminated with error"
                );

                Err(e)
            }
        }
    }

    #[instrument(
        level = "info",
        name = "main_loop",
        skip(self),
        fields(
            marker = "EVENT_LOOP_STARTED",
            operation_type = "event_loop",
            message = "Main event loop started"
        )
    )]
    async fn run_main_loop(&mut self) -> AppResult<()> {
        let mut frame_count: u64 = 0u64;
        let mut render_interval: Interval =
            TokioTime::interval(TokioTime::Duration::from_millis(16));

        // Render notification channel
        let (render_notify_tx, mut render_notify_rx) = mpsc::unbounded_channel::<()>();

        // Share render notifier with StateCoordinator
        self.state_coordinator.set_render_notifier(render_notify_tx);

        // Get shutdown handle before moving event loop
        let shutdown_handle: Arc<Notify> = self
            .event_loop
            .as_ref()
            .expect("EventLoop should be available")
            .shutdown_handle();

        // Move event loop to background task to prevent restarts
        let mut event_loop: EventLoop = self
            .event_loop
            .take()
            .expect("EventLoop should be available");

        let mut event_loop_handle = tokio::spawn(async move { event_loop.run().await });

        // Setup signal handlers
        let mut ctrl_c = std::pin::pin!(async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        });

        #[cfg(unix)]
        let mut terminate = std::pin::pin!(async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        });

        #[cfg(not(unix))]
        let mut terminate = std::pin::pin!(std::future::pending::<()>());

        // Main render loop
        loop {
            tokio::select! {
                // 1. Check if event loop completed
                result = &mut event_loop_handle => {
                    match result {
                        Ok(Ok(_)) => {
                            info!(
                                marker = "EVENT_LOOP_COMPLETED",
                                operation_type = "event_loop",
                                message = "Event loop completed normally"
                            );

                            break;
                        }

                        Ok(Err(e)) => {
                            error!(
                                marker = "TERMINAL_EVENT_ERROR",
                                operation_type = "event_loop",
                                error = %e,
                                message = "Event loop error"
                            );

                            return Err(AppError::Other(format!("Event loop failed: {e}")));
                        }

                        Err(e) => {
                            error!(
                                marker = "TERMINAL_EVENT_ERROR",
                                operation_type = "event_loop",
                                error = %e,
                                message = "Event loop task failed"
                            );

                            return Err(AppError::Other(format!("Event loop task failed: {e}")));
                        }
                    }
                }

                // 2. Immediate render on redraw request (CLEAN ARCHITECTURE COMPLIANT)
                _ = render_notify_rx.recv() => {
                    let action = Action::TriggerImmediateRender {
                        trigger_source: RenderTrigger::MetadataUpdate,
                        frame_count,
                        timestamp: SystemTime::now(),
                    };
                    if let Err(e) = self.action_tx.send(action) {
                        warn!(
                            marker = "ACTION_DISPATCH_FAILED",
                            operation_type = "action_dispatch",
                            error = %e,
                            message = "Failed to send TriggerImmediateRender action"
                        );
                    }
                }

                // 3. Maximum 60_FPS interval render (fallback)
                _ = render_interval.tick() => {
                    // Only render if there's a pending redraw request
                    if self.state_coordinator.needs_redraw() {
                        if let Err(e) = self.render_frame(frame_count).trace_err("INTERVAL_FRAME_RENDER") {
                            warn!(
                                marker = "ERROR_RENDER",
                                operation_type = "ui_render",
                                frame = frame_count,
                                error = %e,
                                message = "Interval frame render failed, dispatching error action"
                            );

                            let error_action = Action::HandleRenderError {
                                error: e.to_string(),
                                frame_count,
                                error_source: "IntervalRender".to_string(),
                                recovery_action: None,
                                timestamp: SystemTime::now(),
                            };

                            if let Err(e) = self.action_tx.send(error_action) {
                                error!(
                                    marker = "ACTION_DISPATCH_FAILED",
                                    operation_type = "action_dispatch",
                                    error = %e,
                                    message = "Failed to send HandleRenderError action"
                                );
                            }
                        }
                        frame_count += 1;
                    };
                }

                // 4. Handle system signals
                _ = &mut ctrl_c => {
                    info!(
                        marker = "SHUTDOWN_SIGNAL_SENT",
                        operation_type = "application_lifecycle",
                        message = "Received Ctrl+C signal"
                    );

                    shutdown_handle.notify_one();

                    break;
                }
                _ = &mut terminate => {
                    info!(
                        marker = "SHUTDOWN_SIGNAL_SENT",
                        operation_type = "application_lifecycle",
                        message = "Received terminate signal"
                    );

                    shutdown_handle.notify_one();

                    break;
                }

                // 5. Handle cancellation token
                _ = self.cancel_token.cancelled() => {
                    info!(
                        marker = "SHUTDOWN_SIGNAL_SENT",
                        operation_type = "application_lifecycle",
                        message = "Received cancellation signal, shutting down gracefully"
                    );

                    shutdown_handle.notify_one();

                    break;
                }
            }
        }

        Ok(())
    }

    fn render_frame(&mut self, frame_count: u64) -> AppResult<()> {
        if !self.state_coordinator.needs_redraw() {
            trace!(
                marker = "UI_FRAME_SKIPPED",
                operation_type = "ui_render",
                frame = frame_count,
                message = "UI frame skipped, no redraw needed"
            );
            return Ok(());
        }
        self._render_frame_instrumented(frame_count)
    }

    #[instrument(
        level = "info",
        name = "render_frame_instrumented",
        skip(self),
        err,
        fields(
            marker = "UI_RENDER_START",
            operation_type = "ui_render",
            frame = frame_count,
            duration_us = tracing::field::Empty
        )
    )]
    fn _render_frame_instrumented(&mut self, frame_count: u64) -> AppResult<()> {
        let render_start: Instant = Instant::now();
        let span = tracing::Span::current();

        let render_result = self.terminal.draw(|frame: &mut Frame| {
            self.ui_renderer.render(frame, &self.state_coordinator);
        });

        let render_duration: Duration = render_start.elapsed();
        span.record("duration_us", render_duration.as_micros());

        match render_result {
            Ok(_) => {
                self.state_coordinator.clear_redraw();
                span.record("marker", "UI_RENDER_COMPLETE");
                info!("UI frame render completed");
                Ok(())
            }

            Err(e) => {
                let error: AppError = AppError::Render {
                    component: "main_ui".to_string(),
                    reason: e.to_string(),
                };
                span.record("marker", "ERROR_RENDER");
                error!("Frame render failed: {e}");
                Err(error)
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _span: EnteredSpan = tracing::info_span!(
            "app_cleanup",
            marker = "APP_EXIT_CLEAN",
            operation_type = "application_lifecycle",
            message = "Application cleanup initiated"
        )
        .entered();

        // Cancel all ongoing operations
        self.cancel_token.cancel();

        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            warn!(
                marker = "TERMINAL_CLEANUP_FAILED",
                operation_type = "terminal",
                error = %e,
                message = "Terminal cleanup failed"
            );
        } else {
            debug!(
                marker = "TERMINAL_CLEANUP_COMPLETE",
                operation_type = "terminal",
                message = "Terminal cleanup completed successfully"
            );
        }

        // Flush logs to ensure all file system operations are recorded
        Logger::flush();
    }
}

#[instrument(
    level = "info",
    name = "setup_terminal",
    fields(
        marker = "TERMINAL_SETUP_START",
        operation_type = "terminal",
        message = "Terminal setup initiated"
    )
)]
fn setup_terminal() -> AppResult<AppTerminal> {
    let _span: EnteredSpan = tracing::info_span!(
        "terminal_initialization_span",
        marker = "TERMINAL_SETUP_START",
        operation_type = "terminal",
        message = "Terminal initialization within span"
    )
    .entered();

    enable_raw_mode().map_err(|e| AppError::Terminal(format!("Failed to enable raw mode: {e}")))?;

    let mut stdout: Stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| AppError::Terminal(format!("Failed to enter alternate screen: {e}")))?;

    let backend: Backend<Stdout> = Backend::new(stdout);
    let terminal: Terminal<Backend<Stdout>> = Terminal::new(backend)
        .map_err(|e| AppError::Terminal(format!("Failed to create terminal: {e}")))?;

    info!(
        marker = "TERMINAL_SETUP_COMPLETE",
        operation_type = "terminal",
        message = "Terminal setup completed successfully"
    );
    Ok(terminal)
}

#[instrument(
    level = "info",
    name = "cleanup_terminal",
    fields(
        marker = "TERMINAL_CLEANUP_START",
        operation_type = "terminal",
        message = "Terminal cleanup initiated"
    )
)]
fn cleanup_terminal(terminal: &mut AppTerminal) -> AppResult<()> {
    disable_raw_mode()
        .map_err(|e| AppError::Terminal(format!("Failed to disable raw mode: {e}")))?;

    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| AppError::Terminal(format!("Failed to leave alternate screen: {e}")))?;

    terminal
        .show_cursor()
        .map_err(|e| AppError::Terminal(format!("Failed to show cursor: {e}")))?;

    info!(
        marker = "TERMINAL_CLEANUP_COMPLETE",
        operation_type = "terminal",
        message = "Terminal cleanup completed"
    );
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
            marker = "APPLICATION_PANIC",
            operation_type = "application_lifecycle",
            panic_info = %info,
            location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            message = "PANIC: Application panicked"
        );

        // Flush logs including file system operation logs
        Logger::flush();

        original(info);
    }));
}
