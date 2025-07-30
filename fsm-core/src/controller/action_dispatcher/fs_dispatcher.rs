//! fsm-core/src/controller/action_dispatcher/fs_dispatcher.rs
//! File operations dispatcher with comprehensive tracing

use anyhow::{Context, Result};
use dashmap::DashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
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

    /// Navigate into a directory, reload entries, and request redraw.
    #[instrument(level = "info", skip(self, target), fields(target = %target.display()))]
    async fn navigate_to(&self, target: PathBuf) -> Result<DispatchResult> {
        info!(
            target_path = %target.display(),
            "ENTER: Starting navigate_to"
        );

        // Validate directory existence
        if !target.exists() || !target.is_dir() {
            error!("navigate_to: invalid directory {}", target.display());
            self.error("Invalid directory");
            return Ok(DispatchResult::Continue);
        }

        // Load entries asynchronously
        let entries: Vec<ObjectInfo> = self.load_directory(&target).await?;

        info!(
            target_path = %target.display(),
            loaded_entries = entries.len(),
            "ENTER: Loaded directory entries"
        );

        let result: DispatchResult;

        // Update FS state using proper navigation method (includes sorting)
        {
            let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            fs.navigate_to(target.clone());

            // Set entries with proper sorting
            let pane: &mut PaneState = fs.active_pane_mut();
            pane.sort_entries(entries);

            result = Self::load_background_metadata(pane, &target, &self.task_tx)?;

            info!(
                target_path = %target.display(),
                final_entries = pane.entries.len(),
                "ENTER: Navigation and sorting completed"
            );
        }

        info!("ENTER: Requesting redraw after navigate_to");

        self.state_provider.request_redraw(RedrawFlag::All);

        Ok(result)
    }

    /// Read a directory and collect non-hidden entries.
    #[instrument(level = "debug", skip(self, dir), fields(dir = %dir.display()))]
    async fn load_directory(&self, dir: &Path) -> Result<Vec<ObjectInfo>> {
        let mut entries: Vec<ObjectInfo> = Vec::new();
        // Open directory reader
        let mut dir_reader: TokioFs::ReadDir = TokioFs::read_dir(dir)
            .await
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        // Iterate entries
        while let Some(entry) = dir_reader.next_entry().await? {
            let path: PathBuf = entry.path();

            // Filter hidden and non-UTF8 names
            if let Some(name) = path.file_name().and_then(OsStr::to_str)
                && !name.starts_with('.')
            {
                // Attempt lightweight metadata
                match ObjectInfo::from_path_light(&path).await {
                    Ok(info) => {
                        entries.push(ObjectInfo::with_placeholder_metadata(info));

                        trace!("load_directory: added entry {}", name);
                    }

                    Err(e) => {
                        debug!("load_directory: failed to load {}: {:?}", name, e);
                    }
                }
            }
        }

        trace!("load_directory: total entries = {}", entries.len());

        Ok(entries)
    }

    /// Handle the EnterSelected action by navigating if selected entry is a directory.
    #[instrument(level = "info", skip(self))]
    async fn handle_enter_selected(&self) -> Result<DispatchResult> {
        info!("=== ENTER: Starting EnterSelected navigation ===");

        // Determine selected path
        let target: Option<PathBuf> = {
            let fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let pane: &PaneState = fs.active_pane();
            let current_path: PathBuf = pane.cwd.clone();
            let current_entries: usize = pane.entries.len();
            let idx: usize = pane.selected.load(Ordering::Relaxed);

            info!(
                current_path = %current_path.display(),
                current_entries = current_entries,
                selected_index = idx,
                "ENTER: Before navigation"
            );

            pane.entries
                .get(idx)
                .filter(|e: &&ObjectInfo| e.is_dir)
                .map(|e: &ObjectInfo| e.path.clone())
        };

        match target {
            Some(path) => {
                info!(
                    target_path = %path.display(),
                    "ENTER: Navigating into directory"
                );

                self.navigate_to(path).await
            }

            None => {
                info!("ENTER: No directory selected or selection is file");

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle the GoToParent action by navigating to parent directory.
    #[instrument(level = "info", skip(self))]
    async fn handle_go_to_parent(&self) -> Result<DispatchResult> {
        info!("=== BACKSPACE: Starting GoToParent navigation ===");

        let parent_path: Option<PathBuf> = {
            let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let current_path: PathBuf = fs.active_pane().cwd.clone();
            let current_entries: usize = fs.active_pane().entries.len();

            info!(
                current_path = %current_path.display(),
                current_entries = current_entries,
                "BACKSPACE: Before navigate_to_parent"
            );

            // Use FSState's navigation method to change directory only
            if let Some(parent_path) = fs.navigate_to_parent() {
                info!(
                    parent_path = %parent_path.display(),
                    "BACKSPACE: Directory changed, entries need reload"
                );

                Some(parent_path)
            } else {
                info!("BACKSPACE: Already at root directory");

                None
            }
        };

        match parent_path {
            Some(path) => {
                info!(
                    path = %path.display(),
                    "BACKSPACE: Always reload parent directory entries"
                );

                // Always reload parent directory entries (never use cache)
                let entries: Vec<ObjectInfo> = self.load_directory(&path).await?;
                let result: DispatchResult;

                {
                    let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
                    let pane: &mut PaneState = fs.active_pane_mut();
                    pane.sort_entries(entries);

                    result = Self::load_background_metadata(pane, &path, &self.task_tx)?;

                    info!(
                        entries_count = pane.entries.len(),
                        "BACKSPACE: Parent directory entries reloaded"
                    );
                }

                info!("BACKSPACE: Requesting redraw after parent reload");

                self.state_provider.request_redraw(RedrawFlag::All);

                Ok(result)
            }

            None => {
                debug!("handle_go_to_parent: already at root");

                Ok(DispatchResult::Continue)
            }
        }
    }

    fn load_background_metadata(
        pane: &mut PaneState,
        target: &PathBuf,
        task_tx: &UnboundedSender<TaskResult>,
    ) -> Result<DispatchResult> {
        let entries_needing_metadata: Vec<ObjectInfo> = pane
            .entries
            .iter()
            .filter(|entry: &&ObjectInfo| !entry.metadata_loaded)
            .cloned()
            .collect();

        if !entries_needing_metadata.is_empty() {
            debug!(
                metadata_entries_count = entries_needing_metadata.len(),
                "BACKGROUND_TASK_PREPARING: Triggering background metadata loading"
            );

            // Convert to LightObjectInfo for metadata loading
            let entries: Vec<LightObjectInfo> = entries_needing_metadata
                .into_iter()
                .map(|entry: ObjectInfo| LightObjectInfo::new(entry))
                .collect();

            FileSystemOperator::spawn_batch_metadata_load(
                1000,
                target.clone(),
                entries,
                task_tx.clone(),
                10,
            );
        }

        info!(
            target_path = %target.display(),
            final_entries = pane.entries.len(),
            "BACKGROUND_TASK: Successfully loaded metadata in the active pane"
        );

        Ok(DispatchResult::Continue)
    }

    /// Create a new file in the current directory.
    #[instrument(level = "info", skip(self, name), fields(name))]
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
                info!("create_file: created {}", file_path.display());
                self.success(&format!("Created file: {name}"));
                self.navigate_to(cwd).await
            }
            Err(e) => {
                error!(
                    "create_file: failed to create {}: {:?}",
                    file_path.display(),
                    e
                );

                self.error(&format!("Failed to create file: {e}"));

                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Create a new directory in the current directory.
    #[instrument(level = "info", skip(self, name), fields(name))]
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
                info!("create_directory: created {}", dir_path.display());

                self.success(&format!("Created directory: {name}"));
                self.navigate_to(cwd).await
            }

            Err(e) => {
                error!(
                    "create_directory: failed to create {}: {:?}",
                    dir_path.display(),
                    e
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

                info!("handle ReloadDirectory: refreshing {}", cwd.display());

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
