//! HandlerRegistry: Centralized handler management for Phase 4.0
//!
//! Manages specialized event handlers with priority-based routing:
//! - Lock-free handler registration and lookup
//! - Priority-based event dispatching
//! - Conflict-free handler coordination
//! - Performance monitoring per handler type

use super::{
    Action,
    ekey_processor::EKeyProcessor,
    event_processor::{Event, EventHandler},
    handlers::{
        clipboard_handler::ClipboardHandler, file_ops_handler::FileOpsHandler,
        keyboard_handler::KeyboardHandler, navigation_handler::NavigationHandler,
        search_handler::SearchHandler,
    },
    state_coordinator::StateCoordinator,
};

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tracing::{debug, info, warn};

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
    pub total_processing_time: std::sync::atomic::AtomicU64, // nanoseconds
}

/// Centralized handler registry with priority management
pub struct HandlerRegistry {
    // Handlers with their registration metadata
    handlers: Vec<HandlerEntry>,

    // Shared state coordinator
    state_coordinator: Arc<StateCoordinator>,

    // EKey processor for legacy compatibility
    ekey_processor: Arc<EKeyProcessor>,
}

/// Entry for each registered handler
struct HandlerEntry {
    handler: Box<dyn EventHandler>,
    metadata: HandlerRegistration,
}

impl HandlerRegistry {
    /// Create new handler registry with all specialized handlers
    pub fn new(
        state_coordinator: Arc<StateCoordinator>,
        ekey_processor: Arc<EKeyProcessor>,
    ) -> Self {
        let mut registry = Self {
            handlers: Vec::new(),
            state_coordinator: state_coordinator.clone(),
            ekey_processor: ekey_processor.clone(),
        };

        // Register all handlers with their priorities
        registry.register_all_handlers();

        info!(
            "HandlerRegistry initialized with {} handlers",
            registry.handlers.len()
        );
        registry
    }

    /// Register all specialized handlers
    fn register_all_handlers(&mut self) {
        // Register NavigationHandler
        let nav_handler = Box::new(NavigationHandler::new());
        self.register_handler(nav_handler, HandlerType::Navigation);

        // Register ClipboardHandler
        let clipboard_handler = Box::new(ClipboardHandler::new());
        self.register_handler(clipboard_handler, HandlerType::Clipboard);

        // Register SearchHandler
        let search_handler = Box::new(SearchHandler::new());
        self.register_handler(search_handler, HandlerType::Search);

        // Register FileOpsHandler
        let file_ops_handler = Box::new(FileOpsHandler::new());
        self.register_handler(file_ops_handler, HandlerType::FileOps);

        // Register KeyboardHandler as fallback
        let keyboard_handler = Box::new(KeyboardHandlerWrapper::new(
            self.ekey_processor.clone(),
            self.state_coordinator.clone(),
        ));
        self.register_handler(keyboard_handler, HandlerType::Keyboard);

        debug!("All handlers registered");
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

        info!("Registered {:?} handler", handler_type);
    }

    /// Process event through handler chain (priority order)
    pub fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Action>, Box<dyn std::error::Error>> {
        let mut actions = Vec::new();
        let event_start = Instant::now();

        // Collect handlers that can handle the event with their current priorities
        let mut candidates: Vec<(&mut Box<dyn EventHandler>, &mut HandlerRegistration, u8)> = self
            .handlers
            .iter_mut()
            .filter(|entry| entry.metadata.is_enabled && entry.handler.can_handle(&event))
            .map(|entry| {
                let priority = entry.handler.priority();
                (&mut entry.handler, &mut entry.metadata, priority)
            })
            .collect();

        // Sort by current priority (lowest number = highest priority)
        candidates.sort_by_key(|(_, _, priority)| *priority);

        // Process handlers in priority order
        for (handler, metadata, _) in candidates {
            let process_start = Instant::now();

            match handler.handle(event.clone()) {
                Ok(mut handler_actions) => {
                    let elapsed = process_start.elapsed().as_nanos() as u64;

                    // Update performance metadata
                    metadata.event_count.fetch_add(1, Ordering::Relaxed);
                    metadata
                        .total_processing_time
                        .fetch_add(elapsed, Ordering::Relaxed);

                    actions.append(&mut handler_actions);

                    debug!("Handler processed event in {:?}", process_start.elapsed());
                }
                Err(e) => {
                    warn!("Handler failed to process event: {}", e);
                    // Continue to next handler
                }
            }
        }

        debug!(
            "Event processed in {:?}, generated {} actions",
            event_start.elapsed(),
            actions.len()
        );

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
        }
    }

    /// Get handler performance statistics
    pub fn get_handler_stats(&self, handler_type: HandlerType) -> Option<HandlerStats> {
        self.handlers
            .iter()
            .find(|entry| entry.metadata.handler_type == handler_type)
            .map(|entry| {
                let metadata = &entry.metadata;
                let event_count = metadata.event_count.load(Ordering::Relaxed);
                let total_time_ns = metadata.total_processing_time.load(Ordering::Relaxed);

                HandlerStats {
                    handler_type,
                    is_enabled: metadata.is_enabled,
                    event_count,
                    total_processing_time_us: total_time_ns as f64 / 1000.0,
                    avg_processing_time_us: if event_count > 0 {
                        (total_time_ns as f64 / 1000.0) / event_count as f64
                    } else {
                        0.0
                    },
                }
            })
    }

    /// Get performance report for all handlers
    pub fn get_performance_report(&self) -> Vec<HandlerStats> {
        self.handlers
            .iter()
            .map(|entry| {
                let metadata = &entry.metadata;
                let event_count = metadata.event_count.load(Ordering::Relaxed);
                let total_time_ns = metadata.total_processing_time.load(Ordering::Relaxed);

                HandlerStats {
                    handler_type: metadata.handler_type,
                    is_enabled: metadata.is_enabled,
                    event_count,
                    total_processing_time_us: total_time_ns as f64 / 1000.0,
                    avg_processing_time_us: if event_count > 0 {
                        (total_time_ns as f64 / 1000.0) / event_count as f64
                    } else {
                        0.0
                    },
                }
            })
            .collect()
    }

    /// Check if any handler can process the given event
    pub fn can_handle_event(&self, event: &Event) -> bool {
        self.handlers
            .iter()
            .any(|entry| entry.metadata.is_enabled && entry.handler.can_handle(event))
    }

    /// Get list of registered handler types
    pub fn get_registered_handlers(&self) -> Vec<HandlerType> {
        self.handlers
            .iter()
            .map(|entry| entry.metadata.handler_type)
            .collect()
    }

    /// Reset handler statistics
    pub fn reset_stats(&mut self) {
        for entry in &mut self.handlers {
            entry.metadata.event_count.store(0, Ordering::Relaxed);
            entry
                .metadata
                .total_processing_time
                .store(0, Ordering::Relaxed);
        }
        info!("Handler statistics reset");
    }
}

/// Handler performance statistics
#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: u64,
    pub total_processing_time_us: f64,
    pub avg_processing_time_us: f64,
}

impl HandlerStats {
    /// Check if handler performance is healthy
    pub fn is_healthy(&self) -> bool {
        // Handler should respond in under 100μs on average for good UX
        self.avg_processing_time_us < 100.0
    }

    /// Generate performance report string
    pub fn report(&self) -> String {
        format!(
            "{:?}: {} events, {:.1}μs avg ({:.1}μs total) [{}]",
            self.handler_type,
            self.event_count,
            self.avg_processing_time_us,
            self.total_processing_time_us,
            if self.is_enabled {
                "enabled"
            } else {
                "disabled"
            }
        )
    }
}

/// Wrapper to make KeyboardHandler compatible with EventHandler trait
pub struct KeyboardHandlerWrapper {
    keyboard_handler: KeyboardHandler,
    ekey_processor: Arc<EKeyProcessor>,
    state_coordinator: Arc<StateCoordinator>,
}

impl KeyboardHandlerWrapper {
    pub fn new(
        ekey_processor: Arc<EKeyProcessor>,
        state_coordinator: Arc<StateCoordinator>,
    ) -> Self {
        Self {
            keyboard_handler: KeyboardHandler::new(),
            ekey_processor,
            state_coordinator,
        }
    }
}

impl EventHandler for KeyboardHandlerWrapper {
    fn can_handle(&self, event: &Event) -> bool {
        // Keyboard handler is the fallback - it can handle any key event
        matches!(event, Event::Key { .. })
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, crate::error::AppError> {
        // Delegate to the actual keyboard handler
        self.keyboard_handler.handle(event)
    }

    fn priority(&self) -> u8 {
        self.keyboard_handler.priority()
    }

    fn name(&self) -> &'static str {
        "KeyboardHandlerWrapper"
    }
}
