//! src/controller/event_loop.rs
//! ============================================================================
//! # Controller: Async Event & State Coordination
//!
//! - Manages terminal input, background task updates, and state transitions.
//! - Mutates AppState/UIState, signals UI redraw via redraw flag.
//! - Never calls UI rendering directly; fully decoupled for immediate-mode TUI.

use crate::controller::actions::Action;
use crate::model::app_state::AppState;
use crate::model::ui_state::{LoadingState, UIOverlay};
use crossterm::event::{self, Event as TermEvent, KeyCode, KeyModifiers};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Result from a background async task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskResult {
    pub task_id: u64,
    pub result: Result<String, String>,
    pub progress: Option<f64>,
    pub current_item: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<String>,
}

pub struct Controller {
    pub app: Arc<tokio::sync::Mutex<AppState>>,
    pub task_rx: mpsc::UnboundedReceiver<TaskResult>,
}

impl Controller {
    pub fn new(
        app: Arc<tokio::sync::Mutex<AppState>>,
        task_rx: mpsc::UnboundedReceiver<TaskResult>,
    ) -> Self {
        Self { app, task_rx }
    }

    /// Main event polling entry: handles both user/terminal and background task events.
    pub async fn poll_events(&mut self) -> Option<Action> {
        // Poll for terminal events
        if let Some(event) = self.next_terminal_event().await {
            return Some(self.handle_terminal_event(event));
        }

        // Poll for background task results
        if let Ok(task_result) = self.task_rx.try_recv() {
            return Some(Action::TaskResult(task_result));
        }

        None
    }

    /// Polls asynchronously for the next terminal event.
    async fn next_terminal_event(&self) -> Option<TermEvent> {
        tokio::task::spawn_blocking(|| {
            event::poll(Duration::from_millis(50))
                .ok()
                .and_then(|ready| if ready { event::read().ok() } else { None })
        })
        .await
        .ok()
        .flatten()
    }

    /// Maps a raw terminal event to a high-level application Action.
    fn handle_terminal_event(&self, event: TermEvent) -> Action {
        match event {
            TermEvent::Key(key_event) => match (key_event.code, key_event.modifiers) {
                (KeyCode::Char('?'), _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                    Action::ToggleHelp
                }
                (KeyCode::Char(':'), _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    Action::ToggleCommandPalette
                }
                (KeyCode::Char('l'), KeyModifiers::CONTROL) => Action::SimulateLoading,
                (KeyCode::Esc, _) => Action::Quit, // Esc can also close overlays, but for now, quit
                (KeyCode::Char('.'), KeyModifiers::CONTROL) => Action::ToggleShowHidden,
                _ => Action::Key(key_event), // Pass through unhandled key events
            },
            TermEvent::Mouse(mouse_event) => Action::Mouse(mouse_event),
            TermEvent::Resize(x, y) => Action::Resize(x, y),
            _ => Action::Tick, // Default to tick for unhandled events
        }
    }

    /// Dispatches an action to update the application state.
    pub async fn dispatch_action(&self, action: Action) {
        let mut app = self.app.lock().await;
        match action {
            Action::Quit => {
                // Handled in main loop for graceful shutdown
            }
            Action::ToggleHelp => {
                app.ui.toggle_help_overlay();
                app.redraw = true;
            }
            Action::ToggleCommandPalette => {
                app.ui.toggle_command_palette();
                app.redraw = true;
            }
            Action::SimulateLoading => {
                app.ui.loading = Some(LoadingState {
                    message: "Simulated loading...".into(),
                    progress: None,
                    spinner_frame: 0,
                    current_item: Some("demo.txt".into()),
                    completed: Some(0),
                    total: Some(100),
                });
                app.ui.overlay = UIOverlay::Loading;
                app.redraw = true;
            }
            Action::ToggleShowHidden => {
                app.ui.toggle_show_hidden();
                app.redraw = true;
            }
            Action::TaskResult(task_result) => {
                // If a loading overlay is active, update its fields.
                if let Some(ref mut loading) = app.ui.loading {
                    if let Some(progress) = task_result.progress {
                        loading.progress = Some(progress);
                    }
                    if let Some(ref item) = task_result.current_item {
                        loading.current_item = Some(item.clone().into());
                    }
                    if let Some(done) = task_result.completed {
                        loading.completed = Some(done);
                    }
                    if let Some(total) = task_result.total {
                        loading.total = Some(total);
                    }
                    if let Some(msg) = task_result.message {
                        loading.message = msg;
                    }
                    loading.spinner_frame = loading.spinner_frame.wrapping_add(1);
                }

                // On completion (progress == 1.0), hide overlay.
                if let Some(p) = task_result.progress {
                    if (p - 1.0).abs() < f64::EPSILON {
                        app.ui.loading = None;
                        // Optionally close overlay if UIOverlay::Loading
                        if app.ui.overlay == UIOverlay::Loading {
                            app.ui.overlay = UIOverlay::None;
                        }
                    }
                }

                // Always update AppState's task table.
                app.complete_task(
                    task_result.task_id,
                    Some(match &task_result.result {
                        Ok(s) => s.clone(),
                        Err(e) => format!("Error: {e}"),
                    }),
                );
                app.redraw = true;
            }
            Action::Key(_) | Action::Mouse(_) | Action::Resize(..) | Action::Tick => {
                // These actions might not directly change AppState, but could trigger redraws
                // or be handled by other parts of the system (e.g., UI components).
                // Ensure redraw is set for any interaction.
                app.redraw = true;
            }
        }
    }
}