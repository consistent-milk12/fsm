//! fsm-core/src/controller/state_coordinator.rs
//! StateCoordinator with comprehensive tracing instrumentation

use anyhow::Result;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard},
    time::Instant,
};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{Span, debug, info, instrument, trace};

use std::result::Result as StdResult;

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

    // Render notifier for immediate redraws
    render_notifier: Arc<Mutex<Option<UnboundedSender<()>>>>,
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
            render_notifier: Arc::new(Mutex::new(None)),
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

    #[instrument(level = "debug", skip(self, notifier))]
    pub fn set_render_notifier(&self, notifier: UnboundedSender<()>) {
        if let Ok(mut render_notifier) = self.render_notifier.lock() {
            *render_notifier = Some(notifier);

            debug!(
                "STATE_ORCHESTRATOR_RENDER_NOTIFIER : Render notifier set for immediate redraws"
            );
        }
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

    /// Enhanced request_redraw with immediate notification
    #[instrument(level = "debug", skip(self))]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        debug!(?flag, "requesting redraw");

        // Set the redraw flag
        self.update_ui_state(Box::new(move |ui: &mut UIState| {
            ui.request_redraw(flag);
        }));

        // Trigger immediate render notification
        if let Ok(render_notifier) = self.render_notifier.lock()
            && let Some(ref notifier) = *render_notifier
            && let Err(_) = notifier.send(())
        {
            trace!("Failed to send render notification (main loop might be shutting down)");
        }
    }

    /// Check if a redraw is needed.
    #[instrument(level = "trace", skip(self))]
    pub fn needs_redraw(&self) -> bool {
        trace!("checking if redraw is needed");
        match self.ui_state.read() {
            Ok(ui) => {
                let need: bool = ui.needs_redraw();

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
        let result: R = f(&app, &fs, &ui);

        info!(
            marker = "WITH_ALL_STATES_EXIT",
            operation_type = "state_management",
            "with_all_states executed"
        );

        Ok(result)
    }

    #[instrument(
        level = "info",
        skip(self, updated_entry),
        fields(
            marker = "METADATA_UPDATE_START",
            operation_type = "update_entry_metadata",
            current_path = %directory_path.display(),
            duration_us = tracing::field::Empty,
            cache_hit = false
        )
    )]
    pub async fn update_entry_metadata(
        &self,
        directory_path: PathBuf,
        entry_path: PathBuf,
        updated_entry: ObjectInfo,
    ) -> Result<()> {
        let start: Instant = Instant::now();
        let span: Span = Span::current();

        tracing::debug!("Locking FSState mutex");
        let mut fs_state: MutexGuard<'_, FSState> = self.fs_state();

        let success = fs_state
            .active_pane_mut()
            .update_entry_metadata(&entry_path, updated_entry);

        drop(fs_state);

        if success {
            // Record completion metrics
            span.record("marker", "METADATA_UPDATE_COMPLETE");
            span.record("duration_us", start.elapsed().as_micros());

            // Request immediate redraw with notification
            tracing::info!(
                marker = "METADATA_UI_REDRAW_TRIGGERED",
                operation_type = "metadata_update",
                current_path = %directory_path.display(),
                target_path = %entry_path.display(),
                "Triggering immediate UI redraw for metadata update"
            );

            self.request_redraw(RedrawFlag::All);

            Ok(())
        } else {
            span.record("marker", "METADATA_UPDATE FAILED");
            span.record("duration_us", start.elapsed().as_micros());

            Err(anyhow::anyhow!(
                "Failed to update metadata for entry: {}",
                entry_path.display()
            ))
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

    fn update_entry_metadata(
        &self,
        directory_path: &Path,
        entry_path: PathBuf,
        updated_entry: ObjectInfo,
    ) -> StdResult<(), String> {
        let mut fs_state: MutexGuard<'_, FSState> = self.fs_state();
        let active_pane: &mut PaneState = fs_state.active_pane_mut();

        // Check if we're updating entries for the current directory
        if active_pane.cwd != *directory_path {
            return Err(format!(
                "Directory mismatch: current={}, requested={}",
                active_pane.cwd.display(),
                directory_path.display()
            ));
        }

        // Find and update the matching entry in the active pane
        if let Some(entry) = active_pane
            .entries
            .iter_mut()
            .find(|entry: &&mut ObjectInfo| entry.path == entry_path)
        {
            *entry = updated_entry;

            Ok(())
        } else {
            Err(format!(
                "Entry not found: {} in directory {}",
                entry_path.display(),
                directory_path.display()
            ))
        }
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
