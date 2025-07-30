// fsm-core/src/controller/event_loop.rs
// Complete event loop with comprehensive tracing using all tracing crate features

use std::{
    path::PathBuf,
    sync::{Arc, MutexGuard, atomic::Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event as TerminalEvent, EventStream, KeyEvent};
use tokio::{
    sync::{Notify, mpsc},
    time::{MissedTickBehavior, interval},
};
use tracing::{
    Level, Span, debug, debug_span, error, event, field::Empty, info, info_span, instrument,
    span::Entered, trace, warn,
};

use futures::StreamExt;

use crate::{
    AppState, FSState, UIState,
    controller::{
        action_dispatcher::{ActionDispatcher, ActionSource},
        actions::{Action, OperationId},
        handlers::{Event, Priority, key_handler_orchestrator::KeyHandlerOrchestrator},
        state_coordinator::StateCoordinator,
    },
    error::AppError,
    fs::object_info::ObjectInfo,
    model::{PaneState, ui_state::RedrawFlag},
};

/// Task results from background operations - matches dir_scanner.rs TaskResult
/// Task results from background operations - now cloneable.
/// We wrap `AppError` in `Arc` so the error payload itself
/// need not be `Clone`.
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Directory load (scan) result.
    DirectoryLoad {
        task_id: u64,                                   // unique task identifier
        path: PathBuf,                                  // directory scanned
        result: Result<Vec<ObjectInfo>, Arc<AppError>>, // wrapped error
        exec: Duration,                                 // execution time
    },

    /// File operation (create/delete/etc.).
    FileOperation {
        op_id: OperationId,                // operation identifier
        op_kind: FileOperationType,        // kind of file op
        result: Result<(), Arc<AppError>>, // wrapped error
        exec: Duration,                    // execution time
    },

    /// Name‑based search completed.
    SearchDone {
        task_id: u64,             // search task id
        query: String,            // search pattern
        results: Vec<ObjectInfo>, // matched entries
        exec: Duration,           // execution time
    },

    /// Content‑based search completed.
    ContentSearchDone {
        task_id: u64,         // content search id
        query: String,        // search pattern
        results: Vec<String>, // matching lines/snippets
        exec: Duration,       // execution time
    },

    /// Metadata update result for individual entries
    Metadata {
        task_id: u64,
        path: PathBuf,
        entry_path: PathBuf,
        result: Result<ObjectInfo, Arc<AppError>>,
        exec: Duration,
    },

    /// Progress update for a long‑running task.
    Progress {
        task_id: u64,        // refers to which task
        pct: f32,            // percentage 0.0–100.0
        msg: Option<String>, // optional message
    },

    /// Clipboard operation via Clipr.
    Clipboard {
        op_id: OperationId,                 // clipboard op id
        op_kind: String,                    // e.g. "copy" / "paste"
        result: Result<u32, Arc<AppError>>, // bytes transferred or err
        exec: Duration,                     // execution time
    },

    /// A generic catch‑all task result.
    Generic {
        task_id: u64,                      // generic task id
        result: Result<(), Arc<AppError>>, // wrapped error
        msg: Option<String>,               // optional info
        exec: Duration,                    // execution time
    },
}

#[derive(Debug, Clone)]
pub enum FileOperationType {
    Create,
    Copy,
    Move,
    Rename,
    Delete,
    Scan,
}

impl std::fmt::Display for FileOperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOperationType::Create => write!(f, "Create"),
            FileOperationType::Copy => write!(f, "Copy"),
            FileOperationType::Move => write!(f, "Move"),
            FileOperationType::Rename => write!(f, "Rename"),
            FileOperationType::Delete => write!(f, "Delete"),
            FileOperationType::Scan => write!(f, "Scan"),
        }
    }
}

/// Performance metrics with comprehensive tracing metadata
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub tasks: u64,
    pub actions: u64,
    pub total: Duration,
    pub avg: Duration,
    pub last: Instant,
    pub queued: usize,
}

pub type MetricsSnap = MetricsSnapshot;

/// Main event loop handling terminal events, tasks, and rendering
#[derive(Debug)]
pub struct EventLoop {
    state_coordinator: Arc<StateCoordinator>,
    action_dispatcher: ActionDispatcher,
    key_orchestrator: KeyHandlerOrchestrator,
    task_rx: mpsc::UnboundedReceiver<TaskResult>,
    action_rx: mpsc::UnboundedReceiver<Action>,
    shutdown: Arc<Notify>,

    // Metrics with tracing integration
    tasks_processed: u64,
    actions_processed: u64,
    start_time: Instant,
}

impl EventLoop {
    /// Create new event loop with comprehensive tracing setup
    #[instrument(
        level = "info",
        name = "event_loop_new",
        fields(
            coordinator_id = Empty,
            dispatcher_config = Empty,
            task_channel_capacity = Empty,
        ),
    )]
    pub fn new(
        state_coordinator: Arc<StateCoordinator>,
        action_rx: mpsc::UnboundedReceiver<Action>,
    ) -> (Self, mpsc::UnboundedSender<TaskResult>) {
        let init_span = info_span!(
            "event_loop_initialization",
            coordinator_addr = format!("{:p}", state_coordinator.as_ref()),
            thread_id = ?std::thread::current().id(),
        );

        let _init_guard = init_span.enter();

        // Create task channel with tracing
        let (task_tx, task_rx) = {
            let channel_span = debug_span!("task_channel_creation");
            let _guard = channel_span.enter();

            let (tx, rx) = mpsc::unbounded_channel();

            event!(
                Level::DEBUG,
                channel_type = "unbounded",
                sender_addr = format!("{:p}", &tx),
                receiver_addr = format!("{:p}", &rx),
                "Task channel created successfully"
            );

            (tx, rx)
        };

        // Initialize action dispatcher with tracing
        let action_dispatcher = {
            let dispatcher_span = debug_span!(
                "action_dispatcher_init",
                coordinator_ref = format!("{:p}", state_coordinator.as_ref()),
                task_sender_ref = format!("{:p}", &task_tx),
            );
            let _guard = dispatcher_span.enter();

            let dispatcher = ActionDispatcher::new(state_coordinator.clone(), task_tx.clone());

            event!(
                Level::DEBUG,
                dispatcher_addr = format!("{:p}", &dispatcher),
                handlers_count = Empty, // Will be filled by dispatcher
                "Action dispatcher initialized"
            );

            dispatcher
        };

        let now = Instant::now();

        // Initialize key handler orchestrator
        let key_orchestrator: KeyHandlerOrchestrator = KeyHandlerOrchestrator::new();
        debug!(
            handlers_count = key_orchestrator.get_handler_names().len(),
            handlers = ?key_orchestrator.get_handler_names(),
            "Key handler orchestrator initialized"
        );

        let event_loop = EventLoop {
            state_coordinator,
            action_dispatcher,
            key_orchestrator,
            task_rx,
            action_rx,
            shutdown: Arc::new(Notify::new()),
            tasks_processed: 0,
            actions_processed: 0,
            start_time: now,
        };

        // Record initialization completion
        event!(
            Level::INFO,
            event_loop_addr = format!("{:p}", &event_loop),
            initialization_time = ?now.elapsed(),
            "Event loop initialization completed"
        );

        (event_loop, task_tx)
    }

    /// Main event processing loop with comprehensive tracing
    #[instrument(
        level = "info",
        name = "event_loop_run",
        fields(
            loop_id = tracing::field::display(nanoid::nanoid!()),
            start_time = ?Instant::now(),
        ),
        err
    )]
    pub async fn run(&mut self) -> Result<()> {
        // Startup log
        info!("Event loop started");

        // Terminal event stream (fused so it ends gracefully)
        let mut event_stream = EventStream::new().fuse();

        // Render timer at ~60 FPS
        let mut render_timer = interval(Duration::from_millis(16));
        render_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // Metrics
        let mut iteration: u64 = 0;
        let mut last_metrics = Instant::now();
        const METRICS_INTERVAL: Duration = Duration::from_secs(30);

        loop {
            iteration += 1;

            tokio::select! {
                // 1) Shutdown requested
                _ = self.shutdown.notified() => {
                    info!(
                        reason = "shutdown_requested",
                        total_iterations = iteration,
                        uptime = ?self.start_time.elapsed(),
                        "Event loop shutdown initiated"
                    );

                    break;
                }

                // 2) Terminal input events
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(ev)) => {
                            let actions = self.process_terminal_event(ev).await;
                            for action in actions {
                                let continue_loop = self
                                    .dispatch_action(action, ActionSource::Keyboard)
                                    .await?;

                                if !continue_loop {
                                    info!("Event loop termination requested by action");
                                    self.state_coordinator.request_redraw(RedrawFlag::All); // Ensure final render
                                    break;
                                }
                            }
                        }

                        Some(Err(err)) => {
                            warn!(error = %err, "Terminal event error, continuing");
                        }

                        None => {
                            warn!("Terminal event stream closed unexpectedly");
                        }
                    }
                }

                // 3) Background task results
                Some(task_res) = self.task_rx.recv() => {
                    self.handle_task_result(task_res).await;

                    self.tasks_processed += 1;
                }

                // 4) Injected actions from other parts of the application (e.g., main render loop)
                Some(action) = self.action_rx.recv() => {
                    debug!(action = ?action, "Received injected action");
                    let continue_loop = self.dispatch_action(action, ActionSource::System).await?;
                    self.state_coordinator.update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.poll_notification();
                    }));
                    if !continue_loop {
                        info!("Event loop termination requested by injected action");
                        self.state_coordinator.request_redraw(RedrawFlag::All); // Ensure final render
                        break;
                    }
                }

                // 5) Idle back‑off
                else => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }

            // Periodic metrics report
            if last_metrics.elapsed() >= METRICS_INTERVAL {
                self.report_performance_metrics(iteration);

                last_metrics = Instant::now();
            }

            // Poll notifications for auto-dismissal
            self.state_coordinator.update_ui_state(Box::new(|ui: &mut UIState| {
                ui.poll_notification();
            }));
        }

        // Shutdown complete
        info!("Event loop completed successfully");

        Ok(())
    }

    /// Process terminal events with detailed tracing
    #[instrument(level = "trace", name = "process_terminal_event", skip(self, event))]
    async fn process_terminal_event(&mut self, event: TerminalEvent) -> Vec<Action> {
        match event {
            TerminalEvent::Key(key) => {
                let actions = self.process_key_event(key).await;

                if !actions.is_empty() {
                    debug!(
                        key = ?key,
                        actions_count = actions.len(),
                        actions = ?actions,
                        "Key event mapped to actions"
                    );
                }

                actions
            }

            TerminalEvent::Resize(w, h) => {
                debug!(width = w, height = h, "Terminal resize event processed");
                vec![Action::Resize(w, h)]
            }

            TerminalEvent::Mouse(mouse_event) => {
                trace!(
                    mouse_event = ?mouse_event,
                    "Mouse event received but not handled"
                );
                vec![]
            }

            TerminalEvent::FocusGained => {
                trace!("Terminal focus gained");
                vec![]
            }

            TerminalEvent::FocusLost => {
                trace!("Terminal focus lost");
                vec![]
            }

            TerminalEvent::Paste(data) => {
                debug!(
                    paste_length = data.len(),
                    "Paste event received but not implemented"
                );
                vec![]
            }
        }
    }

    #[instrument(level = "trace", name = "process_key_event", skip(self, key))]
    async fn process_key_event(&mut self, key: KeyEvent) -> Vec<Action> {
        // Create event for orchestrator
        let event = Event::Key {
            event: key,
            priority: Priority::High,
        };

        // Process through orchestrator
        match self.key_orchestrator.handle_key_event(event) {
            Ok(actions) => {
                if !actions.is_empty() {
                    trace!(
                        key = ?key,
                        actions_count = actions.len(),
                        actions = ?actions,
                        "Key successfully processed by orchestrator"
                    );
                } else {
                    trace!(
                        key = ?key,
                        "No actions generated by orchestrator"
                    );
                }

                actions
            }

            Err(e) => {
                warn!(
                    key = ?key,
                    error = %e,
                    "Orchestrator failed to process key event"
                );

                vec![]
            }
        }
    }

    /// Get selected file path with tracing
    #[instrument(
        level = "trace",
        name = "get_selected_path",
        fields(
            pane_index = Empty,
            selected_index = Empty,
            path_exists = Empty,
            path = Empty,
        )
    )]
    async fn get_selected_path(&self) -> Option<PathBuf> {
        let fs = self.state_coordinator.fs_state();
        let pane = fs.active_pane();
        let idx = pane.selected.load(Ordering::Relaxed);

        tracing::Span::current()
            .record("selected_index", idx)
            .record("pane_entries_count", pane.entries.len());

        if let Some(entry) = pane.entries.get(idx) {
            let path = entry.path.clone();

            tracing::Span::current()
                .record("path_exists", true)
                .record("path", tracing::field::display(&path.display()));

            trace!(
                selected_index = idx,
                path = %path.display(),
                entry_type = ?entry.object_type,
                "Selected path retrieved successfully"
            );

            Some(path)
        } else {
            tracing::Span::current().record("path_exists", false);

            debug!(
                selected_index = idx,
                entries_count = pane.entries.len(),
                "No entry at selected index"
            );

            None
        }
    }

    /// Dispatch action to handler with comprehensive tracing
    #[instrument(
        level = "debug",
        name = "dispatch_action",
        fields(
            action_type = %std::any::type_name_of_val(&action),
            action_debug = ?action,
            source = ?source,
            dispatch_time = Empty,
            result = Empty,
        )
    )]
    async fn dispatch_action(&mut self, action: Action, source: ActionSource) -> Result<bool> {
        let dispatch_start: Instant = Instant::now();

        debug!(
            action = ?action,
            source = ?source,
            actions_processed = self.actions_processed,
            "Dispatching action to handler"
        );

        // Perform the dispatch and take ownership of its Result
        let result: Result<bool> = self.action_dispatcher.dispatch(action, source).await;
        let dispatch_time: Duration = dispatch_start.elapsed();

        // Record metrics on the current span *before* we move `result` out
        let span: Span = tracing::Span::current();
        span.record("dispatch_time", tracing::field::debug(dispatch_time));
        span.record("result", tracing::field::debug(&result));

        // Update our internal counter
        self.actions_processed += 1;

        // Now consume `result` exactly once: log and return
        match result {
            Ok(should_continue) => {
                debug!(
                    should_continue = should_continue,
                    dispatch_time = ?dispatch_time,
                    total_actions = self.actions_processed,
                    "Action dispatched successfully"
                );

                Ok(should_continue)
            }

            Err(err) => {
                error!(
                    error = %err,
                    error_debug = ?err,
                    dispatch_time = ?dispatch_time,
                    total_actions = self.actions_processed,
                    "Action dispatch failed"
                );

                Err(err)
            }
        }
    }

    /// Handle background task results with detailed tracing
    #[instrument(
        level = "debug",
        name = "handle_task_result",
        fields(
            task_variant = Empty,
            task_id = Empty,
            execution_time = Empty,
            result_status = Empty,
        )
    )]
    async fn handle_task_result(&self, result: TaskResult) {
        match result {
            TaskResult::DirectoryLoad {
                task_id,
                path,
                result,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "DirectoryLoad")
                    .record("task_id", task_id)
                    .record("execution_time", tracing::field::debug(exec))
                    .record("path", tracing::field::display(&path.display()));

                match result {
                    Ok(entries) => {
                        tracing::Span::current()
                            .record("result_status", "success")
                            .record("entries_count", entries.len());

                        info!(
                            task_id = task_id,
                            path = %path.display(),
                            entries_count = entries.len(),
                            execution_time = ?exec,
                            "FILE_SYSTEM | BACKGROUND_OPERATION | SUCCESS : Directory load completed successfully"
                        );

                        // Update filesystem state
                        {
                            let update_span: Span = debug_span!(
                                "fs_state_update",
                                task_id = task_id,
                                entries_count = entries.len(),
                            );

                            let _guard: Entered<'_> = update_span.enter();

                            let mut fs: MutexGuard<'_, FSState> = self.state_coordinator.fs_state();
                            let pane: &mut PaneState = fs.active_pane_mut();

                            if pane.cwd == path {
                                // Use sort_entries which includes proper sorting and filtering
                                pane.sort_entries(entries);
                                pane.is_loading.store(false, Ordering::Relaxed);

                                debug!(
                                    pane_path = %pane.cwd.display(),
                                    entries_final_count = pane.entries.len(),
                                    "FILE_SYSTEM | BACKGROUND_OPERATION | SUCCESS : Sorted entries in the active pane"
                                );
                            } else {
                                warn!(
                                    expected_path = %pane.cwd.display(),
                                    received_path = %path.display(),
                                    "ERROR: FILE_SYSTEM | BACKGROUND_OPERATION | FAILURE : Could not sort entries in the active pane"
                                );
                            }
                        }

                        // Update UI state
                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.success(format!("Loaded {}", path.display()));
                            },
                        ));

                        // Complete task tracking
                        {
                            let app: MutexGuard<'_, AppState> = self.state_coordinator.app_state();
                            app.complete_task(task_id);

                            debug!(task_id = task_id, "Task marked as completed in app state");
                        }

                        self.state_coordinator.request_redraw(RedrawFlag::All);
                    }
                    Err(e) => {
                        tracing::Span::current().record("result_status", "error");

                        error!(
                            task_id = task_id,
                            path = %path.display(),
                            error = %e,
                            error_debug = ?e,
                            execution_time = ?exec,
                            "Directory load failed"
                        );

                        // Update filesystem state to clear loading
                        {
                            let mut fs: MutexGuard<'_, FSState> = self.state_coordinator.fs_state();
                            let pane: &mut PaneState = fs.active_pane_mut();
                            if pane.cwd == path {
                                pane.is_loading.store(false, Ordering::Relaxed);
                            }
                        }

                        // Show error in UI
                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.error(format!("Load failed: {e}"));
                            },
                        ));

                        // Complete task tracking
                        {
                            let app: MutexGuard<'_, AppState> = self.state_coordinator.app_state();
                            app.complete_task(task_id);
                        }
                    }
                }
            }

            TaskResult::SearchDone {
                task_id,
                query,
                results,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "SearchDone")
                    .record("task_id", task_id)
                    .record("execution_time", tracing::field::debug(exec))
                    .record("result_status", "success")
                    .record("query", &query)
                    .record("results_count", results.len());

                info!(
                    task_id = task_id,
                    query = %query,
                    results_count = results.len(),
                    execution_time = ?exec,
                    "Search completed successfully"
                );

                // Update search results in filesystem state
                {
                    let search_span: Span = debug_span!(
                        "search_results_update",
                        task_id = task_id,
                        results_count = results.len(),
                    );
                    let _guard: Entered<'_> = search_span.enter();

                    let mut fs: MutexGuard<'_, FSState> = self.state_coordinator.fs_state();
                    fs.active_pane_mut().search_results = results.clone();

                    debug!("Search results updated in filesystem state");
                }

                // Update UI with search completion message
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.info(format!("'{}' → {} results", query, results.len()));
                    }));

                // Complete task
                {
                    let app: MutexGuard<'_, AppState> = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }

            TaskResult::ContentSearchDone {
                task_id,
                query,
                results,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "ContentSearchDone")
                    .record("task_id", task_id)
                    .record("execution_time", tracing::field::debug(exec))
                    .record("result_status", "success")
                    .record("query", &query)
                    .record("results_count", results.len());

                info!(
                    task_id = task_id,
                    query = %query,
                    results_count = results.len(),
                    execution_time = ?exec,
                    "Content search completed successfully"
                );

                // Update UI with content search results
                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.info(format!("Content search: {} files", results.len()));
                    }));

                // Complete task
                {
                    let app: MutexGuard<'_, AppState> = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }

            TaskResult::FileOperation {
                op_id,
                op_kind,
                result,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "FileOperation")
                    .record("operation_id", tracing::field::display(&op_id))
                    .record("operation_kind", tracing::field::display(&op_kind))
                    .record("execution_time", tracing::field::debug(exec));

                match result {
                    Ok(()) => {
                        tracing::Span::current().record("result_status", "success");

                        info!(
                            operation_id = %op_id,
                            operation_kind = %op_kind,
                            execution_time = ?exec,
                            "File operation completed successfully"
                        );

                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.success(format!("{op_kind} completed"));
                            },
                        ));

                        self.state_coordinator.request_redraw(RedrawFlag::Main);
                    }
                    Err(e) => {
                        tracing::Span::current().record("result_status", "error");

                        error!(
                            operation_id = %op_id,
                            operation_kind = %op_kind,
                            error = %e,
                            error_debug = ?e,
                            execution_time = ?exec,
                            "File operation failed"
                        );

                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.error(format!("{op_kind} failed: {e}"));
                            },
                        ));
                    }
                }
            }

            TaskResult::Clipboard {
                op_id,
                op_kind,
                result,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "Clipboard")
                    .record("operation_id", tracing::field::display(&op_id))
                    .record("operation_kind", &op_kind)
                    .record("execution_time", tracing::field::debug(exec));

                match result {
                    Ok(count) => {
                        tracing::Span::current()
                            .record("result_status", "success")
                            .record("bytes_transferred", count);

                        info!(
                            operation_id = %op_id,
                            operation_kind = %op_kind,
                            bytes_transferred = count,
                            execution_time = ?exec,
                            "Clipboard operation completed successfully"
                        );

                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.success(format!("{op_kind} ok ({count})"));
                            },
                        ));
                    }
                    Err(e) => {
                        tracing::Span::current().record("result_status", "error");

                        error!(
                            operation_id = %op_id,
                            operation_kind = %op_kind,
                            error = %e,
                            error_debug = ?e,
                            execution_time = ?exec,
                            "Clipboard operation failed"
                        );

                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.error(format!("Clipboard {op_kind} failed: {e}"));
                            },
                        ));
                    }
                }
            }

            TaskResult::Metadata {
                task_id,
                path,
                entry_path,
                result,
                exec,
            } => {
                Span::current()
                    .record("task_variant", "Metadata")
                    .record("task_id", task_id)
                    .record("directory_path", path.display().to_string())
                    .record("entry_path", entry_path.display().to_string())
                    .record("execution_duration_us", exec.as_micros())
                    .record(
                        "result_status",
                        if result.is_ok() { "success" } else { "error" },
                    );

                match result {
                    Ok(updated_entry) => {
                        info!(
                            marker = "ACTION_DISPATCH_START",
                            operation_type = "metadata_update",
                            current_path = %path.display(),
                            target_path = %entry_path.display(),
                            entries_count = 1,
                            selected_index = "NULL",
                            duration_us = exec.as_micros(),
                            cache_hit = false,
                            area_width = "NULL",
                            area_height = "NULL",
                            updated_entry = %entry_path.display(),
                            "CONVERSION : TaskResult::Metadata -> Action::UpdateEntryMetadata"
                        );

                        // Convert TaskResult::Metadata to Action::UpdateEntryMetadata
                        let action: Action = Action::UpdateEntryMetadata {
                            directory_path: path.clone(),
                            entry_path: entry_path.clone(),
                            updated_entry,
                        };

                        // Dispatch through ActionDispatcher (clean architecture)
                        if let Err(e) = self
                            .action_dispatcher
                            .dispatch(action, ActionSource::System)
                            .await
                        {
                            error!(
                                marker = "ACTION_DISPATCH_FAILED",
                                operation_type = "metadata_update",
                                current_path = %path.display(),
                                target_path = %entry_path.display(),
                                entries_count = 1,
                                selected_index = "NULL",
                                duration_us = exec.as_micros(),
                                cache_hit = false,
                                area_width = "NULL",
                                area_height = "NULL",
                                error = %e,
                                "ACTION_ORCHESTRATOR: Failed to dispatch UpdateEntryMetadata"
                            );
                        } else {
                            info!(
                                marker = "ACTION_DISPATCH_COMPLETE",
                                operation_type = "metadata_update",
                                current_path = %path.display(),
                                target_path = %entry_path.display(),
                                entries_count = 1,
                                selected_index = "NULL",
                                duration_us = exec.as_micros(),
                                cache_hit = false,
                                area_width = "NULL",
                                area_height = "NULL",
                                "ACTION_ORCHESTRATOR: UpdateEntryMetadata action dispatched successfully"
                            );
                        }
                    }

                    Err(error) => {
                        warn!(
                            marker = "METADATA_LOAD_FAILED",
                            operation_type = "metadata_update",
                            current_path = %path.display(),
                            target_path = %entry_path.display(),
                            entries_count = 1,
                            selected_index = "NULL",
                            duration_us = exec.as_micros(),
                            cache_hit = false,
                            area_width = "NULL",
                            area_height = "NULL",
                            entry_path = %entry_path.display(),
                            error = %error,
                            "Conversion: Failed to load metadata for entry"
                        );
                    }
                }
            }

            TaskResult::Progress { task_id, pct, msg } => {
                tracing::Span::current()
                    .record("task_variant", "Progress")
                    .record("task_id", task_id)
                    .record("progress_percent", pct)
                    .record("result_status", "progress_update");

                trace!(
                    task_id = task_id,
                    progress_percent = pct,
                    message = ?msg,
                    "Progress update received"
                );

                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        if let Some(ref mut loading) = ui.loading {
                            loading.set_progress(pct);

                            if let Some(m) = msg {
                                loading.message = m.into();
                            }
                        }
                    }));
            }

            TaskResult::Generic {
                task_id,
                result,
                msg,
                exec,
            } => {
                tracing::Span::current()
                    .record("task_variant", "Generic")
                    .record("task_id", task_id)
                    .record("execution_time", tracing::field::debug(exec));

                match result {
                    Ok(()) => {
                        tracing::Span::current().record("result_status", "success");

                        info!(
                            task_id = task_id,
                            message = ?msg,
                            execution_time = ?exec,
                            "Generic task completed successfully"
                        );

                        if let Some(m) = msg {
                            self.state_coordinator.update_ui_state(Box::new(
                                move |ui: &mut UIState| {
                                    ui.success(&m);
                                },
                            ));
                        }
                    }
                    Err(e) => {
                        tracing::Span::current().record("result_status", "error");

                        error!(
                            task_id = task_id,
                            error = %e,
                            error_debug = ?e,
                            execution_time = ?exec,
                            "Generic task failed"
                        );

                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.error(format!("Task failed: {e}"));
                            },
                        ));
                    }
                }

                // Complete task tracking
                {
                    let app = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }
        }
    }

    /// Report performance metrics with structured tracing
    #[instrument(
        level = "info",
        name = "performance_metrics_report",
        fields(
            report_type = "periodic",
            uptime = Empty,
            tasks_per_second = Empty,
            actions_per_second = Empty,
            avg_render_fps = Empty,
        )
    )]
    fn report_performance_metrics(&self, loop_iteration: u64) {
        let uptime = self.start_time.elapsed();
        let uptime_secs = uptime.as_secs_f64();

        let tasks_per_sec = self.tasks_processed as f64 / uptime_secs;
        let actions_per_sec = self.actions_processed as f64 / uptime_secs;
        // Note: Render FPS tracking moved to main.rs

        tracing::Span::current()
            .record("uptime", tracing::field::debug(uptime))
            .record("tasks_per_second", tasks_per_sec)
            .record("actions_per_second", actions_per_sec);

        info!(
            loop_iteration = loop_iteration,
            uptime = ?uptime,
            tasks_processed = self.tasks_processed,
            actions_processed = self.actions_processed,
            // render_frames tracking moved to main.rs
            tasks_per_sec = tasks_per_sec,
            actions_per_sec = actions_per_sec,
            "Performance metrics report"
        );

        // Log memory usage if available
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        if let Some(memory_kb) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = memory_kb.parse::<u64>() {
                                debug!(
                                    memory_usage_kb = kb,
                                    memory_usage_mb = kb / 1024,
                                    "Current memory usage"
                                );
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    /// Report final metrics on shutdown
    #[allow(dead_code)]
    #[instrument(
        level = "info",
        name = "final_metrics_report",
        fields(
            report_type = "final",
            total_uptime = Empty,
            final_task_count = Empty,
            final_action_count = Empty,
            final_render_count = Empty,
        )
    )]
    fn report_final_metrics(&self, final_iteration: u64) {
        let total_uptime = self.start_time.elapsed();

        tracing::Span::current()
            .record("total_uptime", tracing::field::debug(total_uptime))
            .record("final_task_count", self.tasks_processed)
            .record("final_action_count", self.actions_processed);
        // final_render_count tracking moved to main.rs

        info!(
            final_iteration = final_iteration,
            total_uptime = ?total_uptime,
            total_tasks = self.tasks_processed,
            total_actions = self.actions_processed,
            // total_renders tracking moved to main.rs
            avg_iteration_time = ?total_uptime.checked_div(final_iteration as u32),
            "Event loop final performance summary"
        );
    }

    /// Get detailed performance metrics
    #[allow(dead_code)]
    #[instrument(level = "trace", name = "get_performance_metrics")]
    fn get_performance_metrics(&self) -> std::collections::HashMap<String, String> {
        let uptime = self.start_time.elapsed();
        let uptime_secs = uptime.as_secs_f64();

        let mut metrics = std::collections::HashMap::new();

        metrics.insert("uptime_seconds".to_string(), uptime_secs.to_string());
        metrics.insert(
            "tasks_processed".to_string(),
            self.tasks_processed.to_string(),
        );
        metrics.insert(
            "actions_processed".to_string(),
            self.actions_processed.to_string(),
        );
        metrics.insert(
            "render_frames".to_string(),
            "0".to_string(), // render_frame_count removed
        );

        if uptime_secs > 0.0 {
            metrics.insert(
                "tasks_per_second".to_string(),
                (self.tasks_processed as f64 / uptime_secs).to_string(),
            );
            metrics.insert(
                "actions_per_second".to_string(),
                (self.actions_processed as f64 / uptime_secs).to_string(),
            );
            metrics.insert(
                "render_fps".to_string(),
                "0".to_string(), // render FPS calculation removed
            );
        }

        trace!(
            marker = "PERFORMANCE_METRICS_COLLECTED",
            operation_type = "metrics",
            metrics_count = metrics.len(),
            "Performance metrics collected"
        );

        metrics
    }

    // Legacy compatibility methods with tracing

    /// Get next task result (legacy compatibility)
    #[instrument(level = "trace", name = "next_task_result")]
    pub async fn next_task_result(&mut self) -> Option<TaskResult> {
        let result = self.task_rx.recv().await;

        if let Some(ref task_result) = result {
            trace!(marker = "TASK_RESULT_RECEIVED_LEGACY", operation_type = "task_management",
                task_result = ?task_result,
                "Task result received via legacy method"
            );
        } else {
            trace!("No task result available via legacy method");
        }

        result
    }

    /// Get shutdown handle
    #[instrument(level = "trace", name = "shutdown_handle")]
    pub fn shutdown_handle(&self) -> Arc<Notify> {
        trace!(
            handle_addr = format!("{:p}", self.shutdown.as_ref()),
            "Shutdown handle retrieved"
        );

        self.shutdown.clone()
    }

    /// Initiate shutdown
    #[instrument(level = "info", name = "shutdown_initiate")]
    pub fn shutdown(&self) {
        info!(
            uptime = ?self.start_time.elapsed(),
            tasks_processed = self.tasks_processed,
            actions_processed = self.actions_processed,
            "Shutdown signal sent to event loop"
        );

        self.shutdown.notify_one();
    }

    /// Get metrics snapshot (legacy compatibility)
    #[instrument(level = "trace", name = "metrics_snapshot")]
    pub fn metrics(&self) -> MetricsSnapshot {
        let uptime = self.start_time.elapsed();
        let avg_duration = if self.tasks_processed > 0 {
            uptime / self.tasks_processed as u32
        } else {
            Duration::ZERO
        };

        let snapshot = MetricsSnapshot {
            tasks: self.tasks_processed,
            actions: self.actions_processed,
            total: uptime,
            avg: avg_duration,
            last: self.start_time,
            queued: 0, // No queue in this implementation
        };

        trace!(marker = "METRICS_SNAPSHOT_CREATED", operation_type = "metrics",
            snapshot = ?snapshot,
            "Metrics snapshot created"
        );

        snapshot
    }

    /// Snapshot metrics (legacy alias)
    pub fn snapshot_metrics(&self) -> MetricsSnapshot {
        self.metrics()
    }
}
