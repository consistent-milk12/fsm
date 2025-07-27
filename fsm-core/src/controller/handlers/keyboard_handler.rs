// fsm-core/src/controller/handlers/keyboard_handler.rs

use crate::controller::{
    actions::{Action, InputPromptType},
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// Fallback keyboard handler for unhandled keys and application-level commands
/// Only processes keys NOT handled by specialized handlers (NavigationHandler, ClipboardHandler, etc.)
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

impl KeyboardHandler {
    /// Create new fallback keyboard handler
    pub fn new() -> Self {
        let mut app_bindings: HashMap<KeyEvent, Action> = HashMap::with_capacity(10);
        let mut emergency_bindings: HashMap<KeyEvent, Action> = HashMap::with_capacity(5);

        // ===== APPLICATION-LEVEL BINDINGS =====
        // Only handle keys that don't conflict with specialized handlers

        // Quit commands (multiple alternatives for reliability)
        app_bindings.insert(key('q'), Action::Quit);
        app_bindings.insert(key('Q'), Action::Quit);
        app_bindings.insert(alt('q'), Action::Quit);
        app_bindings.insert(alt('Q'), Action::Quit);

        // Alternative help (? is handled by SearchHandler, but F1 is safe)
        app_bindings.insert(function_key(1), Action::ToggleHelp); // F1

        // Alternative hidden toggle (. is handled by FileOpsHandler, but Alt+. is safe)
        app_bindings.insert(alt('.'), Action::ToggleShowHidden);

        // Window/UI management
        app_bindings.insert(function_key(11), Action::NoOp); // F11 -placeholder for fullscreen
        app_bindings.insert(ctrl('l'), Action::ReloadDirectory); // Ctrl+L refresh

        // ===== EMERGENCY BINDINGS =====
        // Critical functions that must always work
        emergency_bindings.insert(ctrl_alt('q'), Action::Quit); // Emergency quit
        emergency_bindings.insert(function_key(12), Action::Quit); // F12 quit

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
            "KeyboardHandler: fallback processing key {:?} in mode
  {:?}",
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

        // Handle special system keys that might not be bound
        match key_event.code {
            // Escape - always close overlays if nothing else handles it
            KeyCode::Esc => {
                debug!("KeyboardHandler: fallback escape - close overlay");
                Ok(vec![Action::CloseOverlay])
            }

            // F-keys for special functions
            KeyCode::F(n) => {
                debug!("KeyboardHandler: unhandled function key F{}", n);
                self.handle_function_key(n)
            }

            // Alt combinations that might be shortcuts
            _ if key_event.modifiers.contains(KeyModifiers::ALT) => {
                debug!("KeyboardHandler: unhandled Alt combination {:?}", key_event);

                self.handle_alt_combination(key_event)
            }

            // Ctrl combinations that might be shortcuts
            _ if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                debug!(
                    "KeyboardHandler: unhandled Ctrl combination {:?}",
                    key_event
                );

                self.handle_ctrl_combination(key_event)
            }

            // Regular character keys - might be input for overlays
            KeyCode::Char(c) => {
                debug!(
                    "KeyboardHandler: unhandled character '{}' -
  forwarding as input",
                    c
                );

                Ok(vec![Action::Key(key_event)]) // Forward for potential input handling
            }

            // Other keys - log and ignore
            _ => {
                self.unhandled_key_count += 1;

                debug!(
                    "KeyboardHandler: truly unhandled key {:?} (count:
  {})",
                    key_event, self.unhandled_key_count
                );

                // Warn if too many unhandled keys (possible handler misconfiguration)
                if self.unhandled_key_count % 10 == 0 {
                    warn!(
                        "KeyboardHandler: {} unhandled keys - check
  handler configuration",
                        self.unhandled_key_count
                    );
                }

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

        // Log all other keys and forward them
        debug!("KeyboardHandler: debug forwarding key {:?}", key_event);

        Ok(vec![Action::Key(key_event)])
    }

    /// Handle function keys
    fn handle_function_key(&self, n: u8) -> Result<Vec<Action>, AppError> {
        match n {
            // F1 - Help
            1 => Ok(vec![Action::ToggleHelp]),

            // F2 - Rename
            2 => Ok(vec![Action::ShowInputPrompt(InputPromptType::Rename)]),

            // F3 - Find
            3 => Ok(vec![Action::ToggleFileNameSearch]),

            // F4 - Reserved
            4 => Ok(vec![Action::NoOp]),

            // F5 - Refresh
            5 => Ok(vec![Action::ReloadDirectory]),

            // F6 - Reserved
            6 => Ok(vec![Action::NoOp]),

            // F7 - New folder
            7 => Ok(vec![Action::CreateDirectory]),

            // F8 - Delete
            8 => Ok(vec![Action::Delete]),

            // F9 - Reserved
            9 => Ok(vec![Action::NoOp]),

            // F10 - Quit
            10 => Ok(vec![Action::Quit]),

            // F11 - Reserved (fullscreen)
            11 => Ok(vec![Action::NoOp]),

            // F12 - Emergency quit
            12 => Ok(vec![Action::Quit]),
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
                    debug!("KeyboardHandler: switching to debug mode");
                    Ok(vec![]) // Mode switch handled in handle_key
                }

                '.' => Ok(vec![Action::ToggleShowHidden]),

                _ => {
                    debug!("KeyboardHandler: unhandled Alt+{}", c);
                    Ok(vec![])
                }
            }
        } else {
            Ok(vec![])
        }
    }

    /// Handle Ctrl key combinations
    fn handle_ctrl_combination(&self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let KeyCode::Char(c) = key_event.code {
            match c {
                // Ctrl+L refresh
                'l' | 'L' => Ok(vec![Action::ReloadDirectory]),

                'q' | 'Q' => {
                    // Only handle Ctrl+Q if it has additional modifiers (emergency)
                    if key_event.modifiers.contains(KeyModifiers::ALT) {
                        Ok(vec![Action::Quit])
                    } else {
                        debug!(
                            "KeyboardHandler: Ctrl+Q without Alt -
  letting other handlers process"
                        );
                        Ok(vec![])
                    }
                }

                'd' | 'D' => {
                    // Toggle debug mode with Ctrl+Alt+D
                    if key_event.modifiers.contains(KeyModifiers::ALT) {
                        debug!("KeyboardHandler: toggling debug mode");
                        Ok(vec![]) // Mode switch will be handled
                    } else {
                        Ok(vec![])
                    }
                }

                _ => {
                    debug!("KeyboardHandler: unhandled Ctrl+{}", c);
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
            // KeyboardHandler acts as a true fallback - it CAN handle any key,
            // but with lowest priority, so specialized handlers get first chance

            // Always handle emergency keys regardless of other handlers
            if self.emergency_bindings.contains_key(key_event) {
                return true;
            }

            // In debug mode, handle everything for logging
            if matches!(self.mode, KeyboardMode::Debug) {
                return true;
            }

            // For fallback mode, we can handle any key but rely on priority system
            // to ensure specialized handlers run first
            true
        } else {
            false
        }
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            // Check for mode switches in handle method
            if key_event == ctrl_alt('d') {
                match self.mode {
                    KeyboardMode::Fallback => {
                        self.mode = KeyboardMode::Debug;
                        debug!("KeyboardHandler: switched to debug mode");
                    }

                    KeyboardMode::Debug => {
                        self.mode = KeyboardMode::Fallback;
                        debug!(
                            "KeyboardHandler: switched to fallback
  mode"
                        );
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

/// Create Alt+key event
fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
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
    fn test_fallback_handler_creation() {
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
    fn test_debug_mode_toggle() {
        let mut handler = KeyboardHandler::new();

        // Start in fallback mode
        assert_eq!(handler.mode, KeyboardMode::Fallback);

        // Switch to debug mode
        handler.enable_debug_mode();
        assert_eq!(handler.mode, KeyboardMode::Debug);
        assert!(handler.is_debug_mode());

        // Switch back
        handler.disable_debug_mode();
        assert_eq!(handler.mode, KeyboardMode::Fallback);
        assert!(!handler.is_debug_mode());
    }

    #[test]
    fn test_priority_system() {
        let handler = KeyboardHandler::new();

        // Fallback mode should have lowest priority
        assert_eq!(handler.priority(), 255);

        let mut debug_handler = KeyboardHandler::new();
        debug_handler.enable_debug_mode();

        // Debug mode should have highest priority for logging
        assert_eq!(debug_handler.priority(), 1);
    }

    #[test]
    fn test_unhandled_key_statistics() {
        let mut handler = KeyboardHandler::new();

        assert_eq!(handler.unhandled_key_count(), 0);

        // Simulate some unhandled keys
        let _ = handler.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(handler.unhandled_key_count(), 1);

        handler.reset_stats();
        assert_eq!(handler.unhandled_key_count(), 0);
    }
}
