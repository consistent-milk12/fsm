//! AppState: Minimal Core State for Phase 4.0
//!
//! Dramatically simplified for StateCoordinator integration:
//! - Essential business logic only
//! - No UI state duplication
//! - Atomic task tracking
//! - Lock-free operation counters

use crate::cache::cache_manager::ObjectInfoCache;
use crate::config::Config;
use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use crate::model::fs_state::FSState;

use compact_str::CompactString;
use parking_lot::RwLock;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Optimized task info with atomic counters
#[derive(Debug)]
pub struct TaskInfo {
    pub task_id: u64,
    pub description: CompactString,
    pub started_at: Instant,
    pub is_completed: AtomicBool,
    pub result: RwLock<Option<CompactString>>,
    pub progress: AtomicU64,
    pub current_item: RwLock<Option<CompactString>>,
    pub completed: AtomicU64,
    pub total: AtomicU64,
}

impl TaskInfo {
    pub fn new(task_id: u64, description: impl Into<CompactString>) -> Self {
        Self {
            task_id,
            description: description.into(),
            started_at: Instant::now(),
            is_completed: AtomicBool::new(false),
            result: RwLock::new(None),
            progress: AtomicU64::new(0),
            current_item: RwLock::new(None),
            completed: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }

    /// Update progress atomically
    pub fn update_progress(&self, completed: u64, total: u64, current_item: Option<CompactString>) {
        self.completed.store(completed, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);

        if total > 0 {
            let progress = completed as f64 / total as f64;
            self.progress.store(progress as u64, Ordering::Relaxed);
        }

        if let Some(item) = current_item {
            *self.current_item.write() = Some(item);
        }
    }

    /// Mark task as completed
    pub fn complete(&self, result: Option<CompactString>) {
        *self.result.write() = result;
        self.is_completed.store(true, Ordering::Relaxed);
    }
}

/// History event for undo/redo
#[derive(Debug, Clone)]
pub enum AppHistoryEvent {
    Delete {
        paths: SmallVec<[PathBuf; 4]>,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    BatchOp {
        operation: CompactString,
        paths: SmallVec<[PathBuf; 8]>,
        timestamp: Instant,
    },
}

/// Plugin info
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: CompactString,
    pub description: CompactString,
    pub loaded: bool,
    pub version: Option<CompactString>,
}

/// Core application state - minimal and fast
pub struct AppState {
    // Core services (immutable after init)
    pub config: Arc<Config>,
    pub cache: Arc<ObjectInfoCache>,

    // Filesystem state
    pub fs: FSState,

    // Communication channels
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub action_tx: mpsc::UnboundedSender<Action>,

    // Business logic state with optimized collections
    pub marked: HashSet<PathBuf>,
    pub history: SmallVec<[AppHistoryEvent; 32]>, // SmallVec for better cache performance
    pub plugins: HashMap<CompactString, PluginInfo>,

    // Atomic task tracking
    pub tasks: dashmap::DashMap<u64, Arc<TaskInfo>>,
    pub task_counter: AtomicU64,

    // Performance counters
    pub operations_count: AtomicU64,
    pub errors_count: AtomicU64,

    // Timestamps
    pub started_at: Instant,
    pub last_operation: AtomicU64, // Unix timestamp in milliseconds
}

impl AppState {
    /// Create new optimized app state
    pub fn new(
        config: Arc<Config>,
        cache: Arc<ObjectInfoCache>,
        fs: FSState,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        Self {
            config,
            cache,
            fs,
            task_tx,
            action_tx,
            marked: HashSet::with_capacity(16),
            history: SmallVec::new(),
            plugins: HashMap::with_capacity(8),
            tasks: dashmap::DashMap::with_capacity(16),
            task_counter: AtomicU64::new(1),
            operations_count: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
            started_at: Instant::now(),
            last_operation: AtomicU64::new(0),
        }
    }

    /// Generate unique task ID
    pub fn next_task_id(&self) -> u64 {
        self.task_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Add task with atomic operations
    pub fn add_task(&self, description: impl Into<CompactString>) -> u64 {
        let task_id = self.next_task_id();
        let task = Arc::new(TaskInfo::new(task_id, description));
        self.tasks.insert(task_id, task);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        task_id
    }

    /// Complete task atomically
    pub fn complete_task(&self, task_id: u64, result: Option<CompactString>) {
        if let Some(task_ref) = self.tasks.get(&task_id) {
            task_ref.complete(result);
            self.update_last_operation();
        }
    }

    /// Update progress for task
    pub fn update_task_progress(
        &self,
        task_id: u64,
        completed: u64,
        total: u64,
        current_item: Option<CompactString>,
    ) {
        if let Some(task_ref) = self.tasks.get(&task_id) {
            task_ref.update_progress(completed, total, current_item);
        }
    }

    /// Remove completed task
    pub fn remove_task(&self, task_id: u64) {
        self.tasks.remove(&task_id);
    }

    /// Get task info
    pub fn get_task(&self, task_id: u64) -> Option<Arc<TaskInfo>> {
        self.tasks.get(&task_id).map(|entry| entry.value().clone())
    }

    /// Mark entry for batch operations
    #[inline]
    pub fn mark_entry(&mut self, path: PathBuf) {
        self.marked.insert(path);
    }

    /// Unmark entry
    #[inline]
    pub fn unmark_entry(&mut self, path: &PathBuf) {
        self.marked.remove(path);
    }

    /// Clear all marks
    #[inline]
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Add to history with capacity management
    pub fn push_history(&mut self, event: AppHistoryEvent) {
        if self.history.len() >= 32 {
            self.history.remove(0);
        }
        self.history.push(event);
    }

    /// Register plugin
    pub fn register_plugin(&mut self, info: PluginInfo) {
        self.plugins.insert(info.name.clone(), info);
    }

    /// Increment error counter
    #[inline]
    pub fn increment_errors(&self) {
        self.errors_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Update last operation timestamp
    #[inline]
    fn update_last_operation(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_operation.store(now, Ordering::Relaxed);
    }

    /// Get performance metrics
    pub fn get_metrics(&self) -> AppMetrics {
        AppMetrics {
            uptime: self.started_at.elapsed(),
            operations_count: self.operations_count.load(Ordering::Relaxed),
            errors_count: self.errors_count.load(Ordering::Relaxed),
            active_tasks: self.tasks.len(),
            marked_items: self.marked.len(),
            history_size: self.history.len(),
            plugins_loaded: self.plugins.values().filter(|p| p.loaded).count(),
        }
    }
}

/// Performance metrics snapshot
#[derive(Debug, Clone)]
pub struct AppMetrics {
    pub uptime: Duration,
    pub operations_count: u64,
    pub errors_count: u64,
    pub active_tasks: usize,
    pub marked_items: usize,
    pub history_size: usize,
    pub plugins_loaded: usize,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("fs", &self.fs)
            .field("marked_count", &self.marked.len())
            .field("history_size", &self.history.len())
            .field("active_tasks", &self.tasks.len())
            .field("plugins", &self.plugins.len())
            .field(
                "operations_count",
                &self.operations_count.load(Ordering::Relaxed),
            )
            .field("started_at", &self.started_at)
            .finish()
    }
}
