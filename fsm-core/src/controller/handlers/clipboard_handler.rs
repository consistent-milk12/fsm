// fsm-core/src/controller/handlers/clipboard_handler.rs

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
    selected_index: usize,
    overlay_visible: bool,
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

        // Primary clipboard operations
        bindings.insert(key('c'), Action::Copy(PathBuf::new())); // Will be updated with actual path
        bindings.insert(key('x'), Action::Cut(PathBuf::new()));
        bindings.insert(key('v'), Action::Paste);
        bindings.insert(tab_key(), Action::ToggleClipboard);

        // Ctrl combinations
        bindings.insert(ctrl('c'), Action::Copy(PathBuf::new()));
        bindings.insert(ctrl('x'), Action::Cut(PathBuf::new()));
        bindings.insert(ctrl('v'), Action::Paste);

        Self {
            bindings,
            mode: ClipboardMode::Normal,
            selected_index: 0,
            overlay_visible: false,
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
            debug!("ClipboardHandler: normal mode action {:?}", action);

            match key_event.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    // Copy operation
                    Ok(vec![Action::Copy(PathBuf::new())]) // Path will be filled by action dispatcher
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    // Cut operation
                    Ok(vec![Action::Cut(PathBuf::new())])
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    // Paste operation
                    Ok(vec![Action::Paste])
                }
                KeyCode::Tab => {
                    // Toggle clipboard overlay
                    self.overlay_visible = !self.overlay_visible;
                    if self.overlay_visible {
                        self.mode = ClipboardMode::OverlayActive;
                    }
                    Ok(vec![Action::ToggleClipboard])
                }
                _ if key_event.modifiers.contains(KeyModifiers::CONTROL) => match key_event.code {
                    KeyCode::Char('c') => Ok(vec![Action::Copy(PathBuf::new())]),
                    KeyCode::Char('x') => Ok(vec![Action::Cut(PathBuf::new())]),
                    KeyCode::Char('v') => Ok(vec![Action::Paste]),
                    _ => Ok(vec![action]),
                },
                _ => Ok(vec![action]),
            }
        } else {
            Ok(vec![])
        }
    }

    fn handle_overlay_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                Ok(vec![Action::ClipboardUp])
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_index += 1;
                Ok(vec![Action::ClipboardDown])
            }

            // Selection
            KeyCode::Enter => {
                self.mode = ClipboardMode::Normal;
                self.overlay_visible = false;
                Ok(vec![Action::SelectClipboardItem(self.selected_index)])
            }

            // Toggle overlay
            KeyCode::Tab => {
                self.overlay_visible = !self.overlay_visible;
                if !self.overlay_visible {
                    self.mode = ClipboardMode::Normal;
                }
                Ok(vec![Action::ToggleClipboard])
            }

            // Close overlay
            KeyCode::Esc => {
                self.overlay_visible = false;
                self.mode = ClipboardMode::Normal;
                self.selected_index = 0;
                Ok(vec![Action::CloseOverlay])
            }

            // Remove item
            KeyCode::Delete => Ok(vec![Action::RemoveFromClipboard(
                self.selected_index as u64,
            )]),

            // Page navigation
            KeyCode::PageUp => {
                self.selected_index = self.selected_index.saturating_sub(10);
                Ok(vec![Action::ClipboardUp])
            }
            KeyCode::PageDown => {
                self.selected_index += 10;
                Ok(vec![Action::ClipboardDown])
            }

            // Quick navigation
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
                Ok(vec![Action::SelectClipboardItem(0)])
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = usize::MAX;
                Ok(vec![Action::SelectClipboardItem(usize::MAX)])
            }

            // Clear clipboard
            KeyCode::Char('C') if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                Ok(vec![Action::ClearClipboard])
            }

            // Show details
            KeyCode::Char('i') => Ok(vec![Action::ShowClipboardItemDetails(
                self.selected_index as u64,
            )]),

            _ => Ok(vec![]),
        }
    }

    pub fn set_overlay_visible(&mut self, visible: bool) {
        self.overlay_visible = visible;
        self.mode = if visible {
            ClipboardMode::OverlayActive
        } else {
            ClipboardMode::Normal
        };
    }

    pub fn is_overlay_visible(&self) -> bool {
        self.overlay_visible
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

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
                            | KeyCode::PageUp
                            | KeyCode::PageDown
                            | KeyCode::Home
                            | KeyCode::End
                            | KeyCode::Enter
                            | KeyCode::Tab
                            | KeyCode::Esc
                            | KeyCode::Delete
                            | KeyCode::Char('k' | 'j' | 'g' | 'G' | 'i' | 'C')
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
            ClipboardMode::OverlayActive => 1, // Highest priority when overlay active
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
