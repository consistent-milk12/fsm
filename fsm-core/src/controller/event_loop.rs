// fsm-core/src/controller/event_loop.rs
// Complete event loop with proper task handling and terminal integration

use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::{
    sync::{Notify, mpsc},
    time::{MissedTickBehavior, interval},
};
use tracing::{debug, info};

use crate::{
    UIState,
    controller::{
        action_dispatcher::{ActionDispatcher, ActionSource},
        actions::{Action, OperationId},
        state_coordinator::StateCoordinator,
    },
    error::AppError,
    fs::object_info::ObjectInfo,
    model::ui_state::RedrawFlag,
};

/// Task results from background operations - matches dir_scanner.rs TaskResult
#[derive(Debug, Clone)]
pub enum TaskResult {
    DirectoryLoad {
        task_id: u64,
        path: PathBuf,
        result: Result<Vec<ObjectInfo>, AppError>,
        exec: Duration,
    },
    FileOperation {
        op_id: OperationId,
        op_kind: FileOperationType,
        result: Result<(), AppError>,
        exec: Duration,
    },
    SearchDone {
        task_id: u64,
        query: String,
        results: Vec<ObjectInfo>,
        exec: Duration,
    },
    ContentSearchDone {
        task_id: u64,
        query: String,
        results: Vec<String>,
        exec: Duration,
    },
    Progress {
        task_id: u64,
        pct: f32,
        msg: Option<String>,
    },
    Clipboard {
        op_id: OperationId,
        op_kind: String,
        result: Result<u32, AppError>,
        exec: Duration,
    },
    Generic {
        task_id: u64,
        result: Result<(), AppError>,
        msg: Option<String>,
        exec: Duration,
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

/// Performance metrics
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
pub struct EventLoop {
    state_coordinator: Arc<StateCoordinator>,
    action_dispatcher: ActionDispatcher,
    task_rx: mpsc::UnboundedReceiver<TaskResult>,
    shutdown: Arc<Notify>,

    // Metrics
    tasks_processed: u64,
    actions_processed: u64,
    start_time: Instant,
}

impl EventLoop {
    pub fn new(
        task_rx: mpsc::UnboundedReceiver<TaskResult>,
        state_coordinator: Arc<StateCoordinator>,
    ) -> Self {
        let action_dispatcher =
            ActionDispatcher::new(state_coordinator.clone(), mpsc::unbounded_channel().0);

        Self {
            state_coordinator,
            action_dispatcher,
            task_rx,
            shutdown: Arc::new(Notify::new()),
            tasks_processed: 0,
            actions_processed: 0,
            start_time: Instant::now(),
        }
    }

    /// Main event processing loop
    pub async fn run(&mut self) -> Result<()> {
        info!("Starting event loop");

        let mut event_stream = EventStream::new();
        let mut render_timer = interval(Duration::from_millis(16)); // 60 FPS
        render_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = self.shutdown.notified() => {
                    info!("Shutdown requested");
                    break;
                }

                maybe_event = event_stream.next() => {
                    if let Some(Ok(event)) = maybe_event {
                        if let Some(action) = self.process_terminal_event(event) {
                            if !self.dispatch_action(action, ActionSource::Keyboard).await? {
                                break;
                            }
                        }
                    }
                }

                Some(task_result) = self.task_rx.recv() => {
                    self.handle_task_result(task_result).await;
                    self.tasks_processed += 1;
                }

                _ = render_timer.tick() => {
                    // Rendering handled externally
                }
            }
        }

        info!("Event loop completed");
        Ok(())
    }

    /// Process terminal events
    fn process_terminal_event(&self, event: TerminalEvent) -> Option<Action> {
        match event {
            TerminalEvent::Key(key) => self.process_key_event(key),
            TerminalEvent::Resize(w, h) => Some(Action::Resize(w, h)),
            _ => None,
        }
    }

    /// Map key events to actions
    fn process_key_event(&self, key: KeyEvent) -> Option<Action> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),

            // Navigation
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                Some(Action::MoveSelectionUp)
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                Some(Action::MoveSelectionDown)
            }
            (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                Some(Action::GoToParent)
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                Some(Action::EnterSelected)
            }
            (KeyCode::Enter, _) => Some(Action::EnterSelected),
            (KeyCode::Backspace, _) => Some(Action::GoToParent),
            (KeyCode::PageUp, _) => Some(Action::PageUp),
            (KeyCode::PageDown, _) => Some(Action::PageDown),
            (KeyCode::Home, _) => Some(Action::SelectFirst),
            (KeyCode::End, _) => Some(Action::SelectLast),

            // File operations
            (KeyCode::Char('c'), KeyModifiers::NONE) => self.get_selected_path().map(Action::Copy),
            (KeyCode::Char('x'), KeyModifiers::NONE) => self.get_selected_path().map(Action::Cut),
            (KeyCode::Char('v'), KeyModifiers::NONE) => Some(Action::Paste),
            (KeyCode::Delete, _) => Some(Action::Delete),
            (KeyCode::Char('n'), KeyModifiers::NONE) => Some(Action::CreateFile),
            (KeyCode::Char('m'), KeyModifiers::NONE) => Some(Action::CreateDirectory),

            // UI controls
            (KeyCode::F(1), _) | (KeyCode::Char('?'), KeyModifiers::NONE) => {
                Some(Action::ToggleHelp)
            }
            (KeyCode::Char('/'), KeyModifiers::NONE) => Some(Action::ToggleFileNameSearch),
            (KeyCode::Char(':'), KeyModifiers::NONE) => Some(Action::EnterCommandMode),
            (KeyCode::Esc, _) => Some(Action::CloseOverlay),
            (KeyCode::F(5), _) => Some(Action::ReloadDirectory),
            (KeyCode::Tab, _) => Some(Action::ToggleClipboardOverlay),

            _ => None,
        }
    }

    /// Get selected file path
    fn get_selected_path(&self) -> Option<PathBuf> {
        let fs = self.state_coordinator.fs_state();
        let pane = fs.active_pane();
        let idx = pane.selected.load(Ordering::Relaxed);
        pane.entries.get(idx).map(|e| e.path.clone())
    }

    /// Dispatch action to handler
    async fn dispatch_action(&mut self, action: Action, source: ActionSource) -> Result<bool> {
        debug!("Dispatching: {:?}", action);
        let result = self.action_dispatcher.dispatch(action, source).await;
        self.actions_processed += 1;
        Ok(result)
    }

    /// Handle background task results - matches original event_loop patterns
    async fn handle_task_result(&self, result: TaskResult) {
        match result {
            TaskResult::DirectoryLoad {
                task_id,
                path,
                result,
                exec: _,
            } => match result {
                Ok(entries) => {
                    debug!("Directory load complete: {} entries", entries.len());

                    {
                        let mut fs = self.state_coordinator.fs_state();
                        let pane = fs.active_pane_mut();
                        if pane.cwd == path {
                            pane.set_entries(entries);
                            pane.is_loading.store(false, Ordering::Relaxed);
                        }
                    }

                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.success(&format!("Loaded {}", path.display()));
                        }));

                    {
                        let app = self.state_coordinator.app_state();
                        app.complete_task(task_id);
                    }

                    self.state_coordinator.request_redraw(RedrawFlag::All);
                }
                Err(e) => {
                    {
                        let mut fs = self.state_coordinator.fs_state();
                        let pane = fs.active_pane_mut();
                        if pane.cwd == path {
                            pane.is_loading.store(false, Ordering::Relaxed);
                        }
                    }

                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.error(&format!("Load failed: {}", e));
                        }));

                    {
                        let app = self.state_coordinator.app_state();
                        app.complete_task(task_id);
                    }
                }
            },

            TaskResult::SearchDone {
                task_id,
                query,
                results,
                exec: _,
            } => {
                debug!("Search '{}' found {} results", query, results.len());

                {
                    let mut fs = self.state_coordinator.fs_state();
                    fs.active_pane_mut().search_results = results.clone();
                }

                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.info(&format!("'{}' → {} results", query, results.len()));
                    }));

                {
                    let app = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }

            TaskResult::ContentSearchDone {
                task_id,
                query,
                results,
                exec: _,
            } => {
                debug!("Content search '{}' found {} results", query, results.len());

                self.state_coordinator
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.info(&format!("Content search: {} files", results.len()));
                    }));

                {
                    let app = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }

            TaskResult::FileOperation {
                op_id: _,
                op_kind,
                result,
                exec: _,
            } => match result {
                Ok(()) => {
                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.success(&format!("{:?} completed", op_kind));
                        }));
                    self.state_coordinator.request_redraw(RedrawFlag::Main);
                }
                Err(e) => {
                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.error(&format!("{:?} failed: {}", op_kind, e));
                        }));
                }
            },

            TaskResult::Clipboard {
                op_id: _,
                op_kind,
                result,
                exec: _,
            } => match result {
                Ok(count) => {
                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.success(&format!("{} ok ({})", op_kind, count));
                        }));
                }
                Err(e) => {
                    self.state_coordinator
                        .update_ui_state(Box::new(move |ui: &mut UIState| {
                            ui.error(&format!("Clipboard {} failed: {}", op_kind, e));
                        }));
                }
            },

            TaskResult::Progress {
                task_id: _,
                pct,
                msg,
            } => {
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
                exec: _,
            } => {
                match result {
                    Ok(()) => {
                        if let Some(m) = msg {
                            self.state_coordinator.update_ui_state(Box::new(
                                move |ui: &mut UIState| {
                                    ui.success(&m);
                                },
                            ));
                        }
                    }
                    Err(e) => {
                        self.state_coordinator.update_ui_state(Box::new(
                            move |ui: &mut UIState| {
                                ui.error(&format!("Task failed: {}", e));
                            },
                        ));
                    }
                }

                {
                    let app = self.state_coordinator.app_state();
                    app.complete_task(task_id);
                }
            }
        }
    }

    // Legacy compatibility methods
    pub async fn next_task_result(&mut self) -> Option<TaskResult> {
        self.task_rx.recv().await
    }

    pub fn shutdown_handle(&self) -> Arc<Notify> {
        self.shutdown.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        let uptime = self.start_time.elapsed();
        let avg_duration = if self.tasks_processed > 0 {
            uptime / self.tasks_processed as u32
        } else {
            Duration::ZERO
        };

        MetricsSnapshot {
            tasks: self.tasks_processed,
            actions: self.actions_processed,
            total: uptime,
            avg: avg_duration,
            last: self.start_time,
            queued: 0, // No queue in this implementation
        }
    }

    pub fn snapshot_metrics(&self) -> MetricsSnapshot {
        self.metrics()
    }
}
