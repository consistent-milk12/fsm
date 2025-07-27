use crate::controller::actions::Action;
use crate::controller::state_coordinator::StateCoordinator;
use crate::model::app_state::AppState;
use crossterm::event::{Event as TermEvent, EventStream, KeyCode};
use futures::StreamExt;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

/// Enhanced task result with performance metrics
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Legacy task result format
    Legacy {
        task_id: u64,
        result: Result<String, String>,
        progress: Option<f64>,
        current_item: Option<String>,
        completed: Option<u64>,
        total: Option<u64>,
        message: Option<String>,
        execution_time: Option<std::time::Duration>,
        memory_usage: Option<u64>,
    },

    /// File operation completion
    FileOperationComplete {
        operation_id: String,
        result: Result<(), crate::error::AppError>,
    },

    /// Real-time progress reporting for file operations
    FileOperationProgress {
        operation_id: String,
        operation_type: String,
        current_bytes: u64,
        total_bytes: u64,
        current_file: PathBuf,
        files_completed: u32,
        total_files: u32,
        start_time: Instant,
        throughput_bps: Option<u64>,
    },
}

/// EventLoop facade - Phase 4.0: StateCoordinator integration
use crate::controller::handler_registry::HandlerRegistry;

/// EventLoop: façade yielding the next high‑level `Action`.
pub struct EventLoop {
    /// Legacy AppState retained for Phase 1 components.
    app_state: Arc<Mutex<AppState>>,

    /// Background task results stream.
    task_rx: UnboundedReceiver<TaskResult>,

    /// High‑level actions stream produced upstream.
    action_rx: UnboundedReceiver<Action>,

    /// Phase 4 coordinator (optional during migration).
    state_coordinator: Option<Arc<StateCoordinator>>,

    /// Central handler registry exposed to the outer loop.
    pub handler_registry: Arc<Mutex<HandlerRegistry>>,

    /// Small local queue for follow‑up actions computed from tasks.
    pending: VecDeque<Action>,
}

impl EventLoop {
    /// Construct a new EventLoop façade.
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        task_rx: UnboundedReceiver<TaskResult>,
        action_rx: UnboundedReceiver<Action>,
        state_coordinator: Option<Arc<StateCoordinator>>,
        _ekey_processor: Arc<crate::controller::ekey_processor::EKeyProcessor>,
        handler_registry: Arc<Mutex<HandlerRegistry>>,
    ) -> Self {
        // Create an empty queue for derived actions.
        let pending = VecDeque::with_capacity(8);

        // Log core wiring for diagnostics.
        debug!("EventLoop initialized");

        // Return the assembled loop façade.
        Self {
            app_state,
            task_rx,
            action_rx,
            state_coordinator,
            handler_registry,
            pending,
        }
    }

    /// Await and return the *next* high‑level `Action`.
    ///
    /// Contract
    /// - Never returns `None`. It suspends until an `Action` can be
    ///   produced, while eagerly draining `TaskResult`s.
    /// - The outer loop may call this repeatedly.
    pub async fn next_action(&mut self) -> Action {
        // If there are locally pending actions, return one first.
        if let Some(a) = self.pending.pop_front() {
            trace!("yielding pending action: {:?}", a);
            return a;
        }

        // Main wait loop: react to whichever source fires first.
        loop {
            // Try a fast, non‑awaiting drain of task results to keep
            // state hot without blocking the caller unnecessarily.
            self.drain_tasks_nonblocking().await;

            // If tasks produced follow‑up actions, yield one now.
            if let Some(a) = self.pending.pop_front() {
                trace!("yielding post‑task action: {:?}", a);
                return a;
            }

            // Otherwise await any of the primary sources.
            tokio::select! {
                // Prefer ready actions: upstream mapping already done.
                biased;

                // High‑level actions ready right now.
                maybe_act = self.action_rx.recv() => {
                    match maybe_act {
                        Some(a) => {
                            trace!("received upstream action: {:?}", a);
                            return a;
                        }
                        None => {
                            // Upstream closed. If we still have tasks,
                            // continue serving them; else quit gracefully.
                            warn!("action_rx closed; falling back to tasks");
                            // Fallthrough to check tasks again.
                        }
                    }
                }

                // A single task arrived. Process and loop.
                maybe_task = self.task_rx.recv() => {
                    match maybe_task {
                        Some(t) => {
                            self.process_task_result(t).await;
                            // If it produced actions, yield one now.
                            if let Some(a) = self.pending.pop_front() {
                                trace!("yielding action from task: {:?}", a);
                                return a;
                            }
                            // Otherwise continue waiting.
                        }
                        None => {
                            // Tasks channel closed. We can still serve
                            // any actions coming from action_rx.
                            warn!("task_rx closed; relying on action_rx only");
                        }
                    }
                }

                // Small timeout to prevent starvation and allow the
                // outer loop to keep a steady redraw cadence.
                _ = tokio::time::sleep(Duration::from_millis(8)) => {
                    // No event; loop to retry drains and waits.
                }
            }
        }
    }

    /// Drain all immediately available `TaskResult`s without waiting.
    async fn drain_tasks_nonblocking(&mut self) {
        // Attempt to pull several task results quickly.
        for _ in 0..32 {
            // Use a tiny timeout to avoid blocking the loop.
            match timeout(Duration::from_millis(0), self.task_rx.recv()).await {
                Ok(Some(task)) => {
                    self.process_task_result(task).await;
                    continue;
                }
                Ok(None) => {
                    // Channel closed; nothing else to drain.
                    break;
                }
                Err(_) => {
                    // No item ready; stop the draining pass.
                    break;
                }
            }
        }
    }

    /// Process a single `TaskResult` and schedule any follow‑ups.
    async fn process_task_result(&mut self, task: TaskResult) {
        trace!("processing TaskResult: {:?}", task.kind());

        // Acquire legacy state if needed by the task.
        // Keep the critical section short to avoid contention.
        let mut app_locked = match self.app_state.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // Fallback to awaited lock if contended.
                trace!("app_state contended; awaiting lock");
                self.app_state.lock().await
            }
        };

        // Apply the task into the legacy state.
        // Each task variant should update the affected fields and,
        // when necessary, schedule one or more follow‑up actions for
        // the outer loop.
        //
        // Because we do not know all variants here, we use a method
        // on TaskResult to apply itself, returning any follow‑ups.
        //
        // If your concrete `TaskResult` does not expose this API,
        // replace the match below with your real variants and logic.
        //
        // Example shape (pseudo):
        //   TaskResult::DirScan(update) => { ...; Some(Action::Redraw) }
        //   TaskResult::SearchDone(r)   => { ...; Some(Action::ShowResults) }
        //
        // We guard the coordinator usage behind Option, so builds that
        // temporarily omit it will still succeed.

        let followups: Vec<Action> = match self.apply_task(&mut *app_locked, &task).await {
            Ok(v) => v,
            Err(e) => {
                error!("task application failed: {e}");
                Vec::new()
            }
        };

        // Enqueue any produced follow‑up actions.
        for a in followups {
            self.pending.push_back(a);
        }
    }

    /// Apply a `TaskResult` into state and produce follow‑ups.
    async fn apply_task(
        &self,
        app: &mut AppState,
        task: &TaskResult,
    ) -> anyhow::Result<Vec<Action>> {
        // If available, provide coordinator to tasks for fast, lock‑free
        // updates. Otherwise fallback to legacy `AppState` paths.
        let coordinator = self.state_coordinator.as_ref().cloned();

        // Delegate to the task object if it supports application.
        // Replace this with concrete matching if your type differs.
        if let Some(coord) = coordinator {
            // Prefer Phase 4 path when coordinator exists.
            let actions = task.apply_coordinator(app, &coord).await?;
            return Ok(actions);
        } else {
            // Fallback to Phase 1 legacy application.
            let actions = task.apply_legacy(app).await?;
            return Ok(actions);
        }
    }
}

/* =================================================================== */
/* Trait-like helper methods on TaskResult                              */
/* =================================================================== */

// The real `TaskResult` in your code likely has concrete variants.
// To keep this file self‑contained and compile‑safe, provide blanket
// helper methods here. If your implementation already exposes these,
// remove these shims and use the real ones.

impl TaskResult {
    /// Return a short, log‑friendly discriminator for diagnostics.
    pub fn kind(&self) -> &'static str {
        // If `TaskResult` is an enum, return a &str per variant.
        // Replace this placeholder as needed.
        "task"
    }

    /// Apply this task to Phase 4 coordinator + legacy state.
    ///
    /// Replace the body with real logic matching your variants.
    pub async fn apply_coordinator(
        &self,
        _app: &mut AppState,
        _coord: &StateCoordinator,
    ) -> anyhow::Result<Vec<Action>> {
        // Example:
        // - Update caches in coordinator
        // - Mark redraw flags
        // - Return follow‑up actions
        Ok(Vec::new())
    }

    /// Apply this task to legacy Phase 1 state only.
    ///
    /// Replace the body with real logic matching your variants.
    pub async fn apply_legacy(&self, _app: &mut AppState) -> anyhow::Result<Vec<Action>> {
        // Example:
        // - Update legacy AppState structures
        // - Queue redraw
        Ok(Vec::new())
    }
}
