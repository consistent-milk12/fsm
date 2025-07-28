// fsm-core/src/controller/action_dispatcher/fs_dispatcher.rs
// File operations with async safety

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::fs as TokioFs;

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::ObjectInfo;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

#[derive(Clone)]
pub struct FileOpsDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl FileOpsDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    async fn navigate_to(&self, target: PathBuf) -> Result<DispatchResult> {
        if !target.exists() || !target.is_dir() {
            self.error("Invalid directory");
            return Ok(DispatchResult::Continue);
        }

        let entries = self.load_directory(&target).await?;

        {
            let mut fs = self.state_provider.fs_state();
            let pane = fs.active_pane_mut();
            pane.cwd = target;
            pane.entries = entries;
            pane.selected.store(0, Ordering::Relaxed);
        }

        self.state_provider.request_redraw(RedrawFlag::All);
        Ok(DispatchResult::Continue)
    }

    async fn load_directory(&self, dir: &std::path::Path) -> Result<Vec<ObjectInfo>> {
        let mut entries = Vec::new();
        let mut dir_reader = TokioFs::read_dir(dir)
            .await
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        while let Some(entry) = dir_reader.next_entry().await? {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with('.') {
                    if let Ok(info) = ObjectInfo::from_path_light(&path).await {
                        entries.push(ObjectInfo::with_placeholder_metadata(info));
                    }
                }
            }
        }

        Ok(entries)
    }

    async fn handle_enter_selected(&self) -> Result<DispatchResult> {
        let target = {
            let fs = self.state_provider.fs_state();
            let pane = fs.active_pane();
            let current = pane.selected.load(Ordering::Relaxed);

            pane.entries
                .get(current)
                .filter(|entry| entry.is_dir)
                .map(|entry| entry.path.clone())
        };

        if let Some(path) = target {
            self.navigate_to(path).await
        } else {
            Ok(DispatchResult::Continue)
        }
    }

    async fn handle_go_to_parent(&self) -> Result<DispatchResult> {
        let parent = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.parent().map(|p| p.to_path_buf())
        };

        if let Some(path) = parent {
            self.navigate_to(path).await
        } else {
            Ok(DispatchResult::Continue)
        }
    }

    async fn create_file(&self, name: &str) -> Result<DispatchResult> {
        let (current_dir, file_path) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            (current_dir.clone(), current_dir.join(name))
        };

        match TokioFs::File::create(&file_path).await {
            Ok(_) => {
                self.success(&format!("Created file: {}", name));
                self.navigate_to(current_dir).await
            }
            Err(e) => {
                self.error(&format!("Failed to create file: {}", e));
                Ok(DispatchResult::Continue)
            }
        }
    }

    async fn create_directory(&self, name: &str) -> Result<DispatchResult> {
        let (current_dir, dir_path) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            (current_dir.clone(), current_dir.join(name))
        };

        match TokioFs::create_dir(&dir_path).await {
            Ok(_) => {
                self.success(&format!("Created directory: {}", name));
                self.navigate_to(current_dir).await
            }
            Err(e) => {
                self.error(&format!("Failed to create directory: {}", e));
                Ok(DispatchResult::Continue)
            }
        }
    }

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::EnterSelected => self.handle_enter_selected().await,
            Action::GoToParent => self.handle_go_to_parent().await,
            Action::CreateFileWithName(name) => self.create_file(&name).await,
            Action::CreateDirectoryWithName(name) => self.create_directory(&name).await,
            Action::ReloadDirectory => {
                let current = {
                    let fs = self.state_provider.fs_state();
                    fs.active_pane().cwd.clone()
                };
                self.navigate_to(current).await
            }
            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn success(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    fn error(&self, msg: &str) {
        let msg = msg.to_string();
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
