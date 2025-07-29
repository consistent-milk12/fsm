// fsm-core/src/controller/handlers/search_handler.rs
// Search and command mode handler

use crate::controller::actions::Action;
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::trace;

use super::*;

pub struct SearchHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: SearchMode,
    input_buffer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Normal,
    SearchInput,
    CommandInput,
}

impl Default for SearchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(8);

        // Search triggers
        bindings.insert(key('/'), Action::ToggleFileNameSearch);
        bindings.insert(ctrl('f'), Action::ToggleContentSearch);
        bindings.insert(key(':'), Action::EnterCommandMode);
        bindings.insert(key('?'), Action::ToggleHelp);

        // Search navigation
        bindings.insert(key('n'), Action::NextSearchResult);
        bindings.insert(key('N'), Action::PreviousSearchResult);

        Self {
            bindings,
            mode: SearchMode::Normal,
            input_buffer: String::with_capacity(64),
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            marker = "SEARCH_HANDLER_KEY_EVENT",
            operation_type = "input_handling",
            "SearchHandler: key {:?} mode {:?}",
            key_event,
            self.mode
        );

        match self.mode {
            SearchMode::Normal => self.handle_normal_mode(key_event),
            SearchMode::SearchInput => self.handle_search_input(key_event),
            SearchMode::CommandInput => self.handle_command_input(key_event),
        }
    }

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            match &action {
                Action::ToggleFileNameSearch | Action::ToggleContentSearch => {
                    self.mode = SearchMode::SearchInput;
                    self.input_buffer.clear();
                }
                Action::EnterCommandMode => {
                    self.mode = SearchMode::CommandInput;
                    self.input_buffer.clear();
                }
                _ => {}
            }
            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_search_input(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                let query = self.input_buffer.clone();
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::FileNameSearch(query)])
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            _ => Ok(vec![]),
        }
    }

    fn handle_command_input(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::ExitCommandMode])
            }
            KeyCode::Enter => {
                let command = self.input_buffer.clone();
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::SubmitInputPrompt(command)])
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                Ok(vec![Action::UpdateInput(self.input_buffer.clone())])
            }
            _ => Ok(vec![]),
        }
    }
}

impl EventHandler for SearchHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            match self.mode {
                SearchMode::Normal => {
                    matches!(key_event.code, KeyCode::Char('/' | ':' | '?' | 'n' | 'N'))
                        || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(key_event.code, KeyCode::Char('f')))
                }
                _ => true,
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
            SearchMode::Normal => 100,
            _ => 5,
        }
    }

    fn name(&self) -> &'static str {
        "SearchHandler"
    }
}
