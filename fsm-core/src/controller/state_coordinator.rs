//! StateCoordinator: unified state manager without circular dependencies
//!
//! Simplified Phase 4.0 StateCoordinator that manages application state
//! without complex circular dependencies with HandlerRegistry:
//! - Simple thread-safe composition of core state pieces
//! - ArcSwap for lock-free UI state updates
//! - Optional HandlerRegistry integration
//! - Clean separation of concerns

use std::sync::{Arc, Mutex, RwLock};

use arc_swap::ArcSwap;

use crate::controller::state_provider::StateProvider;
use crate::model::app_state::AppState;
use crate::model::fs_state::FSState;
use crate::model::ui_state::UIState;

/// Simplified state coordinator without circular dependencies
pub struct StateCoordinator {
    /// Application state containing configuration, caches and task management
    pub app_state: Arc<Mutex<AppState>>,

    /// Current UI state with lock-free atomic updates
    pub ui_state: ArcSwap<RwLock<UIState>>,

    /// Filesystem state
    pub fs_state: Arc<Mutex<FSState>>,
}

impl StateCoordinator {
    /// Create coordinator (HandlerRegistry integration removed)
    /// Use StateProvider trait for handler access instead
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: RwLock<UIState>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self {
            app_state,
            ui_state: ArcSwap::from_pointee(initial_ui),
            fs_state,
        }
    }

    /// Create simplified coordinator (same as new, kept for compatibility)
    pub fn new_simple(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: RwLock<UIState>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self::new(app_state, initial_ui, fs_state)
    }

    /// Retrieve a snapshot of the current UI state (lock-free)
    pub fn ui_state(&self) -> Arc<RwLock<UIState>> {
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
        F: FnOnce(&mut UIState),
    {
        let ui_state = self.ui_state.load_full();
        let mut ui_guard = ui_state.write().expect("UIState RwLock poisoned");

        update(&mut ui_guard);
    }

    /// HandlerRegistry integration methods removed
    /// Handlers now use StateProvider trait for state access
    /// Event processing moved to dedicated handler management layer
    /// Request UI redraw for specific component
    pub fn request_redraw(&self, flag: crate::model::ui_state::RedrawFlag) {
        self.update_ui_state(|ui| {
            ui.request_redraw(flag);
        });
    }

    /// Clear all redraw flags
    pub fn clear_redraw(&self) {
        self.update_ui_state(|ui| {
            ui.clear_redraw();
        });
    }

    /// Check if UI needs redraw
    pub fn needs_redraw(&self) -> bool {
        let ui_state = self.ui_state.load_full();
        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        ui_guard.needs_redraw()
    }
}

/// Implement StateProvider trait for StateCoordinator
impl StateProvider for StateCoordinator {
    fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state()
    }

    fn update_ui_state(&self, update: Box<dyn FnOnce(&mut UIState) + Send>) {
        let ui_state = self.ui_state.load_full();
        let mut ui_guard = ui_state.write().expect("UIState RwLock poisoned");
        update(&mut ui_guard);
    }

    fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state()
    }

    fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state()
    }

    fn request_redraw(&self, flag: crate::model::ui_state::RedrawFlag) {
        self.request_redraw(flag)
    }

    fn needs_redraw(&self) -> bool {
        self.needs_redraw()
    }

    fn clear_redraw(&self) {
        self.clear_redraw()
    }
}

impl std::fmt::Debug for StateCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateCoordinator")
            .field("implements_state_provider", &true)
            .finish()
    }
}
