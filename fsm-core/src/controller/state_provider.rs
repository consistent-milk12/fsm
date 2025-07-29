//! StateProvider: trait abstraction for breaking circular dependencies
//!
//! Provides clean interface for handlers to access application state
//! without requiring direct StateCoordinator dependencies.

use std::fmt::Debug;
use std::sync::{Arc, MutexGuard, RwLock};

use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};

/// Trait for providing access to application state components
/// Used to break circular dependencies between StateCoordinator and HandlerRegistry
pub trait StateProvider: Send + Sync + Debug {
    /// Get current UI state snapshot (lock-free)
    fn ui_state(&self) -> Arc<RwLock<UIState>>;

    /// Atomically update UI state with boxed transformation function
    fn update_ui_state(&self, update: Box<dyn FnOnce(&mut UIState) + Send>);

    /// Get mutable access to filesystem state
    fn fs_state(&self) -> MutexGuard<'_, FSState>;

    /// Get mutable access to application state
    fn app_state(&self) -> MutexGuard<'_, AppState>;

    /// Request UI redraw for specific component
    fn request_redraw(&self, flag: crate::model::ui_state::RedrawFlag);

    /// Check if UI needs redraw
    fn needs_redraw(&self) -> bool;

    /// Clear all redraw flags
    fn clear_redraw(&self);
}
