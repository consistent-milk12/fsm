//! Enhanced event loop with enhanced state integration

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, error, info};

use crate::controller::actions::{Action, OperationId};
use crate::controller::state_coordinator::StateCoordinator;
use crate::error::AppError;

use crate::fs::object_info::ObjectInfo;
use crate::model::{RedrawFlag, ui_state::UIState};

/// Enhanced task result with operation tracking
#[derive(Debug, Clone)]
pub enum TaskResult {
    DirectoryLoad {
        task_id: u64,
        path: PathBuf,
        result: Result<Vec<ObjectInfo>, AppError>,
        execution_time: Duration,
    },
    FileOperation {
        operation_id: OperationId,
        operation_type: FileOperationType,
        result: Result<(), AppError>,
        execution_time: Duration,
    },
    SearchComplete {
        task_id: u64,
        query: String,
        results: Vec<ObjectInfo>,
        execution_time: Duration,
    },
    ContentSearchComplete {
        task_id: u64,
        query: String,
        results: Vec<String>,
        execution_time: Duration,
    },
    Progress {
        task_id: u64,
        current: u64,
        total: u64,
        message: Option<String>,
    },
    ClipboardOperation {
        operation_id: OperationId,
        operation_type: String,
        result: Result<u32, AppError>, // items processed
        execution_time: Duration,
    },
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

pub struct EventLoop {
    state_coordinator: Arc<StateCoordinator>,
    task_rx: UnboundedReceiver<TaskResult>,
    action_rx: UnboundedReceiver<Action>,
    pending_actions: VecDeque<Action>,
    metrics: EventLoopMetrics,
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
    pub max_actions_per_batch: usize,
    pub task_timeout: Duration,
    pub min_iteration_time: Duration,
}

impl Default for EventLoopConfig {
    fn default() -> Self {
        Self {
            max_actions_per_batch: 10,
            task_timeout: Duration::from_secs(30),
            min_iteration_time: Duration::from_millis(16),
        }
    }
}

impl EventLoop {
    pub fn new(
        task_rx: UnboundedReceiver<TaskResult>,
        action_rx: UnboundedReceiver<Action>,
        state_coordinator: Arc<StateCoordinator>,
    ) -> Self {
        info!("Creating enhanced event loop with state integration");

        Self {
            state_coordinator,
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

    pub async fn next_action(&mut self) -> Action {
        let iteration_start = Instant::now();

        if let Some(action) = self.pending_actions.pop_front() {
            return action;
        }

        tokio::select! {
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
                    None => return Action::Quit,
                }
            }

            action = self.action_rx.recv() => {
                match action {
                    Some(action) => {
                        debug!("Received external action: {:?}", action);
                        self.metrics.actions_processed += 1;
                        return action;
                    }
                    None => return Action::Quit,
                }
            }

            _ = tokio::time::sleep(self.config.min_iteration_time) => {
                return Action::Tick;
            }
        }

        self.pending_actions.pop_front().unwrap_or_else(|| {
            let elapsed = iteration_start.elapsed();
            self.metrics.total_processing_time += elapsed;
            self.metrics.last_activity = Instant::now();
            Action::Tick
        })
    }

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
            TaskResult::ClipboardOperation {
                operation_id,
                operation_type,
                result,
                execution_time,
            } => {
                self.handle_clipboard_operation(
                    operation_id,
                    operation_type,
                    result,
                    execution_time,
                )
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
            "Processing directory load for {} ({}ms)",
            path.display(),
            execution_time.as_millis()
        );

        match result {
            Ok(entries) => {
                // Update filesystem state
                {
                    let mut fs_state = self.state_coordinator.fs_state();
                    let pane = fs_state.active_pane_mut();

                    if pane.cwd == path {
                        pane.set_entries(entries);
                        debug!("Updated directory entries for {}", path.display());
                    }
                }

                // Update UI and complete task
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_success(&format!("Loaded directory: {}", path.display()));
                        ui.request_redraw(RedrawFlag::All);
                    }));

                {
                    let app_state = self.state_coordinator.app_state();
                    app_state.complete_task(task_id, None);
                }

                Ok(vec![Action::ReloadDirectory])
            }
            Err(e) => {
                error!("Directory load failed for {}: {}", path.display(), e);

                let err = e.clone();
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("Failed to load directory: {err}"));
                    }));

                {
                    let app_state = self.state_coordinator.app_state();
                    app_state.complete_task(task_id, Some(e.to_string().into()));
                }

                Ok(vec![])
            }
        }
    }

    async fn handle_file_operation(
        &self,
        operation_id: OperationId,
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
                let op_name = format!("{:?}", operation_type);
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_success(&format!("{} completed successfully", op_name));
                    }));

                // Update app state operation tracking
                {
                    let app_state = self.state_coordinator.app_state();

                    if let Some(task_id) = app_state
                        .operation_tasks
                        .get(&operation_id)
                        .map(|entry| *entry.value())
                    {
                        app_state.complete_task(task_id, None);
                    }
                }

                Ok(vec![Action::ReloadDirectory])
            }
            Err(e) => {
                error!("File operation {:?} failed: {}", operation_type, e);
                let op_name = format!("{:?}", operation_type).to_lowercase();
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("File {} failed: {}", op_name, e));
                    }));

                Ok(vec![])
            }
        }
    }

    async fn handle_clipboard_operation(
        &self,
        operation_id: OperationId,
        operation_type: String,
        result: Result<u32, AppError>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing clipboard {} ({}ms)",
            operation_type,
            execution_time.as_millis()
        );

        match result {
            Ok(items_processed) => {
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_success(&format!(
                            "Clipboard {} completed: {} items",
                            operation_type, items_processed
                        ));
                    }));

                {
                    let app_state = self.state_coordinator.app_state();
                    app_state.complete_clipboard_operation(&operation_id);
                }

                Ok(vec![Action::ReloadDirectory])
            }
            Err(e) => {
                error!("Clipboard operation {} failed: {}", operation_type, e);
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("Clipboard {} failed: {}", operation_type, e));
                    }));

                Ok(vec![])
            }
        }
    }

    async fn handle_search_complete(
        &self,
        task_id: u64,
        query: String,
        results: Vec<ObjectInfo>,
        execution_time: Duration,
    ) -> Result<Vec<Action>> {
        debug!(
            "Processing search completion for '{}' ({}ms, {} results)",
            query,
            execution_time.as_millis(),
            results.len()
        );

        let result_count = results.len();
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.filename_search_results = results;
                ui.show_info(&format!("Found {} matches for '{}'", result_count, query));
            }));

        {
            let app_state = self.state_coordinator.app_state();
            app_state.complete_task(task_id, None);
            app_state.complete_search(&format!("search_{}", task_id), result_count);
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
            "Processing content search for '{}' ({}ms, {} results)",
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
            }));

        {
            let app_state = self.state_coordinator.app_state();
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
        debug!("Progress update for task {task_id}: {current}/{total}",);

        let mut moved: Option<String> = None;

        if let Some(ref tmp) = message {
            moved = Some(tmp.clone());
        }

        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if let Some(loading) = &ui.loading {
                    loading.set_completion(current, total);
                    if let Some(msg) = moved {
                        loading.set_current_item(Some(msg));
                    }
                }
            }));

        // Update task progress via StateProvider
        self.state_coordinator
            .update_task_progress(task_id.to_string(), current, total, message);

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
                        }));
                }

                {
                    let app_state = self.state_coordinator.app_state();
                    app_state.complete_task(task_id, None);
                }

                Ok(vec![])
            }
            Err(e) => {
                error!("Generic task {} failed: {}", task_id, e);

                let error_msg = message.unwrap_or_else(|| format!("Task {} failed", task_id));
                let err_clone = e.clone();
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&format!("{}: {}", error_msg, err_clone));
                    }));

                {
                    let app_state = self.state_coordinator.app_state();
                    app_state.complete_task(task_id, Some(e.to_string().into()));
                }

                Ok(vec![])
            }
        }
    }

    fn queue_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            self.pending_actions.push_back(action);
        }
    }

    async fn get_current_search_results(&self) -> Vec<ObjectInfo> {
        let ui_state = self.state_coordinator.ui_state();
        let ui = ui_state.read().expect("UI state lock poisoned");
        ui.filename_search_results.clone()
    }

    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_coordinator
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_error(&msg);
            }));
    }

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
}

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
    pub fn kind(&self) -> &'static str {
        match self {
            TaskResult::DirectoryLoad { .. } => "directory_load",
            TaskResult::FileOperation { .. } => "file_operation",
            TaskResult::SearchComplete { .. } => "search_complete",
            TaskResult::ContentSearchComplete { .. } => "content_search_complete",
            TaskResult::ClipboardOperation { .. } => "clipboard_operation",
            TaskResult::Progress { .. } => "progress",
            TaskResult::Generic { .. } => "generic",
        }
    }

    pub fn task_id(&self) -> Option<u64> {
        match self {
            TaskResult::DirectoryLoad { task_id, .. }
            | TaskResult::SearchComplete { task_id, .. }
            | TaskResult::ContentSearchComplete { task_id, .. }
            | TaskResult::Progress { task_id, .. }
            | TaskResult::Generic { task_id, .. } => Some(*task_id),
            _ => None,
        }
    }
}
