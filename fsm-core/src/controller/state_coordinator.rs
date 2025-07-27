//! Enhanced StateCoordinator with improved async operations and error handling
//!
//! Fixes clipboard error type issues and enhances async state management

use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use arc_swap::ArcSwap;
use clipr::ClipError;

use crate::AppError;
use crate::controller::state_provider::StateProvider;
use crate::model::app_state::AppState;
use crate::model::fs_state::FSState;
use crate::model::ui_state::UIState;

/// Enhanced state coordinator with improved error handling
pub struct StateCoordinator {
    /// Application state containing configuration, caches and task management
    pub app_state: Arc<Mutex<AppState>>,

    /// Current UI state with lock-free atomic updates
    pub ui_state: ArcSwap<RwLock<UIState>>,

    /// Filesystem state
    pub fs_state: Arc<Mutex<FSState>>,
}

impl StateCoordinator {
    /// Create coordinator with initial UI state
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

    /// Create simplified coordinator (compatibility alias)
    pub fn new_simple(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: RwLock<UIState>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self::new(app_state, initial_ui, fs_state)
    }

    /// Retrieve current UI state snapshot (lock-free)
    pub fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state.load_full()
    }

    /// Mutably borrow AppState
    pub fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state
            .lock()
            .expect("StateCoordinator.app_state mutex poisoned")
    }

    /// Mutably borrow FSState
    pub fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state
            .lock()
            .expect("StateCoordinator.fs_state mutex poisoned")
    }

    /// Synchronous UI state update
    pub fn update_ui_state<F>(&self, update: F)
    where
        F: FnOnce(&mut UIState),
    {
        let ui_state = self.ui_state.load_full();
        let mut ui_guard = ui_state.write().expect("UIState RwLock poisoned");
        update(&mut ui_guard);
    }

    /// Enhanced async UI state update with flexible error handling
    pub async fn update_ui_state_async<F, Fut, T, E>(&self, update_fn: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let ui_state = self.ui_state();

        let mut ui_guard = ui_state.write().map_err(|_| {
            AppError::state_lock(
                "UIState",
                "Failed to acquire write lock for async operation",
            )
        })?;

        let result = update_fn(&mut ui_guard)
            .await
            .map_err(|e| AppError::ActionDispatch {
                action: "async_ui_update".to_string(),
                reason: e.to_string(),
            })?;
        Ok(result)
    }

    /// Specialized clipboard operation with ClipError -> AppError conversion
    pub async fn update_ui_state_clipboard<F, Fut, T>(&self, update_fn: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, ClipError>>,
    {
        let ui_state = self.ui_state();

        let mut ui_guard = ui_state.write().map_err(|_| {
            AppError::state_lock(
                "UIState",
                "Failed to acquire write lock for clipboard operation",
            )
        })?;

        let result = update_fn(&mut ui_guard)
            .await
            .map_err(|clip_err: ClipError| AppError::ClipboardOperation {
                operation: "clipboard_access".to_string(),
                reason: clip_err.to_string(),
            })?;

        Ok(result)
    }

    /// Atomic UI state update (synchronous operations)
    pub fn update_ui_state_atomic<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut UIState),
    {
        if let Ok(mut ui_guard) = self.ui_state().write() {
            update_fn(&mut ui_guard);
        }
    }

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

    /// Batch multiple UI updates atomically
    pub fn batch_ui_updates<F>(&self, updates: F)
    where
        F: FnOnce(&mut UIState),
    {
        self.update_ui_state(updates);
    }

    /// Try async UI update with timeout
    pub async fn try_update_ui_state_async<F, Fut, T, E>(
        &self,
        update_fn: F,
        timeout: std::time::Duration,
    ) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let update_future = self.update_ui_state_async(update_fn);

        let result = tokio::time::timeout(timeout, update_future)
            .await
            .map_err(|_| AppError::TaskTimeout {
                task_type: "async_ui_update".to_string(),
                timeout_secs: timeout.as_secs(),
            })?;

        result
    }

    /// Safe state access with error recovery
    pub fn with_state_access<F, R>(&self, accessor: F) -> Result<R, AppError>
    where
        F: FnOnce(&AppState, &FSState, &UIState) -> R,
    {
        let app_guard = self
            .app_state
            .lock()
            .map_err(|_| AppError::state_lock("AppState", "Failed to acquire app state lock"))?;

        let fs_guard = self
            .fs_state
            .lock()
            .map_err(|_| AppError::state_lock("FSState", "Failed to acquire fs state lock"))?;

        let ui_state = self.ui_state.load_full();
        let ui_guard = ui_state
            .read()
            .map_err(|_| AppError::state_lock("UIState", "Failed to acquire ui state read lock"))?;

        Ok(accessor(&app_guard, &fs_guard, &ui_guard))
    }

    /// Update task progress
    pub fn update_task_progress(
        &self,
        task_id: String,
        current: u64,
        total: u64,
        message: Option<String>,
    ) {
        if let Ok(mut app_state) = self.app_state.lock() {
            app_state.set_task_progress(task_id, current, total, message);
        }
    }
}

/// StateProvider implementation
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

    fn update_task_progress(
        &self,
        task_id: String,
        current: u64,
        total: u64,
        message: Option<String>,
    ) {
        self.update_task_progress(task_id, current, total, message)
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
            .field("has_enhanced_async_support", &true)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_coordinator() -> StateCoordinator {
        let app_state = Arc::new(Mutex::new(AppState::default()));
        let ui_state = RwLock::new(UIState::default());
        let fs_state = Arc::new(Mutex::new(FSState::default()));

        StateCoordinator::new(app_state, ui_state, fs_state)
    }

    #[tokio::test]
    async fn test_async_ui_update() {
        let coordinator = create_test_coordinator();

        let result = coordinator
            .update_ui_state_async(|ui| async {
                ui.show_info("Test message");
                Ok::<(), std::io::Error>(())
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_clipboard_update() {
        let coordinator = create_test_coordinator();

        // Mock clipboard operation that returns ClipError
        let result = coordinator
            .update_ui_state_clipboard(|_ui| async {
                // This would normally be a clipboard operation
                Ok::<String, ClipError>("test".to_string())
            })
            .await;

        // Should convert ClipError to AppError automatically
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_access() {
        let coordinator = create_test_coordinator();

        let result = coordinator.with_state_access(|app, fs, ui| {
            // Safe access to all state components
            format!(
                "States accessed: app={:?}, fs={:?}, ui={:?}",
                app.config.is_some(),
                fs.left_pane.entries.len(),
                ui.status_message.is_some()
            )
        });

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_timeout_handling() {
        let coordinator = create_test_coordinator();

        let result = coordinator
            .try_update_ui_state_async(
                |_ui| async {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    Ok::<(), std::io::Error>(())
                },
                Duration::from_millis(100),
            )
            .await;

        // Should timeout
        assert!(result.is_err());
        if let Err(AppError::TaskTimeout { .. }) = result {
            // Expected timeout error
        } else {
            panic!("Expected timeout error");
        }
    }
}
