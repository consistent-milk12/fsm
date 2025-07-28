use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::controller::{Action, TaskResult};

/// Event priority levels for scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Priority {
    /// ESC, Quit events (highest priority)
    Critical = 0,

    /// User input
    High = 1,

    /// Background tasks
    Normal = 2,

    /// Metrics, logging (lowest priority)
    Low = 3,
}

/// Unified event type without heap allocation.
#[derive(Debug, Clone)]
pub enum Event {
    /// Keyboard input with priority hint.
    Key { event: KeyEvent, priority: Priority },

    /// Terminal resize.
    Resize { width: u16, height: u16 },

    /// Background task completion.
    Task { result: Box<TaskResult> },

    /// Direct actioOptionction.
    Action {
        action: Box<Action>,
        priority: Priority,
    },

    /// Periodic tick for UI updates.
    Tick,
}

/// Event handler trait for modular processing.
pub trait EventHandler: Send + Sync {
    /// Check if this handler can process the event.
    fn can_handle(&self, event: &Event) -> bool;

    /// Process event and return actions.
    fn handle(&mut self, event: Event) -> Result<Vec<Action>, crate::error::AppError>;

    /// Handler priority for ordering (lower numbers run first).
    fn priority(&self) -> u8 {
        100
    }

    /// Handler name for debugging.
    fn name(&self) -> &'static str;
}

/// Internal representation of a registered event handler.
///
/// A `HandlerEntry` bundles a handler’s static metadata (its
/// priority and name) together with the handler itself wrapped in an
/// `Arc<Mutex<...>>` for interior mutability.  The handler is
/// referenced via an `Arc` so that multiple snapshots of the handler
/// list (held in the [`ArcSwap`]) share the same underlying handler
/// instance.  Cloning a `HandlerEntry` only clones the `Arc`, not the
/// underlying handler, so updates via the `Mutex` are visible to all
/// readers.
#[allow(unused)]
#[derive(Clone)]
pub struct HandlerEntry {
    /// Handler execution priority (lower values run first).
    priority: u8,

    /// Handler name for diagnostics.
    name: &'static str,

    /// The actual event handler wrapped in an `Arc<Mutex<...>>`.
    handler: Arc<Mutex<Box<dyn EventHandler>>>,
}

pub fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub fn ctrl_alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT)
}

pub fn arrow_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn function_key(n: u8) -> KeyEvent {
    KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
}

pub fn tab_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
}

pub fn enter_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

pub fn backspace_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
}

pub fn escape_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

pub fn is_valid_filename_char(c: char) -> bool {
    !matches!(
        c,
        '\0' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    )
}
