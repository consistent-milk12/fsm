//! src/controller/event_loop.rs
//! ============================================================================
//! # Controller: Async Event Loop Utilities (Struct-based)
//!
//! The Controller struct manages event handling, task dispatch, and UI coordination.
//! Key async helpers are implemented as instance methods for clarity and modularity.

use crate::model::app_state::AppState;
use crossterm::event::{self, Event as TermEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

// Controller struct: manages app state, event sources, and communication channels.
pub struct Controller {
    pub app: Arc<tokio::sync::Mutex<AppState>>,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub task_rx: mpsc::UnboundedReceiver<TaskResult>,
    // Add more: plugin channels, OS notifiers, timers, etc.
}

impl Controller {
    /// Waits asynchronously for the next terminal event (keyboard, mouse, resize, etc.).
    /// Uses crossterm's nonblocking poll and integrates with Tokio via spawn_blocking.
    pub async fn next_terminal_event(&self) -> Option<TermEvent> {
        // Poll with timeout so we remain responsive to other events.
        // Can adjust timeout/granularity as needed.
        tokio::task::spawn_blocking(|| {
            // Wait up to 100ms for a new event; poll blocks for that duration.
            if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                event::read().ok()
            } else {
                None
            }
        })
        .await
        .ok()
        .flatten()
    }

    /// Triggers a TUI redraw using the current AppState.
    /// Should be called after any state mutation.
    pub async fn redraw(&self) -> anyhow::Result<()> {
        let app = self.app.lock().await;
        crate::view::ui::View::redraw(&*app).await
    }
}

// Example TaskResult used for background completion messages.
#[derive(Debug)]
pub struct TaskResult {
    pub task_id: u64,
    pub result: Result<String, String>,
}
