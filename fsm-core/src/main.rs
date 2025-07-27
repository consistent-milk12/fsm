//! main.rs — Phase 4.0 entrypoint
//! Async TUI with lock‑free StateCoordinator and legacy EventLoop facade.

#![allow(clippy::needless_return)]

use std::{
    env,
    io::{self, Stdout, stdout},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::{signal, sync::Notify};
use tracing::{info, warn};

use fsm_core::{
    Logger,
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::{
        actions::Action,
        ekey_processor::EKeyProcessor,
        event_loop::{EventLoop, TaskResult},
        event_processor::Event, // <-- import Event
        handler_registry::HandlerRegistry,
        state_coordinator::StateCoordinator,
    },
    model::{app_state::AppState, fs_state::FSState, ui_state::UIState},
    view::ui::UIRenderer,
};

use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

enum LoopEvent {
    Shutdown,
    Tick,
    Action(Action),
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    setup_panic_handler();

    let app = App::new().await?;
    app.run().await?;

    info!("FSM exited cleanly");
    Ok(())
}

struct App {
    terminal: AppTerminal,
    event_loop: EventLoop,
    state_coordinator: Arc<StateCoordinator>,
    ui_renderer: UIRenderer,
    shutdown: Arc<Notify>,
}

impl App {
    async fn new() -> Result<Self> {
        Logger::init_tracing();
        info!("Starting FSM Phase 4.0 (lock‑free StateCoordinator)");

        let terminal = setup_terminal()?;

        let config = Arc::new(Config::load().await.unwrap_or_else(|_| {
            warn!("Config load failed; falling back to defaults");
            Config::default()
        }));

        let current_dir: PathBuf = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let cache = Arc::new(ObjectInfoCache::with_config(config.cache.clone()));
        let fs_state = FSState::new(current_dir);

        let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel::<TaskResult>();
        let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel::<Action>();

        let ui_state = UIState::default();
        let state_coordinator = Arc::new(StateCoordinator::new((*config).clone(), ui_state));

        let app_state = Arc::new(tokio::sync::Mutex::new(AppState::new(
            config.clone(),
            cache,
            fs_state,
            task_tx,
            action_tx,
        )));

        let ekey_processor = Arc::new(EKeyProcessor::new(UIState::default().clipboard.clone()));

        let handler_registry = Arc::new(tokio::sync::Mutex::new(HandlerRegistry::new(
            state_coordinator.clone(),
            ekey_processor.clone(),
        )));

        let event_loop = EventLoop::new(
            app_state.clone(),
            task_rx,
            action_rx,
            Some(state_coordinator.clone()),
            ekey_processor.clone(),
            handler_registry.clone(),
        );

        let ui_renderer = UIRenderer::new();
        let shutdown = Arc::new(Notify::new());

        info!("Initialization complete");
        Ok(Self {
            terminal,
            event_loop,
            state_coordinator,
            ui_renderer,
            shutdown,
        })
    }

    async fn run(mut self) -> Result<()> {
        self.setup_shutdown_handler();
        info!("Entering main loop");

        loop {
            self.render()?;

            let evt = tokio::select! {
                _ = self.shutdown.notified() => LoopEvent::Shutdown,
                action = self.event_loop.next_action() => LoopEvent::Action(action),
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(16)) => LoopEvent::Tick,
            };

            match evt {
                LoopEvent::Shutdown => {
                    info!("Shutdown requested");
                    break;
                }
                LoopEvent::Tick => {
                    continue;
                }
                LoopEvent::Action(action) => {
                    if matches!(action, Action::Quit) {
                        info!("Quit action received");
                        break;
                    }

                    if let Err(e) =
                        self.event_loop
                            .handler_registry
                            .lock()
                            .await
                            .handle_event(Event::Action {
                                action,
                                priority: fsm_core::controller::event_processor::Priority::Critical,
                            })
                    // <-- wrap Action
                    {
                        warn!("Handler error: {e}");
                    }
                }
            }
        }

        info!("Main loop ended");
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        let t0 = Instant::now();

        self.terminal.draw(|f| {
            self.ui_renderer.render(f, &self.state_coordinator);
        })?;

        let dt = t0.elapsed();
        if dt.as_millis() > 16 {
            warn!("Slow render: {}ms (>16ms)", dt.as_millis());
        }
        Ok(())
    }

    fn setup_shutdown_handler(&self) {
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};

                let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
                let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");

                tokio::select! {
                    _ = sigterm.recv() => info!("Received SIGTERM"),
                    _ = sigint.recv()  => info!("Received SIGINT"),
                    _ = signal::ctrl_c() => info!("Received Ctrl+C"),
                }
            }

            #[cfg(not(unix))]
            {
                if let Err(e) = signal::ctrl_c().await {
                    warn!("Ctrl+C listen failed: {e}");
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
            warn!("Terminal cleanup failed: {e}");
        }
    }
}

fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode().context("enable raw mode")?;

    let mut out = stdout();
    crossterm::execute!(out, EnterAlternateScreen).context("enter alternate screen")?;

    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend).context("create ratatui Terminal")?;

    info!("Terminal initialized");
    Ok(terminal)
}

fn cleanup_terminal(term: &mut AppTerminal) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    crossterm::execute!(term.backend_mut(), LeaveAlternateScreen)
        .context("leave alternate screen")?;
    term.show_cursor().context("show cursor")?;
    info!("Terminal restored");
    Ok(())
}

fn setup_panic_handler() {
    let original = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info: &PanicHookInfo| {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen);
        warn!("FSM panicked: {}", info);
        original(info);
    }));
}
