// fsm-core/src/model/app_state.rs
// Streamlined AppState focused on business logic and task management

use compact_str::CompactString;
use dashmap::DashMap;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use crate::{
    cache::cache_manager::ObjectInfoCache, config::Config, controller::actions::OperationId,
};

/// Task execution information
#[derive(Debug)]
pub struct TaskInfo {
    pub id: u64,
    pub description: CompactString,
    pub operation_id: Option<OperationId>,
    pub started_at: Instant,
    pub completed: AtomicBool,
    pub cancelled: AtomicBool,
    pub progress: AtomicU64, // 0-10000 (percentage * 100)
}

impl TaskInfo {
    pub fn new(id: u64, description: impl Into<CompactString>) -> Self {
        Self {
            id,
            description: description.into(),
            operation_id: None,
            started_at: Instant::now(),
            completed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            progress: AtomicU64::new(0),
        }
    }

    pub fn with_operation(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    pub fn set_progress(&self, progress: f32) {
        let value = (progress.clamp(0.0, 1.0) * 10000.0) as u64;
        self.progress.store(value, Ordering::Relaxed);
    }

    pub fn get_progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub fn complete(&self) {
        self.progress.store(10000, Ordering::Relaxed);
        self.completed.store(true, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::Relaxed)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

/// Historical operation tracking
#[derive(Debug, Clone)]
pub enum HistoryEvent {
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
    ClipboardOp {
        operation: CompactString,
        count: u32,
    },
}

/// Action execution statistics
#[derive(Debug)]
pub struct ActionStats {
    pub name: CompactString,
    pub count: AtomicU64,
    pub total_duration_ns: AtomicU64,
    pub last_executed: AtomicU64,
}

impl ActionStats {
    pub fn new(name: impl Into<CompactString>) -> Self {
        Self {
            name: name.into(),
            count: AtomicU64::new(0),
            total_duration_ns: AtomicU64::new(0),
            last_executed: AtomicU64::new(0),
        }
    }

    pub fn record_execution(&self, duration: Duration) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        self.last_executed.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    pub fn avg_duration(&self) -> Duration {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return Duration::ZERO;
        }
        let total_ns = self.total_duration_ns.load(Ordering::Relaxed);
        Duration::from_nanos(total_ns / count)
    }
}

/// Search state management
#[derive(Debug)]
pub struct SearchState {
    pub recent_queries: VecDeque<CompactString>,
    pub query_counts: HashMap<CompactString, u32>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            recent_queries: VecDeque::with_capacity(32),
            query_counts: HashMap::new(),
        }
    }
}

impl SearchState {
    pub fn add_query(&mut self, query: CompactString) {
        // Remove if exists, add to front
        if let Some(pos) = self.recent_queries.iter().position(|q| q == query) {
            self.recent_queries.remove(pos);
        }
        self.recent_queries.push_front(query.clone());

        // Limit size
        if self.recent_queries.len() > 32 {
            self.recent_queries.pop_back();
        }

        // Update count
        *self.query_counts.entry(query).or_insert(0) += 1;
    }
}

/// Main application state - business logic and task management
pub struct AppState {
    // Core services
    pub config: Arc<Config>,
    pub cache: Arc<ObjectInfoCache>,

    // Business state
    pub marked_files: HashSet<PathBuf>,
    pub operation_history: SmallVec<[HistoryEvent; 64]>,
    pub search_state: SearchState,

    // Task management
    pub tasks: DashMap<u64, Arc<TaskInfo>>,
    pub operation_tasks: DashMap<OperationId, u64>,
    task_counter: AtomicU64,

    // Performance metrics
    pub action_stats: DashMap<CompactString, Arc<ActionStats>>,
    pub operations_count: AtomicU64,
    pub errors_count: AtomicU64,

    // Session info
    pub session_id: CompactString,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(config: Arc<Config>, cache: Arc<ObjectInfoCache>) -> Self {
        let session_id = format!(
            "fsm_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
        .into();

        Self {
            config,
            cache,
            marked_files: HashSet::with_capacity(32),
            operation_history: SmallVec::new(),
            search_state: SearchState::default(),
            tasks: DashMap::with_capacity(32),
            operation_tasks: DashMap::with_capacity(32),
            task_counter: AtomicU64::new(1),
            action_stats: DashMap::new(),
            operations_count: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
            session_id,
            started_at: Instant::now(),
        }
    }

    // Task management
    pub fn add_task(&self, description: impl Into<CompactString>) -> u64 {
        let id = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let task = Arc::new(TaskInfo::new(id, description));
        self.tasks.insert(id, task);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        id
    }

    pub fn add_task_with_operation(
        &self,
        description: impl Into<CompactString>,
        operation_id: OperationId,
    ) -> u64 {
        let id = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let task = Arc::new(TaskInfo::new(id, description).with_operation(operation_id.clone()));
        self.tasks.insert(id, task);
        self.operation_tasks.insert(operation_id, id);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        id
    }

    pub fn remove_task(&self, id: u64) {
        if let Some((_, task)) = self.tasks.remove(&id) {
            if let Some(op_id) = &task.operation_id {
                self.operation_tasks.remove(op_id);
            }
        }
    }

    pub fn get_task(&self, id: u64) -> Option<Arc<TaskInfo>> {
        self.tasks.get(&id).map(|entry| entry.value().clone())
    }

    pub fn complete_task(&self, id: u64) {
        if let Some(task) = self.tasks.get(&id) {
            task.complete();
        }
    }

    // Action tracking
    pub fn record_action(&self, name: impl Into<CompactString>, duration: Duration) {
        let name = name.into();
        let stats = self
            .action_stats
            .entry(name.clone())
            .or_insert_with(|| Arc::new(ActionStats::new(name)));
        stats.record_execution(duration);
    }

    // File marking
    pub fn mark_file(&mut self, path: PathBuf) {
        self.marked_files.insert(path);
    }

    pub fn unmark_file(&mut self, path: &PathBuf) {
        self.marked_files.remove(path);
    }

    pub fn clear_marks(&mut self) {
        self.marked_files.clear();
    }

    pub fn is_marked(&self, path: &PathBuf) -> bool {
        self.marked_files.contains(path)
    }

    // History management
    pub fn add_history(&mut self, event: HistoryEvent) {
        if self.operation_history.len() >= 64 {
            self.operation_history.remove(0);
        }
        self.operation_history.push(event);
    }

    // Search management
    pub fn add_search_query(&mut self, query: CompactString) {
        self.search_state.add_query(query);
    }

    // Error tracking
    pub fn record_error(&self) {
        self.errors_count.fetch_add(1, Ordering::Relaxed);
    }

    // Cleanup
    pub fn cleanup_completed_tasks(&self) {
        let completed: Vec<u64> = self
            .tasks
            .iter()
            .filter(|entry| entry.value().is_completed())
            .map(|entry| *entry.key())
            .collect();

        for id in completed {
            self.remove_task(id);
        }
    }

    // Metrics
    pub fn get_metrics(&self) -> AppMetrics {
        AppMetrics {
            session_id: self.session_id.clone(),
            uptime: self.started_at.elapsed(),
            operations_count: self.operations_count.load(Ordering::Relaxed),
            errors_count: self.errors_count.load(Ordering::Relaxed),
            active_tasks: self.tasks.len(),
            marked_files: self.marked_files.len(),
            history_size: self.operation_history.len(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(
            Arc::new(Config::default()),
            Arc::new(ObjectInfoCache::new()),
        )
    }
}

#[derive(Debug, Clone)]
pub struct AppMetrics {
    pub session_id: CompactString,
    pub uptime: Duration,
    pub operations_count: u64,
    pub errors_count: u64,
    pub active_tasks: usize,
    pub marked_files: usize,
    pub history_size: usize,
}
