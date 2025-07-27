//! HandlerRegistry: Simplified handler management without circular dependencies
//!
//! Manages specialized event handlers with priority-based routing:
//! - Simple handler registration and lookup
//! - Priority-based event dispatching
//! - Performance monitoring per handler type
//! - Uses StateProvider trait to avoid circular dependencies

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
    pub total_processing_time: std::sync::atomic::AtomicU64, // nanoseconds
}

/// Handler performance statistics
#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: u64,
    pub average_processing_time_ns: u64,
}

/// Simplified handler registry without circular dependencies
pub struct HandlerRegistry {
    // Handlers with their registration metadata
    handlers: Vec<HandlerEntry>,

    // State provider for handlers to access application state
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

    /// Create handler registry with StateProvider for state access
    pub fn with_state_provider(state_provider: Arc<dyn StateProvider>) -> Self {
        let mut registry: HandlerRegistry = Self {
            handlers: Vec::new(),
            state_provider: Some(state_provider),
        };

        registry.register_basic_handlers();

        info!(
            "HandlerRegistry initialized with {} handlers and StateProvider",
            registry.handlers.len()
        );

        registry
    }

    /// Add StateProvider to existing registry
    pub fn set_state_provider(mut self, state_provider: Arc<dyn StateProvider>) -> Self {
        self.state_provider = Some(state_provider);
        self
    }

    /// Create completely empty registry for breaking circular dependencies
    pub fn empty() -> Self {
        Self {
            handlers: Vec::new(),
            state_provider: None,
        }
    }

    /// Register basic handlers that don't require StateProvider
    fn register_basic_handlers(&mut self) {
        // Register NavigationHandler
        let nav_handler: Box<NavigationHandler> = Box::default();
        self.register_handler(nav_handler, HandlerType::Navigation);

        // Register SearchHandler
        let search_handler: Box<SearchHandler> = Box::default();
        self.register_handler(search_handler, HandlerType::Search);

        // Register FileOpsHandler
        let file_ops_handler: Box<FileOpsHandler> = Box::default();
        self.register_handler(file_ops_handler, HandlerType::FileOps);

        // Register ClipboardHandler
        let clipboard_handler: Box<ClipboardHandler> = Box::default();
        self.register_handler(clipboard_handler, HandlerType::Clipboard);

        // Register KeyboardHandler as fallback (simplified without EKey processor)
        let keyboard_handler: Box<KeyboardHandler> = Box::default();
        self.register_handler(keyboard_handler, HandlerType::Keyboard);

        debug!("Basic handlers registered");
    }

    /// Register a single handler
    pub fn register_handler(&mut self, handler: Box<dyn EventHandler>, handler_type: HandlerType) {
        let metadata: HandlerRegistration = HandlerRegistration {
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
        let mut actions: Vec<Action> = Vec::new();
        let event_start: Instant = Instant::now();

        // Collect handlers that can handle the event with their current priorities
        let mut candidates: Vec<(&mut Box<dyn EventHandler>, &mut HandlerRegistration, u8)> = self
            .handlers
            .iter_mut()
            .filter(|entry: &&mut HandlerEntry| {
                entry.metadata.is_enabled && entry.handler.can_handle(&event)
            })
            .map(|entry: &mut HandlerEntry| {
                let priority: u8 = entry.handler.priority();

                (&mut entry.handler, &mut entry.metadata, priority)
            })
            .collect();

        // Sort by current priority (lowest number = highest priority)
        candidates.sort_by_key(|(_, _, priority)| *priority);

        // Process handlers in priority order
        for (handler, metadata, _) in candidates {
            let process_start: Instant = Instant::now();

            match handler.handle(event.clone()) {
                Ok(mut handler_actions) => {
                    let elapsed: u64 = process_start.elapsed().as_nanos() as u64;

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
            .find(|e: &&mut HandlerEntry| e.metadata.handler_type == handler_type)
        {
            entry.metadata.is_enabled = enabled;

            info!(
                "Handler {:?} {}",
                handler_type,
                if enabled { "enabled" } else { "disabled" }
            );
        }
    }

    /// Get performance report for all handlers
    pub fn get_performance_report(&self) -> Vec<HandlerStats> {
        self.handlers
            .iter()
            .map(|entry: &HandlerEntry| {
                let event_count: u64 = entry.metadata.event_count.load(Ordering::Relaxed);
                let total_time: u64 = entry.metadata.total_processing_time.load(Ordering::Relaxed);

                let avg_time: u64 = if event_count > 0 {
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
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Remove the KeyboardHandlerWrapper since we're simplifying
impl std::fmt::Debug for HandlerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerRegistry")
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}
