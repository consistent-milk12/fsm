// fsm-core/src/controller/handler_registry.rs

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use tracing::{debug, info, warn};

use super::{
    event_processor::{Event, EventHandler},
    handlers::{
        clipboard_handler::ClipboardHandler, file_ops_handler::FileOpsHandler,
        keyboard_handler::KeyboardHandler, navigation_handler::NavigationHandler,
        search_handler::SearchHandler,
    },
    state_provider::StateProvider,
};

use crate::controller::actions::Action;

/// Handler types with unique identifiers for performance tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum HandlerType {
    Navigation = 0, // Priority 10 - High-frequency navigation
    Clipboard = 1,  // Priority 1-5 - Instant clipboard operations
    Search = 2,     // Priority 5-100 - Search operations
    FileOps = 3,    // Priority 3-50 - File operations
    Keyboard = 4,   // Priority 1-255 - Fallback handler
}

/// Handler registration with metadata
#[derive(Debug)]
pub struct HandlerRegistration {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: std::sync::atomic::AtomicU64,
    pub total_processing_time: std::sync::atomic::AtomicU64,
}

/// Handler performance statistics
#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: u64,
    pub average_processing_time_ns: u64,
}

/// Fixed handler registry with proper priority ordering
pub struct HandlerRegistry {
    handlers: Vec<HandlerEntry>,
    state_provider: Option<Arc<dyn StateProvider>>,
}

/// Entry for each registered handler
struct HandlerEntry {
    handler: Box<dyn EventHandler>,
    metadata: HandlerRegistration,
}

impl HandlerRegistry {
    /// Create new empty handler registry
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            state_provider: None,
        }
    }

    /// Create handler registry with StateProvider and initialize handlers
    pub fn with_state_provider(state_provider: Arc<dyn StateProvider>) -> Self {
        let mut registry = Self {
            handlers: Vec::new(),
            state_provider: Some(state_provider),
        };

        registry.register_all_handlers();

        info!(
            "HandlerRegistry initialized with {} handlers and StateProvider",
            registry.handlers.len()
        );

        registry
    }

    /// Register all handlers in correct priority order
    fn register_all_handlers(&mut self) {
        // Register handlers in priority order (lower priority number = higher priority)

        // 1. NavigationHandler - Priority 10 (highest for responsive UI)
        let nav_handler = Box::new(NavigationHandler::new());
        self.register_handler(nav_handler, HandlerType::Navigation);

        // 2. ClipboardHandler - Priority 1-5 (high for instant operations)
        let clipboard_handler = Box::new(ClipboardHandler::new());
        self.register_handler(clipboard_handler, HandlerType::Clipboard);

        // 3. FileOpsHandler - Priority 3-50 (medium for file operations)
        let file_ops_handler = Box::new(FileOpsHandler::new());
        self.register_handler(file_ops_handler, HandlerType::FileOps);

        // 4. SearchHandler - Priority 5-100 (medium for search operations)
        let search_handler = Box::new(SearchHandler::new());
        self.register_handler(search_handler, HandlerType::Search);

        // 5. KeyboardHandler - Priority 1-255 (lowest, true fallback)
        let keyboard_handler = Box::new(KeyboardHandler::new());
        self.register_handler(keyboard_handler, HandlerType::Keyboard);

        debug!("All handlers registered in priority order");
    }

    /// Register a single handler
    pub fn register_handler(&mut self, handler: Box<dyn EventHandler>, handler_type: HandlerType) {
        let metadata = HandlerRegistration {
            handler_type,
            is_enabled: true,
            event_count: std::sync::atomic::AtomicU64::new(0),
            total_processing_time: std::sync::atomic::AtomicU64::new(0),
        };

        self.handlers.push(HandlerEntry { handler, metadata });

        // Sort handlers by their current priority
        self.handlers.sort_by_key(|entry| entry.handler.priority());

        info!(
            "Registered {:?} handler with priority {}",
            handler_type,
            self.handlers.last().unwrap().handler.priority()
        );
    }

    /// Process event through handler chain with proper priority ordering
    pub fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Action>, Box<dyn std::error::Error>> {
        let mut actions = Vec::new();
        let event_start = Instant::now();

        debug!("HandlerRegistry: processing event {:?}", event);

        // Re-sort by current priority in case handlers changed their priority dynamically
        self.handlers.sort_by_key(|entry| entry.handler.priority());

        // Try each handler in priority order until one handles the event
        for entry in &mut self.handlers {
            if !entry.metadata.is_enabled {
                continue;
            }

            let process_start = Instant::now();

            // Check if handler can handle this event
            if entry.handler.can_handle(&event) {
                debug!(
                    "HandlerRegistry: {} (priority {}) attempting to handle event",
                    entry.handler.name(),
                    entry.handler.priority()
                );

                match entry.handler.handle(event.clone()) {
                    Ok(mut handler_actions) => {
                        let elapsed = process_start.elapsed().as_nanos() as u64;

                        // Update performance metadata
                        entry.metadata.event_count.fetch_add(1, Ordering::Relaxed);
                        entry
                            .metadata
                            .total_processing_time
                            .fetch_add(elapsed, Ordering::Relaxed);

                        if !handler_actions.is_empty() {
                            debug!(
                                "HandlerRegistry: {} generated {} actions in {:?}",
                                entry.handler.name(),
                                handler_actions.len(),
                                process_start.elapsed()
                            );
                            actions.append(&mut handler_actions);

                            // Event handled successfully, stop trying other handlers
                            break;
                        } else {
                            debug!(
                                "HandlerRegistry: {} returned no actions",
                                entry.handler.name()
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            "HandlerRegistry: {} failed to process event: {}",
                            entry.handler.name(),
                            e
                        );
                    }
                }
            }
        }

        if actions.is_empty() {
            debug!("HandlerRegistry: no handler processed event {:?}", event);
        } else {
            debug!(
                "HandlerRegistry: event processed in {:?}, generated {} total actions",
                event_start.elapsed(),
                actions.len()
            );
        }

        Ok(actions)
    }

    /// Enable/disable specific handler
    pub fn set_handler_enabled(&mut self, handler_type: HandlerType, enabled: bool) {
        if let Some(entry) = self
            .handlers
            .iter_mut()
            .find(|e| e.metadata.handler_type == handler_type)
        {
            entry.metadata.is_enabled = enabled;
            info!(
                "Handler {:?} {}",
                handler_type,
                if enabled { "enabled" } else { "disabled" }
            );
        } else {
            warn!(
                "Attempted to modify non-existent handler {:?}",
                handler_type
            );
        }
    }

    /// Get performance report for all handlers
    pub fn get_performance_report(&self) -> Vec<HandlerStats> {
        self.handlers
            .iter()
            .map(|entry| {
                let event_count = entry.metadata.event_count.load(Ordering::Relaxed);
                let total_time = entry.metadata.total_processing_time.load(Ordering::Relaxed);

                let avg_time = if event_count > 0 {
                    total_time / event_count
                } else {
                    0
                };

                HandlerStats {
                    handler_type: entry.metadata.handler_type,
                    is_enabled: entry.metadata.is_enabled,
                    event_count,
                    average_processing_time_ns: avg_time,
                }
            })
            .collect()
    }

    /// Get handler count
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Check if handlers are registered
    pub fn has_handlers(&self) -> bool {
        !self.handlers.is_empty()
    }

    /// Get list of handler priorities for debugging
    pub fn get_handler_priorities(&self) -> Vec<(HandlerType, u8, &'static str)> {
        self.handlers
            .iter()
            .map(|entry| {
                (
                    entry.metadata.handler_type,
                    entry.handler.priority(),
                    entry.handler.name(),
                )
            })
            .collect()
    }

    /// Reset all handler statistics
    pub fn reset_statistics(&mut self) {
        for entry in &mut self.handlers {
            entry.metadata.event_count.store(0, Ordering::Relaxed);
            entry
                .metadata
                .total_processing_time
                .store(0, Ordering::Relaxed);
        }
        info!("Handler statistics reset");
    }

    /// Get detailed handler info for debugging
    pub fn debug_handler_status(&self) -> String {
        let mut status = String::from("Handler Registry Status:\n");

        for entry in &self.handlers {
            let event_count = entry.metadata.event_count.load(Ordering::Relaxed);
            let total_time = entry.metadata.total_processing_time.load(Ordering::Relaxed);
            let avg_time = if event_count > 0 {
                total_time / event_count
            } else {
                0
            };

            status.push_str(&format!(
                "  {:?} ({}): priority={}, enabled={}, events={}, avg_time={}ns\n",
                entry.metadata.handler_type,
                entry.handler.name(),
                entry.handler.priority(),
                entry.metadata.is_enabled,
                event_count,
                avg_time
            ));
        }

        status
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HandlerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerRegistry")
            .field("handler_count", &self.handlers.len())
            .field("has_state_provider", &self.state_provider.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::event_processor::Priority;

    #[test]
    fn test_handler_registry_creation() {
        let registry = HandlerRegistry::new();
        assert_eq!(registry.handler_count(), 0);
        assert!(!registry.has_handlers());
    }

    #[test]
    fn test_handler_priority_ordering() {
        let mut registry = HandlerRegistry::new();

        // Register handlers
        registry.register_handler(Box::new(NavigationHandler::new()), HandlerType::Navigation);
        registry.register_handler(Box::new(KeyboardHandler::new()), HandlerType::Keyboard);

        let priorities = registry.get_handler_priorities();

        // Should be sorted by priority (lower number = higher priority)
        assert!(priorities.len() >= 2);
        assert!(priorities[0].1 <= priorities[1].1); // First handler should have lower or equal priority number
    }

    #[test]
    fn test_handler_enable_disable() {
        let mut registry = HandlerRegistry::new();
        registry.register_handler(Box::new(NavigationHandler::new()), HandlerType::Navigation);

        // Disable handler
        registry.set_handler_enabled(HandlerType::Navigation, false);

        let stats = registry.get_performance_report();
        assert!(!stats[0].is_enabled);

        // Re-enable handler
        registry.set_handler_enabled(HandlerType::Navigation, true);

        let stats = registry.get_performance_report();
        assert!(stats[0].is_enabled);
    }

    #[test]
    fn test_event_handling() {
        let mut registry = HandlerRegistry::new();
        registry.register_handler(Box::new(NavigationHandler::new()), HandlerType::Navigation);

        let nav_event = Event::Key {
            event: crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyModifiers::NONE,
            ),
            priority: Priority::High,
        };

        let result = registry.handle_event(nav_event);
        assert!(result.is_ok());

        let actions = result.unwrap();
        assert!(!actions.is_empty()); // Should generate navigation action
    }
}
