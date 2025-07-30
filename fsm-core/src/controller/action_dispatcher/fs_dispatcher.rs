//! fsm-core/src/controller/action_dispatcher/fs_dispatcher.rs
//! File operations dispatcher with comprehensive tracing

use anyhow::Result;
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard};
use std::time::Instant;
use tokio::fs as TokioFs;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace};

use crate::FSState;
use crate::controller::actions::OperationId;
use crate::controller::state_provider::StateProvider;
use crate::controller::{Action, TaskResult};
use crate::fs::object_info::ObjectInfo;
use crate::model::PaneState;
use crate::model::ui_state::{RedrawFlag, UIState};
use crate::operators::file_system_operator::FileSystemOperator;

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Dispatcher for filesystem operations with tracing.
#[derive(Clone, Debug)]
pub struct FileOpsDispatcher {
    /// Provides access to shared application state.
    state_provider: Arc<dyn StateProvider>,

    /// Channel to send completed task results.
    task_tx: UnboundedSender<TaskResult>,

    /// Tracks active cancellable operations.
    active_operations: DashMap<OperationId, CancellationToken>,
}

impl FileOpsDispatcher {
    /// Construct a new FileOpsDispatcher.
    #[instrument(level = "debug", skip(state_provider, task_tx))]
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        Self {
            state_provider,
            task_tx,
            active_operations: DashMap::new(),
        }
    }

    /// Navigate into a directory using FileSystemOperator for all loading.
    #[instrument(
        level = "info",
        name = "navigate_to",
        skip(self, target),
        err,
        fields(
            marker = "ENTER_START",
            operation_type = "directory_entry",
            target_path = %target.display(),
            current_path = tracing::field::Empty,
            entries_count = tracing::field::Empty,
            selected_index = tracing::field::Empty,
            duration_us = tracing::field::Empty,
            cache_hit = false
        )
    )]
    async fn navigate_to(&self, target: PathBuf) -> Result<DispatchResult> {
        let start = Instant::now();
        let span = tracing::Span::current();

        // Validate directory existence
        if !target.exists() || !target.is_dir() {
            let message = format!("navigate_to: invalid directory {}", target.display());
            span.record("marker", "ENTER_ERROR");
            self.error("Invalid directory");
            // The error message for the log is taken from the Err variant
            return Err(anyhow::anyhow!(message));
        }

        // Update navigation state first
        let (current_path_before, entries_count_before, selected_index_before) = {
            let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            fs.navigate_to(target.clone());

            let pane: &mut PaneState = fs.active_pane_mut();

            let current_path = pane.cwd.clone();
            let entries_count = pane.entries.len();
            let selected_index = pane.selected.load(Ordering::Relaxed);

            // Clear existing entries while waiting for FileSystemOperator results
            pane.sort_entries(Vec::new()); // Clear entries, they will be populated by TaskResult

            info!(
                marker = "STATE_AFTER",
                operation_type = "directory_entry",
                current_path = %pane.cwd.display(),
                entries_count = pane.entries.len(),
                selected_index = pane.selected.load(Ordering::Relaxed),
                message = "Navigation state updated, entries cleared"
            );
            (current_path, entries_count, selected_index)
        };

        span.record("current_path", &current_path_before.display().to_string());
        span.record("entries_count", entries_count_before);
        span.record("selected_index", selected_index_before);

        // Use FileSystemOperator for complete directory scanning (replaces both load_directory and load_background_metadata)
        FileSystemOperator::spawn_two_phase_directory_scan(
            2001, // task_id for navigation
            target.clone(),
            false, // show_hidden
            self.task_tx.clone(),
            CancellationToken::new(),
        );

        span.record("marker", "ENTER_COMPLETE");
        span.record("duration_us", start.elapsed().as_micros());

        info!("Two-phase directory scan initiated via FileSystemOperator");

        self.state_provider.request_redraw(RedrawFlag::All);

        Ok(DispatchResult::Continue)
    }

    /// Handle the EnterSelected action by navigating if selected entry is a directory.
    #[instrument(
        level = "info",
        skip(self),
        fields(
            marker = "ENTER_SELECTED_START",
            operation_type = "navigation",
            message = "Starting EnterSelected navigation"
        )
    )]
    async fn handle_enter_selected(&self) -> Result<DispatchResult> {
        // Determine selected path
        let target: Option<PathBuf> = {
            let fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let pane: &PaneState = fs.active_pane();
            let current_path: PathBuf = pane.cwd.clone();
            let current_entries: usize = pane.entries.len();
            let idx: usize = pane.selected.load(Ordering::Relaxed);

            info!(
                marker = "STATE_BEFORE",
                operation_type = "navigation",
                current_path = %current_path.display(),
                entries_count = current_entries,
                selected_index = idx,
                message = "Before navigation"
            );

            pane.entries
                .get(idx)
                .filter(|e: &&ObjectInfo| e.is_dir)
                .map(|e: &ObjectInfo| e.path.clone())
        };

        match target {
            Some(path) => {
                info!(
                    marker = "NAVIGATE_TO_START",
                    operation_type = "navigation",
                    target_path = %path.display(),
                    message = "Navigating into directory"
                );

                self.navigate_to(path).await
            }

            None => {
                info!(
                    marker = "NO_ENTRY_AT_INDEX",
                    operation_type = "navigation",
                    message = "No directory selected or selection is file"
                );

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle the GoToParent action by navigating to parent directory.
    #[instrument(
        level = "info",
        skip(self),
        fields(
            marker = "NAVIGATE_PARENT_START",
            operation_type = "navigation",
            message = "Starting GoToParent navigation"
        )
    )]
    async fn handle_go_to_parent(&self) -> Result<DispatchResult> {
        let parent_path: Option<PathBuf> = {
            let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let current_path: PathBuf = fs.active_pane().cwd.clone();
            let current_entries: usize = fs.active_pane().entries.len();

            info!(
                marker = "STATE_BEFORE",
                operation_type = "navigation",
                current_path = %current_path.display(),
                entries_count = current_entries,
                message = "Before navigate_to_parent"
            );

            // Use FSState's navigation method to change directory only
            if let Some(parent_path) = fs.navigate_to_parent() {
                info!(
                    marker = "NAVIGATE_PARENT_COMPLETE",
                    operation_type = "navigation",
                    target_path = %parent_path.display(),
                    message = "Directory changed, entries need reload"
                );

                Some(parent_path)
            } else {
                info!(
                    marker = "NAVIGATE_PARENT_BLOCKED",
                    operation_type = "navigation",
                    message = "Already at root directory"
                );

                None
            }
        };

        match parent_path {
            Some(path) => {
                info!(
                    marker = "DIRECTORY_SCAN_START",
                    operation_type = "file_system",
                    current_path = %path.display(),
                    message = "Loading parent directory via FileSystemOperator"
                );

                // Clear current entries and trigger FileSystemOperator scan
                {
                    let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
                    let pane: &mut PaneState = fs.active_pane_mut();
                    pane.sort_entries(Vec::new()); // Clear entries, they will be populated by TaskResult

                    info!(
                        marker = "DIRECTORY_ENTRIES_CLEARED",
                        operation_type = "file_system",
                        message = "Parent directory entries cleared, awaiting FileSystemOperator"
                    );
                }

                // Use FileSystemOperator for complete directory scanning
                FileSystemOperator::spawn_two_phase_directory_scan(
                    2002, // task_id for parent navigation
                    path.clone(),
                    false, // show_hidden
                    self.task_tx.clone(),
                    CancellationToken::new(),
                );

                info!(
                    marker = "TWO_PHASE_SCAN_START",
                    operation_type = "file_system",
                    current_path = %path.display(),
                    message = "Two-phase directory scan initiated via FileSystemOperator"
                );

                self.state_provider.request_redraw(RedrawFlag::All);

                Ok(DispatchResult::Continue)
            }

            None => {
                debug!("handle_go_to_parent: already at root");

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Create a new file in the current directory.
    #[instrument(
        level = "info",
        skip(self, name),
        fields(
            marker = "FILE_CREATE_START",
            operation_type = "file_system",
            target_path = name,
            message = "Initiating file creation"
        )
    )]
    async fn create_file(&self, name: &str) -> Result<DispatchResult> {
        // Build file path
        let (cwd, file_path) = {
            let fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let dir: PathBuf = fs.active_pane().cwd.clone();
            (dir.clone(), dir.join(name))
        };

        // Attempt file creation
        match TokioFs::File::create(&file_path).await {
            Ok(_) => {
                info!(
                    marker = "FILE_CREATED",
                    operation_type = "file_system",
                    target_path = %file_path.display(),
                    message = format!("File created: {}", file_path.display())
                );
                self.success(&format!("Created file: {name}"));
                self.navigate_to(cwd).await
            }
            Err(e) => {
                error!(
                    marker = "FILE_OPERATION_FAILED",
                    operation_type = "file_system",
                    target_path = %file_path.display(),
                    error = %e,
                    message = format!("Failed to create file: {}: {:?}", file_path.display(), e)
                );

                self.error(&format!("Failed to create file: {e}"));

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Create a new directory in the current directory.
    #[instrument(
        level = "info",
        skip(self, name),
        fields(
            marker = "DIRECTORY_CREATE_START",
            operation_type = "file_system",
            target_path = name,
            message = "Initiating directory creation"
        )
    )]
    async fn create_directory(&self, name: &str) -> Result<DispatchResult> {
        // Build directory path
        let (cwd, dir_path) = {
            let fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let dir: PathBuf = fs.active_pane().cwd.clone();

            (dir.clone(), dir.join(name))
        };

        // Attempt directory creation
        match TokioFs::create_dir(&dir_path).await {
            Ok(_) => {
                info!(
                    marker = "DIRECTORY_CREATED",
                    operation_type = "file_system",
                    target_path = %dir_path.display(),
                    message = format!("Directory created: {}", dir_path.display())
                );

                self.success(&format!("Created directory: {name}"));
                self.navigate_to(cwd).await
            }

            Err(e) => {
                error!(
                    marker = "FILE_OPERATION_FAILED",
                    operation_type = "file_system",
                    target_path = %dir_path.display(),
                    error = %e,
                    message = format!("Failed to create directory: {}: {:?}", dir_path.display(), e)
                );

                self.error(&format!("Failed to create directory: {e}"));

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Entry point for all file‑ops actions.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::EnterSelected => self.handle_enter_selected().await,

            Action::GoToParent => self.handle_go_to_parent().await,

            Action::CreateFileWithName(name) => self.create_file(&name).await,

            Action::CreateDirectoryWithName(name) => self.create_directory(&name).await,

            Action::ReloadDirectory => {
                let cwd: PathBuf = self.state_provider.fs_state().active_pane().cwd.clone();

                info!(
                    marker = "RELOAD_DIRECTORY_START",
                    operation_type = "file_system",
                    current_path = %cwd.display(),
                    message = format!("Reloading directory: {}", cwd.display())
                );

                self.navigate_to(cwd).await
            }

            Action::UpdateEntryMetadata {
                directory_path,
                entry_path,
                updated_entry,
            } => {
                info!(
                    marker = "FILE_OPERATION_DISPATCH_START",
                    operation_type = "update_entry_metadata",
                    current_path = %directory_path.display(),
                    target_path = %entry_path.display(),
                    entries_count = 1,
                    selected_index ="NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    "FILE_OPERATION_DISPATCHER : Handling UpdateEntryMetadata action"
                );

                let start = Instant::now();

                // Update entry metadata through StateCoordinator
                match self.state_provider.update_entry_metadata(
                    &directory_path,
                    entry_path.clone(),
                    updated_entry.clone(),
                ) {
                    Ok(()) => {
                        info!(
                            marker = "FILE_OPERATION_DISPATCH_COMPLETE",
                            operation_type = "update_entry_metadata",
                            current_path = %directory_path.display(),
                            target_path = %entry_path.display(),
                            entries_count = 1,
                            selected_index = "NULL",
                            duration_us = start.elapsed().as_micros(),
                            cache_hit = false,
                            area_width = "NULL",
                            area_height = "NULL",
                            "FILE_OPERATION_DISPATCHER : Entry metadata updated successfully"
                        );

                        // Request UI redraw to show updated metadata
                        self.state_provider.request_redraw(RedrawFlag::All);

                        Ok(DispatchResult::Continue)
                    }

                    Err(e) => {
                        error!(
                            marker = "FILE_OPERATION_DISPATCH_FAILED",
                            operation_type = "update_entry_metadata",
                            current_path = %directory_path.display(),
                            target_path = %entry_path.display(),
                            entries_count = 1,
                            selected_index = "NULL",
                            duration_us = start.elapsed().as_micros(),
                            cache_hit = false,
                            area_width = "NULL",
                            area_height = "NULL",
                            "FILE_OPERATION_DISPATCHER : Failed to update entry metadata: {e}"
                        );

                        self.error(&format!("Failed to update entry metadata: {e}"));

                        Ok(DispatchResult::Continue)
                    }
                }
            }

            _ => {
                trace!("handle: action not handled by file_ops");

                Ok(DispatchResult::NotHandled)
            }
        }
    }

    /// Record a success notification in UI state.
    #[instrument(level = "trace", skip(self, msg))]
    fn success(&self, msg: &str) {
        let msg: String = msg.to_string();

        debug!("success: {}", msg);

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    /// Record an error notification in UI state.
    #[instrument(level = "trace", skip(self, msg))]
    fn error(&self, msg: &str) {
        let msg: String = msg.to_string();

        debug!(
            marker = "NOTIFICATION_ERROR",
            operation_type = "ui_update",
            "error: {}",
            msg
        );

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
    }
}

impl ActionMatcher for FileOpsDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::EnterSelected
                | Action::GoToParent
                | Action::CreateFileWithName(_)
                | Action::CreateDirectoryWithName(_)
                | Action::ReloadDirectory
                | Action::UpdateEntryMetadata { .. }
        )
    }

    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "file_ops"
    }

    fn dynamic_priority(&self, _action: &Action) -> ActionPriority {
        self.priority()
    }

    fn can_disable(&self) -> bool {
        true
    }
}
