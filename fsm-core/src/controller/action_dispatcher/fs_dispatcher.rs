//! Async file operations with proper resource management

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::fs as TokioFs;
use tokio::sync::mpsc::UnboundedSender;

use crate::controller::Action;
use crate::controller::event_loop::TaskResult;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo, ObjectType};
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// File operations dispatcher with async safety
#[derive(Clone)]
pub struct FileOpsDispatcher {
    state_provider: Arc<dyn StateProvider>,
    #[allow(unused)]
    task_tx: UnboundedSender<TaskResult>,
}

impl FileOpsDispatcher {
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        Self {
            state_provider,
            task_tx,
        }
    }

    /// Navigate to directory without holding locks during I/O
    async fn navigate_to_directory(&self, target_path: PathBuf) -> Result<DispatchResult> {
        // Validate path first (fast check)
        if !target_path.exists() {
            self.error("Directory does not exist");
            return Ok(DispatchResult::Continue);
        }

        if !target_path.is_dir() {
            self.error("Path is not a directory");
            return Ok(DispatchResult::Continue);
        }

        // Load directory contents without holding state locks
        match self.load_directory_contents(&target_path).await {
            Ok(entries) => {
                // Apply results to state (short lock duration)
                {
                    let mut fs = self.state_provider.fs_state();
                    let pane = fs.active_pane_mut();
                    pane.cwd = target_path;
                    pane.entries = entries;
                    pane.selected.store(0, Ordering::Relaxed);
                }

                self.state_provider.request_redraw(RedrawFlag::All);
                Ok(DispatchResult::Continue)
            }
            Err(e) => {
                self.error(&format!("Failed to load directory: {e}"));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Load directory contents without state locks
    async fn load_directory_contents(&self, directory: &Path) -> Result<Vec<ObjectInfo>> {
        let mut entries = Vec::new();

        // Add parent directory entry if not at root
        if let Some(parent) = directory.parent() {
            let light_parent = LightObjectInfo {
                path: parent.to_path_buf(),
                name: "..".to_string().into(),
                extension: None,
                object_type: ObjectType::Dir,
                is_dir: true,
                is_symlink: false,
            };
            entries.push(ObjectInfo::with_placeholder_metadata(light_parent));
        }

        // Read directory entries
        let mut dir_reader = TokioFs::read_dir(directory)
            .await
            .with_context(|| format!("Failed to read directory: {}", directory.display()))?;

        while let Some(entry) = dir_reader.next_entry().await? {
            let entry_path = entry.path();

            // Skip hidden files (configurable later)
            if self.should_skip_entry(&entry_path) {
                continue;
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                }
                Err(e) => {
                    tracing::debug!("Failed to read entry {:?}: {}", entry_path, e);
                }
            }
        }

        Ok(entries)
    }

    /// Check if entry should be skipped
    fn should_skip_entry(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|name: &OsStr| name.to_str())
            .map(|name: &str| name.starts_with('.'))
            .unwrap_or(false)
    }

    /// Handle directory entry
    async fn handle_enter_selected(&self) -> Result<DispatchResult> {
        let target_path = {
            let fs = self.state_provider.fs_state();
            let pane = fs.active_pane();
            let current = pane.selected.load(Ordering::Relaxed);

            match pane.entries.get(current) {
                Some(entry) if entry.is_dir => Some(entry.path.clone()),
                Some(entry) => {
                    tracing::info!("Opening file: {:?}", entry.path);
                    // TODO: Implement file opening
                    None
                }
                None => None,
            }
        };

        if let Some(path) = target_path {
            self.navigate_to_directory(path).await
        } else {
            Ok(DispatchResult::Continue)
        }
    }

    /// Handle parent directory navigation
    async fn handle_go_to_parent(&self) -> Result<DispatchResult> {
        let parent_path = {
            let fs = self.state_provider.fs_state();
            let pane = fs.active_pane();
            pane.cwd.parent().map(|p: &Path| p.to_path_buf())
        };

        if let Some(parent) = parent_path {
            self.navigate_to_directory(parent).await
        } else {
            Ok(DispatchResult::Continue)
        }
    }

    /// Create file operation
    async fn create_file(&self, name: &str) -> Result<DispatchResult> {
        let (current_dir, file_path) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            let file_path = current_dir.join(name);
            (current_dir, file_path)
        };

        match TokioFs::File::create(&file_path).await {
            Ok(_) => {
                self.success(&format!("Created file: {name}"));
                // Reload directory
                self.navigate_to_directory(current_dir).await
            }
            Err(e) => {
                self.error(&format!("Failed to create file: {e}"));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Create directory operation
    async fn create_directory(&self, name: &str) -> Result<DispatchResult> {
        let (current_dir, dir_path) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            let dir_path = current_dir.join(name);
            (current_dir, dir_path)
        };

        match TokioFs::create_dir(&dir_path).await {
            Ok(_) => {
                self.success(&format!("Created directory: {name}"));
                // Reload directory
                self.navigate_to_directory(current_dir).await
            }
            Err(e) => {
                self.error(&format!("Failed to create directory: {e}"));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::EnterSelected => self.handle_enter_selected().await,
            Action::GoToParent => self.handle_go_to_parent().await,
            Action::CreateFileWithName(name) => self.create_file(&name).await,
            Action::CreateDirectoryWithName(name) => self.create_directory(&name).await,
            Action::ReloadDirectory => {
                let current_dir = {
                    let fs = self.state_provider.fs_state();
                    fs.active_pane().cwd.clone()
                };
                self.navigate_to_directory(current_dir).await
            }
            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn success(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    fn error(&self, message: &str) {
        let msg = message.to_string();
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

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "file_ops"
    }
}
