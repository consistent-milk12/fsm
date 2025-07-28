use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::controller::Action;

/// Extended ActionMatcher with dynamic priority support
pub trait ActionMatcher {
    fn can_handle(&self, action: &Action) -> bool;
    fn priority(&self) -> ActionPriority;
    fn name(&self) -> &'static str;

    /// Dynamic priority based on specific action
    fn dynamic_priority(&self, _action: &Action) -> ActionPriority {
        self.priority()
    }

    /// Whether this dispatcher can be disabled
    fn can_disable(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionPriority {
    Critical = 0, // System/quit operations
    High = 1,     // Navigation, UI responsiveness
    Normal = 2,   // File operations, clipboard
    Low = 3,      // Background tasks, tick events
}

#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Continue,   // Keep processing
    Terminate,  // Shutdown application
    NotHandled, // Pass to next dispatcher
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
