use crate::UIState;
use crate::controller::actions::Action;
use crate::controller::state_coordinator::StateCoordinator;
use crate::model::RedrawFlag;
use crate::model::app_state::AppState;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::warn;

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

    /// Shared state coordinator for Phase 4 integration.
    state_coordinator: Arc<StateCoordinator>,

    /// Small local queue for follow‑up actions computed from tasks.
    pending: VecDeque<Action>,
}

impl EventLoop {
    /// Construct a new event loop
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

    /// Get next action (simple implementation)
    pub async fn next_action(&mut self) -> Action {
        // For now, return a simple tick action
        // TODO: Implement proper action processing from channels
        tokio::time::sleep(Duration::from_millis(16)).await;
        Action::Tick
    }

    /// Extract data needed for async task processing without holding locks
    fn prepare_task_data(&self, _app: &mut AppState, task: &TaskResult) -> TaskData {
        match task {
            TaskResult::Legacy { task_id, .. } => {
                TaskData::Legacy {
                    task_id: *task_id,
                    // Extract any other needed data from app state
                }
            }
            TaskResult::FileOperationComplete { operation_id, .. } => {
                TaskData::FileOpComplete {
                    operation_id: operation_id.clone(),
                    // Extract operation context if needed
                }
            }
            TaskResult::FileOperationProgress { operation_id, .. } => {
                TaskData::FileOpProgress {
                    operation_id: operation_id.clone(),
                    // Extract progress tracking data if needed
                }
            }
        }
    }

    /// Process task with extracted data (no locks held)
    async fn apply_task(
        &self,
        task_data: TaskData,
        task: &TaskResult,
    ) -> anyhow::Result<Vec<Action>> {
        let coordinator = self.state_coordinator.clone();

        match (task_data, task) {
            (TaskData::Legacy { task_id }, TaskResult::Legacy { result, .. }) => {
                self.handle_legacy_task(task_id, result, &coordinator).await
            }
            (
                TaskData::FileOpComplete { operation_id },
                TaskResult::FileOperationComplete { result, .. },
            ) => {
                self.handle_file_op_complete(operation_id, result, &coordinator)
                    .await
            }
            (
                TaskData::FileOpProgress { operation_id },
                TaskResult::FileOperationProgress {
                    current_bytes,
                    total_bytes,
                    current_file,
                    ..
                },
            ) => {
                self.handle_file_op_progress(
                    operation_id,
                    *current_bytes,
                    *total_bytes,
                    current_file,
                    &coordinator,
                )
                .await
            }
            _ => {
                warn!("Task data mismatch");
                Ok(vec![])
            }
        }
    }

    /// Apply results back to state after async processing
    fn finalize_task_results(&self, app: &mut AppState, actions: &[Action]) {
        // Update any state that needs to be persisted
        // This is typically minimal since most updates go through StateCoordinator
        for action in actions {
            match action {
                Action::UpdateTaskStatus { task_id, completed } => {
                    if let Some(_task_info) = app.get_task(*task_id) {
                        if *completed {
                            app.complete_task(*task_id, None);
                        }
                    }
                }
                // Handle other actions that need legacy state updates
                _ => {}
            }
        }
    }

    // Individual task handlers (async operations without locks)
    async fn handle_legacy_task(
        &self,
        task_id: u64,
        result: &Result<(), crate::error::AppError>,
        coordinator: &StateCoordinator,
    ) -> anyhow::Result<Vec<Action>> {
        // Perform any async operations needed
        match result {
            Ok(()) => {
                // Task completed successfully
                coordinator.request_redraw(crate::model::ui_state::RedrawFlag::StatusBar);
                Ok(vec![Action::UpdateTaskStatus {
                    task_id,
                    completed: true,
                }])
            }
            Err(e) => {
                // Task failed - show notification
                coordinator.update_ui_state(Box::new(move |ui: &mut UIState| {
                    ui.show_error(format!("Task {task_id} failed: {e}"));
                }));

                Ok(vec![Action::UpdateTaskStatus {
                    task_id,
                    completed: true,
                }])
            }
        }
    }

    async fn handle_file_op_complete(
        &self,
        operation_id: String,
        result: &Result<(), crate::error::AppError>,
        coordinator: &StateCoordinator,
    ) -> anyhow::Result<Vec<Action>> {
        match result {
            Ok(()) => {
                coordinator.update_ui_state(Box::new(move |ui: &mut UIState| {
                    ui.show_success(format!("Operation {} completed", operation_id));
                }));
                Ok(vec![Action::ReloadDirectory])
            }

            Err(e) => {
                let error_msg = format!("Operation {} failed: {}", operation_id, e);
                coordinator.update_ui_state(Box::new(move |ui: &mut UIState| {
                    ui.show_error(error_msg);
                }));
                Ok(vec![])
            }
        }
    }

    async fn handle_file_op_progress(
        &self,
        _operation_id: String,
        current_bytes: u64,
        total_bytes: u64,
        current_file: &Path,
        coordinator: &StateCoordinator,
    ) -> anyhow::Result<Vec<Action>> {
        // Update progress in UI state
        let current_file_str = current_file.to_string_lossy().to_string();
        coordinator.update_ui_state(Box::new(move |ui: &mut UIState| {
            if let Some(loading) = &ui.loading {
                loading.set_completion(current_bytes, total_bytes);
                loading.set_current_item(Some(current_file_str));
            }
        }));

        coordinator.request_redraw(RedrawFlag::StatusBar);
        Ok(vec![])
    }
}

/// Data extracted from state for async processing
#[derive(Debug)]
enum TaskData {
    Legacy { task_id: u64 },
    FileOpComplete { operation_id: String },
    FileOpProgress { operation_id: String },
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
    /// your application's task semantics.  The default implementation
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
    /// your application's task semantics.  The default implementation
    /// performs no updates and returns no actions.
    pub async fn apply_legacy(&self, _app: &mut AppState) -> anyhow::Result<Vec<Action>> {
        // TODO: implement legacy task handling if coordinator is unavailable
        Ok(Vec::new())
    }
}
