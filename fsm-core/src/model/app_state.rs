// fsm-core/src/model/app_state.rs
// Streamlined AppState focused on business logic and task management

use clipr::{ClipBoard, ClipBoardConfig};
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
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::controller::actions::ClipboardStats as CliprStats;
use crate::{
    cache::cache_manager::ObjectInfoCache,
    config::Config,
    controller::actions::{OperationId, ProcessData, SystemData},
};

/// clipr operation state tracking for enhanced performance monitoring
#[derive(Debug, Clone)]
pub struct ClipboardOperationState {
    pub operation_id: OperationId,
    pub operation_type: CompactString, // "batch_add", "batch_paste", "search", etc.
    pub started_at: Instant,
    pub items_count: usize,
    pub progress: f32, // 0.0 to 1.0
    pub completed: bool,
    pub cancelled: bool,
    pub error: Option<CompactString>,
}

impl ClipboardOperationState {
    pub fn new(
        operation_id: OperationId,
        operation_type: impl Into<CompactString>,
        items_count: usize,
    ) -> Self {
        Self {
            operation_id,
            operation_type: operation_type.into(),
            started_at: Instant::now(),
            items_count,
            progress: 0.0,
            completed: false,
            cancelled: false,
            error: None,
        }
    }

    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    pub fn complete(&mut self) {
        self.progress = 1.0;
        self.completed = true;
    }

    pub fn fail(&mut self, error: impl Into<CompactString>) {
        self.error = Some(error.into());
        self.completed = true;
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.completed = true;
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

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
    #[instrument(level = "debug", skip(self), fields(operation_id = 
  ?operation_id))]
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
    // clipr-specific operations for enhanced tracking
    ClipboardBatchOp {
        operation_type: CompactString, // "batch_add", "batch_paste", "search"
        operation_id: OperationId,
        items_count: u32,
        duration_ms: u64,
        success: bool,
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
        if let Some(pos) = self.recent_queries.iter().position(|q| q == query) {
            self.recent_queries.remove(pos);
        }
        self.recent_queries.push_front(query.clone());
        if self.recent_queries.len() > 32 {
            self.recent_queries.pop_back();
        }
        *self.query_counts.entry(query.clone()).or_insert(0) += 1;
        info!(
            count = self.query_counts[&query],
            "Search query 
  recorded"
        );
    }
}

/// Main application state with clipr integration
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

    pub system_data: Option<SystemData>,
    pub process_data: Vec<ProcessData>,

    // ===== clipr Integration =====
    /// High-performance clipr clipboard with lock-free operations
    pub clipboard: Arc<ClipBoard>,

    /// Active clipboard operations for progress tracking and cancellation
    pub clipboard_operations: DashMap<OperationId, ClipboardOperationState>,

    /// Real-time clipboard performance metrics with atomic updates
    pub clipboard_metrics: Arc<RwLock<CliprStats>>,

    /// Clipboard operation statistics for performance monitoring
    pub clipboard_stats: Arc<RwLock<CliprStats>>,
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

        // Initialize clipr with optimized configuration for FSM workloads
        let clipboard_config = ClipBoardConfig::default();

        let clipboard = Arc::new(ClipBoard::new(clipboard_config));

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
            system_data: None,
            process_data: Vec::new(),

            // clipr integration
            clipboard,
            clipboard_operations: DashMap::with_capacity(128),
            clipboard_metrics: Arc::new(RwLock::new(CliprStats::default())),
            clipboard_stats: Arc::new(RwLock::new(CliprStats::default())),
        };

        info!(session = %session_id, "AppState with clipr integration 
  initialized");
        state
    }

    // ===== clipr Operation Management =====

    /// Track a new clipboard operation with performance monitoring
    pub fn start_clipboard_operation(
        &self,
        operation_id: OperationId,
        operation_type: impl Into<CompactString>,
        items_count: usize,
    ) {
        let op_state =
            ClipboardOperationState::new(operation_id.clone(), operation_type, items_count);

        self.clipboard_operations
            .insert(operation_id.clone(), op_state);

        info!(
            marker = "CLIPBOARD_OPERATION_START",
            operation_type = "clipboard_operation_tracking",
            operation_id = %operation_id,
            items_count = items_count,
            "Clipboard operation started"
        );
    }

    /// Update clipboard operation progress with atomic safety
    #[instrument(level = "debug", skip(self))]
    pub fn update_clipboard_operation_progress(&self, operation_id: &OperationId, progress: f32) {
        if let Some(mut op) = self.clipboard_operations.get_mut(operation_id) {
            op.set_progress(progress);

            debug!(
                marker = "CLIPBOARD_OPERATION_PROGRESS",
                operation_type = "clipboard_operation_tracking",
                operation_id = %operation_id,
                progress = progress,
                "Clipboard operation progress updated"
            );
        }
    }

    /// Complete clipboard operation with statistics recording
    #[instrument(level = "info", skip(self))]
    pub fn complete_clipboard_operation(&mut self, operation_id: &OperationId) {
        if let Some(mut op) = self.clipboard_operations.get_mut(operation_id) {
            op.complete();

            // Record in operation history for advanced analytics
            let history_event = HistoryEvent::ClipboardBatchOp {
                operation_type: op.operation_type.clone(),
                operation_id: operation_id.clone(),
                items_count: op.items_count as u32,
                duration_ms: op.elapsed().as_millis() as u64,
                success: true,
            };

            // Safe history update with bounds checking
            if let Ok(mut history) = Mutex::new(&mut self.operation_history).lock() {
                if history.len() >= 64 {
                    history.remove(0);
                }
                history.push(history_event);
            }

            info!(
                marker = "CLIPBOARD_OPERATION_COMPLETE",
                operation_type = "clipboard_operation_tracking",
                operation_id = %operation_id,
                duration_ms = op.elapsed().as_millis(),
                items_count = op.items_count,
                "Clipboard operation completed successfully"
            );
        }
    }

    /// Handle clipboard operation failure with comprehensive error tracking
    #[instrument(level = "warn", skip(self, error))]
    pub fn fail_clipboard_operation(
        &self,
        operation_id: &OperationId,
        error: impl Into<CompactString>,
    ) {
        let error_msg = error.into();

        if let Some(mut op) = self.clipboard_operations.get_mut(operation_id) {
            op.fail(error_msg.clone());

            // Record failed operation in history
            let history_event = HistoryEvent::ClipboardBatchOp {
                operation_type: op.operation_type.clone(),
                operation_id: operation_id.clone(),
                items_count: op.items_count as u32,
                duration_ms: op.elapsed().as_millis() as u64,
                success: false,
            };

            if let Ok(mut history) = std::sync::Mutex::new(self.operation_history.clone()).lock() {
                if history.len() >= 64 {
                    history.remove(0);
                }
                history.push(history_event);
            }

            warn!(
                marker = "CLIPBOARD_OPERATION_FAILED",
                operation_type = "clipboard_operation_tracking",
                operation_id = %operation_id,
                error = %error_msg,
                duration_ms = op.elapsed().as_millis(),
                "Clipboard operation failed"
            );
        }
    }

    /// Get real-time clipboard statistics for UI display
    #[instrument(level = "debug", skip(self))]
    pub fn get_clipboard_stats(&self) -> Option<CliprStats> {
        self.clipboard_stats.read().ok().map(|stats| stats.clone())
    }

    /// Update clipboard metrics from clipr for performance monitoring
    #[instrument(level = "debug", skip(self, metrics))]
    pub fn update_clipboard_metrics(&self, metrics: CliprStats) {
        if let Ok(mut current_metrics) = self.clipboard_metrics.write() {
            *current_metrics = metrics;

            // Update aggregate statistics
            if let Ok(mut stats) = self.clipboard_stats.write() {
                stats.total_items = current_metrics.total_items;
                stats.copy_items = current_metrics.copy_items;
                stats.move_items = current_metrics.move_items;
                stats.total_size_bytes = current_metrics.total_size_bytes;
                stats.cache_hit_ratio = current_metrics.cache_hit_ratio;
                stats.last_updated = std::time::SystemTime::now();
            }

            debug!(
                marker = "CLIPBOARD_METRICS_UPDATED",
                operation_type = "clipboard_metrics",
                total_items = current_metrics.total_items,
                cache_hit_ratio = current_metrics.cache_hit_ratio,
                "Clipboard metrics updated from clipr"
            );
        }
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
        debug!(action = %name_str, ns = duration.as_nanos(), "Action 
  recorded");
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
        info!(
            removed = completed.len(),
            "Completed tasks cleaned 
  up"
        );
    }

    // Metrics retrieval with tracing
    #[instrument(level = "debug", skip(self))]
    pub fn get_metrics(&self) -> AppMetrics {
        let clipboard_stats = self.get_clipboard_stats().unwrap_or_default();

        let metrics = AppMetrics {
            session_id: self.session_id.clone(),
            uptime: self.started_at.elapsed(),
            operations_count: self.operations_count.load(Ordering::Relaxed),
            errors_count: self.errors_count.load(Ordering::Relaxed),
            active_tasks: self.tasks.len(),
            marked_files: self.marked_files.len(),
            history_size: self.operation_history.len(),
            clipboard_items: clipboard_stats.total_items,
            clipboard_cache_hit_ratio: clipboard_stats.cache_hit_ratio,
        };
        debug!(
            marker = "METRICS_RETRIEVED",
            operation_type = "metrics",
            ?metrics,
            "Metrics retrieved"
        );
        metrics
    }
}

impl Default for AppState {
    #[instrument(level = "debug")]
    fn default() -> Self {
        debug!(
            marker = "APP_STATE_DEFAULT_CREATED",
            operation_type = "application",
            "Creating default AppState."
        );
        Self::new(
            Arc::new(Config::default()),
            Arc::new(ObjectInfoCache::new()),
        )
    }
}

/// Application metrics snapshot with clipr integration
#[derive(Debug, Clone)]
pub struct AppMetrics {
    pub session_id: CompactString,
    pub uptime: Duration,
    pub operations_count: u64,
    pub errors_count: u64,
    pub active_tasks: usize,
    pub marked_files: usize,
    pub history_size: usize,
    // clipr metrics
    pub clipboard_items: u64,
    pub clipboard_cache_hit_ratio: f32,
}
