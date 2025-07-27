use crate::controller::actions::Action;
use crate::controller::state_coordinator::StateCoordinator;
use crate::model::app_state::AppState;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::select;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{sleep, timeout};
use tracing::{trace, warn};

/// Enhanced task result with performance metrics.
///
/// In the modern architecture tasks may carry rich metadata about their
/// execution.  Variants mirror the legacy implementation but can be
/// extended to include timing and memory usage metrics.  When adding
/// new variants ensure they implement the helper methods defined
/// at the end of this file.
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Legacy task result format.
    Legacy {
        task_id: u64,
        result: Result<(), crate::error::AppError>,
        progress: Option<u64>,
        current_item: Option<PathBuf>,
        completed: Option<u64>,
        total: Option<u64>,
        message: Option<String>,
        execution_time: Option<Duration>,
        memory_usage: Option<usize>,
    },

    /// File operation completion.
    FileOperationComplete {
        operation_id: String,
        result: Result<(), crate::error::AppError>,
    },

    /// Real‑time progress reporting for file operations.
    FileOperationProgress {
        operation_id: String,
        operation_type: String,
        current_bytes: u64,
        total_bytes: u64,
        current_file: PathBuf,
        files_completed: u32,
        total_files: u32,
        start_time: Instant,
        throughput_bps: Option<f64>,
    },
}

/// EventLoop façade yielding high‑level `Action`s.
///
/// This event loop bridges asynchronous task results and upstream actions
/// into a unified stream of `Action`s for the controller.  It can be
/// integrated with the modern [`StateCoordinator`] to apply task
/// results via lock‑free mechanisms, while still supporting legacy
/// updates through the contained [`AppState`].
pub struct EventLoop {
    /// Legacy application state retained for backwards compatibility.
    app_state: Arc<Mutex<AppState>>,

    /// Background task results stream.
    task_rx: UnboundedReceiver<TaskResult>,

    /// High‑level actions produced upstream (e.g. key mappings).
    action_rx: UnboundedReceiver<Action>,

    /// Shared state coordinator for Phase 4 integration.
    state_coordinator: Arc<StateCoordinator>,

    /// Small local queue for follow‑up actions computed from tasks.
    pending: VecDeque<Action>,
}

impl EventLoop {
    /// Construct a new event loop façade.
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        task_rx: UnboundedReceiver<TaskResult>,
        action_rx: UnboundedReceiver<Action>,
        state_coordinator: Arc<StateCoordinator>,
    ) -> Self {
        Self {
            app_state,
            task_rx,
            action_rx,
            state_coordinator,
            pending: VecDeque::with_capacity(16),
        }
    }

    /// Await and return the next high‑level `Action`.
    ///
    /// The method never returns `None`.  It suspends until an action can
    /// be produced.  Task results are drained non‑blocking and any
    /// generated follow‑up actions are queued locally.  A small timeout
    /// ensures the caller can maintain a steady UI redraw cadence.
    pub async fn next_action(&mut self) -> Action {
        // If there are locally queued actions, return one immediately.
        if let Some(a) = self.pending.pop_front() {
            trace!("Yielding pending action: {:?}", a);
            return a;
        }

        loop {
            // Drain available task results without awaiting.
            self.drain_tasks_nonblocking().await;

            // If draining produced actions, yield the first.
            if let Some(a) = self.pending.pop_front() {
                trace!("Yielding action from tasks: {:?}", a);
                return a;
            }

            // Await whichever source fires first: upstream action or task.
            select! {
                biased;

                maybe_act = self.action_rx.recv() => {
                    match maybe_act {
                        Some(a) => {
                            trace!("Received upstream action: {:?}", a);
                            return a;
                        },
                        None => {
                            // action channel closed; continue to serve tasks
                            warn!("action_rx closed; relying on task results");
                        },
                    }
                },

                maybe_task = self.task_rx.recv() => {
                    match maybe_task {
                        Some(task) => {
                            self.process_task_result(task).await;
                            if let Some(a) = self.pending.pop_front() {
                                trace!("Yielding action derived from task: {:?}", a);
                                return a;
                            }
                        },
                        None => {
                            // task channel closed; fall back to upstream actions
                            warn!("task_rx closed; no more background tasks");
                        },
                    }
                },

                // Prevent starvation by sleeping briefly; allows UI to refresh.
                _ = sleep(Duration::from_millis(8)) => {
                    // Continue the loop; tasks may arrive shortly.
                },
            }
        }
    }

    /// Drain available `TaskResult`s without blocking.
    async fn drain_tasks_nonblocking(&mut self) {
        for _ in 0..32 {
            match timeout(Duration::from_millis(0), self.task_rx.recv()).await {
                Ok(Some(task)) => {
                    self.process_task_result(task).await;
                    // Continue draining up to 32 tasks
                }
                Ok(None) => {
                    // Channel closed; stop draining
                    break;
                }
                Err(_) => {
                    // No more tasks ready right now
                    break;
                }
            }
        }
    }

    /// Process a single `TaskResult` and enqueue follow‑up actions.
    async fn process_task_result(&mut self, task: TaskResult) {
        trace!("Processing TaskResult");

        // Acquire legacy AppState.  Try non‑blocking first to reduce contention.
        let mut app_locked = match self.app_state.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // Fallback to awaiting the lock if contended.
                trace!("app_state contended; awaiting lock");
                self.app_state.lock().expect("AppState mutex poisoned")
            }
        };

        // Apply the task; collect any produced actions.
        let followups: Vec<Action> = match self.apply_task(&mut app_locked, &task).await {
            Ok(v) => v,
            Err(e) => {
                warn!("task application failed: {}", e);
                Vec::new()
            }
        };

        // Enqueue follow‑up actions locally.
        for a in followups {
            self.pending.push_back(a);
        }
    }

    /// Apply a `TaskResult` into state and produce follow‑up actions.
    async fn apply_task(
        &self,
        app: &mut AppState,
        task: &TaskResult,
    ) -> anyhow::Result<Vec<Action>> {
        // Provide coordinator to tasks for fast, lock‑free updates.
        let coordinator = self.state_coordinator.clone();

        // Delegate to the task’s apply method if available.
        match task {
            TaskResult::Legacy { .. }
            | TaskResult::FileOperationComplete { .. }
            | TaskResult::FileOperationProgress { .. } => {
                task.apply_coordinator(app, &coordinator).await
            }
        }
    }
}

/* ===================================================================== */
/* Helper methods on TaskResult                                          */
/* ===================================================================== */

impl TaskResult {
    /// Return a short discriminator for logging purposes.
    pub fn kind(&self) -> &'static str {
        match self {
            TaskResult::Legacy { .. } => "legacy",
            TaskResult::FileOperationComplete { .. } => "file_op_complete",
            TaskResult::FileOperationProgress { .. } => "file_op_progress",
        }
    }

    /// Apply this task to the modern coordinator and legacy state.
    ///
    /// Replace this placeholder implementation with real logic matching
    /// your application’s task semantics.  The default implementation
    /// performs no updates and returns no actions.
    pub async fn apply_coordinator(
        &self,
        _app: &mut AppState,
        _coord: &StateCoordinator,
    ) -> anyhow::Result<Vec<Action>> {
        // TODO: integrate concrete task updates here
        Ok(Vec::new())
    }

    /// Apply this task to the legacy state only.
    ///
    /// Replace this placeholder implementation with real logic matching
    /// your application’s task semantics.  The default implementation
    /// performs no updates and returns no actions.
    pub async fn apply_legacy(&self, _app: &mut AppState) -> anyhow::Result<Vec<Action>> {
        // TODO: implement legacy task handling if coordinator is unavailable
        Ok(Vec::new())
    }
}
