//! ActionDispatcher: Enhanced modular action processing and command execution.
//!
//! This module provides a comprehensive `ActionDispatcher` that handles:
//! - Action batching and optimization through `ActionBatcher`
//! - Full overlay system support (command mode, search, help)
//! - Complete command execution system with file operations
//! - Advanced filename search with wildcard pattern matching
//! - Enhanced input handling with history and cursor management
//! - Real-time feedback through notifications and error handling
//!
//! The dispatcher isolates complex action handling from the main application
//! loop, providing a clean separation of concerns and enabling sophisticated
//! user interactions through overlays and command processing.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;

use crate::controller::action_batcher::{ActionBatcher, ActionSource};
use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use crate::controller::state_coordinator::StateCoordinator;

/// Dispatches actions by batching them and applying them to the
/// `StateCoordinator` when flush conditions are met.
pub struct ActionDispatcher {
    batcher: ActionBatcher,
    state: Arc<StateCoordinator>,
    task_tx: UnboundedSender<TaskResult>,
}

impl ActionDispatcher {
    /// Create a new dispatcher given the shared state coordinator and
    /// background task sender.  The dispatcher owns its own
    /// `ActionBatcher` for performance optimizations.
    pub fn new(state: Arc<StateCoordinator>, task_tx: UnboundedSender<TaskResult>) -> Self {
        Self {
            batcher: ActionBatcher::new(),
            state,
            task_tx,
        }
    }

    /// Push an action into the batcher from the specified source.  If
    /// flushing conditions are met, the resulting batch is applied to
    /// the state.  Returns `false` if the application should
    /// terminate.
    pub async fn handle(&mut self, action: Action, source: ActionSource) -> bool {
        // Add the action to the batcher.  If a batch is ready to be
        // flushed, this returns a vector of actions to execute.
        let maybe_batch = self.batcher.add_action(action, source);
        if let Some(actions) = maybe_batch {
            for act in actions {
                if !self.apply_action(act).await {
                    return false;
                }
            }
        }
        true
    }

    /// Force flush all pending actions and apply them.  Returns
    /// `false` if termination is requested by any action.
    pub async fn flush(&mut self) -> bool {
        let actions = self.batcher.flush_all_batches();
        for act in actions {
            if !self.apply_action(act).await {
                return false;
            }
        }
        true
    }

    /// Apply a single action to the state coordinator.  This method
    /// updates the filesystem and UI state as appropriate.  Only a
    /// small set of actions are currently implemented.
    async fn apply_action(&mut self, action: Action) -> bool {
        use crate::model::ui_state::RedrawFlag as RF;
        match action {
            Action::Quit => {
                return false;
            }
            Action::MoveSelectionUp => {
                let fs = self.state.fs_state();
                fs.active_pane().move_selection_up();
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::MoveSelectionDown => {
                let fs = self.state.fs_state();
                fs.active_pane().move_selection_down();

                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }

            Action::PageUp => {
                let fs = self.state.fs_state();
                let lines = fs
                    .active_pane()
                    .viewport_height
                    .load(std::sync::atomic::Ordering::Relaxed);
                for _ in 0..lines {
                    if !fs.active_pane().move_selection_up() {
                        break;
                    }
                }
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::PageDown => {
                let fs = self.state.fs_state();
                let lines = fs
                    .active_pane()
                    .viewport_height
                    .load(std::sync::atomic::Ordering::Relaxed);
                for _ in 0..lines {
                    if !fs.active_pane().move_selection_down() {
                        break;
                    }
                }
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::SelectFirst => {
                let fs = self.state.fs_state();
                fs.active_pane()
                    .selected
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::SelectLast => {
                let fs = self.state.fs_state();
                let len = fs.active_pane().entries.len();
                if len > 0 {
                    fs.active_pane()
                        .selected
                        .store(len - 1, std::sync::atomic::Ordering::Relaxed);
                }
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::SelectIndex(index) => {
                let fs = self.state.fs_state();
                let len = fs.active_pane().entries.len();
                if index < len {
                    fs.active_pane()
                        .selected
                        .store(index, std::sync::atomic::Ordering::Relaxed);
                    self.state.update_ui_state(|ui| {
                        let new_ui = ui.clone();
                        new_ui.request_redraw(RF::Main);
                        new_ui
                    });
                }
            }
            Action::Resize(width, height) => {
                let fs = self.state.fs_state();
                let new_height = height.saturating_sub(2) as usize;
                fs.active_pane()
                    .viewport_height
                    .store(new_height, std::sync::atomic::Ordering::Relaxed);
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::EnterSelected => {
                // Navigate into directories or handle file selection
                let should_navigate = {
                    let fs = self.state.fs_state();
                    let pane = fs.active_pane();
                    let current = pane.selected.load(std::sync::atomic::Ordering::Relaxed);

                    if let Some(entry) = pane.entries.get(current) {
                        if entry.is_dir {
                            Some(entry.path.clone())
                        } else {
                            // TODO: Open file with external editor
                            tracing::info!("Opening file: {:?}", entry.path);
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(new_dir) = should_navigate {
                    // Load new directory asynchronously
                    if let Err(e) = self.load_directory(new_dir).await {
                        tracing::warn!("Failed to load directory: {}", e);
                    }
                }

                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::All);
                    new_ui
                });
            }
            Action::GoToParent => {
                // Navigate to parent directory
                let should_navigate = {
                    let fs = self.state.fs_state();
                    let pane = fs.active_pane();
                    let current_dir = pane.cwd.clone();
                    current_dir.parent().map(|p| p.to_path_buf())
                };

                if let Some(parent_dir) = should_navigate
                    && let Err(e) = self.load_directory(parent_dir).await
                {
                    tracing::warn!("Failed to load parent directory: {}", e);
                }

                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::All);
                    new_ui
                });
            }
            Action::Tick => {
                self.state.update_ui_state(|ui| {
                    let new_ui = ui.clone();
                    new_ui.request_redraw(RF::Main);
                    new_ui
                });
            }
            Action::ToggleHelp => {
                // Toggle help overlay
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.overlay = if new_ui.overlay == crate::model::ui_state::UIOverlay::Help {
                        crate::model::ui_state::UIOverlay::None
                    } else {
                        crate::model::ui_state::UIOverlay::Help
                    };
                    new_ui.request_redraw(RF::Overlay);
                    new_ui
                });
            }
            Action::EnterCommandMode => {
                // Enter command mode overlay
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.overlay = crate::model::ui_state::UIOverlay::Prompt;
                    new_ui.clear_input(); // Use enhanced input management
                    new_ui.input_prompt_type = Some(
                        crate::controller::actions::InputPromptType::Custom("command".to_string()),
                    );
                    new_ui.request_redraw(RF::All); // Redraw everything for overlay transition
                    new_ui
                });
            }
            Action::ToggleFileNameSearch => {
                // Toggle filename search overlay
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.overlay =
                        if new_ui.overlay == crate::model::ui_state::UIOverlay::FileNameSearch {
                            crate::model::ui_state::UIOverlay::None
                        } else {
                            crate::model::ui_state::UIOverlay::FileNameSearch
                        };
                    new_ui.clear_input(); // Use enhanced input management
                    new_ui.request_redraw(RF::All); // Redraw everything for overlay transition
                    new_ui
                });
            }
            Action::CloseOverlay => {
                // Close any active overlay
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                    new_ui.clear_input(); // Use enhanced input management
                    new_ui.input_prompt_type = None; // Clear prompt type
                    new_ui.request_redraw(RF::All); // Redraw everything for overlay transition
                    new_ui
                });
            }
            Action::FileNameSearch(query) => {
                // Update search input and perform filename filtering
                let search_results = self.perform_filename_search(&query);

                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.set_input(&query); // Use enhanced input management
                    new_ui.filename_search_results = search_results;
                    new_ui.request_redraw(RF::Overlay);
                    new_ui
                });

                tracing::debug!("Filename search updated for query: '{}'", query);
            }
            Action::SubmitInputPrompt(input) => {
                // Handle input submission from overlays
                let ui_state = self.state.ui_state();
                let prompt_type = ui_state.input_prompt_type.clone();

                match prompt_type {
                    Some(crate::controller::actions::InputPromptType::Custom(ref name))
                        if name == "command" =>
                    {
                        // Command execution
                        if !input.is_empty() {
                            // Add to command history
                            self.state.update_ui_state(|ui| {
                                let mut new_ui = ui.clone();
                                new_ui.add_to_history(&input);
                                new_ui
                            });

                            // Execute the command
                            if let Err(e) = self.execute_command(&input).await {
                                if e.to_string().contains("quit") {
                                    return false; // Quit requested
                                }
                                self.show_error(&format!("Command failed: {e}"));
                            }
                        }

                        // Close overlay after execution
                        self.state.update_ui_state(|ui| {
                            let mut new_ui = ui.clone();
                            new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                            new_ui.clear_input();
                            new_ui.input_prompt_type = None;
                            new_ui.request_redraw(RF::All);
                            new_ui
                        });
                    }
                    Some(crate::controller::actions::InputPromptType::CreateFile) => {
                        if !input.is_empty() {
                            // Create file using command system
                            if let Err(e) = self.execute_command(&format!("touch {input}")).await {
                                self.show_error(&format!("Failed to create file: {e}"));
                            }
                        }

                        // Close overlay
                        self.state.update_ui_state(|ui| {
                            let mut new_ui = ui.clone();
                            new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                            new_ui.clear_input();
                            new_ui.input_prompt_type = None;
                            new_ui.request_redraw(RF::All);
                            new_ui
                        });
                    }
                    Some(crate::controller::actions::InputPromptType::CreateDirectory) => {
                        if !input.is_empty() {
                            // Create directory using command system
                            if let Err(e) = self.execute_command(&format!("mkdir {input}")).await {
                                self.show_error(&format!("Failed to create directory: {e}"));
                            }
                        }

                        // Close overlay
                        self.state.update_ui_state(|ui| {
                            let mut new_ui = ui.clone();
                            new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                            new_ui.clear_input();
                            new_ui.input_prompt_type = None;
                            new_ui.request_redraw(RF::All);
                            new_ui
                        });
                    }
                    Some(crate::controller::actions::InputPromptType::GoToPath) => {
                        if !input.is_empty() {
                            // Change directory using command system
                            if let Err(e) = self.execute_command(&format!("cd {input}")).await {
                                self.show_error(&format!("Failed to change directory: {e}"));
                            }
                        }

                        // Close overlay
                        self.state.update_ui_state(|ui| {
                            let mut new_ui = ui.clone();
                            new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                            new_ui.clear_input();
                            new_ui.input_prompt_type = None;
                            new_ui.request_redraw(RF::All);
                            new_ui
                        });
                    }
                    _ => {
                        // Generic input handling - just close overlay
                        self.state.update_ui_state(|ui| {
                            let mut new_ui = ui.clone();
                            new_ui.overlay = crate::model::ui_state::UIOverlay::None;
                            new_ui.clear_input();
                            new_ui.input_prompt_type = None;
                            new_ui.request_redraw(RF::All);
                            new_ui
                        });
                    }
                }
            }
            Action::UpdateInput(input) => {
                // Update input field for overlay typing
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.set_input(&input);
                    new_ui.request_redraw(RF::Overlay);
                    new_ui
                });
            }
            // Unhandled actions can be routed here.  To support a
            // particular action, add a match arm that mutates the
            // coordinator appropriately and returns `true` to continue.
            _ => {}
        }
        true
    }

    /// Load directory contents into FSState
    async fn load_directory(&self, directory: std::path::PathBuf) -> anyhow::Result<()> {
        use crate::fs::object_info::{LightObjectInfo, ObjectInfo, ObjectType};
        use tracing::{info, warn};

        let mut fs_state_guard = self.state.fs_state();
        let pane = fs_state_guard.active_pane_mut();
        pane.cwd = directory.clone();

        // Set loading state
        pane.is_loading
            .store(true, std::sync::atomic::Ordering::Relaxed);

        // Load directory entries asynchronously
        let mut entries = Vec::new();

        // Add parent directory entry if not at root
        if let Some(parent) = directory.parent() {
            let light_parent = LightObjectInfo {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                extension: None,
                object_type: ObjectType::Dir,
                is_dir: true,
                is_symlink: false,
            };
            entries.push(ObjectInfo::with_placeholder_metadata(light_parent));
        }

        // Load directory contents
        match tokio::fs::read_dir(&directory).await {
            Ok(mut dir_entries) => {
                while let Ok(Some(entry)) = dir_entries.next_entry().await {
                    let entry_path = entry.path();

                    // Skip hidden files for now (can be made configurable later)
                    if let Some(filename) = entry_path.file_name()
                        && filename.to_string_lossy().starts_with('.')
                    {
                        continue;
                    }

                    // Create lightweight object info
                    match ObjectInfo::from_path_light(&entry_path).await {
                        Ok(light_info) => {
                            entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                        }
                        Err(e) => {
                            info!("Failed to read entry {:?}: {}", entry_path, e);
                        }
                    }
                }

                info!(
                    "Loaded {} entries from {}",
                    entries.len(),
                    directory.display()
                );
            }
            Err(e) => {
                warn!("Failed to read directory {:?}: {}", directory, e);
                return Err(anyhow::anyhow!("Failed to read directory: {}", e));
            }
        }

        pane.entries = entries;
        pane.selected.store(0, std::sync::atomic::Ordering::Relaxed);
        pane.is_loading
            .store(false, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }

    /// Perform filename search against current directory entries
    fn perform_filename_search(&self, query: &str) -> Vec<crate::fs::object_info::ObjectInfo> {
        if query.is_empty() {
            return Vec::new();
        }

        let fs = self.state.fs_state();
        let pane = fs.active_pane();
        let entries = &pane.entries;

        // Convert query to lowercase for case-insensitive search
        let query_lower = query.to_lowercase();

        entries
            .iter()
            .filter(|entry| {
                let name_lower = entry.name.to_lowercase();

                if query.contains('*') {
                    // Simple wildcard matching
                    self.wildcard_match(&query_lower, &name_lower)
                } else {
                    // Simple substring match
                    name_lower.contains(&query_lower)
                }
            })
            .cloned()
            .collect()
    }

    /// Simple wildcard matching without regex
    fn wildcard_match(&self, pattern: &str, text: &str) -> bool {
        self.wildcard_match_recursive(pattern, text, 0, 0)
    }

    fn wildcard_match_recursive(
        &self,
        pattern: &str,
        text: &str,
        p_idx: usize,
        t_idx: usize,
    ) -> bool {
        let pattern_chars: Vec<char> = pattern.chars().collect();
        let text_chars: Vec<char> = text.chars().collect();

        if p_idx == pattern_chars.len() {
            return t_idx == text_chars.len();
        }

        if pattern_chars[p_idx] == '*' {
            // Match zero or more characters
            for i in t_idx..=text_chars.len() {
                if self.wildcard_match_recursive(pattern, text, p_idx + 1, i) {
                    return true;
                }
            }
            false
        } else if t_idx < text_chars.len()
            && (pattern_chars[p_idx] == '?' || pattern_chars[p_idx] == text_chars[t_idx])
        {
            // Match single character or exact match
            self.wildcard_match_recursive(pattern, text, p_idx + 1, t_idx + 1)
        } else {
            false
        }
    }

    /// Execute command from command mode
    async fn execute_command(&mut self, command: &str) -> anyhow::Result<()> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        let cmd = parts[0];
        let args = &parts[1..];

        match cmd {
            "cd" => {
                if let Some(path) = args.first() {
                    let target_path = if path.starts_with('/') {
                        std::path::PathBuf::from(path)
                    } else {
                        let fs = self.state.fs_state();
                        let current_dir = fs.active_pane().cwd.clone();
                        current_dir.join(path)
                    };

                    if target_path.exists() && target_path.is_dir() {
                        self.load_directory(target_path).await?;
                        self.show_success("Directory changed successfully");
                    } else {
                        self.show_error("Directory does not exist");
                    }
                } else {
                    self.show_error("Usage: cd <path>");
                }
            }
            "mkdir" => {
                if let Some(name) = args.first() {
                    let fs = self.state.fs_state();
                    let current_dir = fs.active_pane().cwd.clone();
                    let new_dir = current_dir.join(name);

                    match tokio::fs::create_dir(&new_dir).await {
                        Ok(_) => {
                            self.show_success(&format!("Created directory: {name}"));
                            // Reload current directory to show new folder
                            let current = current_dir.clone();
                            self.load_directory(current).await?;
                        }
                        Err(e) => {
                            self.show_error(&format!("Failed to create directory: {e}"));
                        }
                    }
                } else {
                    self.show_error("Usage: mkdir <name>");
                }
            }
            "touch" => {
                if let Some(name) = args.first() {
                    let fs = self.state.fs_state();
                    let current_dir = fs.active_pane().cwd.clone();
                    let new_file = current_dir.join(name);

                    match tokio::fs::File::create(&new_file).await {
                        Ok(_) => {
                            self.show_success(&format!("Created file: {name}"));
                            // Reload current directory to show new file
                            let current = current_dir.clone();
                            self.load_directory(current).await?;
                        }
                        Err(e) => {
                            self.show_error(&format!("Failed to create file: {e}"));
                        }
                    }
                } else {
                    self.show_error("Usage: touch <filename>");
                }
            }
            "reload" => {
                let fs = self.state.fs_state();
                let current_dir = fs.active_pane().cwd.clone();
                self.load_directory(current_dir).await?;
                self.show_success("Directory reloaded");
            }
            "pwd" => {
                let fs = self.state.fs_state();
                let current_dir = fs.active_pane().cwd.clone();
                self.show_info(&format!("Current directory: {}", current_dir.display()));
            }
            "ls" => {
                let fs = self.state.fs_state();
                let entry_count = fs.active_pane().entries.len();
                self.show_info(&format!("Directory contains {entry_count} entries"));
            }
            "help" => {
                // Toggle help overlay
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.overlay = crate::model::ui_state::UIOverlay::Help;
                    new_ui.request_redraw(crate::model::ui_state::RedrawFlag::Overlay);
                    new_ui
                });
            }
            "quit" | "q" => {
                return Err(anyhow::anyhow!("User requested quit"));
            }
            "config" => {
                self.show_info("Configuration editing not yet implemented");
            }
            "find" => {
                if let Some(pattern) = args.first() {
                    let results = self.perform_filename_search(pattern);
                    self.state.update_ui_state(|ui| {
                        let mut new_ui = ui.clone();
                        new_ui.filename_search_results = results.clone();
                        new_ui.overlay = crate::model::ui_state::UIOverlay::SearchResults;
                        new_ui.request_redraw(crate::model::ui_state::RedrawFlag::All);
                        new_ui
                    });
                    self.show_info(&format!(
                        "Found {} matches for '{}'",
                        results.len(),
                        pattern
                    ));
                } else {
                    self.show_error("Usage: find <pattern>");
                }
            }
            "grep" => {
                if let Some(pattern) = args.first() {
                    self.show_info(&format!(
                        "Content search for '{pattern}' not yet implemented"
                    ));
                } else {
                    self.show_error("Usage: grep <pattern>");
                }
            }
            "clear" => {
                // Clear any notifications
                self.state.update_ui_state(|ui| {
                    let mut new_ui = ui.clone();
                    new_ui.notification = None;
                    new_ui.request_redraw(crate::model::ui_state::RedrawFlag::All);
                    new_ui
                });
            }
            _ => {
                self.show_error(&format!("Unknown command: {cmd}"));
            }
        }

        Ok(())
    }

    /// Show success notification
    fn show_success(&self, message: &str) {
        self.state.update_ui_state(|ui| {
            let mut new_ui = ui.clone();
            new_ui.show_success(message);
            new_ui
        });
    }

    /// Show info notification  
    fn show_info(&self, message: &str) {
        self.state.update_ui_state(|ui| {
            let mut new_ui = ui.clone();
            new_ui.show_info(message);
            new_ui
        });
    }

    /// Show error notification
    fn show_error(&self, message: &str) {
        self.state.update_ui_state(|ui| {
            let mut new_ui = ui.clone();
            new_ui.show_error(message);
            new_ui
        });
    }
}
