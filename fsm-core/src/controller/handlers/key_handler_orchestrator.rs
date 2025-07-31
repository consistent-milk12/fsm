// fsm-core/src/controller/handlers/key_handler_orchestrator.rs
// Orchestrates key event handling through chained handlers

use std::sync::Arc;

use super::*;
use crate::controller::actions::Action;
use crate::error::AppError;
use tracing::{debug, trace, warn};

use super::{
    clipboard_handler::ClipboardHandler, file_ops_handler::FileOpsHandler,
    keyboard_handler::KeyboardHandler, navigation_handler::NavigationHandler,
    search_handler::SearchHandler,
};

/// Orchestrates key event handling by chaining specialized handlers
///
/// Implements clean architecture by routing all key events through
/// appropriate handlers in priority order, eliminating manual key
/// processing in the EventLoop.
pub struct KeyHandlerOrchestrator {
    handlers: Vec<Box<dyn EventHandler>>,
    total_events_processed: u64,
    unhandled_events: u64,
}

impl std::fmt::Debug for KeyHandlerOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyHandlerOrchestrator")
            .field("handlers_count", &self.handlers.len())
            .field("handler_names", &self.get_handler_names())
            .field("total_events_processed", &self.total_events_processed)
            .field("unhandled_events", &self.unhandled_events)
            .finish()
    }
}

use crate::controller::state_coordinator::StateCoordinator;

impl KeyHandlerOrchestrator {
    pub fn new(state_coordinator: Arc<StateCoordinator>) -> Self {
        // Create handlers in priority order (lower priority numbers first)
        let mut handlers: Vec<Box<dyn EventHandler>> = vec![
            Box::new(NavigationHandler::new()), // Priority 10
            Box::new(FileOpsHandler::new()),    // Priority 20
            Box::new(SearchHandler::new()),     // Priority 30
            Box::new(ClipboardHandler::new(state_coordinator.clone())), // Priority 40
            Box::new(KeyboardHandler::new()),   // Priority 255 (fallback)
        ];

        // Sort by priority (ascending - lower numbers first)
        handlers.sort_by_key(|h| h.priority());

        debug!(
            handlers_count = handlers.len(),
            handler_names = ?handlers.iter().map(|h| h.name()).collect::<Vec<_>>(),
            "KeyHandlerOrchestrator initialized with handlers"
        );

        Self {
            handlers,
            total_events_processed: 0,
            unhandled_events: 0,
        }
    }

    /// Process key event through handler chain
    pub fn handle_key_event(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        self.total_events_processed += 1;

        trace!(
            event = ?event,
            handlers_available = self.handlers.len(),
            "KeyHandlerOrchestrator processing event"
        );

        // Try each handler in priority order
        for handler in &mut self.handlers {
            if handler.can_handle(&event) {
                match handler.handle(event.clone()) {
                    Ok(actions) => {
                        if !actions.is_empty() {
                            debug!(
                                handler_name = handler.name(),
                                handler_priority = handler.priority(),
                                actions_count = actions.len(),
                                actions = ?actions,
                                "Handler successfully processed event"
                            );
                            return Ok(actions);
                        }
                    }
                    Err(e) => {
                        warn!(
                            handler_name = handler.name(),
                            error = %e,
                            "Handler failed to process event"
                        );
                        continue;
                    }
                }
            }
        }

        // No handler could process the event
        self.unhandled_events += 1;

        if self.unhandled_events % 100 == 0 {
            warn!(
                unhandled_events = self.unhandled_events,
                total_events = self.total_events_processed,
                unhandled_rate =
                    (self.unhandled_events as f64 / self.total_events_processed as f64) * 100.0,
                "High rate of unhandled events in KeyHandlerOrchestrator"
            );
        }

        trace!(
            event = ?event,
            "No handler could process event"
        );

        Ok(vec![])
    }

    /// Get orchestrator statistics
    pub fn get_stats(&self) -> OrchestratorStats {
        OrchestratorStats {
            total_events_processed: self.total_events_processed,
            unhandled_events: self.unhandled_events,
            handlers_count: self.handlers.len(),
            unhandled_rate: if self.total_events_processed > 0 {
                (self.unhandled_events as f64 / self.total_events_processed as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Reset statistics (useful for testing)
    pub fn reset_stats(&mut self) {
        self.total_events_processed = 0;
        self.unhandled_events = 0;
    }

    /// Get ordered list of handler names for debugging
    pub fn get_handler_names(&self) -> Vec<&'static str> {
        self.handlers.iter().map(|h| h.name()).collect()
    }
}

/// Statistics for the key handler orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorStats {
    pub total_events_processed: u64,
    pub unhandled_events: u64,
    pub handlers_count: usize,
    pub unhandled_rate: f64, // Percentage
}

impl EventHandler for KeyHandlerOrchestrator {
    fn can_handle(&self, event: &Event) -> bool {
        // Orchestrator can handle any key event by delegating to sub-handlers
        matches!(event, Event::Key { .. })
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        self.handle_key_event(event)
    }

    fn priority(&self) -> u8 {
        // Orchestrator should have highest priority to catch all key events
        1
    }

    fn name(&self) -> &'static str {
        "KeyHandlerOrchestrator"
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crossterm::event::{KeyCode, KeyModifiers};

//     fn create_key_event(code: KeyCode, modifiers: KeyModifiers) -> Event {
//         Event::Key {
//             event: crossterm::event::KeyEvent::new(code, modifiers),
//             priority: Priority::High,
//         }
//     }

//     #[test]
//     fn test_orchestrator_creation() {
//         let orchestrator = KeyHandlerOrchestrator::new();
//         assert!(orchestrator.handlers.len() > 0);

//         let stats = orchestrator.get_stats();
//         assert_eq!(stats.total_events_processed, 0);
//         assert_eq!(stats.unhandled_events, 0);
//     }

//     #[test]
//     fn test_handler_ordering() {
//         let orchestrator = KeyHandlerOrchestrator::new();
//         let names = orchestrator.get_handler_names();

//         // Verify handlers are present (exact order may vary based on priorities)
//         assert!(names.contains(&"KeyboardHandler"));
//         assert!(names.contains(&"NavigationHandler"));
//         assert!(names.contains(&"FileOpsHandler"));
//     }

//     #[test]
//     fn test_quit_key_handling() {
//         let mut orchestrator = KeyHandlerOrchestrator::new();
//         let quit_event = create_key_event(KeyCode::Char('q'), KeyModifiers::NONE);

//         let result = orchestrator.handle_key_event(quit_event);
//         assert!(result.is_ok());

//         let actions = result.unwrap();
//         assert!(!actions.is_empty());
//         assert!(matches!(actions[0], Action::Quit));
//     }

//     #[test]
//     fn test_navigation_key_handling() {
//         let mut orchestrator = KeyHandlerOrchestrator::new();
//         let up_event = create_key_event(KeyCode::Up, KeyModifiers::NONE);

//         let result = orchestrator.handle_key_event(up_event);
//         assert!(result.is_ok());

//         let actions = result.unwrap();
//         assert!(!actions.is_empty());
//         assert!(matches!(actions[0], Action::MoveSelectionUp));
//     }

//     #[test]
//     fn test_stats_tracking() {
//         let mut orchestrator = KeyHandlerOrchestrator::new();

//         // Process some events
//         let _result1 =
//             orchestrator.handle_key_event(create_key_event(KeyCode::Char('q'), KeyModifiers::NONE));
//         let _result2 =
//             orchestrator.handle_key_event(create_key_event(KeyCode::Up, KeyModifiers::NONE));

//         let stats = orchestrator.get_stats();
//         assert_eq!(stats.total_events_processed, 2);
//         assert!(stats.handlers_count > 0);
//     }
// }
