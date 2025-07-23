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
use crossterm::event::{Event as TermEvent, EventStream, KeyCode, KeyModifiers};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::debug;

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
    task_rx: mpsc::UnboundedReceiver<TaskResult>,
    event_stream: EventStream,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

impl Controller {
    pub fn new(
        app: Arc<tokio::sync::Mutex<AppState>>,
        task_rx: mpsc::UnboundedReceiver<TaskResult>,
        action_rx: mpsc::UnboundedReceiver<Action>,
    ) -> Self {
        Self {
            app,
            task_rx,
            event_stream: EventStream::new(),
            action_rx,
        }
    }

    /// Asynchronously returns the next action, waiting for user input or background task results.
    pub async fn next_action(&mut self) -> Option<Action> {
        tokio::select! {
            Some(Ok(event)) = self.event_stream.next() => {
                let action = self.handle_terminal_event(event);
                debug!("Received terminal event: {:?}", action);
                Some(action)
            }
            Some(task_result) = self.task_rx.recv() => {
                debug!("Received task result: {:?}", task_result);
                Some(Action::TaskResult(task_result))
            }
            Some(action) = self.action_rx.recv() => {
                debug!("Received action: {:?}", action);
                Some(action)
            }
            else => None,
        }
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
                (KeyCode::Up, _) => Action::MoveSelectionUp,
                (KeyCode::Down, _) => Action::MoveSelectionDown,
                (KeyCode::Enter, _) => Action::EnterSelected,
                (KeyCode::Backspace, _) => Action::GoToParent,
                (KeyCode::Char('q'), _) => Action::Quit,
                _ => Action::Key(key_event), // Pass through unhandled key events
            },
            TermEvent::Mouse(mouse_event) => Action::Mouse(mouse_event),
            TermEvent::Resize(x, y) => Action::Resize(x, y),
            _ => Action::Tick, // Default to tick for unhandled events
        }
    }

    /// Dispatches an action to update the application state.
    pub async fn dispatch_action(&self, action: Action) {
        debug!("Dispatching action: {:?}", action);
        match action {
            Action::Quit => {
                // Handled in main loop for graceful shutdown
            }
            Action::ToggleHelp => {
                let mut app = self.app.lock().await;
                app.ui.toggle_help_overlay();
                app.redraw = true;
            }
            Action::ToggleCommandPalette => {
                let mut app = self.app.lock().await;
                app.ui.toggle_command_palette();
                app.redraw = true;
            }
            Action::SimulateLoading => {
                let mut app = self.app.lock().await;
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
                let mut app = self.app.lock().await;
                app.ui.toggle_show_hidden();
                app.redraw = true;
            }
            Action::TaskResult(task_result) => {
                let mut app = self.app.lock().await;
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
            Action::MoveSelectionUp => {
                let mut app = self.app.lock().await;
                let entries = app.fs.active_pane().entries.clone();
                app.ui.move_selection_up(&entries);
                app.redraw = true;
            }
            Action::MoveSelectionDown => {
                let mut app = self.app.lock().await;
                let entries = app.fs.active_pane().entries.clone();
                app.ui.move_selection_down(&entries);
                app.redraw = true;
            }
            Action::EnterSelected => {
                let mut app = self.app.lock().await;
                app.enter_selected_directory().await;
                app.redraw = true;
            }
            Action::GoToParent => {
                let mut app = self.app.lock().await;
                app.go_to_parent_directory().await;
                app.redraw = true;
            }
            Action::UpdateObjectInfo { parent_dir, info } => {
                let mut app = self.app.lock().await;
                app.update_object_info(parent_dir, info);
                app.redraw = true;
            }
            Action::Key(_) | Action::Mouse(_) | Action::Resize(..) | Action::Tick => {
                let mut app = self.app.lock().await;
                app.redraw = true;
            }
        }
    }
}
