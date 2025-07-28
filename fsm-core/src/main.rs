//! src/main.rs
//! Enhanced File Manager TUI with complete module integration

use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, RwLock, atomic::Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{Event as TerminalEvent, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
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
        action_dispatcher::{DispatcherStats, ModularActionDispatcher},
        actions::Action,
        event_loop::{EventLoop, MetricsSnap, TaskResult},
        state_coordinator::StateCoordinator,
    },
    fs::dir_scanner::spawn_directory_scan,
    model::{
        PaneState,
        app_state::{AppState, TaskType},
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
    view::ui::UIRenderer,
};

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

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

struct App {
    terminal: AppTerminal,
    event_loop: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    action_dispatcher: ModularActionDispatcher,
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
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting File Manager TUI - Enhanced Integration");

        let terminal: Terminal<CrosstermBackend<Stdout>> =
            setup_terminal().context("Failed to initialize terminal")?;

        // Load configuration
        let config: Arc<Config> = Arc::new(Config::load().await.unwrap_or_else(|e| {
            info!("Failed to load config, using defaults: {}", e);
            Config::default()
        }));

        // Initialize core services
        let cache: Arc<ObjectInfoCache> =
            Arc::new(ObjectInfoCache::with_config(config.cache.clone()));

        // Create communication channels
        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        // Initialize filesystem state with current directory
        let current_dir = tokio::fs::canonicalize(".")
            .await
            .context("Failed to get current directory")?;

        let fs_state = FSState::new(current_dir.clone());

        // Create AppState
        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(
            config,
            cache,
            fs_state.clone(),
            task_tx.clone(),
            action_tx,
        )));

        let fs_state_arc: Arc<Mutex<FSState>> = Arc::new(Mutex::new(fs_state));

        // Create StateCoordinator
        let state_coordinator: Arc<StateCoordinator> = Arc::new(StateCoordinator::new(
            app_state,
            RwLock::new(UIState::default()),
            fs_state_arc,
        ));

        // Create ModularActionDispatcher
        let action_dispatcher =
            ModularActionDispatcher::new(state_coordinator.clone(), task_tx.clone());

        // Create EventLoop
        let event_loop: EventLoop = EventLoop::new(task_rx, action_rx, state_coordinator.clone());

        let ui_renderer: UIRenderer = UIRenderer::new();
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        // Load initial directory
        Self::load_initial_directory(&state_coordinator, current_dir, task_tx).await?;

        info!("Application initialized successfully");

        Ok(Self {
            terminal,
            event_loop,
            state_coordinator,
            action_dispatcher,
            ui_renderer,
            shutdown,
            performance_monitor: PerformanceMonitor::new(),
        })
    }

    async fn load_initial_directory(
        state_coordinator: &StateCoordinator,
        current_dir: PathBuf,
        task_tx: mpsc::UnboundedSender<TaskResult>,
    ) -> Result<()> {
        // Set loading state
        {
            let mut fs_state: MutexGuard<'_, FSState> = state_coordinator.fs_state();
            let pane: &mut PaneState = fs_state.active_pane_mut();
            pane.cwd = current_dir.clone();
            pane.is_loading.store(true, Ordering::Relaxed);
        }

        // Start async directory scan
        let task_id = {
            let app_state = state_coordinator.app_state();
            app_state.add_task("Loading initial directory", TaskType::DirectoryScan)
        };

        let _scan_handle = spawn_directory_scan(
            task_id,
            current_dir.clone(),
            false, // show_hidden
            task_tx,
        );

        state_coordinator.request_redraw(RedrawFlag::All);
        info!("Initial directory scan started: {}", current_dir.display());

        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        self.setup_shutdown_handler().await;
        info!("Starting enhanced event loop");

        let mut event_stream: EventStream = EventStream::new();

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

                // Terminal events
                maybe_event = event_stream.next() => {
                    if let Some(Ok(terminal_event)) = maybe_event {
                        if let Some(action) = self.process_terminal_event(terminal_event).await {
                            if matches!(action, Action::Quit) {
                                info!("Quit action from terminal event");
                                break;
                            }
                            if !self.dispatch_action(action).await? {
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

    async fn process_terminal_event(&self, event: TerminalEvent) -> Option<Action> {
        match event {
            TerminalEvent::Key(key_event) => {
                use crossterm::event::{KeyCode, KeyModifiers};

                match (key_event.code, key_event.modifiers) {
                    // Quit actions
                    (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Action::Quit),
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),

                    // Navigation
                    (KeyCode::Up, _) => Some(Action::MoveSelectionUp),
                    (KeyCode::Down, _) => Some(Action::MoveSelectionDown),
                    (KeyCode::Enter, _) => Some(Action::EnterSelected),
                    (KeyCode::Backspace, _) => Some(Action::GoToParent),
                    (KeyCode::PageUp, _) => Some(Action::PageUp),
                    (KeyCode::PageDown, _) => Some(Action::PageDown),

                    // File operations
                    (KeyCode::Char('c'), KeyModifiers::NONE) => {
                        if let Some(path) = self.get_selected_path() {
                            Some(Action::Copy(path))
                        } else {
                            None
                        }
                    }
                    (KeyCode::Char('x'), KeyModifiers::NONE) => {
                        if let Some(path) = self.get_selected_path() {
                            Some(Action::Cut(path))
                        } else {
                            None
                        }
                    }
                    (KeyCode::Char('v'), KeyModifiers::NONE) => Some(Action::Paste),
                    (KeyCode::Delete, _) => Some(Action::Delete),

                    // UI controls
                    (KeyCode::Char('h'), KeyModifiers::NONE) => Some(Action::ToggleHelp),
                    (KeyCode::Char('/'), KeyModifiers::NONE) => Some(Action::ToggleFileNameSearch),
                    (KeyCode::Esc, _) => Some(Action::CloseOverlay),
                    (KeyCode::F(5), _) => Some(Action::ReloadDirectory),
                    (KeyCode::Tab, _) => Some(Action::ToggleClipboard),

                    // Command mode
                    (KeyCode::Char(':'), KeyModifiers::NONE) => Some(Action::EnterCommandMode),

                    _ => None,
                }
            }

            TerminalEvent::Resize(width, height) => Some(Action::Resize(width, height)),

            _ => None,
        }
    }

    fn get_selected_path(&self) -> Option<PathBuf> {
        let fs_state = self.state_coordinator.fs_state();
        let pane = fs_state.active_pane();
        let selected_idx = pane.selected.load(std::sync::atomic::Ordering::Relaxed);
        pane.entries
            .get(selected_idx)
            .map(|entry| entry.path.clone())
    }

    async fn dispatch_action(&mut self, action: Action) -> Result<bool> {
        debug!("Dispatching action: {:?}", action);

        let should_continue = self
            .action_dispatcher
            .handle(action, ActionSource::UserInput)
            .await;

        Ok(should_continue)
    }

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

    async fn monitor_performance(&mut self) {
        let now: Instant = Instant::now();

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
        // Simple memory check without external dependencies
        if std::env::var("RUST_LOG").is_ok() {
            debug!("Memory monitoring active");
        }
    }

    async fn log_performance_metrics(&self) {
        let dispatcher_stats: DispatcherStats = self.action_dispatcher.get_stats();
        let render_stats = self.ui_renderer.get_stats();

        info!(
            "Performance: Actions: {}, Handlers: {}, FPS: {:.1}, Slow frames: {}",
            dispatcher_stats.total_actions,
            dispatcher_stats.total_handlers,
            render_stats.fps(),
            self.performance_monitor.slow_frames
        );

        if self.performance_monitor.slow_frames > 5 {
            warn!(
                "High slow frame count: {}",
                self.performance_monitor.slow_frames
            );
        }
    }

    async fn log_final_metrics(&self) {
        let event_loop_metrics: MetricsSnap = self.event_loop.snapshot_metrics();

        info!("Final metrics:");
        info!("  Tasks processed: {}", event_loop_metrics.tasks);
        info!("  Actions processed: {}", event_loop_metrics.actions);
        info!(
            "  Average latency: {:.2}ms",
            event_loop_metrics.avg.as_millis()
        );
        info!("  Queued actions: {}", event_loop_metrics.queued);
    }

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

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("Failed to create terminal")?;

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
    let original_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info: &PanicHookInfo<'_>| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        error!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
