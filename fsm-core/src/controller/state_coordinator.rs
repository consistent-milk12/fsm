// fsm-core/src/controller/state_coordinator.rs
// Simplified state coordination with clean separation

use anyhow::Result;
use std::sync::{Arc, Mutex, RwLock};

use crate::{
    controller::state_provider::StateProvider,
    model::{
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
};

/// Centralized state coordinator with clean access patterns  
pub struct StateCoordinator {
    app_state: Arc<Mutex<AppState>>,
    fs_state: Arc<Mutex<FSState>>,
    ui_state: Arc<RwLock<UIState>>,
}

impl StateCoordinator {
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        ui_state: Arc<RwLock<UIState>>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        Self {
            app_state,
            fs_state,
            ui_state,
        }
    }

    // Direct state access
    pub fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state.lock().expect("AppState mutex poisoned")
    }

    pub fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state.lock().expect("FSState mutex poisoned")
    }

    pub fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state.clone()
    }

    // UI state helpers
    pub fn update_ui_state<F>(&self, f: F)
    where
        F: FnOnce(&mut UIState),
    {
        if let Ok(mut ui) = self.ui_state.write() {
            f(&mut ui);
        }
    }

    pub fn request_redraw(&self, flag: RedrawFlag) {
        self.update_ui_state(|ui| ui.request_redraw(flag));
    }

    pub fn needs_redraw(&self) -> bool {
        self.ui_state
            .read()
            .map(|ui| ui.needs_redraw())
            .unwrap_or(false)
    }

    pub fn clear_redraw(&self) {
        self.update_ui_state(|ui| ui.clear_redraw());
    }

    // Combined state access for complex operations
    pub fn with_all_states<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&AppState, &FSState, &UIState) -> R,
    {
        let app = self.app_state();
        let fs = self.fs_state();
        let ui = self.ui_state.read().expect("UI state poisoned");

        Ok(f(&app, &fs, &ui))
    }
}

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

impl std::fmt::Debug for StateCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateCoordinator")
            .field("app_poisoned", &self.app_state.is_poisoned())
            .field("fs_poisoned", &self.fs_state.is_poisoned())
            .finish()
    }
}
