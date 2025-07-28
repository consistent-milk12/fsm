//! src/controller/event_loop.rs
//! ============================================================
//! Task / action multiplexer that feeds the reducer.  It is
//! fully aware of the new slim UIState design and writes search
//! results directly into `PaneState`.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, info};

use crate::{
    controller::{
        actions::{Action, OperationId},
        state_coordinator::StateCoordinator,
    },
    error::AppError,
    fs::object_info::ObjectInfo,
    model::{fs_state::PaneState, ui_state::RedrawFlag},
};

/// ---------- task result types -------------------------------------------------
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

/// ---------- event-loop struct --------------------------------------------------
pub struct EventLoop {
    coord: Arc<StateCoordinator>,
    task_rx: UnboundedReceiver<TaskResult>,
    action_rx: UnboundedReceiver<Action>,

    pending: VecDeque<Action>,
    metrics: Metrics,
    cfg: Config,
}

/// ---------- aux structs --------------------------------------------------------
#[derive(Debug)]
struct Metrics {
    tasks: u64,
    actions: u64,
    total: Duration,
    last: Instant,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub max_batch: usize,
    pub task_timeout: Duration,
    pub min_iter: Duration,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_batch: 10,
            task_timeout: Duration::from_secs(30),
            min_iter: Duration::from_millis(16),
        }
    }
}

/// ==============================================================================
/// ctor
/// ==============================================================================
impl EventLoop {
    pub fn new(
        task_rx: UnboundedReceiver<TaskResult>,
        action_rx: UnboundedReceiver<Action>,
        coord: Arc<StateCoordinator>,
    ) -> Self {
        info!("~> event-loop initialised");
        Self {
            coord,
            task_rx,
            action_rx,
            pending: VecDeque::with_capacity(32),
            metrics: Metrics {
                tasks: 0,
                actions: 0,
                total: Duration::ZERO,
                last: Instant::now(),
            },
            cfg: Config::default(),
        }
    }
}

/// ==============================================================================
/// public API
/// ==============================================================================
impl EventLoop {
    /// Blocking until *one* action is ready.
    pub async fn next_action(&mut self) -> Action {
        if let Some(a) = self.pending.pop_front() {
            return a;
        }

        let start = Instant::now();
        tokio::select! {
            Some(t) = self.task_rx.recv() => {
                self.metrics.tasks += 1;
                self.queue(self.handle_task(t).await);
            }
            Some(a) = self.action_rx.recv() => {
                self.metrics.actions += 1;
                return a;
            }
            _ = tokio::time::sleep(self.cfg.min_iter) => { /* idle tick */ }
        }

        self.metrics.total += start.elapsed();
        self.metrics.last = Instant::now();
        self.pending.pop_front().unwrap_or(Action::Tick)
    }

    pub fn snapshot_metrics(&self) -> MetricsSnap {
        MetricsSnap {
            tasks: self.metrics.tasks,
            actions: self.metrics.actions,
            total: self.metrics.total,
            avg: if self.metrics.tasks > 0 {
                self.metrics.total / self.metrics.tasks as u32
            } else {
                Duration::ZERO
            },
            last: self.metrics.last,
            queued: self.pending.len(),
        }
    }
}

/// ---------- task handling ------------------------------------------------------
impl EventLoop {
    async fn handle_task(&self, t: TaskResult) -> Vec<Action> {
        match t {
            TaskResult::DirectoryLoad {
                task_id,
                path,
                result,
                exec,
            } => self.on_dir_loaded(task_id, path, result, exec).await,
            TaskResult::FileOperation {
                op_id,
                op_kind,
                result,
                exec,
            } => self.on_file_op(op_id, op_kind, result, exec).await,
            TaskResult::SearchDone {
                task_id,
                query,
                results,
                exec,
            } => self.on_fname_search(task_id, query, results, exec).await,
            TaskResult::ContentSearchDone {
                task_id,
                query,
                results,
                exec,
            } => self.on_content_search(task_id, query, results, exec).await,
            TaskResult::Clipboard {
                op_id,
                op_kind,
                result,
                exec,
            } => self.on_clipboard(op_id, op_kind, result, exec).await,
            TaskResult::Progress { task_id, pct, msg } => self.on_progress(task_id, pct, msg).await,
            TaskResult::Generic {
                task_id,
                result,
                msg,
                exec,
            } => self.on_generic(task_id, result, msg, exec).await,
        }
    }

    async fn on_dir_loaded(
        &self,
        id: u64,
        path: PathBuf,
        res: Result<Vec<ObjectInfo>, AppError>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!("dir-load {} ({}ms)", path.display(), exec.as_millis());
        match res {
            Ok(entries) => {
                // update pane -------------------------------------------------
                {
                    let mut fs = self.coord.fs_state();
                    let p: &mut PaneState = fs.active_pane_mut();
                    if p.cwd == path {
                        p.set_entries(entries);
                    }
                }
                // user feedback ----------------------------------------------
                self.coord.update_ui_state(|ui| {
                    ui.success(format!("Loaded {}", path.display()));
                    ui.request_redraw(RedrawFlag::All);
                });
                // mark task done
                self.coord.app_state().complete_task(id, None);
                vec![Action::ReloadDirectory]
            }
            Err(e) => {
                self.error(format!("Load {} failed: {}", path.display(), e));
                self.coord
                    .app_state()
                    .complete_task(id, Some(e.to_string().into()));
                vec![]
            }
        }
    }

    #[allow(unused)]
    async fn on_file_op(
        &self,
        op_id: OperationId,
        kind: FileOperationType,
        res: Result<(), AppError>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!("file-op {:?} ({}ms)", kind, exec.as_millis());
        match res {
            Ok(_) => {
                self.coord.update_ui_state(|ui| {
                    ui.success(format!("{kind:?} finished"));
                });
                vec![Action::ReloadDirectory]
            }
            Err(e) => {
                self.error(format!("{kind:?} failed: {e}"));
                vec![]
            }
        }
    }

    async fn on_clipboard(
        &self,
        _id: OperationId,
        kind: String,
        res: Result<u32, AppError>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!("clipboard {kind} ({}ms)", exec.as_millis());
        match res {
            Ok(n) => self
                .coord
                .update_ui_state(|ui| ui.success(format!("{kind} ok ({n})"))),
            Err(e) => self.error(format!("clipboard {kind} failed: {e}")),
        }
        vec![Action::ReloadDirectory]
    }

    async fn on_fname_search(
        &self,
        id: u64,
        q: String,
        hits: Vec<ObjectInfo>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!(
            "search \"{q}\" {} hit(s) ({}ms)",
            hits.len(),
            exec.as_millis()
        );
        {
            let mut fs = self.coord.fs_state();
            fs.active_pane_mut().search_results = hits.clone();
        }
        self.coord.update_ui_state(|ui| {
            ui.info(format!("“{q}” → {} result(s)", hits.len()));
        });
        self.coord.app_state().complete_task(id, None);
        vec![Action::ShowFilenameSearchResults(hits)]
    }

    async fn on_content_search(
        &self,
        id: u64,
        q: String,
        hits: Vec<String>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!(
            "content-search \"{q}\" {} hit(s) ({}ms)",
            hits.len(),
            exec.as_millis()
        );
        self.coord.update_ui_state(|ui| {
            ui.info(format!("Content search done – {} file(s)", hits.len()));
        });
        self.coord.app_state().complete_task(id, None);
        vec![Action::ShowRichSearchResults(hits)]
    }

    async fn on_progress(&self, id: u64, pct: f32, msg: Option<String>) -> Vec<Action> {
        self.coord.update_ui_state(|ui| {
            if let Some(ref mut l) = ui.loading {
                l.set_progress(pct);
                if let Some(m) = &msg {
                    l.message = m.clone().into();
                }
            }
        });
        self.coord
            .update_task_progress(id.to_string(), (pct * 100.0) as u64, 10_000, msg);
        vec![]
    }

    async fn on_generic(
        &self,
        id: u64,
        res: Result<(), AppError>,
        msg: Option<String>,
        exec: Duration,
    ) -> Vec<Action> {
        debug!("generic task {id} ({}ms)", exec.as_millis());
        match res {
            Ok(_) => {
                if let Some(m) = msg {
                    self.coord.update_ui_state(|ui| ui.success(m));
                }
            }
            Err(e) => self.error(format!("task {id} failed: {e}")),
        }
        self.coord.app_state().complete_task(id, None);
        vec![]
    }

    fn error(&self, m: String) {
        self.coord.update_ui_state(|ui| ui.error(m));
    }

    fn queue(&mut self, v: Vec<Action>) {
        self.pending.extend(v);
    }
}

/// ---------- metrics snapshot ---------------------------------------------------
#[derive(Debug, Clone)]
pub struct MetricsSnap {
    pub tasks: u64,
    pub actions: u64,
    pub total: Duration,
    pub avg: Duration,
    pub last: Instant,
    pub queued: usize,
}
