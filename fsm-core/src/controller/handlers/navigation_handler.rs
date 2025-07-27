// fsm-core/src/controller/handlers/navigation_handler.rs
// Fixed to work with your FSState atomic operations

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

pub struct NavigationHandler {
    bindings: HashMap<KeyEvent, Action>,
    sequence_buffer: Vec<KeyEvent>,
}

impl Default for NavigationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(25);

        // Arrow keys - work with FSState atomic operations
        bindings.insert(arrow_key(KeyCode::Up), Action::MoveSelectionUp);
        bindings.insert(arrow_key(KeyCode::Down), Action::MoveSelectionDown);
        bindings.insert(arrow_key(KeyCode::Left), Action::GoToParent);
        bindings.insert(arrow_key(KeyCode::Right), Action::EnterSelected);

        // Page navigation - compatible with PaneState methods
        bindings.insert(arrow_key(KeyCode::PageUp), Action::PageUp);
        bindings.insert(arrow_key(KeyCode::PageDown), Action::PageDown);
        bindings.insert(arrow_key(KeyCode::Home), Action::SelectFirst);
        bindings.insert(arrow_key(KeyCode::End), Action::SelectLast);

        // Vim keys
        bindings.insert(key('k'), Action::MoveSelectionUp);
        bindings.insert(key('j'), Action::MoveSelectionDown);
        bindings.insert(key('h'), Action::GoToParent);
        bindings.insert(key('l'), Action::EnterSelected);
        bindings.insert(key('G'), Action::SelectLast);

        // Entry actions
        bindings.insert(enter_key(), Action::EnterSelected);
        bindings.insert(backspace_key(), Action::GoToParent);

        // Ctrl combinations
        bindings.insert(ctrl('u'), Action::PageUp);
        bindings.insert(ctrl('d'), Action::PageDown);
        bindings.insert(ctrl('b'), Action::PageUp);
        bindings.insert(ctrl('f'), Action::PageDown);

        Self {
            bindings,
            sequence_buffer: Vec::with_capacity(3),
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!("NavigationHandler: processing key {key_event:?}");

        // Check sequences
        if let Some(action) = self.check_sequences(key_event) {
            debug!("NavigationHandler: sequence matched: {action:?}");
            return Ok(vec![action]);
        }

        // Direct lookup
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("NavigationHandler: matched key to action {action:?}");
            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    fn check_sequences(&mut self, key_event: KeyEvent) -> Option<Action> {
        if matches!(key_event.code, KeyCode::Char('g')) {
            self.sequence_buffer.push(key_event);
        }

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
            _ => None,
        };

        if action.is_some() || self.sequence_buffer.len() >= 2 {
            self.sequence_buffer.clear();
        }

        action
    }
}

impl EventHandler for NavigationHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            matches!(
                key_event.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Enter
                    | KeyCode::Backspace
                    | KeyCode::Char('k' | 'j' | 'h' | 'l' | 'g' | 'G')
            ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key_event.code, KeyCode::Char('u' | 'd' | 'b' | 'f')))
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
        10
    }

    fn name(&self) -> &'static str {
        "NavigationHandler"
    }
}

// Helper functions
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn arrow_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn enter_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

fn backspace_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
}
