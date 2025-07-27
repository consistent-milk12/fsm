use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

/// Performance optimized navigation handler for movement operations
pub struct NavigationHandler {
    /// Navigation key bindings
    bindings: HashMap<KeyEvent, Action>,

    /// Multi-key sequence buffer (e.g. 'gg' for goto top)
    sequence_buffer: Vec<KeyEvent>,
}

impl Default for NavigationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationHandler {
    pub fn new() -> Self {
        let mut bindings: HashMap<KeyEvent, Action> = HashMap::with_capacity(15);

        // Arrow keys
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

        // Vim-style navigation
        bindings.insert(key('k'), Action::MoveSelectionUp);
        bindings.insert(key('j'), Action::MoveSelectionDown);
        bindings.insert(key('h'), Action::GoToParent);
        bindings.insert(key('l'), Action::EnterSelected);
        bindings.insert(key('g'), Action::SelectFirst);
        bindings.insert(key('G'), Action::SelectLast);

        // Enhanced navigation
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

        // Check for multi-key sequences first
        if let Some(action) = self.check_sequences(key_event) {
            return Ok(vec![action]);
        }

        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("NavigationHandler: matched key to action {action:?}");

            Ok(vec![action])
        } else {
            // Not a navigation key
            Ok(vec![])
        }
    }

    fn check_sequences(&mut self, key_event: KeyEvent) -> Option<Action> {
        // Add key to buffer for sequence detection
        if matches!(key_event.code, KeyCode::Char('g' | 'd')) {
            self.sequence_buffer.push(key_event);
        }

        // Match against known sequences
        let action: Option<Action> = match self.sequence_buffer.as_slice() {
            // 'gg' - goto top
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

        // Clear buffer if sequence matcher or buffer full
        if action.is_some() || self.sequence_buffer.len() >= 2 {
            self.sequence_buffer.clear();
        }

        action
    }
}

impl EventHandler for NavigationHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event,
            priority: _,
        } = event
        {
            // Check if this is a navigation key
            matches!(
                key_event.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Char('k' | 'j' | 'h' | 'l' | 'g' | 'G')
            ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key_event.code, KeyCode::Char('u' | 'f')))
        } else {
            false
        }
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event,
            priority: _,
        } = event
        {
            self.handle_key(key_event)
        } else {
            Ok(vec![])
        }
    }

    fn priority(&self) -> u8 {
        // High priority for responsive navigation
        10
    }

    fn name(&self) -> &'static str {
        "NavigationHandler"
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}
