// fsm-core/src/controller/handlers/keyboard_handler.rs
// Fallback handler with emergency bindings

use super::*;
use crate::controller::actions::{Action, InputPromptType};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, warn};

pub struct KeyboardHandler {
    app_bindings: HashMap<KeyEvent, Action>,
    emergency_bindings: HashMap<KeyEvent, Action>,
    mode: KeyboardMode,
    unhandled_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardMode {
    Fallback,
    Debug,
}

impl Default for KeyboardHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardHandler {
    pub fn new() -> Self {
        let mut app_bindings = HashMap::with_capacity(10);
        let mut emergency_bindings = HashMap::with_capacity(3);

        // Application bindings
        app_bindings.insert(key('q'), Action::Quit);
        app_bindings.insert(ctrl('c'), Action::Quit);
        app_bindings.insert(escape_key(), Action::CloseOverlay);
        app_bindings.insert(function_key(1), Action::ToggleHelp);
        app_bindings.insert(function_key(5), Action::ReloadDirectory);
        app_bindings.insert(function_key(10), Action::Quit);
        app_bindings.insert(ctrl('l'), Action::ReloadDirectory);

        // Emergency bindings
        emergency_bindings.insert(ctrl_alt('q'), Action::Quit);
        emergency_bindings.insert(function_key(12), Action::Quit);

        Self {
            app_bindings,
            emergency_bindings,
            mode: KeyboardMode::Fallback,
            unhandled_count: 0,
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Emergency keys first
        if let Some(action) = self.emergency_bindings.get(&key_event).cloned() {
            return Ok(vec![action]);
        }

        // Mode switch
        if key_event == ctrl_alt('d') {
            self.mode = match self.mode {
                KeyboardMode::Fallback => KeyboardMode::Debug,
                KeyboardMode::Debug => KeyboardMode::Fallback,
            };
            return Ok(vec![]);
        }

        match self.mode {
            KeyboardMode::Fallback => self.handle_fallback(key_event),
            KeyboardMode::Debug => self.handle_debug(key_event),
        }
    }

    fn handle_fallback(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        if let Some(action) = self.app_bindings.get(&key_event).cloned() {
            Ok(vec![action])
        } else {
            match key_event.code {
                KeyCode::F(n) => self.handle_function_key(n),
                _ => {
                    self.unhandled_count += 1;
                    if self.unhandled_count % 50 == 0 {
                        warn!("KeyboardHandler: {} unhandled keys", self.unhandled_count);
                    }
                    Ok(vec![])
                }
            }
        }
    }

    fn handle_debug(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        debug!(
            marker = "KEYBOARD_DEBUG_MODE",
            operation_type = "input_handling",
            "KeyboardHandler: DEBUG - {:?}",
            key_event
        );

        if let Some(action) = self.emergency_bindings.get(&key_event).cloned() {
            Ok(vec![action])
        } else {
            Ok(vec![Action::Key(key_event)])
        }
    }

    fn handle_function_key(&self, n: u8) -> Result<Vec<Action>, AppError> {
        let action = match n {
            1 => Action::ToggleHelp,
            2 => Action::ShowInputPrompt(InputPromptType::Rename),
            3 => Action::ToggleFileNameSearch,
            5 => Action::ReloadDirectory,
            7 => Action::CreateDirectory,
            8 => Action::Delete,
            10 | 12 => Action::Quit,
            _ => return Ok(vec![]),
        };
        Ok(vec![action])
    }
}

impl EventHandler for KeyboardHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            self.emergency_bindings.contains_key(key_event)
                || matches!(self.mode, KeyboardMode::Debug)
                || self.app_bindings.contains_key(key_event)
                || matches!(key_event.code, KeyCode::F(_) | KeyCode::Esc)
                || key_event.modifiers.contains(KeyModifiers::ALT)
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
            KeyboardMode::Debug => 1,
            KeyboardMode::Fallback => 255,
        }
    }

    fn name(&self) -> &'static str {
        "KeyboardHandler"
    }
}
