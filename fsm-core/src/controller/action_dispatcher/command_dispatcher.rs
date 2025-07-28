//! action_dispatchers/command.rs
//! Command execution with input validation and resource management

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLockReadGuard};
use tokio::fs as TokioFs;
use tokio::sync::mpsc::UnboundedSender;

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::event_loop::TaskResult;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::ObjectInfo;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Command dispatcher with validation and async safety
#[derive(Clone)]
pub struct CommandDispatcher {
    state_provider: Arc<dyn StateProvider>,
    #[allow(unused)]
    task_tx: UnboundedSender<TaskResult>,
}

impl CommandDispatcher {
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        Self {
            state_provider,
            task_tx,
        }
    }

    /// Parse command with validation
    fn parse_command(&self, command: &str) -> Result<(String, Vec<String>)> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Empty command");
        }

        // Simple shell-like parsing (can be enhanced later)
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        Ok((cmd, args))
    }

    /// Validate path for safety
    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        // Basic path traversal protection
        if path.contains("..") || (path.starts_with('/') && !path.starts_with("/tmp")) {
            anyhow::bail!("Potentially unsafe path: {}", path);
        }

        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        let target_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            current_dir.join(path)
        };

        Ok(target_path)
    }

    /// Execute command with proper error handling
    async fn execute_command(&self, command: &str) -> Result<()> {
        let (cmd, args) = self.parse_command(command)?;

        match cmd.as_str() {
            "cd" => self.handle_cd_command(args).await,
            "mkdir" => self.handle_mkdir_command(args).await,
            "touch" => self.handle_touch_command(args).await,
            "reload" => self.handle_reload_command().await,
            "pwd" => self.handle_pwd_command(),
            "ls" => self.handle_ls_command(),
            "help" => self.handle_help_command(),
            "quit" | "q" => Err(anyhow::anyhow!("User requested quit")),
            "find" => self.handle_find_command(args),
            "clear" => self.handle_clear_command(),
            _ => anyhow::bail!("Unknown command: {}", cmd),
        }
    }

    async fn handle_cd_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: cd <path>");
        }

        let target_path = self.validate_path(&args[0])?;

        if !target_path.exists() {
            anyhow::bail!("Directory does not exist: {}", target_path.display());
        }

        if !target_path.is_dir() {
            anyhow::bail!("Path is not a directory: {}", target_path.display());
        }

        // Load directory without holding locks
        let entries = self.load_directory_safely(&target_path).await?;

        // Update state
        {
            let mut fs = self.state_provider.fs_state();
            let pane = fs.active_pane_mut();
            pane.cwd = target_path;
            pane.entries = entries;
            pane.selected.store(0, Ordering::Relaxed);
        }

        self.success("Directory changed successfully");
        Ok(())
    }

    async fn handle_mkdir_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: mkdir <name>");
        }

        let name = &args[0];
        let (current_dir, new_dir) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            let new_dir = current_dir.join(name);
            (current_dir, new_dir)
        };

        TokioFs::create_dir(&new_dir)
            .await
            .with_context(|| format!("Failed to create directory: {}", name))?;

        self.success(&format!("Created directory: {}", name));

        // Reload current directory
        let entries = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_touch_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: touch <filename>");
        }

        let name = &args[0];
        let (current_dir, new_file) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            let new_file = current_dir.join(name);
            (current_dir, new_file)
        };

        TokioFs::File::create(&new_file)
            .await
            .with_context(|| format!("Failed to create file: {}", name))?;

        self.success(&format!("Created file: {}", name));

        // Reload current directory
        let entries = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_reload_command(&self) -> Result<()> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        let entries = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        self.success("Directory reloaded");
        Ok(())
    }

    fn handle_pwd_command(&self) -> Result<()> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        self.info(&format!("Current directory: {}", current_dir.display()));
        Ok(())
    }

    fn handle_ls_command(&self) -> Result<()> {
        let entry_count = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().entries.len()
        };

        self.info(&format!("Directory contains {} entries", entry_count));
        Ok(())
    }

    fn handle_help_command(&self) -> Result<()> {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::Help;
                ui.request_redraw(RedrawFlag::Overlay);
            }));
        Ok(())
    }

    fn handle_find_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: find <pattern>");
        }

        let pattern = &args[0];
        let results = self.perform_filename_search(pattern);
        let matches = results.len();

        // Store results in FSState where search results belong
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().search_results = results;
        }

        // Update UI state for overlay
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::SearchResults;
                ui.request_redraw(RedrawFlag::All);
            }));

        self.info(&format!("Found {} matches for '{}'", matches, pattern));
        Ok(())
    }

    fn handle_clear_command(&self) -> Result<()> {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.notification = None;
                ui.request_redraw(RedrawFlag::All);
            }));
        Ok(())
    }

    /// Load directory safely without state locks
    async fn load_directory_safely(&self, directory: &std::path::Path) -> Result<Vec<ObjectInfo>> {
        use crate::fs::object_info::{LightObjectInfo, ObjectType};

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

        let mut dir_reader = TokioFs::read_dir(directory).await?;
        while let Some(entry) = dir_reader.next_entry().await? {
            let entry_path = entry.path();

            // Skip hidden files
            if entry_path
                .file_name()
                .and_then(|name: &OsStr| name.to_str())
                .map(|name: &str| name.starts_with('.'))
                .unwrap_or(false)
            {
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

    /// Perform filename search (shared with search dispatcher)
    fn perform_filename_search(&self, query: &str) -> Vec<ObjectInfo> {
        if query.is_empty() {
            return Vec::new();
        }

        let fs = self.state_provider.fs_state();
        let entries = &fs.active_pane().entries;
        let query_lower = query.to_lowercase();

        entries
            .iter()
            .filter(|entry| {
                let name_lower = entry.name.to_lowercase();
                if query.contains('*') {
                    // Simple wildcard matching
                    self.wildcard_match(&query_lower, &name_lower)
                } else {
                    name_lower.contains(&query_lower)
                }
            })
            .cloned()
            .collect()
    }

    /// Simple wildcard matching
    fn wildcard_match(&self, pattern: &str, text: &str) -> bool {
        // Simple implementation - can be optimized further
        let pattern_parts: Vec<&str> = pattern.split('*').collect();
        if pattern_parts.len() == 1 {
            return text.contains(pattern);
        }

        let mut text_pos = 0;

        for (i, part) in pattern_parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            if let Some(found_pos) = text[text_pos..].find(part) {
                text_pos += found_pos + part.len();

                // If this is the first part and doesn't start with *, must match from beginning
                if i == 0 && !pattern.starts_with('*') && found_pos != 0 {
                    return false;
                }
            } else {
                return false;
            }
        }

        // If pattern doesn't end with *, the last part must match at the end
        if !pattern.ends_with('*') && !pattern_parts.is_empty() {
            let last_part = pattern_parts.last().unwrap();
            if !last_part.is_empty() && !text.ends_with(last_part) {
                return false;
            }
        }

        true
    }

    /// Handle input prompt submission with routing
    async fn handle_submit_input_prompt(&self, input: String) -> Result<DispatchResult> {
        let prompt_type = {
            let ui_state = self.state_provider.ui_state();
            let ui: RwLockReadGuard<'_, UIState> = ui_state.read().expect("UI state lock poisoned");
            ui.input_prompt_type.clone()
        };

        match prompt_type {
            Some(InputPromptType::Custom(ref name)) if name == "command" => {
                if !input.is_empty() {
                    // Add to command history
                    let input_clone = input.clone();
                    self.state_provider
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.history_push(input_clone);
                        }));

                    // Execute command
                    if let Err(e) = self.execute_command(&input).await {
                        if e.to_string().contains("quit") {
                            return Ok(DispatchResult::Terminate);
                        }
                        self.error(&format!("Command failed: {}", e));
                    }
                }

                // Close overlay
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.overlay = UIOverlay::None;
                        ui.clear_redraw();
                        ui.input_prompt_type = None;
                        ui.request_redraw(RedrawFlag::All);
                    }));

                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::CreateFile) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("touch {}", input)).await {
                        self.error(&format!("Failed to create file: {}", e));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::CreateDirectory) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("mkdir {}", input)).await {
                        self.error(&format!("Failed to create directory: {}", e));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::GoToPath) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("cd {}", input)).await {
                        self.error(&format!("Failed to change directory: {}", e));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }

            _ => {
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }
        }
    }

    fn close_overlay(&self) {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::None;
                ui.clear_redraw();
                ui.input_prompt_type = None;
                ui.request_redraw(RedrawFlag::All);
            }));
    }

    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::SubmitInputPrompt(input) => self.handle_submit_input_prompt(input).await,
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

    fn info(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.info(&msg);
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

impl ActionMatcher for CommandDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(action, Action::SubmitInputPrompt(_))
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "command"
    }
}
