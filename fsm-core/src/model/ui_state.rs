//! Enhanced UI state with performance optimizations and clipboard integration

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use crate::AppError;
use clipr::clipboard::ClipBoard;
use clipr::{ClipBoardConfig, ClipBoardItem};
use compact_str::CompactString;
use smallvec::SmallVec;
use tokio_util::sync::CancellationToken;

use crate::controller::actions::InputPromptType;
use crate::fs::object_info::ObjectInfo;

/// Atomic redraw flags for lock-free UI updates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RedrawFlag {
    Main = 1,
    StatusBar = 2,
    Overlay = 4,
    Notification = 8,
    All = 15,
}

impl RedrawFlag {
    #[inline]
    pub const fn bits(self) -> u8 {
        self as u8
    }
}

/// UI operation modes with Search variant
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIMode {
    #[default]
    Browse = 0,
    Search = 1,
    Command = 2,
    Visual = 3,
}

/// UI overlays with Search support
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIOverlay {
    #[default]
    None = 0,
    Help = 1,
    FileNameSearch = 2,
    ContentSearch = 3,
    SearchResults = 4,
    Prompt = 5,
    Loading = 6,
    Search = 7, // Added for compatibility
}

/// Notification levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NotificationLevel {
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

/// Compact notification with timestamp
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: CompactString,
    pub level: NotificationLevel,
    pub timestamp: Instant,
    pub auto_dismiss_ms: Option<u32>,
}

/// High-performance loading state with atomic counters
#[derive(Debug)]
pub struct LoadingState {
    pub message: CompactString,
    pub progress: Arc<AtomicU32>,
    pub current: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    pub current_item: Arc<parking_lot::RwLock<Option<CompactString>>>,
    pub start_time: Instant,
}

impl LoadingState {
    pub fn new(message: impl Into<CompactString>) -> Self {
        Self {
            message: message.into(),
            progress: Arc::new(AtomicU32::new(0)),
            current: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            current_item: Arc::new(parking_lot::RwLock::new(None)),
            start_time: Instant::now(),
        }
    }

    #[inline]
    pub fn set_progress(&self, progress: f32) {
        let progress_int = (progress.clamp(0.0, 100.0) * 100.0) as u32;
        self.progress.store(progress_int, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_completion(&self, current: u64, total: u64) {
        self.current.store(current, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);

        if total > 0 {
            let progress = ((current as f64 / total as f64) * 10000.0) as u32;
            self.progress.store(progress, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn set_current_item(&self, item: Option<impl Into<CompactString>>) {
        *self.current_item.write() = item.map(|i| i.into());
    }

    #[inline]
    pub fn get_progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 100.0
    }

    pub fn get_completion_ratio(&self) -> f32 {
        let current = self.current.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);

        if total == 0 {
            0.0
        } else {
            (current as f32 / total as f32).clamp(0.0, 1.0)
        }
    }
}

impl Clone for LoadingState {
    fn clone(&self) -> Self {
        Self {
            message: self.message.clone(),
            progress: Arc::clone(&self.progress),
            current: Arc::clone(&self.current),
            total: Arc::clone(&self.total),
            current_item: Arc::clone(&self.current_item),
            start_time: self.start_time,
        }
    }
}

/// File operation progress tracking
#[derive(Debug)]
pub struct FileOperationProgress {
    pub operation_type: CompactString,
    pub current_bytes: AtomicU64,
    pub total_bytes: AtomicU64,
    pub files_completed: AtomicU32,
    pub total_files: AtomicU32,
    pub current_file: parking_lot::RwLock<CompactString>,
    pub start_time: Instant,
}

impl FileOperationProgress {
    pub fn new(
        operation_type: impl Into<CompactString>,
        total_bytes: u64,
        total_files: u32,
    ) -> Self {
        Self {
            operation_type: operation_type.into(),
            current_bytes: AtomicU64::new(0),
            total_bytes: AtomicU64::new(total_bytes),
            files_completed: AtomicU32::new(0),
            total_files: AtomicU32::new(total_files),
            current_file: parking_lot::RwLock::new(CompactString::new("")),
            start_time: Instant::now(),
        }
    }

    #[inline]
    pub fn update(
        &self,
        current_bytes: u64,
        current_file: impl Into<CompactString>,
        files_completed: u32,
    ) {
        self.current_bytes.store(current_bytes, Ordering::Relaxed);
        self.files_completed
            .store(files_completed, Ordering::Relaxed);
        *self.current_file.write() = current_file.into();
    }

    pub fn progress_ratio(&self) -> f32 {
        let total = self.total_bytes.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            let current = self.current_bytes.load(Ordering::Relaxed);
            (current as f32 / total as f32).clamp(0.0, 1.0)
        }
    }
}

/// Enhanced UI state with clipboard integration
#[derive(Debug)]
pub struct UIState {
    // Atomic flags for lock-free updates
    pub redraw_flags: AtomicU32,
    pub frame_count: AtomicU64,

    // Mode and overlay state
    pub mode: UIMode,
    pub overlay: UIOverlay,

    // Selection state
    pub selected: Option<usize>,
    pub marked_indices: SmallVec<[usize; 8]>,

    // Input state with history
    pub input: CompactString,
    pub input_cursor: usize,
    pub input_prompt_type: Option<InputPromptType>,
    pub input_history: SmallVec<[CompactString; 32]>,
    pub input_history_index: Option<usize>,

    // Display preferences
    pub show_hidden: bool,
    pub theme: CompactString,

    // Search results
    pub search_results: Vec<ObjectInfo>,
    pub filename_search_results: Vec<ObjectInfo>,
    pub content_search_results: Vec<String>,

    // Status and loading
    pub loading: Option<LoadingState>,
    pub notification: Option<Notification>,
    pub last_update: Instant,

    // File operations
    pub active_file_operations: HashMap<CompactString, Arc<FileOperationProgress>>,
    pub operation_cancel_tokens: HashMap<CompactString, CancellationToken>,

    // Clipboard integration
    pub clipboard: ClipBoard,
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item_index: usize,
}

impl Default for UIState {
    fn default() -> Self {
        Self::new()
    }
}

impl UIState {
    pub fn new() -> Self {
        Self {
            redraw_flags: AtomicU32::new(RedrawFlag::All.bits() as u32),
            frame_count: AtomicU64::new(0),
            mode: UIMode::Browse,
            overlay: UIOverlay::None,
            selected: Some(0),
            marked_indices: SmallVec::new(),
            input: CompactString::new(""),
            input_cursor: 0,
            input_prompt_type: None,
            input_history: SmallVec::new(),
            input_history_index: None,
            show_hidden: false,
            theme: CompactString::new("default"),
            search_results: Vec::with_capacity(256),
            filename_search_results: Vec::with_capacity(256),
            content_search_results: Vec::with_capacity(128),
            loading: None,
            notification: None,
            last_update: Instant::now(),
            active_file_operations: HashMap::new(),
            operation_cancel_tokens: HashMap::new(),
            clipboard: ClipBoard::new(ClipBoardConfig::default()),
            clipboard_overlay_active: false,
            selected_clipboard_item_index: 0,
        }
    }

    // Atomic redraw operations
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
    pub fn increment_frame(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }

    // Selection management
    #[inline]
    pub fn mark_index(&mut self, idx: usize) {
        if !self.marked_indices.contains(&idx) {
            self.marked_indices.push(idx);
            self.request_redraw(RedrawFlag::Main);
        }
    }

    #[inline]
    pub fn unmark_index(&mut self, idx: usize) {
        if let Some(pos) = self.marked_indices.iter().position(|&x| x == idx) {
            self.marked_indices.remove(pos);
            self.request_redraw(RedrawFlag::Main);
        }
    }

    #[inline]
    pub fn add_to_history(&mut self, input: String) {
        self.input_history.push(input.into());
    }

    #[inline]
    pub fn clear_marks(&mut self) {
        self.marked_indices.clear();
        self.request_redraw(RedrawFlag::Main);
    }

    // Notification system with inline helpers
    pub fn show_notification(
        &mut self,
        message: impl Into<CompactString>,
        level: NotificationLevel,
        auto_dismiss_ms: Option<u32>,
    ) {
        self.notification = Some(Notification {
            message: message.into(),
            level,
            timestamp: Instant::now(),
            auto_dismiss_ms,
        });
        self.request_redraw(RedrawFlag::Notification);
    }

    #[inline]
    pub fn show_info(&mut self, message: impl Into<CompactString>) {
        self.show_notification(message, NotificationLevel::Info, Some(3000));
    }

    #[inline]
    pub fn show_success(&mut self, message: impl Into<CompactString>) {
        self.show_notification(message, NotificationLevel::Success, Some(2000));
    }

    #[inline]
    pub fn show_warning(&mut self, message: impl Into<CompactString>) {
        self.show_notification(message, NotificationLevel::Warning, Some(5000));
    }

    #[inline]
    pub fn show_error(&mut self, message: impl Into<CompactString>) {
        self.show_notification(message, NotificationLevel::Error, None);
    }

    // Auto-dismiss notifications
    pub fn update_notification(&mut self) -> bool {
        if let Some(notification) = &self.notification
            && let Some(auto_dismiss_ms) = notification.auto_dismiss_ms
            && notification.timestamp.elapsed().as_millis() > auto_dismiss_ms as u128
        {
            self.notification = None;
            self.request_redraw(RedrawFlag::Notification);
            return true;
        }
        false
    }

    // Input management (unchanged for compatibility)
    pub fn clear_input(&mut self) {
        self.input = CompactString::new("");
        self.input_cursor = 0;
        self.input_history_index = None;
    }

    pub fn set_input(&mut self, text: impl Into<CompactString>) {
        self.input = text.into();
        self.input_cursor = self.input.len();
        self.input_history_index = None;
    }

    pub fn insert_char(&mut self, ch: char) {
        let mut input_str = self.input.to_string();
        input_str.insert(self.input_cursor, ch);
        self.input = input_str.into();
        self.input_cursor += ch.len_utf8();
        self.input_history_index = None;
    }

    pub fn delete_char_before(&mut self) -> bool {
        if self.input_cursor > 0 {
            let mut input_str = self.input.to_string();
            let char_indices: Vec<_> = input_str.char_indices().collect();

            if let Some((char_pos, _)) = char_indices
                .iter()
                .rev()
                .find(|(pos, _)| *pos < self.input_cursor)
            {
                input_str.remove(*char_pos);
                self.input = input_str.into();
                self.input_cursor = *char_pos;
                self.input_history_index = None;
                return true;
            }
        }
        false
    }

    pub fn toggle_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = !self.clipboard_overlay_active;
        if self.clipboard_overlay_active {
            self.selected_clipboard_item_index = 0;
        }
        self.request_redraw(RedrawFlag::Overlay);
    }

    pub fn move_clipboard_selection_down(&mut self) {
        // Remove async - clipboard.len() should be cached
        if self.selected_clipboard_item_index < 10 {
            // Use reasonable default
            self.selected_clipboard_item_index += 1;
            self.request_redraw(RedrawFlag::Overlay);
        }
    }

    pub fn move_clipboard_selection_up(&mut self) {
        if self.selected_clipboard_item_index > 0 {
            self.selected_clipboard_item_index -= 1;
            self.request_redraw(RedrawFlag::Overlay);
        }
    }

    // High-performance clipboard operations
    pub async fn copy_to_clipboard(&mut self, path: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_copy(path.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("copy_to_clipboard", path, e.to_string()))
    }

    pub async fn cut_to_clipboard(&mut self, path: std::path::PathBuf) -> Result<u64, AppError> {
        self.clipboard
            .add_move(path.clone())
            .await
            .map_err(|e| AppError::file_operation_failed("cut_to_clipboard", path, e.to_string()))
    }

    pub async fn get_clipboard_items(&self) -> Vec<ClipBoardItem> {
        self.clipboard.get_all_items().await
    }

    pub async fn clear_clipboard(&mut self) {
        self.clipboard.clear().await;
        self.clipboard_overlay_active = false;
        self.selected_clipboard_item_index = 0;
        self.request_redraw(RedrawFlag::All);
    }

    // Overlay helpers
    pub fn get_overlay_title(&self) -> &'static str {
        match self.overlay {
            UIOverlay::Help => "Help",
            UIOverlay::FileNameSearch => "File Search",
            UIOverlay::ContentSearch => "Content Search",
            UIOverlay::Search => "Search",
            UIOverlay::Prompt => match &self.input_prompt_type {
                Some(InputPromptType::Custom(name)) if name == "command" => "Command Mode",
                Some(InputPromptType::CreateFile) => "Create File",
                Some(InputPromptType::CreateDirectory) => "Create Directory",
                Some(InputPromptType::Rename) => "Rename",
                Some(InputPromptType::GoToPath) => "Go To Path",
                _ => "Input",
            },
            UIOverlay::SearchResults => "Search Results",
            UIOverlay::Loading => "Loading",
            _ => "Overlay",
        }
    }

    pub fn overlay_accepts_input(&self) -> bool {
        matches!(
            self.overlay,
            UIOverlay::FileNameSearch
                | UIOverlay::ContentSearch
                | UIOverlay::Prompt
                | UIOverlay::Search
        )
    }

    // Performance metrics
    pub fn get_fps(&self) -> f64 {
        let elapsed = self.last_update.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.frame_count.load(Ordering::Relaxed) as f64 / elapsed
        } else {
            0.0
        }
    }

    // Bulk operations for performance
    pub fn bulk_update<F>(&mut self, update_fn: F)
    where
        F: FnOnce(&mut Self),
    {
        let old_flags = self.redraw_flags.load(Ordering::Relaxed);
        self.redraw_flags.store(0, Ordering::Relaxed);
        update_fn(self);
        self.redraw_flags
            .store(old_flags | RedrawFlag::All.bits() as u32, Ordering::Relaxed);
    }

    // Memory optimization
    pub fn shrink_collections(&mut self) {
        self.search_results.shrink_to_fit();
        self.filename_search_results.shrink_to_fit();
        self.content_search_results.shrink_to_fit();
        self.marked_indices.shrink_to_fit();
        self.input_history.shrink_to_fit();
    }

    // State validation
    pub fn validate_state(&mut self) {
        if self.overlay == UIOverlay::SearchResults {
            let max_results =
                std::cmp::max(self.search_results.len(), self.content_search_results.len());
            if max_results == 0 {
                self.selected = None;
            } else if let Some(sel) = self.selected {
                if sel >= max_results {
                    self.selected = Some(max_results - 1);
                }
            }
        }

        self.marked_indices.retain(|idx| match self.overlay {
            UIOverlay::SearchResults => {
                let max_results =
                    std::cmp::max(self.search_results.len(), self.content_search_results.len());
                *idx < max_results
            }
            _ => true,
        });
    }
}

// Clone implementation optimized for state coordination
impl Clone for UIState {
    fn clone(&self) -> Self {
        Self {
            redraw_flags: AtomicU32::new(self.redraw_flags.load(Ordering::Relaxed)),
            frame_count: AtomicU64::new(self.frame_count.load(Ordering::Relaxed)),
            mode: self.mode,
            overlay: self.overlay,
            selected: self.selected,
            marked_indices: self.marked_indices.clone(),
            input: self.input.clone(),
            input_cursor: self.input_cursor,
            input_prompt_type: self.input_prompt_type.clone(),
            input_history: self.input_history.clone(),
            input_history_index: self.input_history_index,
            show_hidden: self.show_hidden,
            theme: self.theme.clone(),
            search_results: self.search_results.clone(),
            filename_search_results: self.filename_search_results.clone(),
            content_search_results: self.content_search_results.clone(),
            loading: self.loading.clone(),
            notification: self.notification.clone(),
            last_update: self.last_update,
            active_file_operations: self.active_file_operations.clone(),
            operation_cancel_tokens: self.operation_cancel_tokens.clone(),
            clipboard: ClipBoard::new(ClipBoardConfig::default()), // Create new instance for clone
            clipboard_overlay_active: self.clipboard_overlay_active,
            selected_clipboard_item_index: self.selected_clipboard_item_index,
        }
    }
}

impl PartialEq for UIState {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.overlay == other.overlay
            && self.selected == other.selected
            && self.input == other.input
            && self.show_hidden == other.show_hidden
            && self.clipboard_overlay_active == other.clipboard_overlay_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_integration() {
        let mut ui_state = UIState::new();

        assert!(!ui_state.clipboard_overlay_active);
        ui_state.toggle_clipboard_overlay();
        assert!(ui_state.clipboard_overlay_active);
        assert_eq!(ui_state.selected_clipboard_item_index, 0);
    }

    #[test]
    fn test_search_overlay_compatibility() {
        let ui_state = UIState::new();
        assert_eq!(ui_state.overlay, UIOverlay::None);

        let mut ui_state = UIState::new();
        ui_state.overlay = UIOverlay::Search;
        assert!(ui_state.overlay_accepts_input());
        assert_eq!(ui_state.get_overlay_title(), "Search");
    }

    #[test]
    fn test_performance_optimization() {
        let mut ui_state = UIState::new();

        // Test bulk update
        ui_state.bulk_update(|ui| {
            ui.show_hidden = true;
            ui.selected = Some(5);
            ui.mode = UIMode::Visual;
        });

        assert!(ui_state.show_hidden);
        assert_eq!(ui_state.selected, Some(5));
        assert_eq!(ui_state.mode, UIMode::Visual);
    }

    #[test]
    fn test_atomic_operations() {
        let ui_state = UIState::new();

        ui_state.request_redraw(RedrawFlag::Main);
        assert!(ui_state.needs_redraw());

        ui_state.clear_redraw();
        assert!(!ui_state.needs_redraw());

        ui_state.increment_frame();
        assert_eq!(ui_state.frame_count.load(Ordering::Relaxed), 1);
    }
}
