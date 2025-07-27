//! src/main.rs  
//! ============================================================================
//! # File Manager TUI Application Entry Point - Phase 4.0 Simplified
//!
//! Clean Phase 4.0 main.rs with no circular dependencies:
//! - StateCoordinator for unified state management  
//! - Optional HandlerRegistry integration
//! - EventLoop with basic action processing
//! - UIRenderer integration
//! - Performance monitoring

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
use tracing::{info, warn};

use fsm_core::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        action_batcher::ActionSource,
        action_dispatcher::ModularActionDispatcher,
        actions::Action,
        event_loop::{EventLoop, TaskResult},
        event_processor::Event,
        handler_registry::HandlerRegistry,
        state_coordinator::StateCoordinator,
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

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    setup_panic_handler();

    let app: App = App::new()
        .await
        .context("Failed to initialize application")?;

    app.run().await.context("Application runtime error")?;

    info!("Application exited cleanly");
    Ok(())
}

/// Application runtime with clean Phase 4.0 architecture
struct App {
    terminal: AppTerminal,
    controller: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    action_dispatcher: ModularActionDispatcher,
    handler_registry: HandlerRegistry,
    ui_renderer: UIRenderer,
    shutdown: Arc<Notify>,
    last_memory_check: Instant,
}

impl App {
    /// Initialize application with clean Phase 4.0 architecture
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting File Manager TUI - Phase 4.0 Clean");

        let terminal: AppTerminal = setup_terminal().context("Failed to initialize terminal")?;

        // Load config and get current directory concurrently
        let config_handle = tokio::spawn(Config::load());
        let dir_handle = tokio::spawn(tokio::fs::canonicalize("."));

        let config: Arc<Config> = Arc::new(config_handle.await?.unwrap_or_else(|e| {
            info!("Failed to load config, using defaults: {}", e);
            Config::default()
        }));

        // Initialize core components
        let cache: Arc<ObjectInfoCache> =
            Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
        let _clipboard: Arc<ClipBoard> = Arc::new(ClipBoard::new(ClipBoardConfig::default()));

        // Create state components
        let fs_state: FSState = FSState::default();
        let ui_state: UIState = UIState::default();

        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = mpsc::unbounded_channel::<Action>();

        // Create AppState with std::Mutex as expected by EventLoop
        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(
            config,
            cache,
            fs_state.clone(),
            task_tx.clone(),
            action_tx,
        )));

        // Create fs_state wrapper for StateCoordinator
        let fs_state_arc: Arc<Mutex<FSState>> = Arc::new(Mutex::new(fs_state));

        // Create StateCoordinator first (no circular dependency)
        let state_coordinator: Arc<StateCoordinator> = Arc::new(StateCoordinator::new_simple(
            app_state.clone(),
            RwLock::new(ui_state),
            fs_state_arc,
        ));

        // Create HandlerRegistry with StateProvider (breaks circular dependency)
        let handler_registry: HandlerRegistry =
            HandlerRegistry::with_state_provider(state_coordinator.clone());

        // Create EventLoop with StateCoordinator
        let controller: EventLoop = EventLoop::new(
            app_state.clone(),
            task_rx,
            action_rx,
            state_coordinator.clone(),
        );

        // Create ActionDispatcher for modularized action handling
        let action_dispatcher: ModularActionDispatcher =
            ModularActionDispatcher::new(state_coordinator.clone(), task_tx.clone());

        let ui_renderer: UIRenderer = UIRenderer::new();
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        // Set initial directory
        let current_dir: PathBuf = dir_handle
            .await?
            .context("Failed to get current directory")?;

        // Set initial directory and loading state (short lock)
        {
            let mut fs_state_guard: MutexGuard<'_, FSState> = state_coordinator.fs_state();
            let pane: &mut PaneState = fs_state_guard.active_pane_mut();

            pane.cwd = current_dir.clone();
            pane.is_loading.store(true, Ordering::Relaxed);
            // Lock automatically dropped here
        }

        // Load directory contents WITHOUT holding the lock
        let loaded_entries: Vec<ObjectInfo> = {
            let mut entries: Vec<ObjectInfo> = Vec::new();

            // Add parent directory entry if not at root
            if let Some(parent) = current_dir.parent() {
                use fsm_core::fs::object_info::{LightObjectInfo, ObjectType};

                let light_parent: LightObjectInfo = LightObjectInfo {
                    path: parent.to_path_buf(),
                    name: "..".to_string(),
                    extension: None,
                    object_type: ObjectType::Dir,
                    is_dir: true,
                    is_symlink: false,
                };

                entries.push(ObjectInfo::with_placeholder_metadata(light_parent));
            }

            // Load directory contents
            match tokio::fs::read_dir(&current_dir).await {
                Ok(mut dir_entries) => {
                    let mut load_errors: i32 = 0;

                    while let Some(dir_entry) = dir_entries.next_entry().await.unwrap_or(None) {
                        let entry_path: PathBuf = dir_entry.path();

                        // Skip hidden files
                        if entry_path
                            .file_name()
                            .and_then(|name: &OsStr| name.to_str())
                            .map(|name: &str| name.starts_with('.'))
                            .unwrap_or(false)
                        {
                            continue;
                        }

                        // Create lightweight object info
                        match ObjectInfo::from_path_light(&entry_path).await {
                            Ok(light_info) => {
                                entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                            }

                            Err(e) => {
                                load_errors += 1;
                                if load_errors <= 5 {
                                    info!("Failed to read entry {:?}: {}", entry_path, e);
                                }
                            }
                        }
                    }

                    if load_errors > 0 {
                        info!(
                            "Loaded {} entries from {} ({} errors)",
                            entries.len(),
                            current_dir.display(),
                            load_errors
                        );
                    } else {
                        info!(
                            "Loaded {} entries from {}",
                            entries.len(),
                            current_dir.display()
                        );
                    }
                }
                Err(e) => {
                    warn!("Failed to read directory {:?}: {}", current_dir, e);
                }
            }

            entries
        };

        // Update state with loaded entries (short lock)
        {
            let mut fs_state_guard: MutexGuard<'_, FSState> = state_coordinator.fs_state();
            let pane: &mut PaneState = fs_state_guard.active_pane_mut();
            pane.entries = loaded_entries;
            pane.is_loading.store(false, Ordering::Relaxed);
            // Lock automatically dropped here
        }

        // Request initial UI redraw
        state_coordinator.request_redraw(RedrawFlag::All);

        info!(
            "Phase 4.0 clean application initialization complete (circular dependencies resolved)"
        );

        Ok(Self {
            terminal,
            controller,
            state_coordinator,
            action_dispatcher,
            handler_registry,
            ui_renderer,
            shutdown,
            last_memory_check: Instant::now(),
        })
    }

    /// Run main event loop with Phase 4.0 architecture and terminal input
    async fn run(mut self) -> Result<()> {
        self.setup_shutdown_handler().await;
        info!("Starting Phase 4.0 clean main event loop with input handling");

        // Setup terminal event stream for input handling
        let mut event_stream: EventStream = EventStream::new();

        loop {
            // Render UI if needed
            self.render().await?;

            // Check memory usage periodically
            self.check_memory_usage();

            // Wait for next event
            tokio::select! {
                _ = self.shutdown.notified() => {
                    info!("Shutdown signal received");
                    break;
                }

                // Handle terminal input events through HandlerRegistry
                maybe_event = event_stream.next() => {
                    if let Some(Ok(terminal_event)) = maybe_event
                        && let Some(actions) = self.process_terminal_event_via_registry(terminal_event).await? {
                            for action in actions {
                                if matches!(action, Action::Quit) {
                                    info!("Quit action received from input");
                                    break;
                                }
                                if !self.dispatch_action(action).await? {
                                    info!("Termination requested by action dispatcher");
                                    break;
                                }
                            }
                        }
                }

                // Handle actions from EventLoop (background tasks, etc.)
                action = self.controller.next_action() => {
                    if matches!(action, Action::Quit) {
                        info!("Quit action received from controller");
                        break;
                    }
                    if !self.dispatch_action(action).await? {
                        info!("Termination requested by action dispatcher");
                        break;
                    }
                }
            }
        }

        info!("Phase 4.0 clean main event loop ended");
        Ok(())
    }

    /// Process terminal events through HandlerRegistry
    async fn process_terminal_event_via_registry(
        &mut self,
        event: TerminalEvent,
    ) -> Result<Option<Vec<Action>>> {
        use fsm_core::controller::event_processor::Priority;

        // Convert TerminalEvent to Event for HandlerRegistry
        let handler_event: Event = match event {
            TerminalEvent::Key(key_event) => Event::Key {
                event: key_event,
                priority: Priority::High,
            },

            TerminalEvent::Resize(width, height) => Event::Resize { width, height },
            _ => return Ok(None), // Ignore other events
        };

        // Process through HandlerRegistry
        match self.handler_registry.handle_event(handler_event) {
            Ok(actions) => {
                if actions.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(actions))
                }
            }

            Err(e) => {
                tracing::warn!("Handler registry error: {}", e);
                Ok(None)
            }
        }
    }

    // Overlay input handling removed - now handled by HandlerRegistry

    /// Dispatch action through ActionDispatcher
    async fn dispatch_action(&mut self, action: Action) -> Result<bool> {
        tracing::debug!("Dispatching action: {:?}", action);

        // Use ActionDispatcher for batched and optimized action handling
        let should_continue: bool = self
            .action_dispatcher
            .handle(action, ActionSource::UserInput)
            .await;

        Ok(should_continue)
    }

    /// Render UI using Phase 4.0 UIRenderer and StateCoordinator
    async fn render(&mut self) -> Result<()> {
        if self.state_coordinator.needs_redraw() {
            let start: Instant = Instant::now();

            self.terminal
                .draw(|frame: &mut Frame<'_>| {
                    self.ui_renderer.render(frame, &self.state_coordinator);
                })
                .context("Failed to draw terminal")?;

            // Clear redraw flags after successful render
            self.state_coordinator.clear_redraw();

            // Monitor render performance
            let duration: Duration = start.elapsed();
            if duration.as_millis() > 16 {
                info!(
                    "Slow render detected: {}ms (target: <16ms for 60fps)",
                    duration.as_millis()
                );
            } else if duration.as_millis() > 8 {
                tracing::debug!("Render time: {}ms", duration.as_millis());
            }
        }

        Ok(())
    }

    fn check_memory_usage(&mut self) {
        let now: Instant = Instant::now();

        if now.duration_since(self.last_memory_check).as_secs() >= 5 {
            self.last_memory_check = now;

            match sys_info::mem_info() {
                Ok(mem_info) => {
                    let available_mb: u64 = mem_info.avail / 1024;
                    let total_mb: u64 = mem_info.total / 1024;
                    let used_percent: f64 =
                        ((total_mb - available_mb) as f64 / total_mb as f64) * 100.0;

                    if available_mb < 100 {
                        warn!(
                            "Critical memory usage: Only {}MB available ({}% used)",
                            available_mb, used_percent as u32
                        );
                    } else if available_mb < 500 {
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
                use tokio::signal::unix::{Signal, SignalKind, signal};

                let mut sigterm: Signal =
                    signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
                let mut sigint: Signal =
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
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        warn!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
