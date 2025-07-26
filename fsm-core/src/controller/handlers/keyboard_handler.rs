// src/controller/keyboard_handler.rs

use crate::controller::{
    actions::{Action, InputPromptType},
    event_processor::{Event, EventHandler, Priority},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{collections::HashMap, path::PathBuf};
use tracing::{debug, trace};

/// High-performance keyboard input handler with optimized key mapping
pub struct KeyboardHandler {
    /// Primary key bindings map
    bindings: HashMap<KeyEvent, Action>,

    /// Command mode bindings (activated after ':')
    command_bindings: HashMap<KeyEvent, Action>,

    /// Current input mode
    mode: InputMode,

    /// Partial key sequence buffer
    sequence_buffer: Vec<KeyEvent>,
}

/// Input mode tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Command,
    Search,
}

impl KeyboardHandler {
    /// Create a new keyboard handler with optimized default bindings
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(50);
        let mut command_bindings = HashMap::with_capacity(20);

        // Navigation
        bindings.insert(key('k'), Action::MoveSelectionUp);
        bindings.insert(key('j'), Action::MoveSelectionDown);
        bindings.insert(key('g'), Action::SelectFirst);
        bindings.insert(key('G'), Action::SelectLast);
        bindings.insert(
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            Action::MoveSelectionUp,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            Action::MoveSelectionDown,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
            Action::PageUp,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
            Action::PageDown,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            Action::SelectFirst,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            Action::SelectLast,
        );

        // Operations
        bindings.insert(key('h'), Action::GoToParent);
        bindings.insert(key('l'), Action::EnterSelected);
        bindings.insert(key('d'), Action::Delete);
        bindings.insert(key('n'), Action::CreateFile);
        bindings.insert(key('m'), Action::CreateDirectory);
        bindings.insert(key('r'), Action::ShowInputPrompt(InputPromptType::Rename));
        bindings.insert(
            key('y'),
            Action::StartCopy {
                source: PathBuf::new(),
            },
        ); // Updated
        bindings.insert(
            key('x'),
            Action::StartMove {
                source: PathBuf::new(),
            },
        ); // Updated
        bindings.insert(key('.'), Action::ToggleShowHidden);
        bindings.insert(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::EnterSelected,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            Action::GoToParent,
        );

        // Mode switching
        bindings.insert(key(':'), Action::EnterCommandMode);
        bindings.insert(key('/'), Action::ToggleFileNameSearch);
        bindings.insert(ctrl('f'), Action::ToggleContentSearch);
        bindings.insert(key('?'), Action::ToggleHelp);
        bindings.insert(ctrl('c'), Action::Quit);
        bindings.insert(key('q'), Action::Quit);

        // Command mode bindings
        command_bindings.insert(key('q'), Action::Quit);
        command_bindings.insert(key('w'), Action::ReloadDirectory);
        command_bindings.insert(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            Action::ExitCommandMode,
        );
        command_bindings.insert(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::SubmitInputPrompt("".to_string()),
        );

        Self {
            bindings,
            command_bindings,
            mode: InputMode::Normal,
            sequence_buffer: Vec::with_capacity(3),
        }
    }

    /// Update input mode based on application state changes
    pub fn set_input_mode(&mut self, mode: InputMode) {
        self.mode = mode;
        self.sequence_buffer.clear();
    }

    /// Handle key event and convert to action
    fn handle_key(&mut self, event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!("Handling key event: {:?} in mode: {:?}", event, self.mode);

        match self.mode {
            InputMode::Command => self.handle_command_mode(event),
            InputMode::Search => self.handle_search_mode(event),
            InputMode::Normal => self.handle_normal_mode(event),
        }
    }

    /// Process keys in normal mode
    fn handle_normal_mode(&mut self, event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check for multi-key sequences
        if let Some(seq_action) = self.check_sequences(event) {
            return Ok(vec![seq_action]);
        }

        // Direct binding lookup
        if let Some(action) = self.bindings.get(&event).cloned() {
            Ok(vec![action])
        } else {
            // Unbound keys produce no action
            debug!("Unbound key in normal mode: {:?}", event);
            Ok(vec![])
        }
    }

    /// Process keys in command mode - FIXED ERROR
    fn handle_command_mode(&mut self, event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.command_bindings.get(&event).cloned() {
            Ok(vec![action])
        } else if let KeyCode::Char(c) = event.code {
            // Build command string directly in handler
            let mut actions = Vec::with_capacity(1);
            if c == '\n' || c == '\r' {
                actions.push(Action::SubmitInputPrompt("".to_string()));
            } else {
                actions.push(Action::Key(event));
            }
            Ok(actions)
        } else {
            debug!("Unbound key in command mode: {:?}", event);
            Ok(vec![])
        }
    }

    /// Process keys in search mode
    fn handle_search_mode(&mut self, event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match event.code {
            KeyCode::Esc => Ok(vec![Action::CloseOverlay]),
            KeyCode::Enter => Ok(vec![Action::SubmitInputPrompt("".to_string())]),
            KeyCode::Char(c) => {
                // Build search string directly
                Ok(vec![Action::Key(event)])
            }
            _ => Ok(vec![]),
        }
    }

    /// Check for multi-key sequences (e.g., 'gg', 'dd')
    fn check_sequences(&mut self, event: KeyEvent) -> Option<Action> {
        self.sequence_buffer.push(event);

        // Match against known sequences
        let action = match self.sequence_buffer.as_slice() {
            [
                KeyEvent {
                    code: KeyCode::Char('g'),
                    ..
                },
                KeyEvent {
                    code: KeyCode::Char('g'),
                    ..
                },
            ] => Some(Action::SelectFirst),

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

        // Reset buffer if sequence matched or buffer full
        if action.is_some() || self.sequence_buffer.len() >= 3 {
            self.sequence_buffer.clear();
        }

        action
    }
}

impl EventHandler for KeyboardHandler {
    fn can_handle(&self, event: &Event) -> bool {
        matches!(event, Event::Key { .. })
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            self.handle_key(key_event)
        } else {
            // Should never happen due to can_handle
            Ok(vec![])
        }
    }

    fn priority(&self) -> u8 {
        10 // High priority for keyboard events
    }

    fn name(&self) -> &'static str {
        "KeyboardHandler"
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
