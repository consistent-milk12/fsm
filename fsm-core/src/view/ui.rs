//! src/model/ui_state.rs
//! ============================================================
//! *Slimmed-down* UIState that matches the new lock-free render
//! pipeline.
//!
//! ‣ No `active_file_operations` – that now lives in `FSState`.  
//! ‣ Keeps only data actually consumed by `UIRenderer` or the
//!   widgets after they receive their immutable snapshots.  
//! ‣ All write-heavy fields use atomics; read-heavy fields are
//!   plain values.  Nothing here requires a mutex.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use compact_str::CompactString;
use smallvec::SmallVec;

use clipr::{ClipBoardConfig, ClipBoardItem, clipboard::ClipBoard};

use crate::AppError;
use crate::controller::actions::InputPromptType;
use crate::fs::object_info::ObjectInfo; // only for tests
use crate::model::UIMode;
use crate::model::fs_state::SearchMode;

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

#[derive(Clone, Debug)]
pub struct LoadingState {
    pub message: CompactString,
    pub progress_pct: u32, // 0-10000 (two decimal places)
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
    /// Atomic “what to repaint” flags
    pub redraw_flags: AtomicU32,

    /// Frame counter (for FPS)
    pub frame_count: AtomicU64,

    /// Current high-level mode
    pub mode: UIMode,

    /// Current modal overlay
    pub overlay: UIOverlay,

    /// ***Prompt / command line*** ------------------------------
    pub input_prompt_type: Option<InputPromptType>,
    pub prompt_buffer: CompactString,
    pub prompt_cursor: usize,
    pub command_history: SmallVec<[CompactString; 32]>,
    pub history_index: Option<usize>,

    /// ***Search*** ---------------------------------------------
    pub search_mode: SearchMode,
    pub search_query: Option<CompactString>,

    /// ***Notifications & loading*** ----------------------------
    pub notification: Option<Notification>,
    pub loading: Option<LoadingState>,
    pub last_update: Instant,

    /// ***Clipboard overlay*** ----------------------------------
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
// Redraw helpers – all lock-free
// ------------------------------------------------------------
impl UIState {
    #[inline]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        self.redraw_flags
            .fetch_or(flag.bits() as u32, Ordering::Relaxed);
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
// Prompt / command input helpers
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
    pub fn info(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Info, Some(3000));
    }
    #[inline]
    pub fn success(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Success, Some(2000));
    }
    #[inline]
    pub fn warn(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Warning, Some(5000));
    }
    #[inline]
    pub fn error(&mut self, msg: impl Into<CompactString>) {
        self.notify(msg, NotificationLevel::Error, None);
    }

    /// Returns `true` if a notification was auto-cleared
    pub fn poll_notification(&mut self) -> bool {
        match &self.notification {
            Some(n)
                if n.auto_dismiss_ms.is_some()
                    && n.timestamp.elapsed().as_millis() > n.auto_dismiss_ms.unwrap() as u128 =>
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

// ------------------------------------------------------------
// Unit tests (compile & behaviour smoke tests)
// ------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_toggle() {
        let mut st = UIState::default();
        assert!(!st.clipboard_overlay_active);
        st.toggle_clipboard_overlay();
        assert!(st.clipboard_overlay_active);
    }

    #[test]
    fn prompt_edit() {
        let mut st = UIState::default();
        st.prompt_set("abc");
        st.prompt_insert('d');
        assert_eq!(st.prompt_buffer.as_str(), "abcd");
        assert!(st.prompt_backspace());
        assert_eq!(st.prompt_buffer.as_str(), "abc");
    }

    #[test]
    fn redraw_bits() {
        let st = UIState::default();
        st.request_redraw(RedrawFlag::Main);
        assert!(st.needs_redraw());
        st.clear_redraw();
        assert!(!st.needs_redraw());
    }
}
