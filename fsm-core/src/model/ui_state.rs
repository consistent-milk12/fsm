//! src/model/ui_state.rs
//! ============================================================
//! *Slimmed-down* UIState that matches the new lock-free render
//! pipeline.
//! Tracing added to notification & redraw helpers.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use compact_str::CompactString;
use smallvec::SmallVec;

use clipr::{ClipBoardConfig, ClipBoardItem, clipboard::ClipBoard};

use crate::AppError;
use crate::controller::actions::InputPromptType;
use crate::model::fs_state::SearchMode;

use tracing::{debug, instrument, trace};

// ------------------------------------------------------------
// Redraw bit-flags (atomic)
// ------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RedrawFlag {
    Main = 1,
    StatusBar = 1 << 1,
    Overlay = 1 << 2,
    Notification = 1 << 3,
    All = 0x0F,
}

impl RedrawFlag {
    #[inline]
    pub const fn bits(self) -> u8 {
        self as u8
    }
}

// ------------------------------------------------------------
// Simple enums reused by logic & UI
// ------------------------------------------------------------
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIMode {
    #[default]
    Browse,
    Search,
    Command,
    Visual,
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
#[repr(u8)]
pub enum UIOverlay {
    #[default]
    None,
    Help,
    FileNameSearch,
    ContentSearch,
    SearchResults,
    Prompt,
    Loading,
    Search,
    SystemMonitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

// ------------------------------------------------------------
// Small data structs
// ------------------------------------------------------------
#[derive(Clone, Debug)]
pub struct Notification {
    pub message: CompactString,
    pub level: NotificationLevel,
    pub timestamp: Instant,
    pub auto_dismiss_ms: Option<u32>,
}

/// Lightweight loading descriptor
#[derive(Clone, Debug)]
pub struct LoadingState {
    pub message: CompactString,
    pub progress_pct: u32,
    pub start_time: Instant,
}

impl LoadingState {
    #[inline]
    pub fn set_progress(&mut self, pct: f32) {
        self.progress_pct = (pct.clamp(0.0, 100.0) * 100.0) as u32;
    }
    #[inline]
    pub fn progress(&self) -> f32 {
        self.progress_pct as f32 / 100.0
    }
}

// ------------------------------------------------------------
//                           UIState
// ------------------------------------------------------------
#[derive(Debug)]
pub struct UIState {
    pub redraw_flags: AtomicU32,
    pub redraw_request_counter: AtomicU64, // Prevents race conditions in redraw requests
    pub redraw_processed_counter: AtomicU64, // Tracks completed redraws
    pub frame_count: AtomicU64,
    pub mode: UIMode,
    pub overlay: UIOverlay,
    pub input_prompt_type: Option<InputPromptType>,
    pub prompt_buffer: CompactString,
    pub prompt_cursor: usize,
    pub command_history: SmallVec<[CompactString; 32]>,
    pub history_index: Option<usize>,
    pub search_mode: SearchMode,
    pub search_query: Option<CompactString>,
    pub notification: Option<Notification>,
    pub loading: Option<LoadingState>,
    pub last_update: Instant,
    pub clipboard: ClipBoard,
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item_idx: usize,
    pub show_process_monitor: bool,
    pub selected_process_idx: usize,
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            redraw_flags: AtomicU32::new(RedrawFlag::All.bits() as u32),
            redraw_request_counter: AtomicU64::new(1), // Start with 1 to force initial render
            redraw_processed_counter: AtomicU64::new(0), // Start with 0 to trigger first render
            frame_count: AtomicU64::new(0),
            mode: UIMode::Browse,
            overlay: UIOverlay::None,
            input_prompt_type: None,
            prompt_buffer: CompactString::new(""),
            prompt_cursor: 0,
            command_history: SmallVec::new(),
            history_index: None,
            search_mode: SearchMode::None,
            search_query: None,
            notification: None,
            loading: None,
            last_update: Instant::now(),
            clipboard: ClipBoard::new(ClipBoardConfig::default()),
            clipboard_overlay_active: false,
            selected_clipboard_item_idx: 0,
            show_process_monitor: false,
            selected_process_idx: 0,
        }
    }
}

// ------------------------------------------------------------
// Redraw helpers – all lock-free
// ------------------------------------------------------------
impl UIState {
    #[inline]
    #[instrument(level = "trace", skip(self), fields(flag = ?flag))]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        trace!("requesting redraw");
        // Atomically increment request counter and set flags
        self.redraw_request_counter.fetch_add(1, Ordering::Relaxed);
        self.redraw_flags
            .fetch_or(flag.bits() as u32, Ordering::Relaxed);
    }

    #[inline]
    #[instrument(level = "trace", skip(self))]
    pub fn needs_redraw(&self) -> bool {
        // Check if there are pending redraw requests by comparing counters
        let requests = self.redraw_request_counter.load(Ordering::Relaxed);
        let processed = self.redraw_processed_counter.load(Ordering::Relaxed);
        let has_pending_requests = requests > processed;

        // Also check traditional flags for backward compatibility
        let has_flags = self.redraw_flags.load(Ordering::Relaxed) != 0;

        let need = has_pending_requests || has_flags;
        trace!(
            needs = need,
            pending_requests = requests - processed,
            has_flags,
            "needs_redraw"
        );
        need
    }

    #[inline]
    #[instrument(level = "trace", skip(self))]
    pub fn clear_redraw(&self) {
        trace!("clearing redraw flags");
        // Sync processed counter to match requests to mark all requests as handled
        let current_requests = self.redraw_request_counter.load(Ordering::Relaxed);
        self.redraw_processed_counter
            .store(current_requests, Ordering::Relaxed);

        // Clear traditional flags
        self.redraw_flags.store(0, Ordering::Relaxed);
    }

    #[inline]
    #[instrument(level = "trace", skip(self))]
    pub fn inc_frame(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }
}

// ------------------------------------------------------------
// Prompt / command input helpers
// ------------------------------------------------------------
impl UIState {
    #[inline]
    #[instrument(level = "trace", skip(self, txt))]
    pub fn prompt_set(&mut self, txt: impl Into<CompactString>) {
        self.prompt_buffer = txt.into();
        self.prompt_cursor = self.prompt_buffer.len();
    }

    #[inline]
    #[instrument(level = "trace", skip(self, ch))]
    pub fn prompt_insert(&mut self, ch: char) {
        let mut s = self.prompt_buffer.to_string();
        s.insert(self.prompt_cursor, ch);
        self.prompt_buffer = s.into();
        self.prompt_cursor += ch.len_utf8();
    }

    #[inline]
    #[instrument(level = "trace", skip(self))]
    pub fn prompt_backspace(&mut self) -> bool {
        if self.prompt_cursor == 0 {
            return false;
        }
        let mut s = self.prompt_buffer.to_string();
        let idx = s
            .char_indices()
            .rev()
            .find(|&(i, _)| i < self.prompt_cursor)
            .map(|(i, _)| i)
            .unwrap_or(0);
        s.remove(idx);
        self.prompt_buffer = s.into();
        self.prompt_cursor = idx;
        true
    }

    #[inline]
    #[instrument(level = "trace", skip(self, _entry))]
    pub fn history_push(&mut self, _entry: impl Into<CompactString>) {
        trace!(
            marker = "HISTORY_PUSH",
            operation_type = "ui_input",
            "history_push"
        );
    }
}

// ------------------------------------------------------------
// Notification helpers
// ------------------------------------------------------------
impl UIState {
    #[instrument(level = "info", skip(self, msg))]
    pub fn notify(
        &mut self,
        msg: impl Into<CompactString>,
        lvl: NotificationLevel,
        ms: Option<u32>,
    ) {
        // Convert message once to avoid moves
        let cs: CompactString = msg.into();
        debug!(level = ?lvl, message = %cs, "showing notification");
        self.notification = Some(Notification {
            message: cs.clone(),
            level: lvl,
            timestamp: Instant::now(),
            auto_dismiss_ms: ms,
        });
        self.request_redraw(RedrawFlag::Notification);
    }

    #[instrument(level = "debug", skip(self, msg))]
    #[inline]
    pub fn info(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Info, Some(3000));
    }

    #[instrument(level = "debug", skip(self, msg))]
    #[inline]
    pub fn success(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Success, Some(2000));
    }

    #[instrument(level = "debug", skip(self, msg))]
    #[inline]
    pub fn warn(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Warning, Some(5000));
    }

    #[instrument(level = "debug", skip(self, msg))]
    #[inline]
    pub fn error(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Error, None);
    }

    /// Returns `true` if a notification was auto-cleared
    #[instrument(level = "debug", skip(self))]
    pub fn poll_notification(&mut self) -> bool {
        trace!(
            notification = ?self.notification.as_ref().map(|n| (&n.level, &n.timestamp, &n.auto_dismiss_ms))
        );

        if let Some(n) = &self.notification {
            if let Some(auto_ms) = n.auto_dismiss_ms
                && n.timestamp.elapsed().as_millis() > auto_ms as u128
            {
                debug!(
                    "auto-dismissing notification (level={:?}) after {}ms",
                    n.level,
                    n.timestamp.elapsed().as_millis()
                );

                self.notification = None;
                self.request_redraw(RedrawFlag::Notification);
                return true;
            }
        }

        false
    }
}

// ------------------------------------------------------------
// Clipboard overlay helpers
// ------------------------------------------------------------
impl UIState {
    #[inline]
    pub fn toggle_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = !self.clipboard_overlay_active;
        self.selected_clipboard_item_idx = 0;
        self.request_redraw(RedrawFlag::Overlay);
    }

    #[inline]
    pub fn clipboard_down(&mut self) {
        self.selected_clipboard_item_idx = self.selected_clipboard_item_idx.saturating_add(1);
        self.request_redraw(RedrawFlag::Overlay);
    }

    #[inline]
    pub fn clipboard_up(&mut self) {
        if self.selected_clipboard_item_idx > 0 {
            self.selected_clipboard_item_idx -= 1;
            self.request_redraw(RedrawFlag::Overlay);
        }
    }

    pub async fn copy_path(&mut self, path: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_copy(path.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("copy_to_clipboard", path, e.to_string()))
    }

    pub async fn cut_path(&mut self, path: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_move(path.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("cut_to_clipboard", path, e.to_string()))
    }

    pub async fn items(&self) -> Vec<ClipBoardItem> {
        self.clipboard.get_all_items().await
    }

    pub async fn clear_clipboard(&mut self) {
        self.clipboard.clear().await;
        self.clipboard_overlay_active = false;
        self.selected_clipboard_item_idx = 0;
        self.request_redraw(RedrawFlag::All);
    }
}

// ------------------------------------------------------------
// Simple FPS metric (UI only – not thread-safe)
// ------------------------------------------------------------
impl UIState {
    pub fn fps(&self) -> f64 {
        let secs = self.last_update.elapsed().as_secs_f64();
        if secs > 0.0 {
            self.frame_count.load(Ordering::Relaxed) as f64 / secs
        } else {
            0.0
        }
    }
}

// ------------------------------------------------------------
// Cheap Clone – atomics copied by value
// ------------------------------------------------------------
impl Clone for UIState {
    fn clone(&self) -> Self {
        Self {
            redraw_flags: AtomicU32::new(self.redraw_flags.load(Ordering::Relaxed)),
            redraw_request_counter: AtomicU64::new(
                self.redraw_request_counter.load(Ordering::Relaxed),
            ),
            redraw_processed_counter: AtomicU64::new(
                self.redraw_processed_counter.load(Ordering::Relaxed),
            ),
            frame_count: AtomicU64::new(self.frame_count.load(Ordering::Relaxed)),
            mode: self.mode,
            overlay: self.overlay,
            input_prompt_type: self.input_prompt_type.clone(),
            prompt_buffer: self.prompt_buffer.clone(),
            prompt_cursor: self.prompt_cursor,
            command_history: self.command_history.clone(),
            history_index: self.history_index,
            search_mode: self.search_mode,
            search_query: self.search_query.clone(),
            notification: self.notification.clone(),
            loading: self.loading.clone(),
            last_update: self.last_update,
            clipboard: ClipBoard::new(ClipBoardConfig::default()),
            clipboard_overlay_active: self.clipboard_overlay_active,
            selected_clipboard_item_idx: self.selected_clipboard_item_idx,
            show_process_monitor: self.show_process_monitor,
            selected_process_idx: self.selected_process_idx,
        }
    }
}

// ------------------------------------------------------------
// Equality – only cheap fields
// ------------------------------------------------------------
impl PartialEq for UIState {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.overlay == other.overlay
            && self.clipboard_overlay_active == other.clipboard_overlay_active
            && self.prompt_buffer == other.prompt_buffer
    }
}
