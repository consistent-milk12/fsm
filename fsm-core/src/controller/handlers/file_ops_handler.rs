// fsm-core/src/controller/handlers/file_ops_handler.rs

use crate::controller::{
    actions::{Action, InputPromptType},
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, trace};

pub struct FileOpsHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: FileOpsMode,
    input_buffer: String,
    sequence_buffer: Vec<KeyEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpsMode {
    Normal,
    CreatingFile,
    CreatingDirectory,
    Renaming,
    AwaitingDestination,
}

impl Default for FileOpsHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl FileOpsHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(20);

        // File creation
        bindings.insert(key('n'), Action::CreateFile);
        bindings.insert(key('N'), Action::CreateFile);
        bindings.insert(key('m'), Action::CreateDirectory);
        bindings.insert(key('M'), Action::CreateDirectory);

        // File modification
        bindings.insert(key('d'), Action::Delete);
        bindings.insert(key('D'), Action::Delete);
        bindings.insert(key('r'), Action::ShowInputPrompt(InputPromptType::Rename));
        bindings.insert(key('R'), Action::ShowInputPrompt(InputPromptType::Rename));

        // Copy/Move (avoiding clipboard conflicts)
        bindings.insert(
            key('C'),
            Action::StartCopy {
                source: PathBuf::new(),
            },
        );
        bindings.insert(
            key('X'),
            Action::StartMove {
                source: PathBuf::new(),
            },
        );

        // Hidden files toggle
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
            "FileOpsHandler: processing key {:?} in mode {:?}",
            key_event, self.mode
        );

        match self.mode {
            FileOpsMode::Normal => self.handle_normal_mode(key_event),
            FileOpsMode::CreatingFile => self.handle_file_creation_mode(key_event),
            FileOpsMode::CreatingDirectory => self.handle_directory_creation_mode(key_event),
            FileOpsMode::Renaming => self.handle_rename_mode(key_event),
            FileOpsMode::AwaitingDestination => self.handle_destination_mode(key_event),
        }
    }

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check sequences first
        if let Some(action) = self.check_sequences(key_event) {
            return Ok(vec![action]);
        }

        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("FileOpsHandler: matched key to action {:?}", action);

            // Update mode based on action
            match &action {
                Action::CreateFile => {
                    self.mode = FileOpsMode::CreatingFile;
                    self.input_buffer.clear();
                }
                Action::CreateDirectory => {
                    self.mode = FileOpsMode::CreatingDirectory;
                    self.input_buffer.clear();
                }
                Action::ShowInputPrompt(InputPromptType::Rename) => {
                    self.mode = FileOpsMode::Renaming;
                    self.input_buffer.clear();
                }
                Action::StartCopy { .. } | Action::StartMove { .. } => {
                    self.mode = FileOpsMode::AwaitingDestination;
                    self.input_buffer.clear();
                }
                _ => {}
            }

            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_file_creation_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    Ok(vec![])
                } else {
                    let filename = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    Ok(vec![Action::CreateFileWithName(filename)])
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

    fn handle_directory_creation_mode(
        &mut self,
        key_event: KeyEvent,
    ) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    Ok(vec![])
                } else {
                    let dirname = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    Ok(vec![Action::CreateDirectoryWithName(dirname)])
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

    fn handle_rename_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    Ok(vec![])
                } else {
                    let new_name = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    Ok(vec![Action::RenameEntry(new_name)])
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

    fn handle_destination_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    Ok(vec![])
                } else {
                    let destination = PathBuf::from(self.input_buffer.clone());
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();

                    // Return action to show destination prompt submission
                    Ok(vec![Action::SubmitInputPrompt(
                        destination.to_string_lossy().to_string(),
                    )])
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            KeyCode::Char(c) => {
                if is_valid_path_char(c) {
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
        if matches!(key_event.code, KeyCode::Char('d' | 'g')) {
            self.sequence_buffer.push(key_event);
        }

        let action = match self.sequence_buffer.as_slice() {
            // 'dd' - delete
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

    pub fn current_mode(&self) -> FileOpsMode {
        self.mode
    }

    pub fn input_buffer(&self) -> &str {
        &self.input_buffer
    }

    pub fn is_input_mode(&self) -> bool {
        !matches!(self.mode, FileOpsMode::Normal)
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
                        KeyCode::Char(
                            'n' | 'N' | 'm' | 'M' | 'd' | 'D' | 'r' | 'R' | '.' | 'C' | 'X'
                        ) | KeyCode::F(2 | 7 | 8)
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('n' | 'd')))
                }
                _ => true, // Handle all keys in input modes
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
            _ => 3, // High priority in input mode
        }
    }

    fn name(&self) -> &'static str {
        "FileOpsHandler"
    }
}

// Helper functions
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn function_key(n: u8) -> KeyEvent {
    KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn is_valid_filename_char(c: char) -> bool {
    !matches!(
        c,
        '\0' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    )
}

fn is_valid_path_char(c: char) -> bool {
    !matches!(c, '\0' | '*' | '?' | '"' | '<' | '>' | '|')
}
