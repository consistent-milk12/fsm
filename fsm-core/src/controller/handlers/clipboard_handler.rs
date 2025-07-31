use crate::controller::actions::Action;
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::trace;

use super::*;

use crate::controller::state_provider::StateProvider;
use std::sync::Arc;

pub struct ClipboardHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: ClipboardMode,
    state_provider: Arc<dyn StateProvider>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Normal,
    OverlayActive,
}

impl ClipboardHandler {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        let mut bindings = HashMap::with_capacity(8);

        // Core operations - path will be determined at runtime
        bindings.insert(key('c'), Action::Copy(PathBuf::from("placeholder")));
        bindings.insert(key('x'), Action::Cut(PathBuf::from("placeholder")));
        bindings.insert(key('v'), Action::Paste);
        bindings.insert(tab_key(), Action::ToggleClipboard);

        // Ctrl alternatives - path will be determined at runtime
        bindings.insert(ctrl('c'), Action::Copy(PathBuf::from("placeholder")));
        bindings.insert(ctrl('x'), Action::Cut(PathBuf::from("placeholder")));
        bindings.insert(ctrl('v'), Action::Paste);

        Self {
            bindings,
            mode: ClipboardMode::Normal,
            state_provider,
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
            match action {
                Action::Copy(_) | Action::Cut(_) => {
                    let fs_state = self.state_provider.fs_state();
                    if let Some(path) = fs_state.get_selected_path() {
                        let new_action = match action {
                            Action::Copy(_) => Action::Copy(path),
                            Action::Cut(_) => Action::Cut(path),
                            _ => unreachable!(), // Should not happen due to outer match
                        };

                        Ok(vec![new_action])
                    } else {
                        // No selected entry, do not perform copy/cut
                        Ok(vec![])
                    }
                }

                Action::ToggleClipboard => {
                    self.mode = ClipboardMode::OverlayActive;

                    Ok(vec![action])
                }

                _ => Ok(vec![action]),
            }
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
