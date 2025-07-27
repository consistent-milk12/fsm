// fsm-core/src/controller/handlers/clipboard_handler.rs

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

/// High-performance clipboard handler for clipr integration
/// Handles clipboard overlay navigation, operations, and management
pub struct ClipboardHandler {
    /// Clipboard overlay key bindings
    bindings: HashMap<KeyEvent, Action>,

    /// Current clipboard mode
    mode: ClipboardMode,

    /// Selected index in clipboard overlay
    selected_index: usize,

    /// Clipboard overlay visibility state
    overlay_visible: bool,
}

/// Clipboard operation modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    /// Normal file browsing mode
    Normal,
    /// Clipboard overlay is active and focused
    OverlayActive,
}

impl Default for ClipboardHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardHandler {
    /// Create new clipboard handler
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(20);

        // Primary clipboard operations (normal mode)
        bindings.insert(key('c'), Action::ToggleClipboardOverlay); // Copy current selection
        bindings.insert(key('x'), Action::ToggleClipboardOverlay); // Cut current selection
        bindings.insert(key('v'), Action::ToggleClipboardOverlay); // Show clipboard to paste
        bindings.insert(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            Action::ToggleClipboardOverlay,
        );

        // Alternative clipboard shortcuts
        bindings.insert(ctrl('c'), Action::ToggleClipboardOverlay);
        bindings.insert(ctrl('x'), Action::ToggleClipboardOverlay);
        bindings.insert(ctrl('v'), Action::ToggleClipboardOverlay);

        // Clipboard overlay navigation (when overlay is active)
        // These will be handled in overlay mode

        Self {
            bindings,
            mode: ClipboardMode::Normal,
            selected_index: 0,
            overlay_visible: false,
        }
    }

    /// Handle clipboard-related key events
    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            "ClipboardHandler: processing key {:?} in mode {:?}",
            key_event, self.mode
        );

        match self.mode {
            ClipboardMode::Normal => self.handle_normal_mode(key_event),
            ClipboardMode::OverlayActive => self.handle_overlay_mode(key_event),
        }
    }

    /// Handle keys in normal file browsing mode
    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check for clipboard trigger keys
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("ClipboardHandler: normal mode action {:?}", action);

            // Determine the actual clipboard operation based on key
            let clipboard_action = match key_event.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    debug!("ClipboardHandler: Copy operation");
                    self.overlay_visible = true;
                    self.mode = ClipboardMode::OverlayActive;
                    Action::ToggleClipboardOverlay
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    debug!("ClipboardHandler: Cut operation");
                    self.overlay_visible = true;
                    self.mode = ClipboardMode::OverlayActive;
                    Action::ToggleClipboardOverlay
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    debug!(
                        "ClipboardHandler: Paste/View clipboard
  operation"
                    );
                    self.overlay_visible = true;
                    self.mode = ClipboardMode::OverlayActive;
                    Action::ToggleClipboardOverlay
                }
                KeyCode::Tab => {
                    debug!("ClipboardHandler: Toggle clipboard overlay");
                    self.overlay_visible = !self.overlay_visible;
                    if self.overlay_visible {
                        self.mode = ClipboardMode::OverlayActive;
                    }
                    Action::ToggleClipboardOverlay
                }
                _ if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl+C, Ctrl+X, Ctrl+V
                    debug!("ClipboardHandler: Ctrl+{:?} operation", key_event.code);
                    self.overlay_visible = true;
                    self.mode = ClipboardMode::OverlayActive;
                    Action::ToggleClipboardOverlay
                }
                _ => action,
            };

            Ok(vec![clipboard_action])
        } else {
            Ok(vec![])
        }
    }

    /// Handle keys when clipboard overlay is active
    /// Based on clipboard_overlay.rs help panel: "↑↓ Navigate, Enter Select, Tab Toggle, Esc Close, Del Remove"
    fn handle_overlay_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            // Navigation within clipboard overlay
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                debug!(
                    "ClipboardHandler: navigate up to index {}",
                    self.selected_index
                );
                Ok(vec![Action::MoveSelectionUp])
            }
            KeyCode::Down => {
                self.selected_index += 1; // Will be bounded by clipboard length
                debug!(
                    "ClipboardHandler: navigate down to index {}",
                    self.selected_index
                );
                Ok(vec![Action::MoveSelectionDown])
            }

            // Arrow key alternatives
            KeyCode::Char('k') => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                Ok(vec![Action::MoveSelectionUp])
            }
            KeyCode::Char('j') => {
                self.selected_index += 1;
                Ok(vec![Action::MoveSelectionDown])
            }

            // Selection and confirmation
            KeyCode::Enter => {
                debug!(
                    "ClipboardHandler: select clipboard item at index
  {}",
                    self.selected_index
                );
                // Paste/apply selected clipboard item
                self.mode = ClipboardMode::Normal;
                self.overlay_visible = false;

                Ok(vec![Action::SubmitInputPrompt(
                    self.selected_index.to_string(),
                )])
            }

            // Toggle overlay visibility
            KeyCode::Tab => {
                debug!("ClipboardHandler: toggle overlay");
                self.overlay_visible = !self.overlay_visible;
                if !self.overlay_visible {
                    self.mode = ClipboardMode::Normal;
                }
                Ok(vec![Action::ToggleClipboardOverlay])
            }

            // Close overlay
            KeyCode::Esc => {
                debug!("ClipboardHandler: close overlay");
                self.overlay_visible = false;
                self.mode = ClipboardMode::Normal;
                self.selected_index = 0;
                Ok(vec![Action::CloseOverlay])
            }

            // Remove item from clipboard
            KeyCode::Delete => {
                debug!(
                    "ClipboardHandler: remove clipboard item at index
  {}",
                    self.selected_index
                );
                // Remove current item and stay in overlay
                Ok(vec![Action::Delete]) // This should be interpreted as"delete from clipboard"
            }

            // Page navigation for large clipboards
            KeyCode::PageUp => {
                self.selected_index = self.selected_index.saturating_sub(10);
                Ok(vec![Action::PageUp])
            }
            KeyCode::PageDown => {
                self.selected_index += 10; // Will be bounded by clipboard length
                Ok(vec![Action::PageDown])
            }

            // Quick navigation
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
                Ok(vec![Action::SelectFirst])
            }
            KeyCode::End | KeyCode::Char('G') => {
                // Set to last item (will be bounded by actual clipboard length)
                self.selected_index = usize::MAX;
                Ok(vec![Action::SelectLast])
            }

            // Copy current selection to clipboard (even from within overlay)
            KeyCode::Char('c') => {
                debug!("ClipboardHandler: copy from overlay");
                Ok(vec![Action::Key(key_event)]) // Forward to main handler
            }

            // Cut current selection to clipboard
            KeyCode::Char('x') => {
                debug!("ClipboardHandler: cut from overlay");
                Ok(vec![Action::Key(key_event)]) // Forward to main handler
            }

            // Search within clipboard (future enhancement)
            KeyCode::Char('/') => {
                debug!("ClipboardHandler: search in clipboard");
                Ok(vec![Action::ToggleFileNameSearch]) // Adapt for clipboard search
            }

            // Help
            KeyCode::Char('?') => Ok(vec![Action::ToggleHelp]),

            // Any other key - might be for input or unhandled
            _ => {
                debug!(
                    "ClipboardHandler: unhandled key in overlay mode:
  {:?}",
                    key_event
                );
                Ok(vec![])
            }
        }
    }

    /// Set clipboard overlay visibility
    pub fn set_overlay_visible(&mut self, visible: bool) {
        self.overlay_visible = visible;
        self.mode = if visible {
            ClipboardMode::OverlayActive
        } else {
            ClipboardMode::Normal
        };
    }

    /// Get current overlay visibility
    pub fn is_overlay_visible(&self) -> bool {
        self.overlay_visible
    }

    /// Get current selected index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Set selected index (bounded by clipboard length externally)
    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = index;
    }

    /// Reset clipboard handler state
    pub fn reset(&mut self) {
        self.mode = ClipboardMode::Normal;
        self.selected_index = 0;
        self.overlay_visible = false;
    }

    /// Get current mode
    pub fn current_mode(&self) -> ClipboardMode {
        self.mode
    }
}

impl EventHandler for ClipboardHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            match self.mode {
                ClipboardMode::Normal => {
                    // Handle clipboard trigger keys in normal mode
                    matches!(
                        key_event.code,
                        KeyCode::Char('c' | 'x' | 'v') | KeyCode::Tab
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('c' | 'x' | 'v')))
                }
                ClipboardMode::OverlayActive => {
                    // In overlay mode, handle navigation and clipboard operations
                    matches!(
                        key_event.code,
                        // Navigation
                        KeyCode::Up | KeyCode::Down | KeyCode::Left |
  KeyCode::Right |
                          KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home
   | KeyCode::End |
                          // Vim navigation
                          KeyCode::Char('k' | 'j' | 'h' | 'l' | 'g' | 'G') |
                          // Operations
                          KeyCode::Enter | KeyCode::Tab | KeyCode::Esc |
  KeyCode::Delete |
                          // Clipboard ops
                          KeyCode::Char('c' | 'x' | 'v' | '/' | '?')
                    )
                }
            }
        } else {
            false
        }
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            self.handle_key(key_event)
        } else {
            Ok(vec![])
        }
    }

    fn priority(&self) -> u8 {
        match self.mode {
            ClipboardMode::Normal => 5,        // High priority for clipboard triggers
            ClipboardMode::OverlayActive => 1, // Highest priority when overlay is active
        }
    }

    fn name(&self) -> &'static str {
        "ClipboardHandler"
    }
}

/// Helper to create simple key events
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Helper to create Ctrl-modified key events
fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_mode_transitions() {
        let mut handler = ClipboardHandler::new();

        // Start in normal mode
        assert_eq!(handler.mode, ClipboardMode::Normal);
        assert!(!handler.overlay_visible);

        // Trigger clipboard overlay
        handler.set_overlay_visible(true);
        assert_eq!(handler.mode, ClipboardMode::OverlayActive);
        assert!(handler.overlay_visible);

        // Close overlay
        handler.set_overlay_visible(false);
        assert_eq!(handler.mode, ClipboardMode::Normal);
        assert!(!handler.overlay_visible);
    }

    #[test]
    fn test_navigation_in_overlay() {
        let mut handler = ClipboardHandler::new();
        handler.set_overlay_visible(true);

        // Test navigation
        assert_eq!(handler.selected_index, 0);

        handler.set_selected_index(5);
        assert_eq!(handler.selected_index, 5);
    }

    #[test]
    fn test_key_bindings() {
        let handler = ClipboardHandler::new();

        // Test normal mode bindings
        assert!(handler.bindings.contains_key(&key('c')));
        assert!(handler.bindings.contains_key(&key('x')));
        assert!(handler.bindings.contains_key(&key('v')));
        assert!(
            handler
                .bindings
                .contains_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        );
    }
}
