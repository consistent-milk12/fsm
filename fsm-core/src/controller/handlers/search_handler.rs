// fsm-core/src/controller/handlers/search_handler.rs

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

/// Search operations handler for file and content search
pub struct SearchHandler {
    /// Search key bindings
    bindings: HashMap<KeyEvent, Action>,

    /// Current search mode
    mode: SearchMode,

    /// Search input buffer for building queries
    input_buffer: String,
}

/// Search operation modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Normal,
    FileNameSearch, // After '/' pressed
    ContentSearch,  // After Ctrl+F pressed
    CommandMode,    // After ':' pressed
}

impl Default for SearchHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchHandler {
    /// Create new search handler
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(10);

        // Search mode triggers
        bindings.insert(key('/'), Action::ToggleFileNameSearch);
        bindings.insert(ctrl('f'), Action::ToggleContentSearch);
        bindings.insert(key(':'), Action::EnterCommandMode);
        bindings.insert(key('?'), Action::ToggleHelp);

        // Quick search patterns (normal mode)
        // Search for word under cursor
        bindings.insert(key('*'), Action::DirectContentSearch("".to_string()));

        // Reverse search
        bindings.insert(key('#'), Action::DirectContentSearch("".to_string()));

        // Search navigation in results
        bindings.insert(key('n'), Action::NoOp); // Next search result  (placeholder)
        bindings.insert(key('N'), Action::NoOp); // Previous search result (placeholder)

        Self {
            bindings,
            mode: SearchMode::Normal,
            input_buffer: String::with_capacity(64),
        }
    }

    /// Handle search-related key events
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

    /// Handle keys in normal search mode
    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check bindings for search triggers
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("SearchHandler: matched key to action {:?}", action);

            // Update mode based on action
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

    /// Handle keys during file name search
    fn handle_filename_search_mode(
        &mut self,
        key_event: KeyEvent,
    ) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("SearchHandler: cancelled filename search");
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                debug!(
                    "SearchHandler: submitting filename search: '{}'",
                    self.input_buffer
                );
                let query = self.input_buffer.clone();
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::FileNameSearch(query)])
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            _ => Ok(vec![Action::Key(key_event)]), // Forward other keys
        }
    }

    /// Handle keys during content search
    fn handle_content_search_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("SearchHandler: cancelled content search");
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                debug!(
                    "SearchHandler: submitting content search: '{}'",
                    self.input_buffer
                );
                let query = self.input_buffer.clone();
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::ContentSearch(query)])
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            _ => Ok(vec![Action::Key(key_event)]), // Forward other keys
        }
    }

    /// Handle keys during command mode
    fn handle_command_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("SearchHandler: exiting command mode");
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();
                Ok(vec![Action::ExitCommandMode])
            }
            KeyCode::Enter => {
                debug!("SearchHandler: submitting command: '{}'", self.input_buffer);
                let command = self.input_buffer.clone();
                self.mode = SearchMode::Normal;
                self.input_buffer.clear();

                // Parse and execute command
                self.parse_command(command)
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            _ => Ok(vec![Action::Key(key_event)]), // Forward other keys
        }
    }

    /// Parse and execute command mode commands
    fn parse_command(&self, command: String) -> Result<Vec<Action>, AppError> {
        let cmd = command.trim();
        debug!("SearchHandler: parsing command '{}'", cmd);

        match cmd {
            "q" | "quit" => Ok(vec![Action::Quit]),
            "w" | "write" => Ok(vec![Action::ReloadDirectory]),
            "wq" => Ok(vec![Action::ReloadDirectory, Action::Quit]),
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
            _ => {
                debug!("SearchHandler: unknown command '{}'", cmd);
                Ok(vec![Action::NoOp])
            }
        }
    }

    /// Reset search handler state
    pub fn reset(&mut self) {
        self.mode = SearchMode::Normal;
        self.input_buffer.clear();
    }

    /// Get current search mode
    pub fn current_mode(&self) -> SearchMode {
        self.mode
    }

    /// Get current input buffer
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
            // Handle search-related keys based on current mode
            match self.mode {
                SearchMode::Normal => {
                    matches!(
                        key_event.code,
                        KeyCode::Char('/' | ':' | '?' | '*' | '#' | 'n' | 'N')
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('f')))
                }
                SearchMode::FileNameSearch
                | SearchMode::ContentSearch
                | SearchMode::CommandMode => {
                    // In search modes, handle all keys for input
                    true
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
            SearchMode::Normal => 100, // Medium priority in normal mode
            _ => 5,                    // High priority when in active search mode
        }
    }

    fn name(&self) -> &'static str {
        "SearchHandler"
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
