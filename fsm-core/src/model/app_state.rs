//! ``src/model/app_state.rs``
//! ============================================================================
//! # `AppState`: Core Business Logic State
//!
//! Refactored AppState containing only business logic and channels.
//! UI and FS state moved to separate Arc<Mutex> containers for fine-grained locking.

use crate::config::Config;
use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use crate::model::metadata_manager::MetadataManager;

use std::sync::Arc;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Represents a pending or running asynchronous task (search, copy, delete, etc.).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: u64,
    pub description: String,
    pub started_at: Instant,
    pub is_completed: bool,
    pub result: Option<String>,
    pub progress: Option<f64>,
    pub current_item: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// History events for undo/redo operations
#[derive(Debug, Clone)]
pub enum AppHistoryEvent {
    Delete { paths: Vec<String> },
    Move { from: String, to: String },
    Rename { from: String, to: String },
    Copy { from: String, to: String },
}

/// Info about a registered plugin
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub loaded: bool,
}

/// Shared application state - Contains only metadata and business logic
/// UI and FS state moved to separate Arc<Mutex> containers for fine-grained locking
pub struct AppState {
    // --- Core Configuration and Services ---
    pub config: Arc<Config>,
    pub metadata: Arc<MetadataManager>,

    // --- Communication Channels ---
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub action_tx: mpsc::UnboundedSender<Action>,

    // --- Business Logic State ---
    /// Marked files/directories by path for batch operations
    pub marked: HashSet<PathBuf>,

    /// Application history for undo/redo operations
    pub history: VecDeque<AppHistoryEvent>,

    /// Plugin registry and information
    pub plugins: HashMap<String, PluginInfo>,

    /// Active background tasks
    pub tasks: HashMap<u64, TaskInfo>,

    /// Last error message (distinct from UI notifications)
    pub last_error: Option<String>,

    /// Application startup time for analytics
    pub started_at: Instant,
}

impl AppState {
    #[must_use]
    pub fn new(
        config: Arc<Config>,
        metadata: Arc<MetadataManager>,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        Self {
            // Core Configuration and Services
            config,
            metadata,

            // Communication Channels
            task_tx,
            action_tx,

            // Business Logic State
            marked: HashSet::new(),
            history: VecDeque::new(),
            plugins: HashMap::new(),
            tasks: HashMap::new(),
            last_error: None,
            started_at: Instant::now(),
        }
    }

    /// Mark a file or directory by its canonical path for batch operations.
    pub fn mark_entry(&mut self, path: impl Into<PathBuf>) {
        self.marked.insert(path.into());
    }

    /// Unmark a previously marked entry.
    pub fn unmark_entry(&mut self, path: &std::path::Path) {
        self.marked.remove(path);
    }

    /// Clear all marked entries.
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Add a reversible event to the history stack (for undo/redo).
    pub fn push_history(&mut self, event: AppHistoryEvent) {
        self.history.push_back(event);

        if self.history.len() > 128 {
            self.history.pop_front();
        }
    }

    /// Register a plugin for later use.
    pub fn register_plugin(&mut self, info: PluginInfo) {
        self.plugins.insert(info.name.clone(), info);
    }

    /// Add or update a running/pending async task.
    pub fn add_task(&mut self, task: TaskInfo) {
        info!("Adding task: {}", task.description);
        self.tasks.insert(task.task_id, task);
    }

    /// Update task completion/result.
    pub fn complete_task(&mut self, task_id: u64, result: Option<String>) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            info!("Completing task: {}", task.description);
            task.is_completed = true;
            task.result = result;
        }
    }

    /// Set the latest error message.
    pub fn set_error(&mut self, msg: impl Into<String>) {
        let msg_str: String = msg.into();
        warn!("Setting error: {}", msg_str);
        self.last_error = Some(msg_str);
    }

    /// Clear error messages.
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("task_tx", &"Transfer Channel")
            .field("action_tx", &"Action Channel")
            .field("config", &"Config")
            .field("metadata", &"MetadataManager")
            .field("marked", &self.marked)
            .field("history", &self.history)
            .field("tasks", &self.tasks)
            .field("plugins", &self.plugins)
            .field("last_error", &self.last_error)
            .field("started_at", &self.started_at)
            .finish()
    }
}
