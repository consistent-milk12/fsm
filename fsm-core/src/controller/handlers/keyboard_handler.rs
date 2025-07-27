// fsm-core/src/controller/handlers/keyboard_handler.rs

use crate::controller::{
    actions::{Action, InputPromptType},
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// Fallback keyboard handler for application-level commands
pub struct KeyboardHandler {
    /// Application-level key bindings (non-conflicting)
    app_bindings: HashMap<KeyEvent, Action>,

    /// Emergency/fallback bindings for critical functions
    emergency_bindings: HashMap<KeyEvent, Action>,

    /// Input mode for modal operations
    mode: KeyboardMode,

    /// Statistics for unhandled keys
    unhandled_key_count: u64,
}

/// Keyboard handler operation modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardMode {
    /// Normal fallback mode - handle unspecialized keys
    Fallback,

    /// Debug mode - log all key events for troubleshooting
    Debug,
}

impl Default for KeyboardHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardHandler {
    /// Create new fallback keyboard handler
    pub fn new() -> Self {
        let mut app_bindings = HashMap::with_capacity(15);
        let mut emergency_bindings = HashMap::with_capacity(5);

        // ===== APPLICATION-LEVEL BINDINGS =====
        // Quit commands (multiple alternatives for reliability)
        app_bindings.insert(key('q'), Action::Quit);
        app_bindings.insert(key('Q'), Action::Quit);
        app_bindings.insert(ctrl('c'), Action::Quit);
        app_bindings.insert(ctrl('q'), Action::Quit);

        // Escape - universal overlay closer
        app_bindings.insert(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            Action::CloseOverlay,
        );

        // Function keys for common operations
        app_bindings.insert(function_key(1), Action::ToggleHelp); // F1
        app_bindings.insert(function_key(5), Action::ReloadDirectory); // F5
        app_bindings.insert(function_key(10), Action::Quit); // F10

        // Refresh commands
        app_bindings.insert(ctrl('l'), Action::ReloadDirectory);
        app_bindings.insert(ctrl('r'), Action::ReloadDirectory);

        // ===== EMERGENCY BINDINGS =====
        // Critical functions that must always work
        emergency_bindings.insert(ctrl_alt('q'), Action::Quit); // Emergency quit
        emergency_bindings.insert(function_key(12), Action::Quit); // F12 quit
        emergency_bindings.insert(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL),
            Action::Quit,
        ); // Ctrl+Esc

        Self {
            app_bindings,
            emergency_bindings,
            mode: KeyboardMode::Fallback,
            unhandled_key_count: 0,
        }
    }

    /// Handle keyboard events as fallback
    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            "KeyboardHandler: processing key {:?} in mode {:?}",
            key_event, self.mode
        );

        match self.mode {
            KeyboardMode::Fallback => self.handle_fallback_mode(key_event),
            KeyboardMode::Debug => self.handle_debug_mode(key_event),
        }
    }

    /// Handle keys in fallback mode
    fn handle_fallback_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check emergency bindings first (highest priority)
        if let Some(action) = self.emergency_bindings.get(&key_event).cloned() {
            debug!(
                "KeyboardHandler: emergency action {:?} for key {:?}",
                action, key_event
            );
            return Ok(vec![action]);
        }

        // Check application-level bindings
        if let Some(action) = self.app_bindings.get(&key_event).cloned() {
            debug!(
                "KeyboardHandler: app-level action {:?} for key {:?}",
                action, key_event
            );
            return Ok(vec![action]);
        }

        // Handle special system keys that might not be bound elsewhere
        match key_event.code {
            // F-keys for special functions
            KeyCode::F(n) => {
                debug!("KeyboardHandler: function key F{}", n);
                self.handle_function_key(n)
            }

            // Alt combinations that might be shortcuts
            _ if key_event.modifiers.contains(KeyModifiers::ALT) => {
                debug!("KeyboardHandler: Alt combination {:?}", key_event);
                self.handle_alt_combination(key_event)
            }

            // Regular character keys in fallback - usually ignored unless debug mode
            KeyCode::Char(_) => {
                self.unhandled_key_count += 1;
                debug!("KeyboardHandler: unhandled character key {:?}", key_event);

                // Only warn occasionally to avoid spam
                if self.unhandled_key_count % 20 == 0 {
                    warn!(
                        "KeyboardHandler: {} unhandled keys - possible handler misconfiguration",
                        self.unhandled_key_count
                    );
                }

                Ok(vec![])
            }

            // Other keys - log and ignore
            _ => {
                self.unhandled_key_count += 1;
                debug!("KeyboardHandler: unhandled key {:?}", key_event);
                Ok(vec![])
            }
        }
    }

    /// Handle keys in debug mode
    fn handle_debug_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        debug!("KeyboardHandler: DEBUG MODE - key {:?}", key_event);

        // In debug mode, still handle emergency keys
        if let Some(action) = self.emergency_bindings.get(&key_event).cloned() {
            debug!("KeyboardHandler: debug emergency action {:?}", action);
            return Ok(vec![action]);
        }

        // Toggle back to fallback mode with Ctrl+Alt+D
        if key_event == ctrl_alt('d') {
            debug!("KeyboardHandler: exiting debug mode");
            self.mode = KeyboardMode::Fallback;
            return Ok(vec![]);
        }

        // In debug mode, forward all keys for analysis
        debug!("KeyboardHandler: debug forwarding key {:?}", key_event);
        Ok(vec![Action::Key(key_event)])
    }

    /// Handle function keys with appropriate fallbacks
    fn handle_function_key(&self, n: u8) -> Result<Vec<Action>, AppError> {
        match n {
            1 => Ok(vec![Action::ToggleHelp]),
            2 => Ok(vec![Action::ShowInputPrompt(InputPromptType::Rename)]),
            3 => Ok(vec![Action::ToggleFileNameSearch]),
            5 => Ok(vec![Action::ReloadDirectory]),
            7 => Ok(vec![Action::CreateDirectory]),
            8 => Ok(vec![Action::Delete]),
            10 | 12 => Ok(vec![Action::Quit]),
            _ => {
                debug!("KeyboardHandler: unhandled function key F{}", n);
                Ok(vec![])
            }
        }
    }

    /// Handle Alt key combinations
    fn handle_alt_combination(&self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let KeyCode::Char(c) = key_event.code {
            match c {
                'q' | 'Q' => Ok(vec![Action::Quit]),
                'd' | 'D' => {
                    debug!("KeyboardHandler: Alt+D - debug mode toggle requested");
                    Ok(vec![]) // Mode switch handled in main handler
                }
                _ => {
                    debug!("KeyboardHandler: unhandled Alt+{}", c);
                    Ok(vec![])
                }
            }
        } else {
            Ok(vec![])
        }
    }

    /// Switch to debug mode for troubleshooting
    pub fn enable_debug_mode(&mut self) {
        debug!("KeyboardHandler: enabling debug mode");
        self.mode = KeyboardMode::Debug;
    }

    /// Switch back to fallback mode
    pub fn disable_debug_mode(&mut self) {
        debug!("KeyboardHandler: disabling debug mode");
        self.mode = KeyboardMode::Fallback;
    }

    /// Get statistics about unhandled keys
    pub fn unhandled_key_count(&self) -> u64 {
        self.unhandled_key_count
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.unhandled_key_count = 0;
    }

    /// Check if currently in debug mode
    pub fn is_debug_mode(&self) -> bool {
        matches!(self.mode, KeyboardMode::Debug)
    }
}

impl EventHandler for KeyboardHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            // Always handle emergency keys regardless of other handlers
            if self.emergency_bindings.contains_key(key_event) {
                return true;
            }

            // In debug mode, handle everything for logging
            if matches!(self.mode, KeyboardMode::Debug) {
                return true;
            }

            // Handle app-level keys (quit, help, etc.)
            if self.app_bindings.contains_key(key_event) {
                return true;
            }

            // Handle function keys and Alt combinations
            matches!(key_event.code, KeyCode::F(_) | KeyCode::Esc)
                || key_event.modifiers.contains(KeyModifiers::ALT)
        } else {
            false
        }
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            // Check for mode switches
            if key_event == ctrl_alt('d') {
                match self.mode {
                    KeyboardMode::Fallback => {
                        self.mode = KeyboardMode::Debug;
                        debug!("KeyboardHandler: switched to debug mode");
                    }
                    KeyboardMode::Debug => {
                        self.mode = KeyboardMode::Fallback;
                        debug!("KeyboardHandler: switched to fallback mode");
                    }
                }
                return Ok(vec![]);
            }

            self.handle_key(key_event)
        } else {
            Ok(vec![])
        }
    }

    fn priority(&self) -> u8 {
        match self.mode {
            // Highest priority in debug mode for logging
            KeyboardMode::Debug => 1,
            // Lowest priority - true fallback
            KeyboardMode::Fallback => 255,
        }
    }

    fn name(&self) -> &'static str {
        "KeyboardHandler"
    }
}

// Helper functions for creating key events

/// Create simple character key event
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Create Ctrl+key event
fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Create Ctrl+Alt+key event
fn ctrl_alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Create function key event
fn function_key(n: u8) -> KeyEvent {
    KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyboard_handler_creation() {
        let handler = KeyboardHandler::new();
        assert_eq!(handler.mode, KeyboardMode::Fallback);
        assert_eq!(handler.unhandled_key_count, 0);
        assert!(!handler.is_debug_mode());
    }

    #[test]
    fn test_emergency_bindings() {
        let handler = KeyboardHandler::new();

        // Emergency quit should always be bound
        assert!(handler.emergency_bindings.contains_key(&ctrl_alt('q')));
        assert!(handler.emergency_bindings.contains_key(&function_key(12)));
    }

    #[test]
    fn test_can_handle_emergency_keys() {
        let handler = KeyboardHandler::new();

        let emergency_event = Event::Key {
            event: ctrl_alt('q'),
            priority: crate::controller::event_processor::Priority::Critical,
        };

        assert!(handler.can_handle(&emergency_event));
    }
}
