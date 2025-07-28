//! FSM-Core main.rs - Minimal entry point delegating to EventLoop
use std::{
    io::{self, Stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend as Backend};
use tokio::{signal, sync::mpsc};
use tracing::{error, info, warn};

use fsm_core::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        event_loop::{EventLoop, TaskResult},
        state_coordinator::StateCoordinator,
    },
    fs::dir_scanner::spawn_directory_scan,
    model::{
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

    info!("FSM-Core exited cleanly");
    Ok(())
}

struct App {
    terminal: AppTerminal,
    event_loop: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    ui_renderer: UIRenderer,
}

impl App {
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting FSM-Core");

        let terminal: Terminal<Backend<Stdout>> = setup_terminal()?;

        // Load config and initialize services
        let config: Arc<Config> = Arc::new(Config::load().await.unwrap_or_else(|e| {
            warn!("Using default config: {}", e);
            Config::default()
        }));

        let cache: Arc<ObjectInfoCache> =
            Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
        let (task_tx, task_rx) = mpsc::unbounded_channel::<TaskResult>();

        // Get current directory
        let current_dir: PathBuf = tokio::fs::canonicalize(".")
            .await
            .context("Failed to get current directory")?;

        // Create states
        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(config, cache)));
        let fs_state: Arc<Mutex<FSState>> = Arc::new(Mutex::new(FSState::new(current_dir.clone())));
        let ui_state: Arc<RwLock<UIState>> = Arc::new(RwLock::new(UIState::default()));

        // Create coordinator and event loop
        let state_coordinator: Arc<StateCoordinator> =
            Arc::new(StateCoordinator::new(app_state, ui_state, fs_state));

        let event_loop: EventLoop = EventLoop::new(task_rx, state_coordinator.clone());

        // Load initial directory
        Self::load_directory(&state_coordinator, current_dir, task_tx).await?;

        let ui_renderer: UIRenderer = UIRenderer::new();

        info!("Application initialized");

        Ok(Self {
            terminal,
            event_loop,
            state_coordinator,
            ui_renderer,
        })
    }

    async fn load_directory(
        coordinator: &StateCoordinator,
        dir: PathBuf,
        task_tx: mpsc::UnboundedSender<TaskResult>,
    ) -> Result<()> {
        let task_id = {
            let app_state = coordinator.app_state();
            app_state.add_task("Loading directory")
        };

        let _handle = spawn_directory_scan(task_id, dir, false, task_tx);
        coordinator.request_redraw(RedrawFlag::All);

        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        self.setup_shutdown().await;

        // Main loop: event processing + rendering
        loop {
            tokio::select! {
                // Event loop processing
                result = self.event_loop.run() => {
                    match result {
                        Ok(_) => break,
                        Err(e) => {
                            error!("Event loop error: {}", e);
                            break;
                        }
                    }
                }

                // Render when needed
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(16)) => {
                    self.render()?;
                }
            }
        }

        self.log_metrics();
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        if self.state_coordinator.needs_redraw() {
            self.terminal.draw(|frame: &mut Frame| {
                self.ui_renderer.render(frame, &self.state_coordinator);
            })?;
            self.state_coordinator.clear_redraw();
        }
        Ok(())
    }

    async fn setup_shutdown(&self) {
        let shutdown: Arc<tokio::sync::Notify> = self.event_loop.shutdown_handle();

        tokio::spawn(async move {
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
                _ = ctrl_c => info!("Received Ctrl+C"),
                _ = terminate => info!("Received terminate signal"),
            }

            shutdown.notify_one();
        });
    }

    fn log_metrics(&self) {
        let metrics = self.event_loop.metrics();
        info!("Session metrics:");
        info!("  Tasks: {}", metrics.tasks);
        info!("  Actions: {}", metrics.actions);
        info!("  Uptime: {:.1}s", metrics.total.as_secs_f64());
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(e) = cleanup_terminal(&mut self.terminal) {
            warn!("Terminal cleanup failed: {}", e);
        }
    }
}

fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();

    execute!(stdout, EnterAlternateScreen)?;

    let backend = Backend::new(stdout);

    Ok(Terminal::new(backend)?)
}

fn cleanup_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn setup_panic_handler() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        error!("Panic: {}", info);
        original(info);
    }));
}
