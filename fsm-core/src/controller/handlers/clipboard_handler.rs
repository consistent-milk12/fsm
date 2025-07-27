// fsm-core/src/controller/handlers/clipboard_handler.rs
// Fixed to work with UIState clipboard integration

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, trace};

pub struct ClipboardHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: ClipboardMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Normal,
    OverlayActive,
}

impl Default for ClipboardHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(15);

        // Basic clipboard operations - compatible with UIState async methods
        bindings.insert(key('c'), Action::Copy(PathBuf::new()));
        bindings.insert(key('x'), Action::Cut(PathBuf::new()));
        bindings.insert(key('v'), Action::Paste);

        // Clipboard overlay toggle - matches UIState::toggle_clipboard_overlay
        bindings.insert(tab_key(), Action::ToggleClipboardOverlay);

        // Ctrl alternatives
        bindings.insert(ctrl('c'), Action::Copy(PathBuf::new()));
        bindings.insert(ctrl('x'), Action::Cut(PathBuf::new()));
        bindings.insert(ctrl('v'), Action::Paste);

        Self {
            bindings,
            mode: ClipboardMode::Normal,
        }
    }

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

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("ClipboardHandler: action {:?}", action);

            // Update mode for overlay
            if matches!(action, Action::ToggleClipboardOverlay) {
                self.mode = ClipboardMode::OverlayActive;
            }

            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_overlay_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            // Navigation - compatible with UIState selection methods
            KeyCode::Up | KeyCode::Char('k') => Ok(vec![Action::ClipboardUp]),
            KeyCode::Down | KeyCode::Char('j') => Ok(vec![Action::ClipboardDown]),

            // Selection
            KeyCode::Enter => {
                self.mode = ClipboardMode::Normal;
                Ok(vec![Action::SelectClipboardItem(0)]) // Index will be filled by dispatcher
            }

            // Toggle overlay
            KeyCode::Tab => {
                self.mode = if self.mode == ClipboardMode::OverlayActive {
                    ClipboardMode::Normal
                } else {
                    ClipboardMode::OverlayActive
                };
                Ok(vec![Action::ToggleClipboardOverlay])
            }

            // Close overlay
            KeyCode::Esc => {
                self.mode = ClipboardMode::Normal;
                Ok(vec![Action::CloseOverlay])
            }

            // Remove item - compatible with UIState::clear_clipboard
            KeyCode::Delete => {
                Ok(vec![Action::RemoveFromClipboard(0)]) // ID will be filled by dispatcher
            }

            // Clear all - matches UIState::clear_clipboard
            KeyCode::Char('C') if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                Ok(vec![Action::ClearClipboard])
            }

            _ => Ok(vec![]),
        }
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
                    matches!(
                        key_event.code,
                        KeyCode::Char('c' | 'x' | 'v') | KeyCode::Tab
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('c' | 'x' | 'v')))
                }
                ClipboardMode::OverlayActive => {
                    matches!(
                        key_event.code,
                        KeyCode::Up
                            | KeyCode::Down
                            | KeyCode::Enter
                            | KeyCode::Tab
                            | KeyCode::Esc
                            | KeyCode::Delete
                            | KeyCode::Char('k' | 'j' | 'C')
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
            ClipboardMode::Normal => 5,
            ClipboardMode::OverlayActive => 1, // Highest when overlay active
        }
    }

    fn name(&self) -> &'static str {
        "ClipboardHandler"
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn tab_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
}
