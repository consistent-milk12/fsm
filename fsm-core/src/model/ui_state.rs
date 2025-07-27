//! Optimized UI state for high-performance TUI with atomic operations

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

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

/// UI operation modes
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIMode {
    #[default]
    Browse = 0,
    Search = 1,
    Command = 2,
    Visual = 3,
}

/// UI overlays
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
    pub progress: Arc<AtomicU32>, // Progress * 100 for precision
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

/// High-performance UI state with atomic operations
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
    pub fn clear_marks(&mut self) {
        self.marked_indices.clear();
        self.request_redraw(RedrawFlag::Main);
    }

    // Notification system
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

    // Input management
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

    pub fn move_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            let input_str = self.input.as_str();
            let char_indices: Vec<_> = input_str.char_indices().collect();

            if let Some((pos, _)) = char_indices
                .iter()
                .rev()
                .find(|(pos, _)| *pos < self.input_cursor)
            {
                self.input_cursor = *pos;
            }
        }
    }

    pub fn move_cursor_right(&mut self) {
        let input_str = self.input.as_str();
        let char_indices: Vec<_> = input_str.char_indices().collect();

        if let Some((pos, _)) = char_indices
            .iter()
            .find(|(pos, _)| *pos > self.input_cursor)
        {
            self.input_cursor = *pos;
        } else if self.input_cursor < input_str.len() {
            self.input_cursor = input_str.len();
        }
    }

    // History management
    pub fn add_to_history(&mut self, input: impl Into<CompactString>) {
        let input_str = input.into();
        if !input_str.is_empty() {
            if let Some(pos) = self.input_history.iter().position(|x| *x == input_str) {
                self.input_history.remove(pos);
            }

            self.input_history.push(input_str);

            if self.input_history.len() > 32 {
                self.input_history.remove(0);
            }
        }
        self.input_history_index = None;
    }

    pub fn history_prev(&mut self) -> bool {
        if self.input_history.is_empty() {
            return false;
        }

        match self.input_history_index {
            None => self.input_history_index = Some(self.input_history.len() - 1),
            Some(idx) if idx > 0 => self.input_history_index = Some(idx - 1),
            _ => return false,
        }

        if let Some(idx) = self.input_history_index
            && let Some(history_item) = self.input_history.get(idx)
        {
            self.input = history_item.clone();
            self.input_cursor = self.input.len();
            return true;
        }
        false
    }

    pub fn history_next(&mut self) -> bool {
        if let Some(idx) = self.input_history_index {
            if idx < self.input_history.len() - 1 {
                self.input_history_index = Some(idx + 1);
                if let Some(history_item) = self.input_history.get(idx + 1) {
                    self.input = history_item.clone();
                    self.input_cursor = self.input.len();
                    return true;
                }
            } else {
                self.input_history_index = None;
                self.clear_input();
                return true;
            }
        }
        false
    }

    // Overlay helpers
    pub fn get_overlay_title(&self) -> &'static str {
        match self.overlay {
            UIOverlay::Help => "Help",
            UIOverlay::FileNameSearch => "File Search",
            UIOverlay::ContentSearch => "Content Search",
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
            UIOverlay::FileNameSearch | UIOverlay::ContentSearch | UIOverlay::Prompt
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

    // File operation management
    pub fn add_file_operation(
        &mut self,
        operation_id: impl Into<CompactString>,
        progress: Arc<FileOperationProgress>,
    ) {
        let id = operation_id.into();
        self.active_file_operations.insert(id.clone(), progress);
        self.request_redraw(RedrawFlag::StatusBar);
    }

    pub fn remove_file_operation(&mut self, operation_id: &str) {
        self.active_file_operations.remove(operation_id);
        self.operation_cancel_tokens.remove(operation_id);
        self.request_redraw(RedrawFlag::StatusBar);
    }

    pub fn cancel_file_operation(&mut self, operation_id: &str) {
        if let Some(token) = self.operation_cancel_tokens.get(operation_id) {
            token.cancel();
        }
        self.remove_file_operation(operation_id);
    }

    // Loading state management
    pub fn set_loading(&mut self, loading: LoadingState) {
        self.loading = Some(loading);
        self.overlay = UIOverlay::Loading;
        self.request_redraw(RedrawFlag::All);
    }

    pub fn clear_loading(&mut self) {
        self.loading = None;
        if self.overlay == UIOverlay::Loading {
            self.overlay = UIOverlay::None;
        }
        self.request_redraw(RedrawFlag::All);
    }

    pub fn update_loading_progress(
        &mut self,
        current: u64,
        total: u64,
        message: Option<impl Into<CompactString>>,
    ) {
        if let Some(loading) = &self.loading {
            loading.set_completion(current, total);
            if let Some(msg) = message {
                loading.set_current_item(Some(msg));
            }
            self.request_redraw(RedrawFlag::StatusBar);
        }
    }

    // Search result management
    pub fn set_search_results(&mut self, results: Vec<ObjectInfo>) {
        self.search_results = results;
        self.overlay = UIOverlay::SearchResults;
        self.request_redraw(RedrawFlag::All);
    }

    pub fn set_filename_search_results(&mut self, results: Vec<ObjectInfo>) {
        self.filename_search_results = results;
        self.request_redraw(RedrawFlag::Overlay);
    }

    pub fn set_content_search_results(&mut self, results: Vec<String>) {
        self.content_search_results = results;
        self.overlay = UIOverlay::SearchResults;
        self.request_redraw(RedrawFlag::All);
    }

    pub fn clear_search_results(&mut self) {
        self.search_results.clear();
        self.filename_search_results.clear();
        self.content_search_results.clear();
        if self.overlay == UIOverlay::SearchResults {
            self.overlay = UIOverlay::None;
        }
        self.request_redraw(RedrawFlag::All);
    }

    // State validation and cleanup
    pub fn validate_state(&mut self) {
        // Ensure selection is within bounds when search results change
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

        // Clean up marked indices that are out of bounds
        self.marked_indices.retain(|idx: &mut usize| {
            match self.overlay {
                UIOverlay::SearchResults => {
                    let max_results: usize =
                        std::cmp::max(self.search_results.len(), self.content_search_results.len());
                    *idx < max_results
                }
                _ => true, // Keep all marks for normal browsing (will be validated by pane state)
            }
        });
    }

    // Bulk update helpers for performance
    pub fn bulk_update<F>(&mut self, update_fn: F)
    where
        F: FnOnce(&mut Self),
    {
        // Disable redraws during bulk update
        let old_flags = self.redraw_flags.load(Ordering::Relaxed);
        self.redraw_flags.store(0, Ordering::Relaxed);

        // Apply updates
        update_fn(self);

        // Restore and set redraw flags
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

    // Debug helpers
    pub fn get_memory_usage(&self) -> UIStateMemoryUsage {
        UIStateMemoryUsage {
            search_results: self.search_results.capacity() * std::mem::size_of::<ObjectInfo>(),
            filename_search_results: self.filename_search_results.capacity()
                * std::mem::size_of::<ObjectInfo>(),
            content_search_results: self.content_search_results.capacity()
                * std::mem::size_of::<String>(),
            marked_indices: self.marked_indices.capacity() * std::mem::size_of::<usize>(),
            input_history: self.input_history.capacity() * 32, // Approximate CompactString size
            active_operations: self.active_file_operations.len()
                * std::mem::size_of::<Arc<FileOperationProgress>>(),
        }
    }
}

/// Memory usage statistics for debugging
#[derive(Debug, Clone)]
pub struct UIStateMemoryUsage {
    pub search_results: usize,
    pub filename_search_results: usize,
    pub content_search_results: usize,
    pub marked_indices: usize,
    pub input_history: usize,
    pub active_operations: usize,
}

impl UIStateMemoryUsage {
    pub fn total_bytes(&self) -> usize {
        self.search_results
            + self.filename_search_results
            + self.content_search_results
            + self.marked_indices
            + self.input_history
            + self.active_operations
    }

    pub fn total_kb(&self) -> f64 {
        self.total_bytes() as f64 / 1024.0
    }
}

// Clone implementation for UIState (needed for state coordination)
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
        }
    }
}

// Equality comparison for state changes
impl PartialEq for UIState {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.overlay == other.overlay
            && self.selected == other.selected
            && self.input == other.input
            && self.show_hidden == other.show_hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_operations() {
        let ui_state = UIState::new();

        // Test atomic redraw flags
        ui_state.request_redraw(RedrawFlag::Main);
        assert!(ui_state.needs_redraw());

        ui_state.clear_redraw();
        assert!(!ui_state.needs_redraw());

        // Test frame counting
        ui_state.increment_frame();
        assert_eq!(ui_state.frame_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_loading_state() {
        let loading = LoadingState::new("Testing");

        loading.set_completion(50, 100);
        assert_eq!(loading.get_completion_ratio(), 0.5);

        loading.set_progress(75.0);
        assert_eq!(loading.get_progress(), 75.0);
    }

    #[test]
    fn test_notification_auto_dismiss() {
        let mut ui_state = UIState::new();

        ui_state.show_info("Test message");
        assert!(ui_state.notification.is_some());

        // Auto dismiss won't trigger immediately
        assert!(!ui_state.update_notification());
        assert!(ui_state.notification.is_some());
    }

    #[test]
    fn test_input_management() {
        let mut ui_state = UIState::new();

        ui_state.set_input("test");
        assert_eq!(ui_state.input, "test");
        assert_eq!(ui_state.input_cursor, 4);

        ui_state.insert_char('!');
        assert_eq!(ui_state.input, "test!");
        assert_eq!(ui_state.input_cursor, 5);

        assert!(ui_state.delete_char_before());
        assert_eq!(ui_state.input, "test");
        assert_eq!(ui_state.input_cursor, 4);
    }

    #[test]
    fn test_selection_management() {
        let mut ui_state = UIState::new();

        ui_state.mark_index(5);
        assert!(ui_state.marked_indices.contains(&5));

        ui_state.unmark_index(5);
        assert!(!ui_state.marked_indices.contains(&5));

        ui_state.mark_index(1);
        ui_state.mark_index(3);
        ui_state.clear_marks();
        assert!(ui_state.marked_indices.is_empty());
    }

    #[test]
    fn test_memory_usage() {
        let ui_state = UIState::new();
        let usage = ui_state.get_memory_usage();

        assert!(usage.total_bytes() > 0);
        assert!(usage.total_kb() > 0.0);
    }
}
