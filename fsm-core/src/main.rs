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
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context, Result};
use clipr::{ClipBoard, config::ClipBoardConfig};
use crossterm::{
    event::{Event as TerminalEvent, EventStream, KeyCode, KeyEvent},
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
        action_dispatcher::ActionDispatcher,
        actions::Action,
        ekey_processor::EKeyProcessor,
        event_loop::{EventLoop, TaskResult},
        handler_registry::HandlerRegistry,
        state_coordinator::StateCoordinator,
    },
    fs::object_info::ObjectInfo,
    model::{
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
    action_dispatcher: ActionDispatcher,
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
        let clipboard: Arc<ClipBoard> = Arc::new(ClipBoard::new(ClipBoardConfig::default()));

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

        // Initialize EKey processor
        let ekey_processor: Arc<EKeyProcessor> = Arc::new(EKeyProcessor::new(clipboard));

        // Create HandlerRegistry independently (no circular dependency)
        let handler_registry = HandlerRegistry::with_ekey_processor(ekey_processor);

        // Create StateCoordinator with HandlerRegistry
        let state_coordinator: Arc<StateCoordinator> = Arc::new(
            StateCoordinator::new_simple(app_state.clone(), ui_state, fs_state_arc)
                .with_handler_registry(handler_registry),
        );

        // Create EventLoop with StateCoordinator
        let controller: EventLoop = EventLoop::new(
            app_state.clone(),
            task_rx,
            action_rx,
            state_coordinator.clone(),
        );

        // Create ActionDispatcher for modularized action handling
        let action_dispatcher = ActionDispatcher::new(state_coordinator.clone(), task_tx.clone());

        let ui_renderer: UIRenderer = UIRenderer::new();
        let shutdown: Arc<Notify> = Arc::new(Notify::new());

        // Set initial directory
        let current_dir: PathBuf = dir_handle
            .await?
            .context("Failed to get current directory")?;

        // Load initial directory into FSState
        {
            let mut fs_state_guard = state_coordinator.fs_state();
            let pane = fs_state_guard.active_pane_mut();
            pane.cwd = current_dir.clone();

            // Load actual directory contents
            pane.is_loading
                .store(true, std::sync::atomic::Ordering::Relaxed);

            // Load directory entries asynchronously
            let mut entries = Vec::new();

            // Add parent directory entry if not at root
            if let Some(parent) = current_dir.parent() {
                use fsm_core::fs::object_info::{LightObjectInfo, ObjectType};
                let light_parent = LightObjectInfo {
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
                    while let Ok(Some(entry)) = dir_entries.next_entry().await {
                        let entry_path = entry.path();

                        // Skip hidden files for now (can be made configurable later)
                        if let Some(filename) = entry_path.file_name()
                            && filename.to_string_lossy().starts_with('.')
                        {
                            continue;
                        }

                        // Create lightweight object info
                        match ObjectInfo::from_path_light(&entry_path).await {
                            Ok(light_info) => {
                                entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                            }
                            Err(e) => {
                                info!("Failed to read entry {:?}: {}", entry_path, e);
                            }
                        }
                    }

                    info!(
                        "Loaded {} entries from {}",
                        entries.len(),
                        current_dir.display()
                    );
                }
                Err(e) => {
                    warn!("Failed to read directory {:?}: {}", current_dir, e);
                }
            }

            pane.entries = entries;
            pane.is_loading
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }

        // Request initial UI redraw
        state_coordinator.request_redraw(RedrawFlag::All);

        info!(
            "Phase 4.0 clean application initialization complete (handlers: {})",
            state_coordinator.handler_count()
        );

        Ok(Self {
            terminal,
            controller,
            state_coordinator,
            action_dispatcher,
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
        let mut event_stream = EventStream::new();

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

                // Handle terminal input events
                maybe_event = event_stream.next() => {
                    if let Some(Ok(terminal_event)) = maybe_event
                        && let Some(action) = self.process_terminal_event(terminal_event).await? {
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

    /// Process terminal events and convert to actions
    async fn process_terminal_event(&self, event: TerminalEvent) -> Result<Option<Action>> {
        match event {
            TerminalEvent::Key(key_event) => {
                tracing::debug!("Key event: {:?}", key_event);

                // Check if an overlay is active and handle overlay-specific input
                let ui_state = self.state_coordinator.ui_state();
                if ui_state.overlay != fsm_core::model::ui_state::UIOverlay::None {
                    return self.handle_overlay_input(key_event, &ui_state).await;
                }

                // Handle basic keys
                match key_event.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        return Ok(Some(Action::Quit));
                    }
                    KeyCode::Esc => return Ok(Some(Action::CloseOverlay)), // Handle Esc globally
                    KeyCode::Up => return Ok(Some(Action::MoveSelectionUp)),
                    KeyCode::Down => return Ok(Some(Action::MoveSelectionDown)),
                    KeyCode::Left => return Ok(Some(Action::GoToParent)), // Left arrow = parent dir
                    KeyCode::PageUp => return Ok(Some(Action::PageUp)),
                    KeyCode::PageDown => return Ok(Some(Action::PageDown)),
                    KeyCode::Home => return Ok(Some(Action::SelectFirst)),
                    KeyCode::End => return Ok(Some(Action::SelectLast)),
                    KeyCode::Enter => return Ok(Some(Action::EnterSelected)),
                    KeyCode::Backspace => return Ok(Some(Action::GoToParent)),
                    KeyCode::Char(':') => return Ok(Some(Action::EnterCommandMode)),
                    KeyCode::Char('/') => return Ok(Some(Action::ToggleFileNameSearch)),
                    KeyCode::Char('h') | KeyCode::Char('?') => return Ok(Some(Action::ToggleHelp)),
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        // Number keys for quick file selection (1-9)
                        if let Some(digit) = c.to_digit(10) {
                            let index = if digit == 0 { 9 } else { digit - 1 } as usize; // 1-9 maps to 0-8, 0 maps to 9
                            return Ok(Some(Action::SelectIndex(index)));
                        }
                        tracing::debug!("Unhandled key: {:?}", key_event);
                    }
                    _ => {
                        // TODO: Process through HandlerRegistry for more complex key handling
                        tracing::debug!("Unhandled key: {:?}", key_event);
                    }
                }
            }
            TerminalEvent::Resize(width, height) => {
                return Ok(Some(Action::Resize(width, height)));
            }
            _ => {}
        }
        Ok(None)
    }

    /// Handle input when overlays are active
    async fn handle_overlay_input(
        &self,
        key_event: KeyEvent,
        ui_state: &fsm_core::model::ui_state::UIState,
    ) -> Result<Option<Action>> {
        use fsm_core::model::ui_state::UIOverlay;

        if key_event.code == KeyCode::Esc {
            // Always close overlay on Esc
            return Ok(Some(Action::CloseOverlay));
        }

        match ui_state.overlay {
            UIOverlay::Help => {
                // Help overlay - close on most keys
                match key_event.code {
                    KeyCode::Char('h') | KeyCode::Char('?') => {
                        return Ok(Some(Action::ToggleHelp));
                    }
                    _ => return Ok(Some(Action::CloseOverlay)),
                }
            }
            UIOverlay::FileNameSearch => {
                // Filename search overlay - handle input
                match key_event.code {
                    KeyCode::Char(c) => {
                        // Add character to search input
                        let mut new_input = ui_state.input.to_string();
                        new_input.push(c);
                        return Ok(Some(Action::FileNameSearch(new_input)));
                    }
                    KeyCode::Backspace => {
                        // Remove last character from search input
                        let mut new_input = ui_state.input.to_string();
                        new_input.pop();
                        return Ok(Some(Action::FileNameSearch(new_input)));
                    }
                    KeyCode::Enter => {
                        // Execute search and close overlay
                        let search_query = ui_state.input.to_string();
                        if !search_query.is_empty() {
                            // TODO: Implement actual filename search
                            tracing::info!("Executing filename search: {}", search_query);
                        }
                        return Ok(Some(Action::CloseOverlay));
                    }
                    _ => {}
                }
            }
            UIOverlay::Prompt => {
                // Command mode overlay - handle input
                match key_event.code {
                    KeyCode::Char(c) => {
                        // Add character to command input via action dispatcher
                        let mut new_input = ui_state.input.to_string();
                        new_input.push(c);
                        return Ok(Some(Action::UpdateInput(new_input)));
                    }
                    KeyCode::Backspace => {
                        // Remove last character from command input via action dispatcher
                        let mut new_input = ui_state.input.to_string();
                        new_input.pop();
                        return Ok(Some(Action::UpdateInput(new_input)));
                    }
                    KeyCode::Enter => {
                        // Submit command via action dispatcher
                        let command = ui_state.input.to_string();
                        return Ok(Some(Action::SubmitInputPrompt(command)));
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        Ok(None)
    }

    /// Dispatch action through ActionDispatcher
    async fn dispatch_action(&mut self, action: Action) -> Result<bool> {
        tracing::debug!("Dispatching action: {:?}", action);

        // Use ActionDispatcher for batched and optimized action handling
        let should_continue = self
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
            let duration = start.elapsed();
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
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);

        warn!("Application panicked: {}", panic_info);
        original_hook(panic_info);
    }));
}
