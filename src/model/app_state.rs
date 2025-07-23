//! src/model/app_state.rs
//! ============================================================================
//! # AppState: Advanced, Power-User-Oriented Application State
//!
//! `AppState` unifies all persistent and transient state for the terminal file
//! manager. Its design is optimized for power users, async workflows, and advanced
//! batch/file operations. This state struct makes it easy to support:
//! - Multiple concurrent tasks (search, copy, batch delete/rename)
//! - Undo/redo for destructive operations
//! - Multi-selection and mark/stage workflows
//! - Plugins and extensibility
//! - UI overlays, detailed history/status reporting, and responsive TUI
//!
//! ## Highlights
//! - Shares core handles (cache, config) as `Arc` for async/background use
//! - Clean sub-state for filesystem, UI, tasks, plugin registry, and history
//! - Robust message/status/error management
//! - Extensible for plugins, user scripting, and power tools

use crate::cache::cache_manager::ObjectInfoCache;
use crate::config::config::Config;
use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use crate::fs::dir_scanner;
use crate::fs::object_info::ObjectInfo;
use crate::model::fs_state::{FSState, PaneState};
use crate::model::ui_state::UIState;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Represents a pending or running asynchronous task (search, copy, delete, etc.).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: u64,
    pub description: String,
    pub started_at: Instant,
    pub is_completed: bool,
    pub result: Option<String>, // Could be success message, error, etc.
    // For progress-based/overlay reporting:
    pub progress: Option<f64>,
    pub current_item: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// Core application state struct, designed for fast, extensible power-user features.
pub struct AppState {
    pub config: Arc<Config>,
    pub cache: Arc<ObjectInfoCache>,
    pub ui: UIState,
    pub fs: FSState,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub action_tx: mpsc::UnboundedSender<Action>,
    pub redraw: bool,
    pub marked: HashSet<PathBuf>,
    pub history: VecDeque<AppHistoryEvent>,
    pub plugins: HashMap<String, PluginInfo>,
    pub tasks: HashMap<u64, TaskInfo>,
    pub last_error: Option<String>,
    pub last_status: Option<String>,
    pub started_at: Instant,
    pub search_results: Vec<ObjectInfo>,
    pub filename_search_results: Vec<ObjectInfo>,
    pub rich_search_results: Vec<String>,
    pub raw_search_results: Option<crate::tasks::search_task::RawSearchResult>,
    pub raw_search_selected: usize,
}

#[derive(Debug, Clone)]
pub enum AppHistoryEvent {
    Delete { paths: Vec<String> },
    Move { from: String, to: String },
    Rename { from: String, to: String },
    Copy { from: String, to: String },
    // Extend with plugin hooks, batch ops, custom undo, etc.
}

/// Info about a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub loaded: bool,
    // Optionally: plugin type, API version, path, config, etc.
}

impl AppState {
    /// Construct a new, ready-to-use AppState.
    pub fn new(
        config: Arc<Config>,
        cache: Arc<ObjectInfoCache>,
        fs: FSState,
        ui: UIState,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        Self {
            config,
            cache,
            ui,
            fs,
            task_tx,
            action_tx,
            redraw: true,
            marked: HashSet::new(),
            history: VecDeque::new(),
            plugins: HashMap::new(),
            tasks: HashMap::new(),
            last_error: None,
            last_status: None,
            started_at: Instant::now(),
            search_results: Vec::new(),
            filename_search_results: Vec::new(),
            rich_search_results: Vec::new(),
            raw_search_results: None,
            raw_search_selected: 0,
        }
    }

    /// Mark a file or directory by its canonical path for batch operations.
    pub fn mark_entry(&mut self, path: impl Into<PathBuf>) {
        self.marked.insert(path.into());
        self.redraw = true;
    }

    /// Unmark a previously marked entry.
    pub fn unmark_entry(&mut self, path: &Path) {
        self.marked.remove(path);
        self.redraw = true;
    }

    /// Clear all marked entries.
    pub fn clear_marks(&mut self) {
        self.marked.clear();
        self.redraw = true;
    }

    /// Add a reversible event to the history stack (for undo/redo).
    pub fn push_history(&mut self, event: AppHistoryEvent) {
        self.history.push_back(event);
        if self.history.len() > 128 {
            self.history.pop_front();
        }
        self.redraw = true;
    }

    /// Register a plugin for later use.
    pub fn register_plugin(&mut self, info: PluginInfo) {
        self.plugins.insert(info.name.clone(), info);
        self.redraw = true;
    }

    /// Add or update a running/pending async task.
    pub fn add_task(&mut self, task: TaskInfo) {
        info!("Adding task: {}", task.description);
        self.tasks.insert(task.task_id, task);
        self.redraw = true;
    }

    /// Update task completion/result.
    pub fn complete_task(&mut self, task_id: u64, result: Option<String>) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            info!("Completing task: {}", task.description);
            task.is_completed = true;
            task.result = result;
            self.redraw = true;
        }
    }

    /// Set the latest error message (display in UI).
    pub fn set_error(&mut self, msg: impl Into<String>) {
        let msg_str = msg.into();
        error!("Setting error: {}", msg_str);
        self.last_error = Some(msg_str.clone());
        self.ui.show_error(msg_str);
        self.redraw = true;
    }

    /// Set the latest info/status message (display in UI).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        let msg_str = msg.into();
        info!("Setting status: {}", msg_str);
        self.last_status = Some(msg_str.clone());
        self.ui.show_info(msg_str);
        self.redraw = true;
    }

    /// Show a success notification
    pub fn show_success(&mut self, msg: impl Into<String>) {
        let success_msg = msg.into();
        self.ui.show_success(success_msg.clone());
        info!("Success: {}", success_msg);
        self.redraw = true;
    }

    /// Show a warning notification
    pub fn show_warning(&mut self, msg: impl Into<String>) {
        let warning_msg = msg.into();
        self.ui.show_warning(warning_msg.clone());
        warn!("Warning: {}", warning_msg);
        self.redraw = true;
    }

    /// Clear error and status messages.
    pub fn clear_msgs(&mut self) {
        self.last_error = None;
        self.last_status = None;
        self.redraw = true;
    }

    /// Navigate to a new directory, updating the active pane.
    pub async fn enter_directory(&mut self, path: PathBuf) {
        info!("Entering directory: {}", path.display());
        let canonical_path = match tokio::fs::canonicalize(&path).await {
            Ok(p) => p,
            Err(e) => {
                self.set_error(format!("Invalid path: {}: {}", path.display(), e));
                self.redraw = true;
                return;
            }
        };

        let current_pane = self.fs.active_pane_mut();
        current_pane.cwd = canonical_path.clone();
        current_pane.is_loading = true;
        self.redraw = true;

        // Use streaming directory scan for better responsiveness
        self.enter_directory_streaming(canonical_path).await;
    }

    /// Go to the parent directory of the current active pane.
    pub async fn go_to_parent_directory(&mut self) {
        let current_pane_cwd = self.fs.active_pane().cwd.clone();
        if let Some(parent) = current_pane_cwd.parent() {
            info!("Going to parent directory: {}", parent.display());
            self.enter_directory(parent.to_path_buf()).await;
        } else {
            warn!("Already at root, cannot go to parent.");
            self.set_status("Already at root.");
        }
        self.redraw = true;
    }

    pub async fn reload_directory(&mut self) {
        let current_dir = self.fs.active_pane().cwd.clone();
        self.enter_directory(current_dir).await;
    }

    /// Enter directory using streaming scan for better responsiveness
    async fn enter_directory_streaming(&mut self, path: PathBuf) {
        let current_pane = self.fs.active_pane_mut();
        current_pane.start_incremental_loading();

        let (mut rx, _handle) = dir_scanner::scan_dir_streaming_with_background_metadata(
            path.clone(),
            self.ui.show_hidden,
            10, // Batch size for yielding
            self.action_tx.clone(),
        )
        .await;

        // Spawn task to handle streaming updates
        let action_tx: mpsc::UnboundedSender<Action> = self.action_tx.clone();
        let scan_path: PathBuf = path.clone();

        tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                let _ = action_tx.send(Action::DirectoryScanUpdate {
                    path: scan_path.clone(),
                    update,
                });
            }
        });

        self.redraw = true;
    }

    /// Enter the currently selected directory or open the file.
    pub async fn enter_selected_directory(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();

        if let Some(selected_idx) = self.ui.selected {
            if let Some(selected_entry) = active_pane.entries.get(selected_idx) {
                if selected_entry.is_dir {
                    info!(
                        "Entering selected directory: {}",
                        selected_entry.path.display()
                    );

                    self.enter_directory(selected_entry.path.clone()).await;
                } else {
                    // Open file with external editor
                    info!("Opening selected file: {}", selected_entry.path.display());

                    self.open_file_with_editor(selected_entry.path.clone())
                        .await;

                    self.set_status(&format!("Opened file: {}", selected_entry.name));
                }
            }
        } else {
            warn!("No entry selected to enter.");
            self.set_status("No entry selected.");
        }

        self.redraw = true;
    }

    /// Open a file with external editor (VS Code)
    pub async fn open_file_with_editor(&mut self, file_path: std::path::PathBuf) {
        let path_str = file_path.to_string_lossy().to_string();
        let open_result = tokio::spawn(async move {
            let mut cmd: Command = Command::new("code");
            cmd.arg(&path_str);
            match cmd.spawn() {
                Ok(_) => Ok(path_str),
                Err(e) => Err(format!("Failed to open file with code: {}", e)),
            }
        })
        .await;

        match open_result {
            Ok(Ok(path)) => {
                self.show_success(format!(
                    "Opened {} in VS Code",
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                ));
            }

            Ok(Err(e)) => {
                self.set_error(e);
            }

            Err(e) => {
                self.set_error(format!("Task failed: {}", e));
            }
        }
    }

    pub async fn delete_entry(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();

        if let Some(selected_idx) = self.ui.selected {
            if let Some(selected_entry) = active_pane.entries.get(selected_idx) {
                let path: &PathBuf = &selected_entry.path;

                let result: Result<(), Error> = if selected_entry.is_dir {
                    tokio::fs::remove_dir_all(path).await
                } else {
                    tokio::fs::remove_file(path).await
                };

                if let Err(e) = result {
                    self.set_error(format!("Failed to delete {}: {}", path.display(), e));
                } else {
                    self.show_success(format!("Deleted {}", path.display()));
                    self.reload_directory().await;
                }
            }
        }
    }

    pub async fn create_file(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_file_path: PathBuf = active_pane.cwd.join("new_file.txt");

        if let Err(e) = tokio::fs::File::create(&new_file_path).await {
            self.set_error(format!("Failed to create file: {}", e));
        } else {
            self.show_success(format!("Created file: {}", new_file_path.display()));
            self.reload_directory().await;
        }
    }

    pub async fn create_directory(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_dir_path: PathBuf = active_pane.cwd.join("new_directory");

        if let Err(e) = tokio::fs::create_dir(&new_dir_path).await {
            self.set_error(format!("Failed to create directory: {}", e));
        } else {
            self.show_success(format!("Created directory: {}", new_dir_path.display()));
            self.reload_directory().await;
        }
    }

    pub async fn create_file_with_name(&mut self, name: String) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_file_path: PathBuf = active_pane.cwd.join(&name);

        if let Err(e) = tokio::fs::File::create(&new_file_path).await {
            self.set_error(format!("Failed to create file '{}': {}", name, e));
        } else {
            self.show_success(format!("Created file: {}", name));
            self.reload_directory().await;
        }
    }

    pub async fn create_directory_with_name(&mut self, name: String) {
        let active_pane = self.fs.active_pane().clone();
        let new_dir_path = active_pane.cwd.join(&name);
        if let Err(e) = tokio::fs::create_dir(&new_dir_path).await {
            self.set_error(format!("Failed to create directory '{}': {}", name, e));
        } else {
            self.show_success(format!("Created directory: {}", name));
            self.reload_directory().await;
        }
    }

    /// Perform a file name search (recursive, background task)
    pub fn filename_search(&mut self, pattern: String) {
        if pattern.trim().is_empty() {
            return;
        }

        // Clear previous results and start new search
        self.filename_search_results.clear();

        let task_id = self.tasks.len() as u64;
        let task = TaskInfo {
            task_id,
            description: format!("Filename search for '{}'", pattern),
            started_at: Instant::now(),
            is_completed: false,
            result: None,
            progress: None,
            current_item: None,
            completed: None,
            total: None,
            message: None,
        };
        self.tasks.insert(task_id, task);

        // Start background filename search task
        let current_dir = self.fs.active_pane().cwd.clone();
        crate::tasks::filename_search_task::filename_search_task(
            task_id,
            pattern,
            current_dir,
            self.task_tx.clone(),
            self.action_tx.clone(),
        );

        self.redraw = true;
    }

    /// Start a content search using ripgrep
    pub fn start_content_search(&mut self, pattern: String) {
        if pattern.trim().is_empty() {
            return;
        }

        self.search_results.clear();
        self.rich_search_results.clear();
        self.ui.last_query = Some(pattern.clone());

        // Keep the ContentSearch overlay active and show search state
        let task_id = self.tasks.len() as u64;
        let task = TaskInfo {
            task_id,
            description: format!("Content search for '{}'", pattern),
            started_at: Instant::now(),
            is_completed: false,
            result: None,
            progress: None,
            current_item: None,
            completed: None,
            total: None,
            message: None,
        };
        self.add_task(task);

        // Start ripgrep search task
        let path = self.fs.active_pane().cwd.clone();
        let task_tx = self.task_tx.clone();
        let action_tx = self.action_tx.clone();

        crate::tasks::search_task::search_task(task_id, pattern, path, task_tx, action_tx);
    }

    /// Updates an ObjectInfo in the active pane with new data from a background task.
    pub fn update_object_info(&mut self, parent_dir: PathBuf, info: ObjectInfo) {
        if let Some(pane) = self.fs.panes.iter_mut().find(|p| p.cwd == parent_dir) {
            if let Some(entry) = pane.entries.iter_mut().find(|e| e.path == info.path) {
                entry.size = info.size;
                entry.items_count = info.items_count;
                entry.modified = info.modified;
                entry.metadata_loaded = info.metadata_loaded;
                debug!(
                    "Updating object info for {}: modified = {}",
                    info.path.display(),
                    info.modified.format("%Y-%m-%d")
                );
                self.redraw = true;
            }
        }
    }

    pub fn sort_entries(&mut self, sort_criteria: &str) {
        let active_pane = self.fs.active_pane_mut();
        match sort_criteria {
            "name_asc" => active_pane.entries.sort_by(|a, b| a.name.cmp(&b.name)),
            "name_desc" => active_pane.entries.sort_by(|a, b| b.name.cmp(&a.name)),
            "size_asc" => active_pane.entries.sort_by(|a, b| a.size.cmp(&b.size)),
            "size_desc" => active_pane.entries.sort_by(|a, b| b.size.cmp(&a.size)),
            "modified_asc" => active_pane
                .entries
                .sort_by(|a, b| a.modified.cmp(&b.modified)),
            "modified_desc" => active_pane
                .entries
                .sort_by(|a, b| b.modified.cmp(&a.modified)),
            _ => {}
        }
        self.redraw = true;
    }

    pub fn filter_entries(&mut self, filter_criteria: &str) {
        let active_pane = self.fs.active_pane_mut();
        // This is a placeholder for a more complex filtering implementation.
        // For now, we'll just filter by a simple string contains.
        let entries = active_pane.entries.clone();
        active_pane.entries = entries
            .into_iter()
            .filter(|entry| entry.name.contains(filter_criteria))
            .collect();
        self.redraw = true;
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &"Config")
            .field("cache", &"ObjectInfoCache")
            .field("fs", &self.fs)
            .field("ui", &self.ui)
            .field("marked", &self.marked)
            .field("history", &self.history)
            .field("tasks", &self.tasks)
            .field("plugins", &self.plugins)
            .field("last_error", &self.last_error)
            .field("last_status", &self.last_status)
            .field("started_at", &self.started_at)
            .field("redraw", &self.redraw)
            .finish()
    }
}
