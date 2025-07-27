// fsm-core/src/controller/handlers/navigation_handler.rs

use crate::controller::{
    actions::Action,
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use tracing::{debug, trace};

/// Navigation handler matching your Action enum
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
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            Action::GoToParent,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            Action::EnterSelected,
        );

        // Page navigation
        bindings.insert(
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
            Action::PageUp,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
            Action::PageDown,
        );

        // Home/End
        bindings.insert(
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            Action::SelectFirst,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
            Action::SelectLast,
        );

        // Vim navigation
        bindings.insert(key('k'), Action::MoveSelectionUp);
        bindings.insert(key('j'), Action::MoveSelectionDown);
        bindings.insert(key('h'), Action::GoToParent);
        bindings.insert(key('l'), Action::EnterSelected);
        bindings.insert(key('G'), Action::SelectLast);

        // Enter and Backspace
        bindings.insert(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::EnterSelected,
        );
        bindings.insert(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            Action::GoToParent,
        );

        // Ctrl combinations
        bindings.insert(ctrl('u'), Action::PageUp);
        bindings.insert(ctrl('d'), Action::PageDown);
        bindings.insert(ctrl('b'), Action::PageUp);
        bindings.insert(ctrl('f'), Action::PageDown);

        // Navigation history
        bindings.insert(alt('h'), Action::NavigateBack);
        bindings.insert(alt('l'), Action::NavigateForward);

        // Bookmarks
        bindings.insert(key('b'), Action::BookmarkDirectory);
        bindings.insert(key('B'), Action::ShowBookmarks);

        Self {
            bindings,
            sequence_buffer: Vec::with_capacity(3),
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!("NavigationHandler: processing key {key_event:?}");

        // Check sequences first
        if let Some(action) = self.check_sequences(key_event) {
            debug!("NavigationHandler: sequence matched: {action:?}");
            return Ok(vec![action]);
        }

        // Direct binding lookup
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
            ] => {
                debug!("NavigationHandler: 'gg' sequence - goto first");
                Some(Action::SelectFirst)
            }
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
                    | KeyCode::Char('k' | 'j' | 'h' | 'l' | 'g' | 'G' | 'b' | 'B')
            ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key_event.code, KeyCode::Char('u' | 'd' | 'b' | 'f')))
                || (key_event.modifiers.contains(KeyModifiers::ALT)
                    && matches!(key_event.code, KeyCode::Char('h' | 'l')))
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
        10 // High priority for responsive navigation
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

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}
