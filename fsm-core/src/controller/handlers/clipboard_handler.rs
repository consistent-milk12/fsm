use crate::controller::actions::Action;
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::trace;

use super::*;

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
        let mut bindings = HashMap::with_capacity(8);

        // Core operations
        bindings.insert(key('c'), Action::Copy(PathBuf::new()));
        bindings.insert(key('x'), Action::Cut(PathBuf::new()));
        bindings.insert(key('v'), Action::Paste);
        bindings.insert(tab_key(), Action::ToggleClipboard);

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
            marker = "CLIPBOARD_HANDLER_KEY_EVENT",
            operation_type = "input_handling",
            "ClipboardHandler: key {:?} mode {:?}",
            key_event,
            self.mode
        );

        match self.mode {
            ClipboardMode::Normal => self.handle_normal_mode(key_event),
            ClipboardMode::OverlayActive => self.handle_overlay_mode(key_event),
        }
    }

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            if matches!(action, Action::ToggleClipboard) {
                self.mode = ClipboardMode::OverlayActive;
            }

            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_overlay_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => Ok(vec![Action::ClipboardUp]),

            KeyCode::Down | KeyCode::Char('j') => Ok(vec![Action::ClipboardDown]),

            KeyCode::Enter => {
                self.mode = ClipboardMode::Normal;

                Ok(vec![Action::SelectClipboardItem(0)])
            }

            KeyCode::Tab => {
                self.mode = ClipboardMode::Normal;

                Ok(vec![Action::ToggleClipboard])
            }
            KeyCode::Esc => {
                self.mode = ClipboardMode::Normal;
                Ok(vec![Action::CloseOverlay])
            }

            KeyCode::Delete => Ok(vec![Action::RemoveFromClipboard(0)]),

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

                ClipboardMode::OverlayActive => matches!(
                    key_event.code,
                    KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Enter
                        | KeyCode::Tab
                        | KeyCode::Esc
                        | KeyCode::Delete
                        | KeyCode::Char('k' | 'j' | 'C')
                ),
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
            ClipboardMode::Normal => 40,

            ClipboardMode::OverlayActive => 1,
        }
    }

    fn name(&self) -> &'static str {
        "ClipboardHandler"
    }
}
