//! Enhanced event loop with proper async integration and task management
//!
//! This module provides a robust event loop that:
//! - Properly integrates background task results with the modular action dispatcher
//! - Handles async operations without deadlocks
//! - Provides clean separation between event processing and state updates
//! - Includes comprehensive error handling and resource management
//! - Supports performance monitoring and graceful shutdown

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::FSState;
use crate::controller::actions::Action;
use crate::controller::state_coordinator::StateCoordinator;
use crate::error::AppError;
use crate::fs::object_info::ObjectInfo;
use crate::model::PaneState;
use crate::model::{RedrawFlag, app_state::AppState, ui_state::UIState};

/// Enhanced task result with comprehensive metadata
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Directory loading completed
    DirectoryLoad {
        task_id: u64,
        path: PathBuf,
        result: Result<Vec<crate::fs::object_info::ObjectInfo>, AppError>,
        execution_time: Duration,
    },

    /// File operation completed
    FileOperation {
        operation_id: String,
        operation_type: FileOperationType,
        result: Result<(), AppError>,
        execution_time: Duration,
    },

    /// Search operation completed
    SearchComplete {
        task_id: u64,
        query: String,
        results: Vec<crate::fs::object_info::ObjectInfo>,
        execution_time: Duration,
    },

    /// Content search completed
    ContentSearchComplete {
        task_id: u64,
        query: String,
        results: Vec<String>,
        execution_time: Duration,
    },

    /// Background task progress update
    Progress {
        task_id: u64,
        current: u64,
        total: u64,
        message: Option<String>,
    },

    /// Generic task completion
    Generic {
        task_id: u64,
        result: Result<(), AppError>,
        message: Option<String>,
        execution_time: Duration,
    },
}

#[derive(Debug, Clone)]
pub enum FileOperationType {
    Copy,
    Move,
    Delete,
    Create,
    Rename,
}

/// Event loop with proper async integration
pub struct EventLoop {
    /// Shared state coordinator
    state_coordinator: Arc<StateCoordinator>,

    /// Legacy app state for compatibility
    app_state: Arc<Mutex<AppState>>,

    /// Task results from background operations
    task_rx: UnboundedReceiver<TaskResult>,

    /// Actions from external sources (UI, etc.)
    action_rx: UnboundedReceiver<Action>,

    /// Pending actions queue for batch processing
    pending_actions: VecDeque<Action>,

    /// Performance metrics
    metrics: EventLoopMetrics,

    /// Configuration
    config: EventLoopConfig,
}

#[derive(Debug)]
struct EventLoopMetrics {
    tasks_processed: u64,
    actions_processed: u64,
    total_processing_time: Duration,
    last_activity: Instant,
}

#[derive(Debug, Clone)]
pub struct EventLoopConfig {
    /// Maximum actions to process per iteration
    pub max_actions_per_batch: usize,
    /// Timeout for task processing
    pub task_timeout: Duration,
    /// Minimum time between iterations
    pub min_iteration_time: Duration,
}

impl Default for EventLoopConfig {
    fn default() -> Self {
        Self {
            max_actions_per_batch: 10,
            task_timeout: Duration::from_secs(30),
            min_iteration_time: Duration::from_millis(16), // 60fps target
        }
    }
}

impl EventLoop {
    /// Create new event loop with proper async integration
    pub fn new(
        app_state: Arc<Mutex<AppState>>,
        task_rx: UnboundedReceiver<TaskResult>,
        action_rx: UnboundedReceiver<Action>,
        state_coordinator: Arc<StateCoordinator>,
    ) -> Self {
        info!("Creating enhanced event loop with async integration");

        Self {
            state_coordinator,
            app_state,
            task_rx,
            action_rx,
            pending_actions: VecDeque::with_capacity(32),
            metrics: EventLoopMetrics {
                tasks_processed: 0,
                actions_processed: 0,
                total_processing_time: Duration::ZERO,
                last_activity: Instant::now(),
            },
            config: EventLoopConfig::default(),
        }
    }

    /// Get next action with proper async event processing
    pub async fn next_action(&mut self) -> Action {
        let iteration_start: Instant = Instant::now();

        // Process pending actions first
        if let Some(action) = self.pending_actions.pop_front() {
            debug!("Returning pending action: {:?}", action);

            return action;
        }

        // Try to get new events with timeout
        tokio::select! {
            // Handle task results
            task_result = self.task_rx.recv() => {
                match task_result {
                    Some(result) => {
                        debug!("Processing task result: {:?}", result.kind());

                        match self.process_task_result(result).await {
                            Ok(actions) => {
                                self.queue_actions(actions);
                                self.metrics.tasks_processed += 1;
                            }

                            Err(e) => {
                                error!("Failed to process task result: {}", e);
                                self.show_error(&format!("Task processing failed: {}", e));
                            }
                        }
                    }

                    None => {
                        warn!("Task channel closed");

                        return Action::Quit;
                    }
                }
            }

            // Handle external actions
            action = self.action_rx.recv() => {
                match action {
                    Some(action) => {
                        debug!("Received external action: {:?}", action);

                        self.metrics.actions_processed += 1;
                        return action;
                    }

                    None => {
                        warn!("Action channel closed");

                        return Action::Quit;
                    }
                }
            }

            // Timeout fallback
            _ = tokio::time::sleep(self.config.min_iteration_time) => {
                // Return tick for UI updates
                return Action::Tick;
            }
        }

        // Return next pending action or tick
        self.pending_actions.pop_front().unwrap_or_else(|| {
            // Update metrics
            let elapsed = iteration_start.elapsed();
            self.metrics.total_processing_time += elapsed;
            self.metrics.last_activity = Instant::now();

            Action::Tick
        })
    }

    /// Process task result and generate appropriate actions
    async fn process_task_result(&self, task_result: TaskResult) -> Result<Vec<Action>> {
        match task_result {
            TaskResult::DirectoryLoad {
                task_id,
                path,
                result,
                execution_time,
            } => {
                self.handle_directory_load(task_id, path, result, execution_time)
                    .await
            }

            TaskResult::FileOperation {
                operation_id,
                operation_type,
                result,
                execution_time,
            } => {
                self.handle_file_operation(operation_id, operation_type, result, execution_time)
                    .await
            }

            TaskResult::SearchComplete {
                task_id,
                query,
                results,
                execution_time,
            } => {
                self.handle_search_complete(task_id, query, results, execution_time)
                    .await
            }

            TaskResult::ContentSearchComplete {
                task_id,
                query,
                results,
                execution_time,
            } => {
                self.handle_content_search_complete(task_id, query, results, execution_time)
                    .await
            }

            TaskResult::Progress {
                task_id,
                current,
                total,
                message,
            } => {
                self.handle_progress_update(task_id, current, total, message)
                    .await
            }

            TaskResult::Generic {
                task_id,
                result,
                message,
                execution_time,
            } => {
                self.handle_generic_task(task_id, result, message, execution_time)
                    .await
            }
        }
    }

    async fn handle_directory_load(
        &self,
        task_id: u64,
        path: PathBuf,
        result: Result<Vec<ObjectInfo>, AppError>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing directory load for path: {} ({}ms)",
            path.display(),
            execution_time.as_millis()
        );

        match result {
            Ok(entries) => {
                // Update filesystem state without holding locks during async operations
                {
                    let mut fs_state: MutexGuard<'_, FSState> = self.state_coordinator.fs_state();
                    let pane: &mut PaneState = fs_state.active_pane_mut();

                    // Only update if this is still the current directory
                    if pane.cwd == path {
                        pane.entries = entries;
                        pane.is_loading
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        debug!("Updated directory entries for {}", path.display());
                    } else {
                        debug!("Skipping stale directory load for {}", path.display());
                    }
                }

                // Update UI state
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_success(&format!("Loaded directory: {}", path.display()));
                        ui.request_redraw(RedrawFlag::All);
                    }));

                // Complete task in app state
                {
                    let app_state: MutexGuard<'_, AppState> = self.app_state.lock().map_err(
                        |e: PoisonError<MutexGuard<'_, AppState>>| {
                            anyhow::anyhow!("Failed to acquire app state lock: {e}")
                        },
                    )?;

                    app_state.complete_task(task_id, None);
                }

                Ok(vec![Action::ReloadDirectory])
            }

            Err(e) => {
                error!("Directory load failed for {}: {}", path.display(), e);

                // Show error to user
                let err: AppError = e.clone();
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("Failed to load directory: {err}"));
                        ui.request_redraw(RedrawFlag::StatusBar);
                    }));

                // Complete task with error
                {
                    let app_state: MutexGuard<'_, AppState> = self.app_state.lock().map_err(
                        |e: PoisonError<MutexGuard<'_, AppState>>| {
                            anyhow::anyhow!("Failed to acquire app state lock: {e}")
                        },
                    )?;

                    app_state.complete_task(task_id, Some(e.to_string().into()));
                }

                Ok(vec![])
            }
        }
    }

    async fn handle_file_operation(
        &self,
        operation_id: String,
        operation_type: FileOperationType,
        result: Result<(), AppError>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing file operation {:?} ({}ms)",
            operation_type,
            execution_time.as_millis()
        );

        match result {
            Ok(()) => {
                let op_name: String = format!("ID: {operation_id} | Type: {operation_type:?}");

                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_success(&format!("File {} completed successfully", op_name));
                        ui.request_redraw(RedrawFlag::All);
                    }));

                // Reload directory to show changes
                Ok(vec![Action::ReloadDirectory])
            }

            Err(e) => {
                error!("File operation {:?} failed: {}", operation_type, e);
                let op_name = format!("{:?}", operation_type).to_lowercase();
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("File {} failed: {}", op_name, e));
                        ui.request_redraw(RedrawFlag::StatusBar);
                    }));

                Ok(vec![])
            }
        }
    }

    async fn handle_search_complete(
        &self,
        task_id: u64,
        query: String,
        results: Vec<crate::fs::object_info::ObjectInfo>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing search completion for '{}' ({}ms, {} results)",
            query,
            execution_time.as_millis(),
            results.len()
        );

        // Update search results
        let result_count = results.len();
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.filename_search_results = results;
                ui.show_info(&format!("Found {} matches for '{}'", result_count, query));
                ui.request_redraw(RedrawFlag::All);
            }));

        // Complete task
        {
            let app_state = self
                .app_state
                .lock()
                .map_err(|e| anyhow::anyhow!("Failed to acquire app state lock: {}", e))?;
            app_state.complete_task(task_id, None);
        }

        Ok(vec![Action::ShowFilenameSearchResults(
            self.get_current_search_results().await,
        )])
    }

    async fn handle_content_search_complete(
        &self,
        task_id: u64,
        query: String,
        results: Vec<String>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing content search completion for '{}' ({}ms, {} results)",
            query,
            execution_time.as_millis(),
            results.len()
        );

        let result_count = results.len();
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_info(&format!(
                    "Content search found {} matches for '{}'",
                    result_count, query
                ));
                ui.request_redraw(RedrawFlag::All);
            }));

        // Complete task
        {
            let app_state =
                self.app_state
                    .lock()
                    .map_err(|e: PoisonError<MutexGuard<'_, AppState>>| {
                        anyhow::anyhow!("Failed to acquire app state lock: {e}")
                    })?;

            app_state.complete_task(task_id, None);
        }

        Ok(vec![Action::ShowRichSearchResults(results)])
    }

    async fn handle_progress_update(
        &self,
        task_id: u64,
        current: u64,
        total: u64,
        message: Option<String>,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing progress update for task {}: {}/{}",
            task_id, current, total
        );

        // Update progress in UI
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if let Some(loading) = &ui.loading {
                    loading.set_completion(current, total);

                    if let Some(msg) = message {
                        loading.set_current_item(Some(msg));
                    }
                }

                ui.request_redraw(RedrawFlag::StatusBar);
            }));

        Ok(vec![])
    }

    async fn handle_generic_task(
        &self,
        task_id: u64,
        result: Result<(), AppError>,
        message: Option<String>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing generic task {} ({}ms)",
            task_id,
            execution_time.as_millis()
        );

        match result {
            Ok(()) => {
                if let Some(msg) = message {
                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.show_success(&msg);
                            ui.request_redraw(RedrawFlag::StatusBar);
                        }));
                }

                // Complete task
                {
                    let app_state: MutexGuard<'_, AppState> = self.app_state.lock().map_err(
                        |e: PoisonError<MutexGuard<'_, AppState>>| {
                            anyhow::anyhow!("Failed to acquire app state lock: {e}")
                        },
                    )?;

                    app_state.complete_task(task_id, None);
                }

                Ok(vec![])
            }

            Err(e) => {
                error!("Generic task {task_id} failed: {e}");

                let error_msg: String = message.unwrap_or_else(|| format!("Task {task_id} failed"));

                let value: AppError = e.clone();

                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("{}: {}", error_msg, value));
                        ui.request_redraw(RedrawFlag::StatusBar);
                    }));

                // Complete task with error
                {
                    let app_state: MutexGuard<'_, AppState> = self.app_state.lock().map_err(
                        |e: PoisonError<MutexGuard<'_, AppState>>| {
                            anyhow::anyhow!("Failed to acquire app state lock: {e}")
                        },
                    )?;

                    app_state.complete_task(task_id, Some(e.to_string().into()));
                }

                Ok(vec![])
            }
        }
    }

    /// Queue multiple actions for processing
    fn queue_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            self.pending_actions.push_back(action);
        }
    }

    /// Get current search results safely
    async fn get_current_search_results(&self) -> Vec<crate::fs::object_info::ObjectInfo> {
        let ui_state: Arc<RwLock<UIState>> = self.state_coordinator.ui_state();
        let ui: RwLockReadGuard<'_, UIState> = ui_state.read().expect("UI state lock poisoned");
        ui.filename_search_results.clone()
    }

    /// Show error message to user
    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_error(&msg);
                ui.request_redraw(RedrawFlag::StatusBar);
            }));
    }

    /// Get event loop performance metrics
    pub fn get_metrics(&self) -> EventLoopMetricsSnapshot {
        EventLoopMetricsSnapshot {
            tasks_processed: self.metrics.tasks_processed,
            actions_processed: self.metrics.actions_processed,
            total_processing_time: self.metrics.total_processing_time,
            average_processing_time: if self.metrics.tasks_processed > 0 {
                self.metrics.total_processing_time / self.metrics.tasks_processed as u32
            } else {
                Duration::ZERO
            },
            last_activity: self.metrics.last_activity,
            pending_actions: self.pending_actions.len(),
        }
    }

    /// Flush all pending actions and process remaining tasks
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down event loop");

        let start: Instant = Instant::now();
        let mut remaining_tasks: i32 = 0;

        // Process remaining task results with timeout
        while let Ok(Some(task_result)) =
            timeout(Duration::from_millis(100), self.task_rx.recv()).await
        {
            match self.process_task_result(task_result).await {
                Ok(actions) => {
                    self.queue_actions(actions);

                    remaining_tasks += 1;
                }

                Err(e) => {
                    warn!("Error processing task during shutdown: {e}");
                }
            }

            // Prevent hanging during shutdown
            if start.elapsed() > Duration::from_secs(5) {
                warn!("Shutdown timeout reached, forcing termination");

                break;
            }
        }

        if remaining_tasks > 0 {
            info!("Processed {remaining_tasks} remaining tasks during shutdown",);
        }

        info!("Event loop shutdown complete");

        Ok(())
    }
}

/// Performance metrics snapshot
#[derive(Debug, Clone)]
pub struct EventLoopMetricsSnapshot {
    pub tasks_processed: u64,
    pub actions_processed: u64,
    pub total_processing_time: Duration,
    pub average_processing_time: Duration,
    pub last_activity: Instant,
    pub pending_actions: usize,
}

impl TaskResult {
    /// Get task result type for logging
    pub fn kind(&self) -> &'static str {
        match self {
            TaskResult::DirectoryLoad { .. } => "directory_load",

            TaskResult::FileOperation { .. } => "file_operation",

            TaskResult::SearchComplete { .. } => "search_complete",

            TaskResult::ContentSearchComplete { .. } => "content_search_complete",

            TaskResult::Progress { .. } => "progress",

            TaskResult::Generic { .. } => "generic",
        }
    }

    /// Get task ID if available
    pub fn task_id(&self) -> Option<u64> {
        match self {
            TaskResult::DirectoryLoad { task_id, .. }
            | TaskResult::SearchComplete { task_id, .. }
            | TaskResult::ContentSearchComplete { task_id, .. }
            | TaskResult::Progress { task_id, .. }
            | TaskResult::Generic { task_id, .. } => Some(*task_id),
            TaskResult::FileOperation { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
    use std::sync::{Mutex, RwLock};
    use tokio::sync::mpsc;

    fn create_test_setup() -> (
        EventLoop,
        UnboundedSender<TaskResult>,
        UnboundedSender<Action>,
    ) {
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let app_state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::default()));
        let ui_state: RwLock<UIState> = RwLock::new(UIState::default());
        let fs_state: Arc<Mutex<FSState>> = Arc::new(Mutex::new(FSState::default()));

        let state_coordinator: Arc<StateCoordinator> =
            Arc::new(StateCoordinator::new(app_state.clone(), ui_state, fs_state));
        let event_loop: EventLoop =
            EventLoop::new(app_state, task_rx, action_rx, state_coordinator);

        (event_loop, task_tx, action_tx)
    }

    #[tokio::test]
    async fn test_task_processing() {
        let (mut event_loop, task_tx, _action_tx) = create_test_setup();

        // Send a task result
        let task_result: TaskResult = TaskResult::Generic {
            task_id: 1,
            result: Ok(()),
            message: Some("Test task completed".to_string()),
            execution_time: Duration::from_millis(100),
        };

        task_tx.send(task_result).unwrap();

        // Process the task
        let action: Action = event_loop.next_action().await;

        // Should return Tick since generic tasks don't generate specific actions
        assert!(matches!(action, Action::Tick));
        assert_eq!(event_loop.metrics.tasks_processed, 1);
    }

    #[tokio::test]
    async fn test_action_forwarding() {
        let (mut event_loop, _task_tx, action_tx) = create_test_setup();

        // Send an action
        action_tx.send(Action::MoveSelectionUp).unwrap();

        // Should receive the action
        let action: Action = event_loop.next_action().await;
        assert!(matches!(action, Action::MoveSelectionUp));
        assert_eq!(event_loop.metrics.actions_processed, 1);
    }

    #[tokio::test]
    async fn test_metrics_tracking() {
        let (mut event_loop, task_tx, action_tx) = create_test_setup();

        // Send multiple events
        task_tx
            .send(TaskResult::Generic {
                task_id: 1,
                result: Ok(()),
                message: None,
                execution_time: Duration::from_millis(50),
            })
            .unwrap();

        action_tx.send(Action::Tick).unwrap();

        // Process events
        event_loop.next_action().await;
        event_loop.next_action().await;

        let metrics = event_loop.get_metrics();
        assert_eq!(metrics.tasks_processed, 1);
        assert_eq!(metrics.actions_processed, 1);
    }
}
