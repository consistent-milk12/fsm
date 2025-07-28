//! src/model/ui_state.rs
//! ============================================================
//! Minimal UI state shared between logic and renderer.
//! • No `active_file_operations`  (moved to `FSState`)
//! • Only fields directly required by widgets / snapshots
//! • All write-heavy flags are atomics; nothing requires a mutex.

use std::{
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::Instant,
};

use clipr::{ClipBoardConfig, ClipBoardItem, clipboard::ClipBoard};
use compact_str::CompactString;
use smallvec::SmallVec;

use crate::{AppError, controller::actions::InputPromptType, model::fs_state::SearchMode};

// ------------------------------------------------------------
// Redraw bit-flags
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
// UI high-level mode
// ------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UIMode {
    Browse,
    Search,
    Command,
    Visual,
}

// ------------------------------------------------------------
// Overlay kinds (only those still used by widgets)
// ------------------------------------------------------------
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}
impl UIOverlay {
    /// Helper used by the status-bar widget
    pub fn is_search_mode(self) -> bool {
        matches!(self, UIOverlay::FileNameSearch | UIOverlay::ContentSearch)
    }
}

// ------------------------------------------------------------
// Notification & loading helpers
// ------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Notification {
    pub message: CompactString,
    pub level: NotificationLevel,
    pub timestamp: Instant,
    pub auto_dismiss_ms: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct LoadingState {
    pub message: CompactString,
    pub progress_pct: u32, // 0-10000  (two decimal places)
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
//                         UIState
// ------------------------------------------------------------
#[derive(Debug)]
pub struct UIState {
    // redraw bits
    pub redraw_flags: AtomicU32,
    // FPS counter
    pub frame_count: AtomicU64,

    // global mode + overlay
    pub mode: UIMode,
    pub overlay: UIOverlay,

    // prompt / command
    pub input_prompt_type: Option<InputPromptType>,
    pub prompt_buffer: CompactString,
    pub prompt_cursor: usize,
    pub command_history: SmallVec<[CompactString; 32]>,
    pub history_index: Option<usize>,

    // search
    pub search_mode: SearchMode,
    pub search_query: Option<CompactString>,

    // notifications & loading
    pub notification: Option<Notification>,
    pub loading: Option<LoadingState>,
    pub last_update: Instant,

    // clipboard overlay
    pub clipboard: ClipBoard,
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item_idx: usize,
}

// ------------------------------------------------------------
// ctor / defaults
// ------------------------------------------------------------
impl Default for UIState {
    fn default() -> Self {
        Self {
            redraw_flags: AtomicU32::new(RedrawFlag::All.bits() as u32),
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
        }
    }
}

// ------------------------------------------------------------
// Redraw helpers (lock-free)
// ------------------------------------------------------------
impl UIState {
    #[inline]
    pub fn request_redraw(&self, f: RedrawFlag) {
        self.redraw_flags
            .fetch_or(f.bits() as u32, Ordering::Relaxed);
    }
    #[inline]
    pub fn needs_redraw(&self) -> bool {
        self.redraw_flags.load(Ordering::Relaxed) != 0
    }
    #[inline]
    pub fn clear_redraw(&self) {
        self.redraw_flags.store(0, Ordering::Relaxed);
    }
    #[inline]
    pub fn inc_frame(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }
}

// ------------------------------------------------------------
// Prompt helpers
// ------------------------------------------------------------
impl UIState {
    #[inline]
    pub fn prompt_set(&mut self, txt: impl Into<CompactString>) {
        self.prompt_buffer = txt.into();
        self.prompt_cursor = self.prompt_buffer.len();
    }
    #[inline]
    pub fn prompt_insert(&mut self, ch: char) {
        let mut s = self.prompt_buffer.to_string();
        s.insert(self.prompt_cursor, ch);
        self.prompt_buffer = s.into();
        self.prompt_cursor += ch.len_utf8();
    }
    #[inline]
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
    pub fn history_push(&mut self, entry: impl Into<CompactString>) {
        self.command_history.push(entry.into());
        self.history_index = None;
    }
}

// ------------------------------------------------------------
// Notification helpers
// ------------------------------------------------------------
impl UIState {
    pub fn notify(
        &mut self,
        msg: impl Into<CompactString>,
        lvl: NotificationLevel,
        ms: Option<u32>,
    ) {
        self.notification = Some(Notification {
            message: msg.into(),
            level: lvl,
            timestamp: Instant::now(),
            auto_dismiss_ms: ms,
        });
        self.request_redraw(RedrawFlag::Notification);
    }
    #[inline]
    pub fn info(&mut self, m: impl Into<CompactString>) {
        self.notify(m, NotificationLevel::Info, Some(3000));
    }
    #[inline]
    pub fn success(&mut self, m: impl Into<CompactString>) {
        self.notify(m, NotificationLevel::Success, Some(2000));
    }
    #[inline]
    pub fn warn(&mut self, m: impl Into<CompactString>) {
        self.notify(m, NotificationLevel::Warning, Some(5000));
    }
    #[inline]
    pub fn error(&mut self, m: impl Into<CompactString>) {
        self.notify(m, NotificationLevel::Error, None);
    }

    /// Auto-dismiss timer; returns `true` when a notification is cleared.
    pub fn poll_notification(&mut self) -> bool {
        match &self.notification {
            Some(n)
                if n.auto_dismiss_ms
                    .map_or(false, |ms| n.timestamp.elapsed().as_millis() > ms as u128) =>
            {
                self.notification = None;
                self.request_redraw(RedrawFlag::Notification);
                true
            }
            _ => false,
        }
    }
}

// ------------------------------------------------------------
// Clipboard helpers
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

    pub async fn copy_path(&mut self, p: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_copy(p.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("copy_to_clipboard", p, e.to_string()))
    }
    pub async fn cut_path(&mut self, p: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_move(p.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("cut_to_clipboard", p, e.to_string()))
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
// FPS helper (UI thread only)
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
// Clone (cheap – atomics copied by value)
// ------------------------------------------------------------
impl Clone for UIState {
    fn clone(&self) -> Self {
        Self {
            redraw_flags: AtomicU32::new(self.redraw_flags.load(Ordering::Relaxed)),
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
