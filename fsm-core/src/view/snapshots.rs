//! src/view/snapshots.rs
//! ============================================================
//! Immutable *data-transfer* structs harvested from the
//! live states (`UIState`, `FSState`, `PaneState`, …) **after**
//! locks are taken, and consumed by renderer widgets **after**
//! all locks have been released.
//!
//! Because every snapshot is `Clone + Send + Sync` and contains
//! *no interior mutability*, it is impossible for any widget or
//! overlay to re-enter a `Mutex` / `RwLock` while drawing – the
//! core guarantee of the new render pipeline.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use compact_str::CompactString;
use tracing::{debug, instrument, trace, warn};

use crate::UIState;
use crate::controller::actions::{InputPromptType, OperationId};
use crate::fs::object_info::ObjectInfo;
use crate::model::UIMode;
use crate::model::fs_state::SearchMode;
use crate::model::fs_state::{EntryFilter, EntrySort, PaneState};
use crate::model::ui_state::{LoadingState, Notification, UIOverlay};

/// Immutable slice of the live UI state, captured once per frame
/// and handed to widgets after *all* locks have been released.
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    /// Current modal overlay (or `None`)
    pub overlay: UIOverlay,

    /// Whether the clipboard overlay is showing
    pub clipboard_active: bool,

    /// Optional toast / banner
    pub notification: Option<Notification>,

    /// Bit-flags originally requested by the logic layer
    pub redraw_flags: u32,

    /// Optional long-running loading indicator
    pub loading: Option<LoadingState>,

    /// Prompt type if the prompt overlay is active
    pub prompt_type: Option<InputPromptType>,

    /// Search mode (filename / content / advanced)
    pub search_mode: SearchMode,

    /// Raw query string for search overlays
    pub search_query: Option<CompactString>,

    /// Row highlighted in the clipboard overlay
    pub selected_clipboard_item_idx: usize,

    /// Current UI mode
    pub mode: UIMode,

    /// Frame count for performance monitoring
    pub frame_count: u64,

    /// Whether system monitor overlay is active
    pub show_system_monitor: bool,

    /// Whether process monitor overlay is active  
    pub show_process_monitor: bool,

    /// Selected process index for process monitor
    pub selected_process_idx: Option<usize>,
}

impl From<&UIState> for UiSnapshot {
    #[instrument(level = "trace", skip_all, fields(overlay = ?src.overlay, frame_count = src.frame_count.load(Ordering::Relaxed)))]
    fn from(src: &UIState) -> Self {
        trace!("Creating UI snapshot from UIState");
        Self {
            overlay: src.overlay,
            clipboard_active: src.clipboard_overlay_active,
            notification: src.notification.clone(),
            redraw_flags: src.redraw_flags.load(Ordering::Relaxed),
            loading: src.loading.clone(),
            prompt_type: src.input_prompt_type.clone(),
            search_mode: src.search_mode,
            search_query: src.search_query.clone(),
            selected_clipboard_item_idx: src.selected_clipboard_item_idx,
            mode: src.mode,
            frame_count: src.frame_count.load(Ordering::Relaxed),
            show_system_monitor: src.show_system_monitor,
            show_process_monitor: src.show_process_monitor,
            selected_process_idx: if src.selected_process_idx > 0 { Some(src.selected_process_idx) } else { None },
        }
    }
}

// ------------------------------------------------------------
// SearchSnapshot
// ------------------------------------------------------------

/// Captures the dynamic state that the *search* overlay needs
/// (user input + live results). Building this snapshot avoids
/// holding the FS or UI locks while the overlay renders a potentially
/// large table of matches.
#[derive(Debug, Clone)]
pub struct SearchSnapshot {
    /// The textual query the user typed
    pub query: CompactString,

    /// Caret position inside `query`
    pub cursor: usize,

    /// Immutable array of search hits
    pub results: Arc<[ObjectInfo]>,

    /// Which kind of search we are running
    pub mode: SearchMode,

    /// Whether search is currently running
    pub is_searching: bool,

    /// Total matches found
    pub total_matches: usize,

    /// Currently selected result index
    pub selected_idx: usize,
}

impl SearchSnapshot {
    /// Builder extracted from a locked `UIState` and `PaneState`
    #[instrument(level = "trace", skip_all, fields(search_mode = ?ui.search_mode))]
    pub fn from_states(ui: &UIState, pane: &PaneState) -> Option<Self> {
        trace!("Creating search snapshot from states");
        let query: CompactString = ui.search_query.clone()?;
        let results: Arc<[ObjectInfo]> = if ui.search_mode == SearchMode::None {
            trace!("No search mode - empty results");
            Arc::from(vec![])
        } else {
            let result_count = pane.search_results.len();
            trace!("Search results: {} items", result_count);
            Arc::from(pane.search_results.clone().into_boxed_slice().into_vec())
        };

        let query_len = query.len();

        Some(Self {
            query,
            cursor: query_len, // cursor is at end by default
            results: results.clone(),
            mode: ui.search_mode,
            is_searching: pane.is_loading.load(Ordering::Relaxed),
            total_matches: results.len(),
            selected_idx: 0,
        })
    }
}

// ------------------------------------------------------------
// PromptSnapshot
// ------------------------------------------------------------

/// Contains everything the *prompt / command-line* overlay
/// needs to paint its single-line input widget.
#[derive(Debug, Clone)]
pub struct PromptSnapshot {
    /// Prompt kind (`:` command, `/` search, etc.)
    pub prompt_type: InputPromptType,

    /// Current text in the input buffer
    pub buffer: CompactString,

    /// Cursor index in `buffer`
    pub cursor: usize,

    /// Command history (oldest → newest)
    pub history: Arc<[CompactString]>,

    /// Current history position (for up/down navigation)
    pub history_idx: Option<usize>,

    /// Whether input validation failed
    pub has_error: bool,

    /// Error message if validation failed
    pub error_msg: Option<CompactString>,
}

impl PromptSnapshot {
    /// Safe extractor – returns `None` if prompt overlay inactive
    #[instrument(level = "trace", skip_all, fields(prompt_active = ui.input_prompt_type.is_some()))]
    pub fn from_ui(ui: &crate::model::ui_state::UIState) -> Option<Self> {
        trace!("Creating prompt snapshot from UI state");
        let prompt_type = ui.input_prompt_type.clone()?;
        debug!("Prompt type: {:?}", prompt_type);
        Some(Self {
            prompt_type,
            buffer: ui.prompt_buffer.clone(),
            cursor: ui.prompt_cursor,
            history: Arc::from(ui.command_history.clone().into_boxed_slice()),
            history_idx: None,
            has_error: false,
            error_msg: None,
        })
    }
}

// ------------------------------------------------------------
// OpsProgressSnapshot
// ------------------------------------------------------------

/// A very small DTO used by the *file-operation progress* bar
/// at the bottom-right of the screen.
#[derive(Debug, Clone)]
pub struct OpsProgressSnapshot {
    /// Average progress 0.0 – 1.0 across all active operations
    pub average: f32,

    /// Number of parallel operations
    pub count: usize,

    /// Current operation details
    pub operations: Arc<[OperationSnapshot]>,

    /// Whether any operation is paused
    pub has_paused: bool,

    /// Total bytes transferred across all operations
    pub total_bytes: u64,

    /// Total bytes to transfer
    pub total_size: u64,
}

impl OpsProgressSnapshot {
    /// Create from operation details
    #[instrument(level = "trace", skip_all, fields(operation_count = operations.len()))]
    pub fn from_operations(operations: &[(OperationId, f32, u64, u64)]) -> Self {
        trace!("Creating operations progress snapshot");
        let count = operations.len();
        let average = if count > 0 {
            let avg = operations
                .iter()
                .map(|(_, progress, _, _)| progress)
                .sum::<f32>()
                / count as f32;
            debug!(
                "Average progress across {} operations: {:.2}%",
                count,
                avg * 100.0
            );
            avg
        } else {
            trace!("No active operations");
            0.0
        };

        let total_bytes = operations.iter().map(|(_, _, bytes, _)| bytes).sum();
        let total_size = operations.iter().map(|(_, _, _, size)| size).sum();

        let op_snapshots: Vec<OperationSnapshot> = operations
            .iter()
            .map(|(id, progress, bytes, size)| OperationSnapshot {
                id: id.clone(),
                progress: *progress,
                bytes_transferred: *bytes,
                total_bytes: *size,
                status: if *progress >= 1.0 {
                    OperationStatus::Complete
                } else {
                    OperationStatus::InProgress
                },
            })
            .collect();

        Self {
            average,
            count,
            operations: Arc::from(op_snapshots.into_boxed_slice()),
            has_paused: false,
            total_bytes,
            total_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OperationSnapshot {
    pub id: OperationId,
    pub progress: f32,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub status: OperationStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperationStatus {
    InProgress,
    Paused,
    Complete,
    Failed,
}

// ------------------------------------------------------------
// PaneSnapshot
// ------------------------------------------------------------

/// An immutable view onto a `PaneState`. Unlike `PaneState`
/// it owns no atomics – just plain data – so the renderer can
/// inspect it freely without worrying about concurrent mutation.
#[derive(Debug, Clone)]
pub struct PaneSnapshot {
    /// Working directory of the pane
    pub cwd: PathBuf,

    /// Already sorted + filtered entries
    pub entries: Arc<[ObjectInfo]>,

    /// Which row is currently selected
    pub selected: usize,

    /// Current scroll offset (first visible row)
    pub scroll_offset: usize,

    /// Height of the viewport in rows
    pub viewport_height: usize,

    /// Sorting mode applied to `entries`
    pub sort: EntrySort,

    /// Filter applied when `entries` were generated
    pub filter: EntryFilter,

    /// Whether a directory scan is still in progress
    pub is_loading: bool,
}

impl From<&PaneState> for PaneSnapshot {
    #[instrument(level = "trace", skip_all, fields(cwd = ?pane.cwd, entry_count = pane.entries.len()))]
    fn from(pane: &PaneState) -> Self {
        trace!("Creating pane snapshot from PaneState");
        Self {
            cwd: pane.cwd.clone(),
            entries: Arc::from(pane.entries.clone().into_boxed_slice()),
            selected: pane.selected.load(Ordering::Relaxed),
            scroll_offset: pane.scroll_offset.load(Ordering::Relaxed),
            viewport_height: pane.viewport_height.load(Ordering::Relaxed),
            sort: pane.sort,
            filter: pane.filter.clone(),
            is_loading: pane.is_loading.load(Ordering::Relaxed),
        }
    }
}

// ------------------------------------------------------------
// ClipboardSnapshot
// ------------------------------------------------------------

/// Immutable view of clipboard state for overlay rendering
#[derive(Debug, Clone)]
pub struct ClipboardSnapshot {
    /// Clipboard items to display
    pub items: Arc<[ClipboardItemSnapshot]>,

    /// Currently selected item index
    pub selected_idx: usize,

    /// Whether clipboard is empty
    pub is_empty: bool,

    /// Total items in clipboard
    pub item_count: usize,

    /// Total size of clipboard items
    pub total_size: u64,
}

#[derive(Debug, Clone)]
pub struct ClipboardItemSnapshot {
    /// Item ID for operations
    pub id: u64,

    /// Display path
    pub path: PathBuf,

    /// Item size in bytes
    pub size: u64,

    /// Whether item is cut (vs copied)
    pub is_cut: bool,

    /// Item type (file/directory)
    pub item_type: ClipboardItemType,

    /// When item was added to clipboard
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardItemType {
    File,
    Directory,
    Symlink,
}

impl ClipboardSnapshot {
    /// Create empty clipboard snapshot
    #[instrument(level = "trace")]
    pub fn empty() -> Self {
        trace!("Creating empty clipboard snapshot");
        Self {
            items: Arc::from([]),
            selected_idx: 0,
            is_empty: true,
            item_count: 0,
            total_size: 0,
        }
    }

    /// Create from clipboard items
    #[instrument(level = "trace", skip_all, fields(item_count = items.len()))]
    pub fn from_items(items: Vec<ClipboardItemSnapshot>, selected_idx: usize) -> Self {
        trace!("Creating clipboard snapshot from {} items", items.len());
        let item_count = items.len();
        let is_empty = item_count == 0;
        let total_size = items.iter().map(|item| item.size).sum();

        Self {
            items: Arc::from(items.into_boxed_slice()),
            selected_idx: if is_empty {
                0
            } else {
                selected_idx.min(item_count - 1)
            },
            is_empty,
            item_count,
            total_size,
        }
    }
}

// ------------------------------------------------------------
// TaskSnapshot
// ------------------------------------------------------------

/// Immutable view of background task status
#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    /// Active tasks
    pub tasks: Arc<[TaskInfo]>,

    /// Total task count
    pub total_count: usize,

    /// Number of running tasks
    pub running_count: usize,

    /// Number of completed tasks
    pub completed_count: usize,

    /// Number of failed tasks
    pub failed_count: usize,
}

#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Task ID
    pub id: u64,

    /// Task type/name
    pub task_type: CompactString,

    /// Current status
    pub status: TaskStatus,

    /// Progress (0.0 - 1.0)
    pub progress: f32,

    /// Task start time
    pub started_at: std::time::Instant,

    /// Task duration (if completed)
    pub duration: Option<Duration>,

    /// Associated operation ID (if any)
    pub operation_id: Option<OperationId>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed(CompactString),
    Cancelled,
}

impl TaskSnapshot {
    /// Create from task collection
    #[instrument(level = "trace", skip_all, fields(task_count = tasks.len()))]
    pub fn from_tasks(tasks: Vec<TaskInfo>) -> Self {
        trace!("Creating task snapshot from {} tasks", tasks.len());
        let total_count = tasks.len();
        let running_count = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .count();
        let completed_count = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let failed_count = tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Failed(_)))
            .count();

        Self {
            tasks: Arc::from(tasks.into_boxed_slice()),
            total_count,
            running_count,
            completed_count,
            failed_count,
        }
    }

    /// Create empty task snapshot
    #[instrument(level = "trace")]
    pub fn empty() -> Self {
        trace!("Creating empty task snapshot");
        Self {
            tasks: Arc::from([]),
            total_count: 0,
            running_count: 0,
            completed_count: 0,
            failed_count: 0,
        }
    }
}

// ------------------------------------------------------------
// AppSnapshot
// ------------------------------------------------------------

/// Complete application state snapshot combining all subsystems
#[derive(Debug, Clone)]
pub struct AppSnapshot {
    /// UI state snapshot
    pub ui: UiSnapshot,

    /// Current pane snapshot
    pub pane: PaneSnapshot,

    /// Search state (if active)
    pub search: Option<SearchSnapshot>,

    /// Prompt state (if active)
    pub prompt: Option<PromptSnapshot>,

    /// File operation progress
    pub progress: OpsProgressSnapshot,

    /// Clipboard state
    pub clipboard: ClipboardSnapshot,

    /// Background tasks
    pub tasks: TaskSnapshot,

    /// Snapshot timestamp
    pub timestamp: std::time::Instant,
}

impl AppSnapshot {
    /// Create complete application snapshot
    #[instrument(level = "debug", skip_all, fields(operation_count = operations.len(), clipboard_item_count = clipboard_items.len(), task_count = task_infos.len()))]
    pub fn capture(
        ui_state: &UIState,
        pane_state: &PaneState,
        operations: &[(OperationId, f32, u64, u64)],
        clipboard_items: Vec<ClipboardItemSnapshot>,
        clipboard_selected_idx: usize,
        task_infos: Vec<TaskInfo>,
    ) -> Self {
        debug!("Capturing complete application snapshot");
        let snapshot = Self {
            ui: UiSnapshot::from(ui_state),
            pane: PaneSnapshot::from(pane_state),
            search: SearchSnapshot::from_states(ui_state, pane_state),
            prompt: PromptSnapshot::from_ui(ui_state),
            progress: OpsProgressSnapshot::from_operations(operations),
            clipboard: ClipboardSnapshot::from_items(clipboard_items, clipboard_selected_idx),
            tasks: TaskSnapshot::from_tasks(task_infos),
            timestamp: std::time::Instant::now(),
        };

        debug!("Application snapshot captured successfully");
        snapshot
    }
}
