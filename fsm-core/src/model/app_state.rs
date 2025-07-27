//! Enhanced AppState with action integration and performance optimizations

use crate::cache::cache_manager::ObjectInfoCache;
use crate::config::Config;
use crate::controller::actions::{Action, OperationId};
use crate::controller::event_loop::TaskResult;
use crate::model::fs_state::FSState;

use compact_str::CompactString;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Task progress information
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub current: u64,
    pub total: u64,
    pub message: Option<String>,
}

/// Enhanced task info with action integration
#[derive(Debug)]
pub struct TaskInfo {
    pub task_id: u64,
    pub operation_id: Option<OperationId>,
    pub description: CompactString,
    pub started_at: Instant,
    pub is_completed: AtomicBool,
    pub is_cancelled: AtomicBool,
    pub result: RwLock<Option<CompactString>>,
    pub progress: AtomicU64, // 0-10000 (0.00% - 100.00%)
    pub current_item: RwLock<Option<CompactString>>,
    pub completed: AtomicU64,
    pub total: AtomicU64,
    pub task_type: TaskType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    DirectoryScan,
    FileOperation,
    Search,
    ClipboardOperation,
    Custom,
}

impl TaskInfo {
    pub fn new(task_id: u64, description: impl Into<CompactString>, task_type: TaskType) -> Self {
        Self {
            task_id,
            operation_id: None,
            description: description.into(),
            started_at: Instant::now(),
            is_completed: AtomicBool::new(false),
            is_cancelled: AtomicBool::new(false),
            result: RwLock::new(None),
            progress: AtomicU64::new(0),
            current_item: RwLock::new(None),
            completed: AtomicU64::new(0),
            total: AtomicU64::new(0),
            task_type,
        }
    }

    pub fn with_operation_id(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    /// Update progress atomically
    pub fn update_progress(&self, completed: u64, total: u64, current_item: Option<CompactString>) {
        self.completed.store(completed, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);

        if total > 0 {
            let progress = ((completed as f64 / total as f64) * 10000.0) as u64;
            self.progress.store(progress.min(10000), Ordering::Relaxed);
        }

        if let Some(item) = current_item {
            *self.current_item.write() = Some(item);
        }
    }

    /// Set progress directly (0.0 - 1.0)
    pub fn set_progress(&self, progress: f32) {
        let progress_int = (progress.clamp(0.0, 1.0) * 10000.0) as u64;
        self.progress.store(progress_int, Ordering::Relaxed);
    }

    /// Get progress as percentage (0.0 - 1.0)
    pub fn get_progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 10000.0
    }

    /// Cancel task
    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Relaxed)
    }

    /// Complete task
    pub fn complete(&self, result: Option<CompactString>) {
        *self.result.write() = result;
        self.is_completed.store(true, Ordering::Relaxed);
        self.progress.store(10000, Ordering::Relaxed);
    }

    /// Get completion status
    pub fn is_completed(&self) -> bool {
        self.is_completed.load(Ordering::Relaxed)
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

/// Enhanced history event with operation tracking
#[derive(Debug, Clone)]
pub enum AppHistoryEvent {
    Delete {
        paths: SmallVec<[PathBuf; 4]>,
        operation_id: Option<OperationId>,
    },
    Move {
        from: PathBuf,
        to: PathBuf,
        operation_id: Option<OperationId>,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
        operation_id: Option<OperationId>,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
        operation_id: Option<OperationId>,
    },
    BatchOp {
        operation: CompactString,
        paths: SmallVec<[PathBuf; 8]>,
        timestamp: Instant,
        operation_id: Option<OperationId>,
    },
    ClipboardOp {
        operation: CompactString,
        items: u32,
        operation_id: Option<OperationId>,
        timestamp: Instant,
    },
}

/// Plugin info with enhanced metadata
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: CompactString,
    pub description: CompactString,
    pub loaded: bool,
    pub version: Option<CompactString>,
    pub author: Option<CompactString>,
    pub actions: Vec<CompactString>,
    pub load_time: Option<Duration>,
}

/// Enhanced search state
#[derive(Debug)]
pub struct SearchState {
    pub active_searches: HashMap<CompactString, SearchInfo>,
    pub recent_queries: VecDeque<CompactString>,
    pub search_history: HashMap<CompactString, u32>, // query -> usage count
}

#[derive(Debug)]
pub struct SearchInfo {
    pub query: CompactString,
    pub started_at: Instant,
    pub task_id: Option<u64>,
    pub results_count: AtomicUsize,
    pub is_completed: AtomicBool,
}

/// Enhanced application state with action integration
pub struct AppState {
    // Core services (immutable after init)
    pub config: Arc<Config>,
    pub cache: Arc<ObjectInfoCache>,

    // Filesystem state
    pub fs: FSState,

    // Communication channels
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub action_tx: mpsc::UnboundedSender<Action>,

    // Enhanced business logic state
    pub marked: HashSet<PathBuf>,
    pub history: SmallVec<[AppHistoryEvent; 64]>, // Increased capacity
    pub plugins: HashMap<CompactString, PluginInfo>,

    // Task tracking with operation correlation
    pub tasks: DashMap<u64, Arc<TaskInfo>>,
    pub operation_tasks: DashMap<OperationId, u64>, // operation_id -> task_id
    pub task_counter: AtomicU64,

    // Performance counters
    pub operations_count: AtomicU64,
    pub errors_count: AtomicU64,
    pub action_count: AtomicU64,

    // Search state
    pub search_state: Mutex<SearchState>,

    // Session state
    pub session_id: CompactString,
    pub started_at: Instant,
    pub last_operation: AtomicU64,
    pub last_action: AtomicU64,

    // Clipboard integration
    pub clipboard_operations: DashMap<OperationId, ClipboardOperationInfo>,

    // Action statistics
    pub action_stats: DashMap<CompactString, ActionStats>,
    
    // Task progress tracking
    pub task_progress: HashMap<String, TaskProgress>,
}

#[derive(Debug)]
pub struct ClipboardOperationInfo {
    pub operation_id: OperationId,
    pub operation_type: CompactString,
    pub items_count: u32,
    pub started_at: Instant,
    pub progress: AtomicU64,
}

#[derive(Debug)]
pub struct ActionStats {
    pub action_name: CompactString,
    pub count: AtomicU64,
    pub total_duration: AtomicU64, // nanoseconds
    pub last_executed: AtomicU64,  // timestamp
}

impl AppState {
    /// Create enhanced app state
    pub fn new(
        config: Arc<Config>,
        cache: Arc<ObjectInfoCache>,
        fs: FSState,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        let session_id = format!("fsm_{}", chrono::Utc::now().timestamp()).into();

        Self {
            config,
            cache,
            fs,
            task_tx,
            action_tx,
            marked: HashSet::with_capacity(32),
            history: SmallVec::new(),
            plugins: HashMap::with_capacity(16),
            tasks: DashMap::with_capacity(32),
            operation_tasks: DashMap::with_capacity(32),
            task_counter: AtomicU64::new(1),
            operations_count: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
            action_count: AtomicU64::new(0),
            search_state: Mutex::new(SearchState {
                active_searches: HashMap::new(),
                recent_queries: VecDeque::with_capacity(32),
                search_history: HashMap::new(),
            }),
            session_id,
            started_at: Instant::now(),
            last_operation: AtomicU64::new(0),
            last_action: AtomicU64::new(0),
            clipboard_operations: DashMap::new(),
            action_stats: DashMap::new(),
            task_progress: HashMap::new(),
        }
    }

    /// Update task progress
    pub fn set_task_progress(
        &mut self,
        task_id: String,
        current: u64,
        total: u64,
        message: Option<String>,
    ) {
        self.task_progress.insert(
            task_id,
            TaskProgress {
                current,
                total,
                message,
            },
        );
    }

    /// Enhanced task management
    pub fn add_task(&self, description: impl Into<CompactString>, task_type: TaskType) -> u64 {
        let task_id = self.next_task_id();
        let task = Arc::new(TaskInfo::new(task_id, description, task_type));
        self.tasks.insert(task_id, task);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        task_id
    }

    pub fn add_task_with_operation(
        &self,
        description: impl Into<CompactString>,
        task_type: TaskType,
        operation_id: OperationId,
    ) -> u64 {
        let task_id = self.next_task_id();
        let task = Arc::new(
            TaskInfo::new(task_id, description, task_type).with_operation_id(operation_id.clone()),
        );
        self.tasks.insert(task_id, task);
        self.operation_tasks.insert(operation_id, task_id);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        task_id
    }

    /// Action tracking
    pub fn track_action(&self, action: &Action) {
        let action_name: CompactString = self.get_action_name(action);
        let now = chrono::Utc::now().timestamp_millis() as u64;

        self.action_count.fetch_add(1, Ordering::Relaxed);
        self.last_action.store(now, Ordering::Relaxed);

        // Update action statistics
        let stats = self
            .action_stats
            .entry(action_name.clone())
            .or_insert_with(|| ActionStats {
                action_name: action_name.clone(),
                count: AtomicU64::new(0),
                total_duration: AtomicU64::new(0),
                last_executed: AtomicU64::new(0),
            });

        stats.count.fetch_add(1, Ordering::Relaxed);
        stats.last_executed.store(now, Ordering::Relaxed);
    }

    fn get_action_name(&self, action: &Action) -> CompactString {
        match action {
            Action::MoveSelectionUp => "move_selection_up".into(),
            Action::MoveSelectionDown => "move_selection_down".into(),
            Action::EnterSelected => "enter_selected".into(),
            Action::GoToParent => "go_to_parent".into(),
            Action::Copy(_) => "copy".into(),
            Action::Cut(_) => "cut".into(),
            Action::Paste => "paste".into(),
            Action::Delete => "delete".into(),
            Action::CreateFile => "create_file".into(),
            Action::CreateDirectory => "create_directory".into(),
            Action::Quit => "quit".into(),
            _ => "other".into(),
        }
    }

    /// Search management
    pub fn start_search(&self, query: CompactString, task_id: Option<u64>) -> CompactString {
        let search_id: CompactString =
            format!("search_{}", chrono::Utc::now().timestamp_millis()).into();

        let mut search_state = self.search_state.lock();
        search_state.active_searches.insert(
            search_id.clone(),
            SearchInfo {
                query: query.clone(),
                started_at: Instant::now(),
                task_id,
                results_count: AtomicUsize::new(0),
                is_completed: AtomicBool::new(false),
            },
        );

        // Add to recent queries
        if let Some(pos) = search_state.recent_queries.iter().position(|q| q == &query) {
            search_state.recent_queries.remove(pos);
        }
        search_state.recent_queries.push_front(query.clone());
        if search_state.recent_queries.len() > 32 {
            search_state.recent_queries.pop_back();
        }

        // Update search history
        *search_state.search_history.entry(query).or_insert(0) += 1;

        search_id
    }

    pub fn complete_search(&self, search_id: &str, results_count: usize) {
        let search_state = self.search_state.lock();
        if let Some(search_info) = search_state.active_searches.get(search_id) {
            search_info
                .results_count
                .store(results_count, Ordering::Relaxed);
            search_info.is_completed.store(true, Ordering::Relaxed);
        }
    }

    /// Clipboard operation tracking
    pub fn start_clipboard_operation(
        &self,
        operation_id: OperationId,
        operation_type: CompactString,
        items_count: u32,
    ) {
        self.clipboard_operations.insert(
            operation_id.clone(),
            ClipboardOperationInfo {
                operation_id: operation_id.clone(),
                operation_type,
                items_count,
                started_at: Instant::now(),
                progress: AtomicU64::new(0),
            },
        );
    }

    pub fn update_clipboard_progress(&self, operation_id: &OperationId, progress: f32) {
        if let Some(op_info) = self.clipboard_operations.get(operation_id) {
            let progress_int = (progress.clamp(0.0, 1.0) * 10000.0) as u64;
            op_info.progress.store(progress_int, Ordering::Relaxed);
        }
    }

    pub fn complete_clipboard_operation(&self, operation_id: &OperationId) {
        self.clipboard_operations.remove(operation_id);
    }

    /// Enhanced history management
    pub fn push_history(&mut self, event: AppHistoryEvent) {
        if self.history.len() >= 64 {
            self.history.remove(0);
        }
        self.history.push(event);
    }

    /// Get enhanced metrics
    pub fn get_metrics(&self) -> AppMetrics {
        let search_state = self.search_state.lock();

        AppMetrics {
            session_id: self.session_id.clone(),
            uptime: self.started_at.elapsed(),
            operations_count: self.operations_count.load(Ordering::Relaxed),
            errors_count: self.errors_count.load(Ordering::Relaxed),
            action_count: self.action_count.load(Ordering::Relaxed),
            active_tasks: self.tasks.len(),
            marked_items: self.marked.len(),
            history_size: self.history.len(),
            plugins_loaded: self.plugins.values().filter(|p| p.loaded).count(),
            active_searches: search_state.active_searches.len(),
            clipboard_operations: self.clipboard_operations.len(),
        }
    }

    /// Cleanup completed tasks
    pub fn cleanup_completed_tasks(&self) {
        let completed_tasks: Vec<u64> = self
            .tasks
            .iter()
            .filter(|entry| entry.value().is_completed())
            .map(|entry| *entry.key())
            .collect();

        for task_id in completed_tasks {
            self.tasks.remove(&task_id);
        }
    }

    /// Get task by operation ID
    pub fn get_task_by_operation(&self, operation_id: &OperationId) -> Option<Arc<TaskInfo>> {
        self.operation_tasks
            .get(operation_id)
            .and_then(|task_id| self.tasks.get(&task_id).map(|entry| entry.value().clone()))
    }

    // Existing methods...
    pub fn next_task_id(&self) -> u64 {
        self.task_counter.fetch_add(1, Ordering::Relaxed)
    }

    pub fn complete_task(&self, task_id: u64, result: Option<CompactString>) {
        if let Some(task_ref) = self.tasks.get(&task_id) {
            task_ref.complete(result);
            self.update_last_operation();
        }
    }

    pub fn get_task(&self, task_id: u64) -> Option<Arc<TaskInfo>> {
        self.tasks.get(&task_id).map(|entry| entry.value().clone())
    }

    #[inline]
    pub fn mark_entry(&mut self, path: PathBuf) {
        self.marked.insert(path);
    }

    #[inline]
    pub fn unmark_entry(&mut self, path: &PathBuf) {
        self.marked.remove(path);
    }

    #[inline]
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    #[inline]
    pub fn increment_errors(&self) {
        self.errors_count.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn update_last_operation(&self) {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        self.last_operation.store(now, Ordering::Relaxed);
    }
}

/// Enhanced performance metrics
#[derive(Debug, Clone)]
pub struct AppMetrics {
    pub session_id: CompactString,
    pub uptime: Duration,
    pub operations_count: u64,
    pub errors_count: u64,
    pub action_count: u64,
    pub active_tasks: usize,
    pub marked_items: usize,
    pub history_size: usize,
    pub plugins_loaded: usize,
    pub active_searches: usize,
    pub clipboard_operations: usize,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("session_id", &self.session_id)
            .field("fs", &self.fs)
            .field("marked_count", &self.marked.len())
            .field("history_size", &self.history.len())
            .field("active_tasks", &self.tasks.len())
            .field("plugins", &self.plugins.len())
            .field(
                "operations_count",
                &self.operations_count.load(Ordering::Relaxed),
            )
            .field("action_count", &self.action_count.load(Ordering::Relaxed))
            .field("started_at", &self.started_at)
            .finish()
    }
}