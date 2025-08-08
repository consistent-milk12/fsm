//! Async task performance profiler
//! Tracks task spawn/completion times, operation durations, and async patterns

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tracing::{Span, debug, instrument};

use super::collector::ProfileCollector;

/// Async operation profiler
pub struct AsyncProfiler {
    collector: Arc<ProfileCollector>,
    active_tasks: Arc<std::sync::Mutex<HashMap<String, TaskMetrics>>>,
}

/// Metrics for individual async tasks
#[derive(Debug, Clone)]
pub struct TaskMetrics {
    pub task_id: String,
    pub operation_type: String,
    pub started_at: Instant,
    pub spawn_location: Option<String>,
}

/// Builder for creating profiled async tasks
pub struct TaskBuilder {
    profiler: Arc<ProfileCollector>,
    operation_type: String,
    task_id: Option<String>,
    metadata: Option<String>,
}

impl AsyncProfiler {
    /// Create a new async profiler with collector
    pub fn new(collector: Arc<ProfileCollector>) -> Self {
        Self {
            collector,
            active_tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Create a task builder for profiled async operations
    pub fn task(&self, operation_type: &str) -> TaskBuilder {
        TaskBuilder {
            profiler: self.collector.clone(),
            operation_type: operation_type.to_string(),
            task_id: None,
            metadata: None,
        }
    }

    // /// Record task spawn
    // fn record_task_spawn(&self, task_id: &str, operation_type: &str) {
    //     let metrics = TaskMetrics {
    //         task_id: task_id.to_string(),
    //         operation_type: operation_type.to_string(),
    //         started_at: Instant::now(),
    //         spawn_location: Some(format!("{}:{}", file!(), line!())),
    //     };

    //     if let Ok(mut active_tasks) = self.active_tasks.lock() {
    //         active_tasks.insert(task_id.to_string(), metrics);
    //     }

    //     self.collector
    //         .record_async_operation(operation_type, 0, false);
    //     debug!("Async task spawned: {} ({})", task_id, operation_type);
    // }

    // /// Record task completion
    // fn record_task_completion(&self, task_id: &str) {
    //     if let Ok(mut active_tasks) = self.active_tasks.lock() {
    //         if let Some(metrics) = active_tasks.remove(task_id) {
    //             let duration = metrics.started_at.elapsed();
    //             let duration_ms = duration.as_millis() as u64;

    //             self.collector
    //                 .record_async_operation(&metrics.operation_type, duration_ms, true);

    //             if duration > Duration::from_millis(100) {
    //                 debug!(
    //                     "Long-running task completed: {} took {}ms",
    //                     task_id, duration_ms
    //                 );
    //             }
    //         }
    //     }
    // }

    /// Get active task count
    pub fn active_task_count(&self) -> usize {
        self.active_tasks
            .lock()
            .map(|tasks| tasks.len())
            .unwrap_or(0)
    }

    /// Get active tasks summary
    pub fn get_active_tasks(&self) -> Vec<TaskMetrics> {
        self.active_tasks
            .lock()
            .map(|tasks| tasks.values().cloned().collect())
            .unwrap_or_default()
    }
}

impl TaskBuilder {
    /// Set custom task ID
    pub fn with_id(mut self, task_id: String) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Set task metadata
    pub fn with_metadata(mut self, metadata: String) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Spawn a profiled async task
    #[instrument(skip(self, future), fields(operation_type = %self.operation_type, task_id = %self.task_id.as_deref().unwrap_or("unknown")))]
    pub fn spawn<F, T>(self, future: F) -> JoinHandle<T>
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let task_id = self
            .task_id
            .unwrap_or_else(|| format!("{}_{}", self.operation_type, nanoid::nanoid!(8)));

        let profiler = self.profiler.clone();
        let operation_type = self.operation_type.clone();
        let _task_id_clone = task_id.clone();

        // Record task spawn
        profiler.record_async_operation(&operation_type, 0, false);

        tokio::spawn(async move {
            let start = Instant::now();
            let result = future.await;
            let duration_ms = start.elapsed().as_millis() as u64;

            // Record completion
            profiler.record_async_operation(&operation_type, duration_ms, true);

            result
        })
    }

    /// Spawn a profiled async task with custom instrumentation
    pub fn spawn_with_span<F, T>(self, future: F, span: Span) -> JoinHandle<T>
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let task_id = self
            .task_id
            .unwrap_or_else(|| format!("{}_{}", self.operation_type, nanoid::nanoid!(8)));

        let profiler = self.profiler.clone();
        let operation_type = self.operation_type.clone();
        let _task_id_clone = task_id.clone();

        profiler.record_async_operation(&operation_type, 0, false);

        tokio::spawn(async move {
            let _entered = span.enter();
            let start = Instant::now();
            let result = future.await;
            let duration_ms = start.elapsed().as_millis() as u64;
            profiler.record_async_operation(&operation_type, duration_ms, true);
            result
        })
    }
}

/// Profiled async operations utilities
pub struct AsyncOps;

impl AsyncOps {
    /// Time an async operation and record the duration
    #[instrument(skip(future, collector), fields(operation = %operation_name))]
    pub async fn timed_operation<F, T>(
        operation_name: &str,
        future: F,
        collector: &ProfileCollector,
    ) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let start = Instant::now();
        let result = future.await;
        let duration = start.elapsed();

        collector.record_event(
            operation_name,
            duration.as_millis() as u64,
            Some("async_operation"),
        );

        result
    }

    /// Profile a series of concurrent operations
    #[instrument(skip(futures, collector))]
    pub async fn profile_concurrent<F, T>(
        operation_name: &str,
        futures: Vec<F>,
        collector: &ProfileCollector,
    ) -> Vec<T>
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let start = Instant::now();
        let future_count = futures.len();

        let handles: Vec<_> = futures
            .into_iter()
            .enumerate()
            .map(|(i, future)| {
                let _task_name = format!("{}_{}", operation_name, i);
                tokio::spawn(async move {
                    let task_start = Instant::now();
                    let result = future.await;
                    (result, task_start.elapsed())
                })
            })
            .collect();

        let mut results = Vec::new();
        let mut total_individual_time = Duration::from_nanos(0);

        for handle in handles {
            let (result, individual_time) = handle.await.expect("Task should not panic");
            results.push(result);
            total_individual_time += individual_time;
        }

        let total_elapsed = start.elapsed();
        let parallelism_efficiency = if total_elapsed.as_nanos() > 0 {
            total_individual_time.as_nanos() as f64
                / (total_elapsed.as_nanos() as f64 * future_count as f64)
        } else {
            0.0
        };

        collector.record_event(
            &format!("{}_concurrent", operation_name),
            total_elapsed.as_millis() as u64,
            Some(&format!(
                "tasks:{},parallelism_efficiency:{:.2}",
                future_count, parallelism_efficiency
            )),
        );

        debug!(
            "Concurrent operation {} completed: {} tasks in {}ms (efficiency: {:.2})",
            operation_name,
            future_count,
            total_elapsed.as_millis(),
            parallelism_efficiency
        );

        results
    }

    /// Profile async operations with backpressure monitoring
    #[instrument(skip(operation, collector))]
    pub async fn with_backpressure_monitoring<F, T>(
        operation_name: &str,
        operation: F,
        max_concurrent: usize,
        collector: &ProfileCollector,
    ) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let permit_start = Instant::now();

        let _permit = semaphore
            .acquire()
            .await
            .expect("Semaphore should not be closed");
        let permit_wait = permit_start.elapsed();

        if permit_wait > Duration::from_millis(10) {
            debug!(
                "Backpressure detected for {}: waited {}ms for permit",
                operation_name,
                permit_wait.as_millis()
            );

            collector.record_event(
                &format!("{}_backpressure", operation_name),
                permit_wait.as_millis() as u64,
                Some("backpressure_wait"),
            );
        }

        let op_start = Instant::now();
        let result = operation.await;
        let op_duration = op_start.elapsed();

        collector.record_event(
            operation_name,
            op_duration.as_millis() as u64,
            Some(&format!(
                "with_backpressure,max_concurrent:{}",
                max_concurrent
            )),
        );

        result
    }
}

/// Macro for easy async operation profiling
#[macro_export]
macro_rules! profile_async {
    ($collector:expr, $operation:expr, $future:expr) => {{
        let start = std::time::Instant::now();
        let result = $future.await;
        let duration = start.elapsed();

        $collector.record_event(
            $operation,
            duration.as_millis() as u64,
            Some("profile_async_macro"),
        );

        result
    }};
}
