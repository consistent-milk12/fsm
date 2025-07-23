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
use crate::model::fs_state::FSState;
use crate::model::ui_state::UIState;
use crate::tasks::size_task;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

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
        self.last_error = Some(msg_str);
        self.redraw = true;
    }

    /// Set the latest info/status message (display in UI).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        let msg_str = msg.into();
        info!("Setting status: {}", msg_str);
        self.last_status = Some(msg_str);
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

        let result = dir_scanner::scan_dir(&canonical_path, self.ui.show_hidden).await;
        current_pane.is_loading = false;

        match result {
            Ok(entries) => {
                let parent_path = canonical_path.clone();
                for entry in &entries {
                    if entry.is_dir {
                        size_task::calculate_size_task(
                            parent_path.clone(),
                            entry.clone(),
                            self.action_tx.clone(),
                        );
                    }
                }
                current_pane.set_entries(entries);
                current_pane.last_error = None;
                self.fs.add_recent_dir(canonical_path);
            }
            Err(e) => {
                current_pane.entries.clear();
                current_pane.selected = None;
                let err_msg = format!("Failed to read directory: {}", e);
                current_pane.last_error = Some(err_msg.clone());
                self.set_error(err_msg);
            }
        }
        self.redraw = true;
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

    /// Enter the currently selected directory or open the file.
    pub async fn enter_selected_directory(&mut self) {
        let active_pane = self.fs.active_pane().clone();
        if let Some(selected_idx) = self.ui.selected {
            if let Some(selected_entry) = active_pane.entries.get(selected_idx) {
                if selected_entry.is_dir {
                    info!("Entering selected directory: {}", selected_entry.path.display());
                    self.enter_directory(selected_entry.path.clone()).await;
                } else {
                    // TODO: Implement file opening logic
                    info!("Opening selected file: {}", selected_entry.path.display());
                    self.set_status(&format!("Opening file: {}", selected_entry.name));
                }
            }
        } else {
            warn!("No entry selected to enter.");
            self.set_status("No entry selected.");
        }
        self.redraw = true;
    }

    /// Updates an ObjectInfo in the active pane with new data from a background task.
    pub fn update_object_info(&mut self, parent_dir: PathBuf, info: ObjectInfo) {
        if let Some(pane) = self.fs.panes.iter_mut().find(|p| p.cwd == parent_dir) {
            if let Some(entry) = pane.entries.iter_mut().find(|e| e.path == info.path) {
                entry.size = info.size;
                entry.items_count = info.items_count;
                self.redraw = true;
            }
        }
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