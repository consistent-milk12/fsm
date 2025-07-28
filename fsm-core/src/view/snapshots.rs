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
//!
//! All comments are placed *above* the line they describe and
//! no line exceeds 70 columns (per user coding guidelines).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use compact_str::CompactString;
use ratatui::prelude::Rect;

use crate::fs::object_info::ObjectInfo;
use crate::model::fs_state::{EntryFilter, EntrySort, PaneState};
use crate::model::ui_state::{
    LoadingState, Notification, NotificationLevel, RedrawFlag, UIOverlay,
};

use crate::UIState;
use crate::controller::actions::InputPromptType;
use crate::model::UIMode;
use crate::model::fs_state::SearchMode;

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

    pub mode: UIMode,
}

impl From<&UIState> for UiSnapshot {
    fn from(src: &UIState) -> Self {
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
            mode: src.mode.clone(),
        }
    }
}

// ------------------------------------------------------------
// SearchSnapshot
// ------------------------------------------------------------

/// Captures the dynamic state that the *search* overlay needs
/// (user input + live results).  Building this snapshot avoids
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
}

impl SearchSnapshot {
    /// Builder extracted from a locked `UIState` and `PaneState`
    pub fn from_states(ui: &crate::model::ui_state::UIState, pane: &PaneState) -> Option<Self> {
        let query = ui.search_query.clone()?;
        let results = if ui.search_mode == SearchMode::None {
            Arc::from([])
        } else {
            Arc::from(pane.search_results.clone().into_boxed_slice())
        };

        let query_len = query.len();

        Some(Self {
            query,
            cursor: query_len, // cursor is at end by default
            results,
            mode: ui.search_mode,
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
}

impl PromptSnapshot {
    /// Safe extractor – returns `None` if prompt overlay inactive
    pub fn from_ui(ui: &crate::model::ui_state::UIState) -> Option<Self> {
        let prompt_type = ui.input_prompt_type.clone()?;
        Some(Self {
            prompt_type,
            buffer: ui.prompt_buffer.clone(),
            cursor: ui.prompt_cursor,
            history: Arc::from(ui.command_history.clone().into_boxed_slice()),
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
}

// ------------------------------------------------------------
// PaneSnapshot
// ------------------------------------------------------------

/// An immutable view onto a `PaneState`.  Unlike `PaneState`
/// it owns no atomics – just plain data – so the renderer can
/// inspect it freely without worrying about concurrent mutation.
#[derive(Debug, Clone)]
pub struct PaneSnapshot {
    /// Working directory of the pane
    pub cwd: std::path::PathBuf,

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
    fn from(pane: &PaneState) -> Self {
        Self {
            cwd: pane.cwd.clone(),
            entries: Arc::from(pane.entries.clone().into_boxed_slice()),
            selected: pane.selected.load(std::sync::atomic::Ordering::Relaxed),
            scroll_offset: pane
                .scroll_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            viewport_height: pane
                .viewport_height
                .load(std::sync::atomic::Ordering::Relaxed),
            sort: pane.sort,
            filter: pane.filter.clone(),
            is_loading: pane.is_loading.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}
