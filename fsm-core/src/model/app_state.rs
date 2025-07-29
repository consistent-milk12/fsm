// fsm-core/src/model/app_state.rs
// Streamlined AppState focused on business logic and task management

// Import tracing macros for spans and events
use tracing::{debug, info, instrument, warn};

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
    pub progress: AtomicU64, // 0–10000 (percentage ×100)
}

impl TaskInfo {
    // Instrument creation; record task_id, skip description param
    #[instrument(
        level = "info",
        fields(task_id = id),
        skip(description)
    )]
    pub fn new(id: u64, description: impl Into<CompactString>) -> Self {
        // Convert description into CompactString
        let desc = description.into();
        debug!(description = %desc, "Initializing TaskInfo");
        Self {
            id,
            description: desc,
            operation_id: None,
            started_at: Instant::now(),
            completed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            progress: AtomicU64::new(0),
        }
    }

    // Instrument operation attach; skip self for logging
    #[instrument(level = "debug", skip(self), fields(operation_id = ?operation_id))]
    pub fn with_operation(mut self, operation_id: OperationId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    // Instrument progress update; skip self
    #[instrument(level = "debug", skip(self))]
    pub fn set_progress(&self, progress: f32) {
        let value = (progress.clamp(0.0, 1.0) * 10000.0) as u64;
        self.progress.store(value, Ordering::Relaxed);
        debug!(pct = progress, "Task progress updated");
    }

    // Instrument progress read; skip self
    #[instrument(level = "trace", skip(self))]
    pub fn get_progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 10000.0
    }

    // Instrument completion; skip self
    #[instrument(level = "info", skip(self))]
    pub fn complete(&self) {
        self.progress.store(10000, Ordering::Relaxed);
        self.completed.store(true, Ordering::Relaxed);
        info!("Task marked completed");
    }

    // Instrument cancellation; skip self
    #[instrument(level = "warn", skip(self))]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        warn!("Task marked cancelled");
    }

    // Instrument completion check; skip self
    #[instrument(level = "trace", skip(self))]
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::Relaxed)
    }

    // Instrument cancellation check; skip self
    #[instrument(level = "trace", skip(self))]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    // Instrument elapsed time; skip self
    #[instrument(level = "debug", skip(self))]
    pub fn elapsed(&self) -> Duration {
        let d = self.started_at.elapsed();
        debug!(?d, "Task elapsed time");
        d
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
    // Instrument creation; record internal state
    pub fn new(name: impl Into<CompactString>) -> Self {
        Self {
            name: name.into(),
            count: AtomicU64::new(0),
            total_duration_ns: AtomicU64::new(0),
            last_executed: AtomicU64::new(0),
        }
    }

    // Instrument execution recording; skip self for performance
    #[instrument(level = "debug", skip(self, duration))]
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
        debug!(ns = duration.as_nanos(), "Action execution recorded");
    }

    // Instrument average duration calculation; skip self
    #[instrument(level = "trace", skip(self))]
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
    #[instrument(level = "debug")]
    fn default() -> Self {
        Self {
            recent_queries: VecDeque::with_capacity(32),
            query_counts: HashMap::new(),
        }
    }
}

impl SearchState {
    // Instrument adding a query; skip self
    #[instrument(level = "info", skip(self), fields(query = ?query))]
    pub fn add_query(&mut self, query: CompactString) {
        if let Some(pos) = self.recent_queries.iter().position(|q| q == &query) {
            self.recent_queries.remove(pos);
        }
        self.recent_queries.push_front(query.clone());
        if self.recent_queries.len() > 32 {
            self.recent_queries.pop_back();
        }
        *self.query_counts.entry(query.clone()).or_insert(0) += 1;
        info!(count = self.query_counts[&query], "Search query recorded");
    }
}

/// Main application state - business logic and task management
pub struct AppState {
    pub config: Arc<Config>,         // Application configuration
    pub cache: Arc<ObjectInfoCache>, // Metadata cache

    pub marked_files: HashSet<PathBuf>, // User-selected files
    pub operation_history: SmallVec<[HistoryEvent; 64]>,
    pub search_state: SearchState, // Search usage state

    pub tasks: DashMap<u64, Arc<TaskInfo>>, // In-flight tasks
    pub operation_tasks: DashMap<OperationId, u64>,
    task_counter: AtomicU64,

    pub action_stats: DashMap<CompactString, Arc<ActionStats>>,
    pub operations_count: AtomicU64,
    pub errors_count: AtomicU64,

    pub session_id: CompactString, // Unique session identifier
    pub started_at: Instant,       // State creation timestamp
}

impl AppState {
    // Instrument initialization; skip heavy parameters
    #[instrument(level = "info", skip(config, cache))]
    pub fn new(config: Arc<Config>, cache: Arc<ObjectInfoCache>) -> Self {
        let session_id: String = format!(
            "fsm_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        let state = Self {
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
            session_id: session_id.clone().into(),
            started_at: Instant::now(),
        };

        info!(session = %session_id, "AppState initialized");
        state
    }

    // Instrument task addition; skip description param
    #[instrument(level = "info", skip(self, description))]
    pub fn add_task(&self, description: impl Into<CompactString>) -> u64 {
        // Convert description into CompactString
        let desc = description.into();
        let id: u64 = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let task: Arc<TaskInfo> = Arc::new(TaskInfo::new(id, desc.clone()));
        self.tasks.insert(id, task);

        self.operations_count.fetch_add(1, Ordering::Relaxed);
        info!(task_id = id, description = %desc, "Task added");

        id
    }

    // Instrument task+operation addition; skip description param
    #[instrument(
        level = "info", skip(self, description),
        fields(operation_id = ?operation_id)
    )]
    pub fn add_task_with_operation(
        &self,
        description: impl Into<CompactString>,
        operation_id: OperationId,
    ) -> u64 {
        // Convert description into CompactString
        let desc = description.into();
        let id: u64 = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let task: Arc<TaskInfo> =
            Arc::new(TaskInfo::new(id, desc.clone()).with_operation(operation_id.clone()));

        self.tasks.insert(id, task);
        self.operation_tasks.insert(operation_id.clone(), id);
        self.operations_count.fetch_add(1, Ordering::Relaxed);
        info!(
            task_id = id,
            operation_id = ?operation_id,
            description = %desc,
            "Task with operation added"
        );
        id
    }

    // Instrument removal of a task; skip self
    #[instrument(level = "debug", skip(self), fields(task_id = id))]
    pub fn remove_task(&self, id: u64) {
        if let Some((_, task)) = self.tasks.remove(&id) {
            if let Some(op_id) = &task.operation_id {
                self.operation_tasks.remove(op_id);
            }
            debug!("Task removed");
        }
    }

    // Instrument retrieval of a task; skip self
    #[instrument(level = "trace", skip(self), fields(task_id = id))]
    pub fn get_task(&self, id: u64) -> Option<Arc<TaskInfo>> {
        let found = self.tasks.get(&id).map(|e| e.value().clone());
        debug!(found = found.is_some(), "Get task");
        found
    }

    // Instrument completion of a task; skip self
    #[instrument(level = "info", skip(self), fields(task_id = id))]
    pub fn complete_task(&self, id: u64) {
        if let Some(task) = self.tasks.get(&id) {
            task.complete();
            info!("Task completion signaled");
        }
    }

    // Instrument action recording; skip name and duration
    #[instrument(level = "debug", skip(self, name, duration))]
    pub fn record_action(&self, name: impl Into<CompactString>, duration: Duration) {
        let name_str = name.into();
        let stats = self
            .action_stats
            .entry(name_str.clone())
            .or_insert_with(|| Arc::new(ActionStats::new(name_str.clone())));
        stats.record_execution(duration);
        debug!(action = %name_str, ns = duration.as_nanos(), "Action recorded");
    }

    // File marking methods with tracing
    #[instrument(level = "debug", skip(self), fields(path = ?path))]
    pub fn mark_file(&mut self, path: PathBuf) {
        self.marked_files.insert(path.clone());
        debug!("File marked");
    }

    #[instrument(level = "debug", skip(self), fields(path = ?path))]
    pub fn unmark_file(&mut self, path: &PathBuf) {
        self.marked_files.remove(path);
        debug!("File unmarked");
    }

    #[instrument(level = "debug", skip(self))]
    pub fn clear_marks(&mut self) {
        self.marked_files.clear();
        debug!("All file marks cleared");
    }

    #[instrument(level = "trace", skip(self), fields(path = ?path))]
    pub fn is_marked(&self, path: &PathBuf) -> bool {
        let result = self.marked_files.contains(path);
        debug!(marked = result, "Is marked query");
        result
    }

    // History management with tracing
    #[instrument(level = "info", skip(self), fields(event = ?event))]
    pub fn add_history(&mut self, event: HistoryEvent) {
        if self.operation_history.len() >= 64 {
            self.operation_history.remove(0);
        }
        self.operation_history.push(event.clone());
        info!("History event added");
    }

    // Search management with tracing
    #[instrument(level = "debug", skip(self, query))]
    pub fn add_search_query(&mut self, query: CompactString) {
        self.search_state.add_query(query.clone());
        debug!("Search query added");
    }

    // Error tracking with tracing
    #[instrument(level = "warn", skip(self))]
    pub fn record_error(&self) {
        self.errors_count.fetch_add(1, Ordering::Relaxed);
        warn!("Error recorded");
    }

    // Cleanup completed tasks with tracing
    #[instrument(level = "info", skip(self))]
    pub fn cleanup_completed_tasks(&self) {
        let completed: Vec<u64> = self
            .tasks
            .iter()
            .filter(|e| e.value().is_completed())
            .map(|e| *e.key())
            .collect();
        for id in &completed {
            self.remove_task(*id);
        }
        info!(removed = completed.len(), "Completed tasks cleaned up");
    }

    // Metrics retrieval with tracing
    #[instrument(level = "debug", skip(self))]
    pub fn get_metrics(&self) -> AppMetrics {
        let metrics = AppMetrics {
            session_id: self.session_id.clone(),
            uptime: self.started_at.elapsed(),
            operations_count: self.operations_count.load(Ordering::Relaxed),
            errors_count: self.errors_count.load(Ordering::Relaxed),
            active_tasks: self.tasks.len(),
            marked_files: self.marked_files.len(),
            history_size: self.operation_history.len(),
        };
        debug!(?metrics, "Metrics retrieved");
        metrics
    }
}

impl Default for AppState {
    #[instrument(level = "debug")]
    fn default() -> Self {
        Self::new(
            Arc::new(Config::default()),
            Arc::new(ObjectInfoCache::new()),
        )
    }
}

/// Application metrics snapshot
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
