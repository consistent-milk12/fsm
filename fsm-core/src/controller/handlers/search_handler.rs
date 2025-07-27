// fsm-core/src/controller/handlers/search_handler.rs

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

/// Search handler matching your Action enum
pub struct SearchHandler {
    bindings: HashMap<KeyEvent, Action>,
    mode: SearchMode,
    input_buffer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Normal,
    FileNameSearch,
    ContentSearch,
    CommandMode,
}

impl Default for SearchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(15);

        // Search triggers
        bindings.insert(key('/'), Action::ToggleFileNameSearch);
        bindings.insert(ctrl('f'), Action::ToggleContentSearch);
        bindings.insert(key(':'), Action::EnterCommandMode);
        bindings.insert(key('?'), Action::ToggleHelp);

        // Search navigation
        bindings.insert(key('n'), Action::SelectSearchResult(0)); // Will be updated with actual index
        bindings.insert(key('N'), Action::SelectSearchResult(0)); // Previous result

        // Quick search
        bindings.insert(key('*'), Action::DirectContentSearch("".to_string()));
        bindings.insert(key('#'), Action::DirectContentSearch("".to_string()));

        Self {
            bindings,
            mode: SearchMode::Normal,
            input_buffer: String::with_capacity(64),
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            "SearchHandler: processing key {:?} in mode {:?}",
            key_event, self.mode
        );

        match self.mode {
            SearchMode::Normal => self.handle_normal_mode(key_event),
            SearchMode::FileNameSearch => self.handle_filename_search_mode(key_event),
            SearchMode::ContentSearch => self.handle_content_search_mode(key_event),
            SearchMode::CommandMode => self.handle_command_mode(key_event),
        }
    }

    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("SearchHandler: matched key to action {:?}", action);

            match &action {
                Action::ToggleFileNameSearch => {
                    self.mode = SearchMode::FileNameSearch;
                    self.input_buffer.clear();
                }
                Action::ToggleContentSearch => {
                    self.mode = SearchMode::ContentSearch;
                    self.input_buffer.clear();
                }
                Action::EnterCommandMode => {
                    self.mode = SearchMode::CommandMode;
                    self.input_buffer.clear();
                }
                _ => {}
            }

            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn handle_filename_search_mode(
        &mut self,
        key_event: KeyEvent,
    ) -> Result<Vec<Action>, AppError> {
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

    fn handle_content_search_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
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
                Ok(vec![Action::ContentSearch(query)])
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

    fn handle_command_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
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
                self.parse_command(command)
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

    fn parse_command(&self, command: String) -> Result<Vec<Action>, AppError> {
        let cmd = command.trim();

        match cmd {
            "q" | "quit" => Ok(vec![Action::Quit]),
            "reload" | "r" => Ok(vec![Action::ReloadDirectory]),
            cmd if cmd.starts_with("cd ") => {
                let path = cmd.strip_prefix("cd ").unwrap_or("");
                Ok(vec![Action::GoToPath(path.to_string())])
            }
            cmd if cmd.starts_with("find ") => {
                let pattern = cmd.strip_prefix("find ").unwrap_or("");
                Ok(vec![Action::FileNameSearch(pattern.to_string())])
            }
            cmd if cmd.starts_with("grep ") => {
                let pattern = cmd.strip_prefix("grep ").unwrap_or("");
                Ok(vec![Action::ContentSearch(pattern.to_string())])
            }
            _ => Ok(vec![Action::NoOp]),
        }
    }

    pub fn current_mode(&self) -> SearchMode {
        self.mode
    }

    pub fn input_buffer(&self) -> &str {
        &self.input_buffer
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
                    matches!(
                        key_event.code,
                        KeyCode::Char('/' | ':' | '?' | '*' | '#' | 'n' | 'N')
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('f')))
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
            SearchMode::Normal => 100,
            _ => 5, // High priority in input modes
        }
    }

    fn name(&self) -> &'static str {
        "SearchHandler"
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}
