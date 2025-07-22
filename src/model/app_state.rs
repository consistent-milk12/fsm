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
use crate::controller::event_loop::TaskResult;
use crate::model::fs_state::FSState;
use crate::model::ui_state::UIState;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

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
    ) -> Self {
        Self {
            config,
            cache,
            ui,
            fs,
            task_tx,
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
        self.tasks.insert(task.task_id, task);
        self.redraw = true;
    }

    /// Update task completion/result.
    pub fn complete_task(&mut self, task_id: u64, result: Option<String>) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            task.is_completed = true;
            task.result = result;
            self.redraw = true;
        }
    }

    /// Set the latest error message (display in UI).
    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.last_error = Some(msg.into());
        self.redraw = true;
    }

    /// Set the latest info/status message (display in UI).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.last_status = Some(msg.into());
        self.redraw = true;
    }

    /// Clear error and status messages.
    pub fn clear_msgs(&mut self) {
        self.last_error = None;
        self.last_status = None;
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

