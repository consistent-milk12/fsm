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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum HandlerType {
    Navigation = 0,
    Clipboard = 1,
    Search = 2,
    FileOps = 3,
    Keyboard = 4,
}

#[derive(Debug)]
pub struct HandlerRegistration {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: std::sync::atomic::AtomicU64,
    pub total_processing_time: std::sync::atomic::AtomicU64,
}

#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub handler_type: HandlerType,
    pub is_enabled: bool,
    pub event_count: u64,
    pub average_processing_time_ns: u64,
}

pub struct HandlerRegistry {
    handlers: Vec<HandlerEntry>,
    _state_provider: Option<Arc<dyn StateProvider>>,
}

struct HandlerEntry {
    handler: Box<dyn EventHandler>,
    metadata: HandlerRegistration,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            _state_provider: None,
        }
    }

    pub fn with_state_provider(state_provider: Arc<dyn StateProvider>) -> Self {
        let mut registry = Self {
            handlers: Vec::new(),
            _state_provider: Some(state_provider),
        };

        registry.register_all_handlers();
        info!(
            "HandlerRegistry initialized with {} handlers",
            registry.handlers.len()
        );
        registry
    }

    fn register_all_handlers(&mut self) {
        // Register in priority order
        self.register_handler(Box::new(NavigationHandler::new()), HandlerType::Navigation);
        self.register_handler(Box::new(ClipboardHandler::new()), HandlerType::Clipboard);
        self.register_handler(Box::new(SearchHandler::new()), HandlerType::Search);
        self.register_handler(Box::new(FileOpsHandler::new()), HandlerType::FileOps);
        self.register_handler(Box::new(KeyboardHandler::new()), HandlerType::Keyboard);

        debug!("All handlers registered");
    }

    pub fn register_handler(&mut self, handler: Box<dyn EventHandler>, handler_type: HandlerType) {
        let metadata = HandlerRegistration {
            handler_type,
            is_enabled: true,
            event_count: std::sync::atomic::AtomicU64::new(0),
            total_processing_time: std::sync::atomic::AtomicU64::new(0),
        };

        self.handlers.push(HandlerEntry { handler, metadata });

        // Sort by priority after each registration
        self.handlers.sort_by_key(|entry| entry.handler.priority());

        info!(
            "Registered {:?} handler with priority {}",
            handler_type,
            self.handlers.last().unwrap().handler.priority()
        );
    }

    /// Main event processing with priority ordering
    pub fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Action>, Box<dyn std::error::Error>> {
        debug!("HandlerRegistry: processing event {:?}", event);

        let mut actions = Vec::new();
        let event_start = Instant::now();

        // Sort by current priority to ensure correct order
        self.handlers.sort_by_key(|entry| entry.handler.priority());

        // Try handlers in priority order until one handles the event
        for entry in &mut self.handlers {
            if !entry.metadata.is_enabled {
                continue;
            }

            let process_start = Instant::now();

            if entry.handler.can_handle(&event) {
                debug!(
                    "HandlerRegistry: {} (priority {}) handling event",
                    entry.handler.name(),
                    entry.handler.priority()
                );

                match entry.handler.handle(event.clone()) {
                    Ok(mut handler_actions) => {
                        let elapsed = process_start.elapsed().as_nanos() as u64;

                        entry.metadata.event_count.fetch_add(1, Ordering::Relaxed);
                        entry
                            .metadata
                            .total_processing_time
                            .fetch_add(elapsed, Ordering::Relaxed);

                        if !handler_actions.is_empty() {
                            debug!(
                                "HandlerRegistry: {} generated {} actions",
                                entry.handler.name(),
                                handler_actions.len()
                            );
                            actions.append(&mut handler_actions);
                            break; // Stop after first successful handler
                        }
                    }
                    Err(e) => {
                        warn!("HandlerRegistry: {} failed: {}", entry.handler.name(), e);
                    }
                }
            }
        }

        if actions.is_empty() {
            debug!("HandlerRegistry: no handler processed event");
        } else {
            debug!(
                "HandlerRegistry: processed in {:?}, {} actions generated",
                event_start.elapsed(),
                actions.len()
            );
        }

        Ok(actions)
    }

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

    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    pub fn has_handlers(&self) -> bool {
        !self.handlers.is_empty()
    }

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
            .finish()
    }
}
