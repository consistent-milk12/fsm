//! action_dispatchers/command.rs
//! Command execution with input validation and resource management

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard, RwLock, RwLockReadGuard};
use tokio::fs as TokioFs;
use tokio::sync::mpsc::UnboundedSender;

use super::{ActionHandler, DispatchResult};
use crate::UIState;
use crate::controller::actions::InputPromptType;
use crate::controller::event_loop::TaskResult;
use crate::controller::{Action, state_coordinator::StateCoordinator};
use crate::fs::object_info::ObjectInfo;
use crate::model::{FSState, PaneState, RedrawFlag, UIOverlay};

/// Command dispatcher with validation and async safety
pub struct CommandDispatcher {
    state: Arc<StateCoordinator>,

    #[allow(unused)]
    task_tx: UnboundedSender<TaskResult>,
}

impl CommandDispatcher {
    pub fn new(state: Arc<StateCoordinator>, task_tx: UnboundedSender<TaskResult>) -> Self {
        Self { state, task_tx }
    }

    /// Parse command with validation
    fn parse_command(&self, command: &str) -> Result<(String, Vec<String>)> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Empty command");
        }

        // Simple shell-like parsing (can be enhanced later)
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd: String = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        Ok((cmd, args))
    }

    /// Validate path for safety
    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        // Basic path traversal protection
        if path.contains("..") || path.starts_with('/') && !path.starts_with("/tmp") {
            anyhow::bail!("Potentially unsafe path: {}", path);
        }

        let current_dir: PathBuf = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();
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

            _ => {
                anyhow::bail!("Unknown command: {}", cmd);
            }
        }
    }

    async fn handle_cd_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: cd <path>");
        }

        let target_path: PathBuf = self.validate_path(&args[0])?;

        if !target_path.exists() {
            anyhow::bail!("Directory does not exist: {}", target_path.display());
        }

        if !target_path.is_dir() {
            anyhow::bail!("Path is not a directory: {}", target_path.display());
        }

        // Load directory without holding locks
        let entries: Vec<ObjectInfo> = self.load_directory_safely(&target_path).await?;

        // Update state
        {
            let mut fs: MutexGuard<'_, FSState> = self.state.fs_state();
            let pane: &mut PaneState = fs.active_pane_mut();
            pane.cwd = target_path;
            pane.entries = entries;
            pane.selected.store(0, Ordering::Relaxed);
        }

        self.show_success("Directory changed successfully");
        Ok(())
    }

    async fn handle_mkdir_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: mkdir <name>");
        }

        let name: &String = &args[0];

        let (current_dir, new_dir) = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();
            let current_dir: PathBuf = fs.active_pane().cwd.clone();
            let new_dir: PathBuf = current_dir.join(name);
            (current_dir, new_dir)
        };

        TokioFs::create_dir(&new_dir)
            .await
            .with_context(|| format!("Failed to create directory: {}", name))?;

        self.show_success(&format!("Created directory: {}", name));

        // Reload current directory
        let entries: Vec<ObjectInfo> = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs: MutexGuard<'_, FSState> = self.state.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_touch_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: touch <filename>");
        }

        let name: &String = &args[0];

        let (current_dir, new_file) = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();
            let current_dir: PathBuf = fs.active_pane().cwd.clone();
            let new_file: PathBuf = current_dir.join(name);
            (current_dir, new_file)
        };

        TokioFs::File::create(&new_file)
            .await
            .with_context(|| format!("Failed to create file: {}", name))?;

        self.show_success(&format!("Created file: {}", name));

        // Reload current directory
        let entries: Vec<ObjectInfo> = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs: MutexGuard<'_, FSState> = self.state.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_reload_command(&self) -> Result<()> {
        let current_dir: PathBuf = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();

            fs.active_pane().cwd.clone()
        };

        let entries: Vec<ObjectInfo> = self.load_directory_safely(&current_dir).await?;
        {
            let mut fs: MutexGuard<'_, FSState> = self.state.fs_state();

            fs.active_pane_mut().entries = entries;
        }

        self.show_success("Directory reloaded");

        Ok(())
    }

    fn handle_pwd_command(&self) -> Result<()> {
        let current_dir: PathBuf = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();
            fs.active_pane().cwd.clone()
        };

        self.show_info(&format!("Current directory: {}", current_dir.display()));
        Ok(())
    }

    fn handle_ls_command(&self) -> Result<()> {
        let entry_count: usize = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();

            fs.active_pane().entries.len()
        };

        self.show_info(&format!("Directory contains {} entries", entry_count));
        Ok(())
    }

    fn handle_help_command(&self) -> Result<()> {
        self.state.update_ui_state(Box::new(|ui: &mut UIState| {
            ui.overlay = UIOverlay::Help;
            ui.request_redraw(RedrawFlag::Overlay);
        }));
        Ok(())
    }

    fn handle_find_command(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: find <pattern>");
        }

        let pattern: &String = &args[0];
        let results: Vec<ObjectInfo> = self.perform_filename_search(pattern);
        let matches: usize = results.len();

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.filename_search_results = results;
                ui.overlay = UIOverlay::SearchResults;
                ui.request_redraw(RedrawFlag::All);
            }));

        self.show_info(&format!("Found {} matches for '{}'", matches, pattern));

        Ok(())
    }

    fn handle_clear_command(&self) -> Result<()> {
        self.state.update_ui_state(Box::new(|ui: &mut UIState| {
            ui.notification = None;
            ui.request_redraw(RedrawFlag::All);
        }));

        Ok(())
    }

    /// Load directory safely without state locks
    async fn load_directory_safely(&self, directory: &std::path::Path) -> Result<Vec<ObjectInfo>> {
        use crate::fs::object_info::{LightObjectInfo, ObjectType};

        let mut entries: Vec<ObjectInfo> = Vec::new();

        // Add parent directory entry if not at root
        if let Some(parent) = directory.parent() {
            let light_parent: LightObjectInfo = LightObjectInfo {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                extension: None,
                object_type: ObjectType::Dir,
                is_dir: true,
                is_symlink: false,
            };
            entries.push(ObjectInfo::with_placeholder_metadata(light_parent));
        }

        let mut dir_reader: TokioFs::ReadDir = TokioFs::read_dir(directory).await?;
        while let Some(entry) = dir_reader.next_entry().await? {
            let entry_path: PathBuf = entry.path();

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

        let fs: MutexGuard<'_, FSState> = self.state.fs_state();
        let entries: &Vec<ObjectInfo> = &fs.active_pane().entries;
        let query_lower: String = query.to_lowercase();

        entries
            .iter()
            .filter(|entry: &&ObjectInfo| {
                let name_lower: String = entry.name.to_lowercase();
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

        let mut text_pos: usize = 0;

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
            let last_part: &&str = pattern_parts.last().unwrap();

            if !last_part.is_empty() && !text.ends_with(last_part) {
                return false;
            }
        }

        true
    }

    /// Handle input prompt submission with routing
    async fn handle_submit_input_prompt(&self, input: String) -> Result<DispatchResult> {
        let prompt_type: Option<InputPromptType> = {
            let ui_state: Arc<RwLock<UIState>> = self.state.ui_state();
            let ui: RwLockReadGuard<'_, UIState> = ui_state.read().expect("UI state lock poisoned");
            ui.input_prompt_type.clone()
        };

        match prompt_type {
            Some(InputPromptType::Custom(ref name)) if name == "command" => {
                if !input.is_empty() {
                    // Add to command history
                    let input_clone: String = input.clone();

                    self.state
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.add_to_history(&input_clone);
                        }));

                    // Execute command
                    if let Err(e) = self.execute_command(&input).await {
                        if e.to_string().contains("quit") {
                            return Ok(DispatchResult::Terminate);
                        }

                        self.show_error(&format!("Command failed: {}", e));
                    }
                }

                // Close overlay
                self.state.update_ui_state(Box::new(|ui: &mut UIState| {
                    ui.overlay = UIOverlay::None;
                    ui.clear_input();
                    ui.input_prompt_type = None;
                    ui.request_redraw(RedrawFlag::All);
                }));

                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::CreateFile) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("touch {}", input)).await {
                        self.show_error(&format!("Failed to create file: {}", e));
                    }
                }

                self.close_overlay();

                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::CreateDirectory) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("mkdir {}", input)).await {
                        self.show_error(&format!("Failed to create directory: {}", e));
                    }
                }

                self.close_overlay();

                Ok(DispatchResult::Continue)
            }

            Some(InputPromptType::GoToPath) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("cd {}", input)).await {
                        self.show_error(&format!("Failed to change directory: {}", e));
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
        self.state.update_ui_state(Box::new(|ui: &mut UIState| {
            ui.overlay = UIOverlay::None;
            ui.clear_input();

            ui.input_prompt_type = None;
            ui.request_redraw(RedrawFlag::All);
        }));
    }

    fn show_success(&self, message: &str) {
        let msg: String = message.to_string();

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_success(&msg);
            }));
    }

    fn show_info(&self, message: &str) {
        let msg: String = message.to_string();

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_info(&msg);
            }));
    }

    fn show_error(&self, message: &str) {
        let msg = message.to_string();

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_error(&msg);
            }));
    }
}

impl ActionHandler for CommandDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(action, Action::SubmitInputPrompt(_))
    }

    async fn handle(&mut self, action: &Action) -> Result<DispatchResult> {
        match action {
            Action::SubmitInputPrompt(input) => {
                self.handle_submit_input_prompt(input.clone()).await
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn priority(&self) -> u8 {
        40
    } // Medium-low priority

    fn name(&self) -> &'static str {
        "command"
    }
}
