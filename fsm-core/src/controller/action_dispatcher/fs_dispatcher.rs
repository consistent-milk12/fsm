//! fsm-core/src/controller/action_dispatcher/fs_dispatcher.rs
//! File operations dispatcher with comprehensive tracing

use anyhow::{Context, Result};
use dashmap::DashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};
use std::sync::atomic::Ordering;
use tokio::fs as TokioFs;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace};

use crate::FSState;
use crate::controller::actions::OperationId;
use crate::controller::state_provider::StateProvider;
use crate::controller::{Action, TaskResult};
use crate::fs::object_info::ObjectInfo;
use crate::model::PaneState;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Dispatcher for filesystem operations with tracing.
#[derive(Clone, Debug)]
pub struct FileOpsDispatcher {
    /// Provides access to shared application state.
    state_provider: Arc<dyn StateProvider>,
    /// Channel to send completed task results.
    task_tx: mpsc::UnboundedSender<TaskResult>,
    /// Tracks active cancellable operations.
    active_operations: DashMap<OperationId, CancellationToken>,
}

impl FileOpsDispatcher {
    /// Construct a new FileOpsDispatcher.
    #[instrument(level = "debug", skip(state_provider, task_tx))]
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: mpsc::UnboundedSender<TaskResult>,
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
        // Validate directory existence
        if !target.exists() || !target.is_dir() {
            error!("navigate_to: invalid directory {}", target.display());
            self.error("Invalid directory");
            return Ok(DispatchResult::Continue);
        }

        // Load entries asynchronously
        let entries: Vec<ObjectInfo> = self.load_directory(&target).await?;

        // Update FS state
        {
            let mut fs: MutexGuard<'_, FSState> = self.state_provider.fs_state();
            let pane: &mut PaneState = fs.active_pane_mut();
            pane.cwd = target.clone();
            pane.entries = entries;
            pane.selected.store(0, Ordering::Relaxed);
        }

        info!("navigate_to: directory loaded, requesting full redraw");
        self.state_provider.request_redraw(RedrawFlag::All);
        Ok(DispatchResult::Continue)
    }

    /// Read a directory and collect non-hidden entries.
    #[instrument(level = "debug", skip(self, dir), fields(dir = %dir.display()))]
    async fn load_directory(&self, dir: &Path) -> Result<Vec<ObjectInfo>> {
        let mut entries = Vec::new();
        // Open directory reader
        let mut dir_reader = TokioFs::read_dir(dir)
            .await
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        // Iterate entries
        while let Some(entry) = dir_reader.next_entry().await? {
            let path = entry.path();
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
        // Determine selected path
        let target = {
            let fs = self.state_provider.fs_state();
            let pane = fs.active_pane();
            let idx = pane.selected.load(Ordering::Relaxed);
            pane.entries
                .get(idx)
                .filter(|e| e.is_dir)
                .map(|e| e.path.clone())
        };

        match target {
            Some(path) => {
                info!("handle_enter_selected: navigating into {}", path.display());
                self.navigate_to(path).await
            }
            None => {
                debug!("handle_enter_selected: no directory selected");
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle the GoToParent action by navigating to parent directory.
    #[instrument(level = "info", skip(self))]
    async fn handle_go_to_parent(&self) -> Result<DispatchResult> {
        let parent = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.parent().map(Path::to_path_buf)
        };

        match parent {
            Some(path) => {
                info!("handle_go_to_parent: navigating to {}", path.display());
                self.navigate_to(path).await
            }
            None => {
                debug!("handle_go_to_parent: already at root");
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Create a new file in the current directory.
    #[instrument(level = "info", skip(self, name), fields(name))]
    async fn create_file(&self, name: &str) -> Result<DispatchResult> {
        // Build file path
        let (cwd, file_path) = {
            let fs = self.state_provider.fs_state();
            let dir = fs.active_pane().cwd.clone();
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
            let fs = self.state_provider.fs_state();
            let dir = fs.active_pane().cwd.clone();
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
                let cwd = self.state_provider.fs_state().active_pane().cwd.clone();
                info!("handle ReloadDirectory: refreshing {}", cwd.display());
                self.navigate_to(cwd).await
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
        let msg = msg.to_string();
        debug!("success: {}", msg);
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    /// Record an error notification in UI state.
    #[instrument(level = "trace", skip(self, msg))]
    fn error(&self, msg: &str) {
        let msg = msg.to_string();
        debug!("error: {}", msg);
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
}
