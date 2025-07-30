use crate::controller::actions::{Action, InputPromptType};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::trace;

use super::*;

pub struct FileOpsHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: FileOpsMode,
    input_buffer: String,
    sequence_buffer: Vec<KeyEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpsMode {
    Normal,
    InputMode,
}

impl Default for FileOpsHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl FileOpsHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(15);

        // File operations
        bindings.insert(key('n'), Action::CreateFile);
        bindings.insert(key('m'), Action::CreateDirectory);
        bindings.insert(key('d'), Action::Delete);
        bindings.insert(key('r'), Action::ShowInputPrompt(InputPromptType::Rename));
        bindings.insert(key('.'), Action::ToggleShowHidden);

        // Function keys
        bindings.insert(
            function_key(2),
            Action::ShowInputPrompt(InputPromptType::Rename),
        );
        bindings.insert(function_key(7), Action::CreateDirectory);
        bindings.insert(function_key(8), Action::Delete);

        // Ctrl combinations
        bindings.insert(ctrl('n'), Action::CreateFile);
        bindings.insert(ctrl('d'), Action::Delete);

        Self {
            bindings,
            mode: FileOpsMode::Normal,
            input_buffer: String::with_capacity(128),
            sequence_buffer: Vec::with_capacity(3),
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            marker = "FILE_OPS_HANDLER_KEY_EVENT",
            operation_type = "input_handling",
            "FileOpsHandler: key {:?} mode {:?}",
            key_event,
            self.mode
        );

        match self.mode {
            FileOpsMode::Normal => self.handle_normal_mode(key_event),
            FileOpsMode::InputMode => self.handle_input_mode(key_event),
        }
    }

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check sequences
        if let Some(action) = self.check_sequences(key_event) {
            return Ok(vec![action]);
        }

        if let Some(action) = self.bindings.get(&key_event).cloned() {
            // Switch to input mode for interactive operations
            if matches!(
                action,
                Action::CreateFile | Action::CreateDirectory | Action::ShowInputPrompt(_)
            ) {
                self.mode = FileOpsMode::InputMode;
                self.input_buffer.clear();
            }
            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_input_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if !self.input_buffer.trim().is_empty() {
                    let input = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    Ok(vec![Action::SubmitInputPrompt(input)])
                } else {
                    Ok(vec![])
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            KeyCode::Char(c) => {
                if is_valid_filename_char(c) {
                    self.input_buffer.push(c);
                    Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
                } else {
                    Ok(vec![])
                }
            }
            _ => Ok(vec![]),
        }
    }

    fn check_sequences(&mut self, key_event: KeyEvent) -> Option<Action> {
        if matches!(key_event.code, KeyCode::Char('d')) {
            self.sequence_buffer.push(key_event);
        }

        let action = match self.sequence_buffer.as_slice() {
            [
                KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                },
                KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                },
            ] => Some(Action::Delete),
            _ => None,
        };

        if action.is_some() || self.sequence_buffer.len() >= 2 {
            self.sequence_buffer.clear();
        }

        action
    }
}

impl EventHandler for FileOpsHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            match self.mode {
                FileOpsMode::Normal => {
                    matches!(
                        key_event.code,
                        KeyCode::Char('n' | 'm' | 'd' | 'r' | '.' | 'C' | 'X')
                            | KeyCode::F(2 | 7 | 8)
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('n' | 'd')))
                }
                FileOpsMode::InputMode => true,
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
            FileOpsMode::Normal => 50,
            FileOpsMode::InputMode => 3,
        }
    }

    fn name(&self) -> &'static str {
        "FileOpsHandler"
    }
}
