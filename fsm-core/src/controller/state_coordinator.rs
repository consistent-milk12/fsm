//! src/controller/state_coordinator.rs
//! ============================================================
//! Central access point for `AppState`, `FSState`, and the new
//! slim `UIState`.  All UI-mutating helpers are thin wrappers
//! around a single locking primitive, so that widgets remain
//! lock-free during rendering.

use std::{
    future::Future,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::Result;
use arc_swap::ArcSwap;
use clipr::ClipError;
use tokio::time::timeout;

use crate::{
    AppError,
    controller::state_provider::StateProvider,
    model::{
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
};

/// Thread-safe coordinator for global states.
///
/// * `AppState` – config, async tasks, etc. (guarded by `Mutex`)
/// * `FSState`  – panes, operations, history (guarded by `Mutex`)
/// * `UIState`  – atomics + small fields (lock-free reads via
///                `ArcSwap`, writes behind `RwLock`)
pub struct StateCoordinator {
    app_state: Arc<Mutex<AppState>>,
    ui_state: ArcSwap<RwLock<UIState>>,
    fs_state: Arc<Mutex<FSState>>,
}

// ------------------------------------------------------------
// ctor & cheap getters
// ------------------------------------------------------------
impl StateCoordinator {
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

    /// Convenience alias kept for compatibility
    pub fn new_simple(
        app_state: Arc<Mutex<AppState>>,
        initial_ui: RwLock<UIState>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self::new(app_state, initial_ui, fs_state)
    }

    #[inline]
    pub fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state.load_full()
    }
    #[inline]
    pub fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state.lock().expect("AppState mutex poisoned")
    }
    #[inline]
    pub fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state.lock().expect("FSState mutex poisoned")
    }
}

// ------------------------------------------------------------
// Synchronous mutators
// ------------------------------------------------------------
impl StateCoordinator {
    /// Lock `UIState` for a short, synchronous mutation.
    pub fn update_ui_state<F>(&self, f: F)
    where
        F: FnOnce(&mut UIState),
    {
        if let Ok(mut guard) = self.ui_state().write() {
            f(&mut guard);
        }
    }

    /// Atomic mutation helper (no error propagation).
    #[inline]
    pub fn update_ui_state_atomic<F>(&self, f: F)
    where
        F: FnOnce(&mut UIState),
    {
        self.update_ui_state(f);
    }

    /// Request a redraw for the given component(s).
    #[inline]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        self.update_ui_state(|ui| ui.request_redraw(flag));
    }

    #[inline]
    pub fn clear_redraw(&self) {
        self.update_ui_state(|ui| ui.clear_redraw());
    }

    #[inline]
    pub fn needs_redraw(&self) -> bool {
        let binding = self.ui_state();

        let guard = binding.read().expect("UIState RwLock poisoned");
        guard.needs_redraw()
    }
}

// ------------------------------------------------------------
// Asynchronous mutators with error mapping
// ------------------------------------------------------------
impl StateCoordinator {
    /// Generic async mutation with custom error mapping.
    pub async fn update_ui_state_async<F, Fut, T, E>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let binding: Arc<RwLock<UIState>> = self.ui_state();

        let mut guard = binding
            .write()
            .map_err(|_| AppError::state_lock("UIState", "write lock poisoned"))?;

        f(&mut guard).await.map_err(|e| AppError::ActionDispatch {
            action: "async_ui_update".into(),
            reason: e.to_string(),
        })
    }

    /// Same as above, but maps `ClipError` automatically.
    pub async fn update_ui_state_clipboard<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, ClipError>>,
    {
        let binding: Arc<RwLock<UIState>> = self.ui_state();

        let mut guard = binding
            .write()
            .map_err(|_| AppError::state_lock("UIState", "write lock poisoned"))?;

        f(&mut guard)
            .await
            .map_err(|e| AppError::ClipboardOperation {
                operation: "clipboard".into(),
                reason: e.to_string(),
            })
    }

    /// Async mutation with timeout.
    pub async fn try_update_ui_state_async<F, Fut, T, E>(
        &self,
        f: F,
        timeout_dur: Duration,
    ) -> Result<T, AppError>
    where
        F: FnOnce(&mut UIState) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let fut = self.update_ui_state_async(f);
        timeout(timeout_dur, fut)
            .await
            .map_err(|_| AppError::TaskTimeout {
                task_type: "async_ui_update".into(),
                timeout_secs: timeout_dur.as_secs(),
            })?
    }
}

// ------------------------------------------------------------
// Safe read-only accessor
// ------------------------------------------------------------
impl StateCoordinator {
    pub fn with_state_access<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&AppState, &FSState, &UIState) -> R,
    {
        let app = self
            .app_state
            .lock()
            .map_err(|_| AppError::state_lock("AppState", "lock poisoned"))?;

        let fs = self
            .fs_state
            .lock()
            .map_err(|_| AppError::state_lock("FSState", "lock poisoned"))?;

        let binding = self.ui_state();
        let ui = binding
            .read()
            .map_err(|_| AppError::state_lock("UIState", "read lock poisoned"))?;

        Ok(f(&app, &fs, &ui))
    }
}

// ------------------------------------------------------------
// Task progress passthrough
// ------------------------------------------------------------
impl StateCoordinator {
    pub fn update_task_progress(&self, id: String, current: u64, total: u64, msg: Option<String>) {
        if let Ok(mut app) = self.app_state.lock() {
            app.set_task_progress(id, current, total, msg);
        }
    }
}

// ------------------------------------------------------------
// StateProvider impl (for action dispatcher)
// ------------------------------------------------------------
impl StateProvider for StateCoordinator {
    fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state()
    }
    fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state()
    }
    fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state()
    }

    fn update_ui_state(&self, f: Box<dyn FnOnce(&mut UIState) + Send>) {
        self.update_ui_state(|ui| f(ui));
    }

    fn update_task_progress(&self, id: String, cur: u64, tot: u64, msg: Option<String>) {
        self.update_task_progress(id, cur, tot, msg);
    }

    fn request_redraw(&self, flag: RedrawFlag) {
        self.request_redraw(flag);
    }
    fn needs_redraw(&self) -> bool {
        self.needs_redraw()
    }
    fn clear_redraw(&self) {
        self.clear_redraw()
    }
}

// ------------------------------------------------------------
// Debug
// ------------------------------------------------------------
impl std::fmt::Debug for StateCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateCoordinator")
            .field("app_state_locked", &self.app_state.is_poisoned())
            .field("fs_state_locked", &self.fs_state.is_poisoned())
            .finish()
    }
}
