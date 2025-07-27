//! StateCoordinator: unified state manager without circular dependencies
//!
//! Simplified Phase 4.0 StateCoordinator that manages application state
//! without complex circular dependencies with HandlerRegistry:
//! - Simple thread-safe composition of core state pieces
//! - ArcSwap for lock-free UI state updates
//! - Optional HandlerRegistry integration
//! - Clean separation of concerns

use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use crate::controller::actions::Action;
use crate::controller::event_processor::Event;
use crate::controller::handler_registry::{HandlerRegistry, HandlerStats, HandlerType};
use crate::error::AppError;
use crate::model::app_state::AppState;
use crate::model::fs_state::FSState;
use crate::model::ui_state::UIState;

/// Simplified state coordinator without circular dependencies
pub struct StateCoordinator {
    /// Application state containing configuration, caches and task management
    pub app_state: Arc<Mutex<AppState>>,

    /// Current UI state with lock-free atomic updates
    pub ui_state: ArcSwap<UIState>,

    /// Filesystem state
    pub fs_state: Arc<Mutex<FSState>>,

    /// Optional handler registry (can be None to break circular dependencies)
    pub handler_registry: Option<Arc<Mutex<HandlerRegistry>>>,
}

impl StateCoordinator {
    /// Create coordinator with full HandlerRegistry integration
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: UIState,
        fs_state: Arc<Mutex<FSState>>,
        handler_registry: HandlerRegistry,
    ) -> Self {
        Self {
            app_state,
            ui_state: ArcSwap::from_pointee(initial_ui),
            fs_state,
            handler_registry: Some(Arc::new(Mutex::new(handler_registry))),
        }
    }

    /// Create simplified coordinator without HandlerRegistry
    pub fn new_simple(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: UIState,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self {
            app_state,
            ui_state: ArcSwap::from_pointee(initial_ui),
            fs_state,
            handler_registry: None,
        }
    }

    /// Create coordinator and later attach HandlerRegistry
    pub fn with_handler_registry(mut self, handler_registry: HandlerRegistry) -> Self {
        self.handler_registry = Some(Arc::new(Mutex::new(handler_registry)));
        self
    }

    /// Retrieve a snapshot of the current UI state (lock-free)
    pub fn ui_state(&self) -> Arc<UIState> {
        self.ui_state.load_full()
    }

    /// Mutably borrow the underlying AppState
    pub fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state
            .lock()
            .expect("StateCoordinator.app_state mutex poisoned")
    }

    /// Mutably borrow the underlying FSState
    pub fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state
            .lock()
            .expect("StateCoordinator.fs_state mutex poisoned")
    }

    /// Atomically update the UI state
    pub fn update_ui_state<F>(&self, update: F)
    where
        F: FnOnce(&UIState) -> UIState,
    {
        let current = self.ui_state.load();
        let new_state = update(&current);
        self.ui_state.store(Arc::new(new_state));
    }

    /// Process an event through the handler registry if available
    pub fn handle_event(&self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Some(registry) = &self.handler_registry {
            let mut registry_guard = registry
                .lock()
                .expect("StateCoordinator.handler_registry mutex poisoned");

            registry_guard
                .handle_event(event)
                .map_err(|_| AppError::Cancelled)
        } else {
            // No handlers available, return empty action list
            Ok(Vec::new())
        }
    }

    /// Enable or disable a specific handler if registry is available
    pub fn set_handler_enabled(&self, ty: HandlerType, enabled: bool) {
        if let Some(registry) = &self.handler_registry {
            let mut registry_guard = registry
                .lock()
                .expect("StateCoordinator.handler_registry mutex poisoned");
            registry_guard.set_handler_enabled(ty, enabled);
        }
    }

    /// Retrieve performance report for all handlers if registry is available
    pub fn handler_performance_report(&self) -> Vec<HandlerStats> {
        if let Some(registry) = &self.handler_registry {
            let registry_guard = registry
                .lock()
                .expect("StateCoordinator.handler_registry mutex poisoned");
            registry_guard.get_performance_report()
        } else {
            Vec::new()
        }
    }

    /// Check if HandlerRegistry is available
    pub fn has_handler_registry(&self) -> bool {
        self.handler_registry.is_some()
    }

    /// Get handler count if registry is available
    pub fn handler_count(&self) -> usize {
        if let Some(registry) = &self.handler_registry {
            let registry_guard = registry
                .lock()
                .expect("StateCoordinator.handler_registry mutex poisoned");
            registry_guard.handler_count()
        } else {
            0
        }
    }

    /// Request UI redraw for specific component
    pub fn request_redraw(&self, flag: crate::model::ui_state::RedrawFlag) {
        self.update_ui_state(|ui| {
            let new_ui = ui.clone();
            new_ui.request_redraw(flag);
            new_ui
        });
    }

    /// Clear all redraw flags
    pub fn clear_redraw(&self) {
        self.update_ui_state(|ui| {
            let new_ui = ui.clone();
            new_ui.clear_redraw();
            new_ui
        });
    }

    /// Check if UI needs redraw
    pub fn needs_redraw(&self) -> bool {
        self.ui_state().needs_redraw()
    }
}

impl std::fmt::Debug for StateCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateCoordinator")
            .field("has_handler_registry", &self.has_handler_registry())
            .field("handler_count", &self.handler_count())
            .finish()
    }
}
