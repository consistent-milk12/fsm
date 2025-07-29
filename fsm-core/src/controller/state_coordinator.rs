//! fsm-core/src/controller/state_coordinator.rs
//! StateCoordinator with comprehensive tracing instrumentation

use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard},
};
use tracing::{debug, info, instrument, trace};

use crate::{
    controller::state_provider::StateProvider,
    fs::object_info::ObjectInfo,
    model::{
        PaneState,
        app_state::AppState,
        fs_state::FSState,
        ui_state::{RedrawFlag, UIState},
    },
};

/// Centralized state coordinator with tracing on all operations.
#[derive(Clone)]
pub struct StateCoordinator {
    pub app_state: Arc<Mutex<AppState>>,
    pub fs_state: Arc<Mutex<FSState>>,
    pub ui_state: Arc<RwLock<UIState>>,
}

impl StateCoordinator {
    /// Create a new StateCoordinator.
    #[instrument(level = "debug", skip(app_state, ui_state, fs_state))]
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        ui_state: Arc<RwLock<UIState>>,
        fs_state: Arc<Mutex<FSState>>,
    ) -> Self {
        debug!("constructing StateCoordinator");
        Self {
            app_state,
            fs_state,
            ui_state,
        }
    }

    /// Acquire the AppState lock.
    #[instrument(level = "trace", skip(self))]
    pub fn app_state(&self) -> MutexGuard<'_, AppState> {
        debug!("locking AppState mutex");

        self.app_state.lock().expect("AppState mutex poisoned")
    }

    /// Acquire the FSState lock.
    #[instrument(level = "trace", skip(self))]
    pub fn fs_state(&self) -> MutexGuard<'_, FSState> {
        debug!("locking FSState mutex");

        self.fs_state.lock().expect("FSState mutex poisoned")
    }

    /// Clone and return the UIState Arc.
    #[instrument(level = "trace", skip(self))]
    pub fn ui_state(&self) -> Arc<RwLock<UIState>> {
        trace!("cloning UIState Arc");

        self.ui_state.clone()
    }

    /// Apply a closure to UIState under write lock.
    #[instrument(level = "debug", skip(self, f))]
    pub fn update_ui_state<F>(&self, f: F)
    where
        F: FnOnce(&mut UIState),
    {
        debug!("acquiring UIState write lock for update");
        // Attempt to acquire write lock on UIState
        match self.ui_state.write() {
            Ok(mut ui) => {
                // Invoke the provided closure to mutate UIState
                f(&mut ui);

                info!("UIState updated successfully");
            }
            Err(poison_err) => {
                // If the lock is poisoned, log and skip the update
                debug!(error = %poison_err, "UIState write lock poisoned; update skipped");
            }
        }
    }

    /// Request a UI redraw with given flag.
    #[instrument(level = "debug", skip(self))]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        debug!(?flag, "requesting redraw");

        self.update_ui_state(|ui| ui.request_redraw(flag));
    }

    /// Check if a redraw is needed.
    #[instrument(level = "trace", skip(self))]
    pub fn needs_redraw(&self) -> bool {
        trace!("checking if redraw is needed");
        match self.ui_state.read() {
            Ok(ui) => {
                let need = ui.needs_redraw();

                debug!(needs = need, "needs_redraw result");

                need
            }

            Err(_) => {
                debug!("UIState read lock poisoned, defaulting needs_redraw=false");

                false
            }
        }
    }

    /// Clear all pending redraw flags.
    #[instrument(level = "debug", skip(self))]
    pub fn clear_redraw(&self) {
        debug!("clearing redraw flags");

        self.update_ui_state(|ui| ui.clear_redraw());
    }

    /// Execute a closure with all three states held.
    #[instrument(level = "debug", skip(self, f))]
    pub fn with_all_states<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&AppState, &FSState, &UIState) -> R,
    {
        debug!("entering with_all_states");

        let app: MutexGuard<'_, AppState> = self.app_state();
        let fs: MutexGuard<'_, FSState> = self.fs_state();
        let ui: RwLockReadGuard<'_, UIState> = self.ui_state.read().expect("UIState lock poisoned");
        let result = f(&app, &fs, &ui);

        info!(
            marker = "WITH_ALL_STATES_EXIT",
            operation_type = "state_management",
            "with_all_states executed"
        );

        Ok(result)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn update_entry_metadata(
        &self,
        directory_path: PathBuf,
        entry_path: PathBuf,
        updated_entry: ObjectInfo,
    ) {
        let mut fs_state: MutexGuard<'_, FSState> = self.fs_state();
        let active_pane: &mut PaneState = fs_state.active_pane_mut();

        // Find and update the matching entry in the active pane
        if let Some(entry) = active_pane
            .entries
            .iter_mut()
            .find(|e: &&mut ObjectInfo| e.path == entry_path)
        {
            *entry = updated_entry;

            debug!(
                entry_path = %entry_path.display(),
                entries_count = active_pane.entries.len(),
                "Updated entry metadata in activepane"
            );
        }
    }
}

impl StateProvider for StateCoordinator {
    #[instrument(level = "trace", skip(self))]
    fn ui_state(&self) -> Arc<RwLock<UIState>> {
        self.ui_state()
    }

    #[instrument(level = "trace", skip(self))]
    fn fs_state(&self) -> MutexGuard<'_, FSState> {
        self.fs_state()
    }

    #[instrument(level = "trace", skip(self))]
    fn app_state(&self) -> MutexGuard<'_, AppState> {
        self.app_state()
    }

    #[instrument(level = "trace", skip(self, f))]
    fn update_ui_state(&self, f: Box<dyn FnOnce(&mut UIState) + Send>) {
        self.update_ui_state(|ui| f(ui));
    }

    #[instrument(level = "trace", skip(self, flag))]
    fn request_redraw(&self, flag: RedrawFlag) {
        self.request_redraw(flag);
    }

    #[instrument(level = "trace", skip(self))]
    fn needs_redraw(&self) -> bool {
        self.needs_redraw()
    }

    #[instrument(level = "trace", skip(self))]
    fn clear_redraw(&self) {
        self.clear_redraw();
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
