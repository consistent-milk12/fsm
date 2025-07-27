//! src/main.rs  
//! ============================================================================
//! # File Manager TUI Application Entry Point - Phase 4.0 Complete Integration
//!
//! Full integration of all modules:
//! - ModularActionDispatcher for enhanced action handling
//! - EventProcessor for prioritized event management
//! - StateCoordinator with StateProvider abstraction
//! - HandlerRegistry for modular event processing
//! - Enhanced error handling and performance monitoring

use std::{
    ffi::OsStr,
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, RwLock, atomic::Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clipr::{ClipBoard, config::ClipBoardConfig};
use crossterm::{
    event::{Event as TerminalEvent, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{
    signal,
    sync::{Notify, mpsc},
};
use tracing::{debug, error, info, warn};

use fsm_core::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        action_batcher::ActionSource,
        action_dispatcher::ModularActionDispatcher,
        actions::Action,
        event_loop::{EventLoop, TaskResult},
        event_processor::{Event, EventProcessor, EventSenders, terminal_event_to_event},
        handler_registry::HandlerRegistry,
        state_coordinator::StateCoordinator,
        state_provider::StateProvider,
    },
    fs::object_info::ObjectInfo,
    model::{
        PaneState,
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
    view::ui::UIRenderer,
};

type AppTerminal = Terminal<Backend<Stdout>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    setup_panic_handler();

    let app = App::new()
        .await
        .context("Failed to initialize application")?;
    app.run().await.context("Application runtime error")?;

    info!("Application exited cleanly");
    Ok(())
}

/// Complete application with all modules integrated
struct App {
    terminal: AppTerminal,
    event_loop: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    action_dispatcher: ModularActionDispatcher,
    event_processor: EventProcessor,
    event_senders: EventSenders,
    handler_registry: HandlerRegistry,
    ui_renderer: UIRenderer,
    shutdown: Arc<Notify>,
    performance_monitor: PerformanceMonitor,
}

#[derive(Debug)]
struct PerformanceMonitor {
    last_memory_check: Instant,
    last_metrics_log: Instant,
    frame_times: Vec<Duration>,
    slow_frames: u32,
}

impl PerformanceMonitor {
    fn new() -> Self {
        Self {
            last_memory_check: Instant::now(),
            last_metrics_log: Instant::now(),
            frame_times: Vec::with_capacity(60),
            slow_frames: 0,
        }
    }

    fn record_frame_time(&mut self, duration: Duration) {
        self.frame_times.push(duration);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }

        if duration.as_millis() > 16 {
            self.slow_frames += 1;
        }
    }

    fn should_log_metrics(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_metrics_log).as_secs() >= 30 {
            self.last_metrics_log = now;
            true
        } else {
            false
        }
    }
}

impl App {
    /// Initialize application with complete module integration
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting File Manager TUI - Phase 4.0 Complete Integration");

        let terminal = setup_terminal().context("Failed to initialize terminal")?;

        // Load configuration
        let config = Arc::new(Config::load().await.unwrap_or_else(|e| {
            info!("Failed to load config, using defaults: {}", e);
            Config::default()
        }));

        // Initialize core services
        let cache = Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
        let _clipboard = Arc::new(ClipBoard::new(ClipBoardConfig::default()));

        // Create communication channels
        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        // Initialize state components
        let fs_state = FSState::default();
        let ui_state = UIState::default();

        // Create AppState
        let app_state = Arc::new(Mutex::new(AppState::new(
            config,
            cache,
            fs_state.clone(),
            task_tx.clone(),
            action_tx,
        )));

        let fs_state_arc: Arc<Mutex<FSState>> = Arc::new(Mutex::new(fs_state));

        // Create StateCoordinator (central state management)
        let state_coordinator: Arc<StateCoordinator> = Arc::new(StateCoordinator::new(
            app_state,
            RwLock::new(ui_state),
            fs_state_arc,
        ));

        // Create EventProcessor for prioritized event handling
        let event_processor: EventProcessor = EventProcessor::new();
        let event_senders: EventSenders = event_processor.senders();

        // Create ModularActionDispatcher
        let action_dispatcher: ModularActionDispatcher =
            ModularActionDispatcher::new(state_coordinator.clone(), task_tx.clone());

        // Create HandlerRegistry with StateProvider integration
        let handler_registry: HandlerRegistry = HandlerRegistry::with_state_provider(state_coordinator.clone());

        // Create EventLoop
        let event_loop: EventLoop = EventLoop::new(task_rx, action_rx, state_coordinator.clone());

        let ui_renderer: UIRenderer = UIRenderer::new();
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        // Initialize directory
        let current_dir: PathBuf = tokio::fs::canonicalize(".")
            .await
            .context("Failed to get current directory")?;

        Self::load_initial_directory(&state_coordinator, current_dir).await?;

        info!("Complete module integration initialized successfully");

        Ok(Self {
            terminal,
            event_loop,
            state_coordinator,
            action_dispatcher,
            event_processor,
            event_senders,
            handler_registry,
            ui_renderer,
            shutdown,
            performance_monitor: PerformanceMonitor::new(),
        })
    }

    /// Load initial directory contents
    async fn load_initial_directory(
        state_coordinator: &StateCoordinator,
        current_dir: PathBuf,
    ) -> Result<()> {
        // Set loading state
        {
            let mut fs_state: MutexGuard<'_, FSState> = state_coordinator.fs_state();
            let pane: &mut PaneState = fs_state.active_pane_mut();
            pane.cwd = current_dir.clone();
            pane.is_loading.store(true, Ordering::Relaxed);
        }

        // Load directory contents
        let entries: Vec<ObjectInfo> = Self::load_directory_entries(&current_dir).await;

        // Update state with loaded entries
        {
            let mut fs_state: MutexGuard<'_, FSState> = state_coordinator.fs_state();
            let pane: &mut PaneState = fs_state.active_pane_mut();
            pane.entries = entries;
            pane.is_loading.store(false, Ordering::Relaxed);
        }

        state_coordinator.request_redraw(RedrawFlag::All);
        info!("Initial directory loaded: {}", current_dir.display());

        Ok(())
    }

    /// Load directory entries with error handling
    async fn load_directory_entries(dir_path: &PathBuf) -> Vec<ObjectInfo> {
        let mut entries: Vec<ObjectInfo> = Vec::new();

        // Add parent directory if not at root
        if let Some(parent) = dir_path.parent() {
            use fsm_core::fs::object_info::{LightObjectInfo, ObjectType};

            let light_parent: LightObjectInfo = LightObjectInfo {
                path: parent.to_path_buf(),
                name: "..".to_string().into(),
                extension: None,
                object_type: ObjectType::Dir,
                is_dir: true,
                is_symlink: false,
            };

            entries.push(ObjectInfo::with_placeholder_metadata(light_parent));
        }

        // Load directory contents
        match tokio::fs::read_dir(dir_path).await {
            Ok(mut dir_entries) => {
                let mut load_errors = 0;

                while let Some(dir_entry) = dir_entries.next_entry().await.unwrap_or(None) {
                    let entry_path: PathBuf = dir_entry.path();

                    // Skip hidden files
                    if entry_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name: &str| name.starts_with('.'))
                        .unwrap_or(false)
                    {
                        continue;
                    }

                    match ObjectInfo::from_path_light(&entry_path).await {
                        Ok(light_info) => {
                            entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                        }
                        Err(e) => {
                            load_errors += 1;
                            if load_errors <= 5 {
                                debug!("Failed to read entry {:?}: {}", entry_path, e);
                            }
                        }
                    }
                }

                info!(
                    "Loaded {} entries from {} ({} errors)",
                    entries.len(),
                    dir_path.display(),
                    load_errors
                );
            }
            Err(e) => {
                warn!("Failed to read directory {:?}: {}", dir_path, e);
            }
        }

        entries
    }

    /// Main event loop with complete integration
    async fn run(mut self) -> Result<()> {
        self.setup_shutdown_handler().await;
        info!("Starting complete integrated event loop");

        let mut event_stream = EventStream::new();

        loop {
            // Render UI
            self.render().await?;

            // Monitor performance
            self.monitor_performance().await;

            tokio::select! {
                // Shutdown signal
                _ = self.shutdown.notified() => {
                    info!("Shutdown signal received");
                    break;
                }

                // Terminal input events
                maybe_event = event_stream.next() => {
                    if let Some(Ok(terminal_event)) = maybe_event {
                        if let Some(processed_event) = terminal_event_to_event(terminal_event) {
                            // Submit to EventProcessor for prioritized handling
                            if let Err(dropped_event) = self.event_processor.submit(processed_event) {
                                warn!("Event queue full, dropped event: {:?}", dropped_event);
                            }
                        }
                    }
                }

                // Process events from EventProcessor
                actions = self.event_processor.process_batch() => {
                    if let Some(actions) = actions {
                        for action in actions {
                            if matches!(action, Action::Quit) {
                                info!("Quit action from event processor");
                                break;
                            }
                            if !self.dispatch_action(action).await? {
                                info!("Termination from event processor action");
                                break;
                            }
                        }
                    }
                }

                // Background task results
                action = self.event_loop.next_action() => {
                    if matches!(action, Action::Quit) {
                        info!("Quit action from event loop");
                        break;
                    }
                    if !self.dispatch_action(action).await? {
                        info!("Termination from event loop action");
                        break;
                    }
                }
            }
        }

        // Final metrics before shutdown
        self.log_final_metrics().await;
        info!("Event loop terminated cleanly");
        Ok(())
    }

    /// Dispatch action through ModularActionDispatcher
    async fn dispatch_action(&mut self, action: Action) -> Result<bool> {
        debug!("Dispatching action: {:?}", action);

        let should_continue = self
            .action_dispatcher
            .handle(action, ActionSource::UserInput)
            .await;

        Ok(should_continue)
    }

    /// Render UI with performance monitoring
    async fn render(&mut self) -> Result<()> {
        if self.state_coordinator.needs_redraw() {
            let start: Instant = Instant::now();

            self.terminal
                .draw(|frame: &mut Frame<'_>| {
                    self.ui_renderer.render(frame, &self.state_coordinator);
                })
                .context("Failed to draw terminal")?;

            self.state_coordinator.clear_redraw();

            let duration = start.elapsed();
            self.performance_monitor.record_frame_time(duration);

            if duration.as_millis() > 16 {
                warn!("Slow render: {}ms (target: <16ms)", duration.as_millis());
            }
        }

        Ok(())
    }

    /// Monitor system performance
    async fn monitor_performance(&mut self) {
        let now = Instant::now();

        // Memory monitoring
        if now
            .duration_since(self.performance_monitor.last_memory_check)
            .as_secs()
            >= 10
        {
            self.performance_monitor.last_memory_check = now;
            self.check_memory_usage();
        }

        // Performance metrics
        if self.performance_monitor.should_log_metrics() {
            self.log_performance_metrics().await;
        }
    }

    fn check_memory_usage(&self) {
        match sys_info::mem_info() {
            Ok(mem_info) => {
                let available_mb = mem_info.avail / 1024;
                let total_mb = mem_info.total / 1024;
                let used_percent = ((total_mb - available_mb) as f64 / total_mb as f64) * 100.0;

                if available_mb < 100 {
                    error!(
                        "Critical memory: {}MB available ({}% used)",
                        available_mb, used_percent as u32
                    );
                } else if available_mb < 500 {
                    warn!(
                        "High memory usage: {}MB available ({}% used)",
                        available_mb, used_percent as u32
                    );
                }
            }
            Err(e) => debug!("Failed to get memory info: {}", e),
        }
    }

    async fn log_performance_metrics(&self) {
        let event_metrics = self.event_processor.metrics();
        let dispatcher_stats = self.action_dispatcher.get_stats();

        info!(
            "Performance: Events: {}, Actions: {}, Avg latency: {:.2}ms, Dropped: {}",
            event_metrics.total_events,
            dispatcher_stats.total_handlers,
            event_metrics.avg_latency_ms(),
            event_metrics.dropped_events
        );

        if self.performance_monitor.slow_frames > 0 {
            warn!(
                "Rendering: {} slow frames in last 30s",
                self.performance_monitor.slow_frames
            );
        }
    }

    async fn log_final_metrics(&self) {
        let event_metrics = self.event_processor.metrics();
        let event_loop_metrics = self.event_loop.get_metrics();

        info!("Final metrics:");
        info!("  Events processed: {}", event_metrics.total_events);
        info!("  Tasks processed: {}", event_loop_metrics.tasks_processed);
        info!(
            "  Actions processed: {}",
            event_loop_metrics.actions_processed
        );
        info!("  Average latency: {:.2}ms", event_metrics.avg_latency_ms());
        info!("  Dropped events: {}", event_metrics.dropped_events);
        info!("  Slow frames: {}", self.performance_monitor.slow_frames);
    }

    async fn setup_shutdown_handler(&self) {
        let shutdown: Arc<Notify> = self.shutdown.clone();

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};

                let mut sigterm: signal::unix::Signal =
                    signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
                let mut sigint: signal::unix::Signal =
                    signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");

                tokio::select! {
                    _ = sigterm.recv() => info!("Received SIGTERM"),
                    _ = sigint.recv() => info!("Received SIGINT"),
                    _ = signal::ctrl_c() => info!("Received Ctrl+C"),
                }
            }

            #[cfg(not(unix))]
            {
                if let Err(e) = signal::ctrl_c().await {
                    warn!("Failed to listen for Ctrl+C: {}", e);
                    return;
                }
                info!("Received Ctrl+C");
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

fn cleanup_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;

    info!("Terminal cleanup complete");
    Ok(())
}

fn setup_panic_handler() {
    let original_hook: Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static> =
        std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info: &PanicHookInfo<'_>| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        error!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
