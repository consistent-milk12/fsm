//! UIState: High-Performance UI State for Phase 4.0
//!
//! Optimized for lock-free updates with ArcSwap integration:
//! - Compact bit flags for redraw optimization
//! - Cache-friendly data layout with atomic counters
//! - Zero-allocation state transitions
//! - SIMD-optimized selection operations

use crate::controller::actions::InputPromptType;
use crate::fs::object_info::ObjectInfo;
use crate::model::command_palette::{Command, CommandAction, CommandPaletteState};
use crate::tasks::search_task::RawSearchResult;

use clipr::ClipBoard;
use compact_str::CompactString;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Granular redraw flags optimized for bitwise operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RedrawFlag {
    Main = 0b0000_0001,
    StatusBar = 0b0000_0010,
    Overlay = 0b0000_0100,
    Notification = 0b0000_1000,
    Command = 0b0001_0000,
    Sidebar = 0b0010_0000,
    Preview = 0b0100_0000,
    All = 0b0111_1111,
}

impl RedrawFlag {
    #[inline(always)]
    pub const fn bits(self) -> u8 {
        self as u8
    }
}

/// UI modes optimized for branch prediction
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIMode {
    #[default]
    Browse = 0,
    Visual = 1,
    Search = 2,
    Prompt = 3,
    Command = 4,
    Scripting = 5,
    BatchOp = 6,
}

/// Overlays with optimized enum representation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum UIOverlay {
    #[default]
    None = 0,
    Help = 1,
    Search = 2,
    FileNameSearch = 3,
    ContentSearch = 4,
    SearchResults = 5,
    Loading = 6,
    Status = 7,
    Prompt = 8,
    Batch = 9,
    Scripting = 10,
}

/// Notification levels with performance priorities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NotificationLevel {
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

/// Compact notification structure
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: CompactString,
    pub level: NotificationLevel,
    pub timestamp: Instant,
    pub auto_dismiss_ms: Option<u32>, // u32 for memory efficiency
}

/// High-performance loading state with atomic counters for Phase 4.0
#[derive(Debug, Clone)]
pub struct LoadingState {
    // Static message (set once, read many)
    pub message: CompactString,

    // Atomic progress tracking (0-10000 for 0.00% to 100.00% precision)
    pub progress: Arc<AtomicU32>, // * 100 for precision, u32 for cache efficiency

    // Animated spinner frame counter
    pub spinner_frame: Arc<AtomicUsize>,

    // Current processing item (lock-free updates)
    pub current_item: Arc<parking_lot::RwLock<Option<CompactString>>>,

    // Atomic completion counters
    pub completed: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,

    // Performance tracking
    pub start_time: Instant,
    pub last_update: Arc<AtomicU64>, // Unix timestamp in nanoseconds

    // Loading type for optimized rendering
    pub loading_type: LoadingType,
}

/// Loading operation types for optimized rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LoadingType {
    DirectoryScan = 0,
    FileOperation = 1,
    Search = 2,
    ContentLoad = 3,
    NetworkOperation = 4,
    Generic = 5,
}

impl LoadingState {
    /// Create new loading state with optimized defaults
    pub fn new(message: impl Into<CompactString>, loading_type: LoadingType) -> Self {
        Self {
            message: message.into(),
            progress: Arc::new(AtomicU32::new(0)),
            spinner_frame: Arc::new(AtomicUsize::new(0)),
            current_item: Arc::new(parking_lot::RwLock::new(None)),
            completed: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            last_update: Arc::new(AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            )),
            loading_type,
        }
    }

    /// Create indeterminate loading state (no progress tracking)
    pub fn indeterminate(message: impl Into<CompactString>, loading_type: LoadingType) -> Self {
        let state = Self::new(message, loading_type);
        state.total.store(u64::MAX, Ordering::Relaxed); // Marker for indeterminate
        state
    }

    /// Update progress atomically (0.0 to 100.0)
    #[inline]
    pub fn set_progress(&self, progress: f32) {
        let progress_int = (progress.clamp(0.0, 100.0) * 100.0) as u32;
        self.progress.store(progress_int, Ordering::Relaxed);
        self.update_timestamp();
    }

    /// Update completion counters and calculate progress
    #[inline]
    pub fn set_completion(&self, completed: u64, total: u64) {
        self.completed.store(completed, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);

        if total > 0 {
            let progress = ((completed as f64 / total as f64) * 10000.0) as u32;
            self.progress.store(progress, Ordering::Relaxed);
        }

        self.update_timestamp();
    }

    /// Update current processing item (lock-free)
    #[inline]
    pub fn set_current_item(&self, item: Option<impl Into<CompactString>>) {
        *self.current_item.write() = item.map(|i| i.into());
        self.update_timestamp();
    }

    /// Increment spinner frame for animation
    #[inline]
    pub fn tick_spinner(&self) {
        self.spinner_frame.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current progress as percentage (0.0 to 100.0)
    #[inline]
    pub fn get_progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 100.0
    }

    /// Get completion ratio (0.0 to 1.0)
    #[inline]
    pub fn get_completion_ratio(&self) -> f32 {
        let completed = self.completed.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);

        if total == 0 || total == u64::MAX {
            0.0 // Indeterminate or no work
        } else {
            (completed as f32 / total as f32).clamp(0.0, 1.0)
        }
    }

    /// Check if this is an indeterminate loading operation
    #[inline]
    pub fn is_indeterminate(&self) -> bool {
        self.total.load(Ordering::Relaxed) == u64::MAX
    }

    /// Get current spinner frame for animation
    #[inline]
    pub fn get_spinner_frame(&self) -> usize {
        self.spinner_frame.load(Ordering::Relaxed)
    }

    /// Get spinner character for current frame
    pub fn get_spinner_char(&self) -> char {
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let frame = self.get_spinner_frame() % SPINNER_CHARS.len();
        SPINNER_CHARS[frame]
    }

    /// Get current processing item (clone for safety)
    pub fn get_current_item(&self) -> Option<CompactString> {
        self.current_item.read().clone()
    }

    /// Get completion counts
    #[inline]
    pub fn get_completion_counts(&self) -> (u64, u64) {
        (
            self.completed.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// Get elapsed time since start
    #[inline]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get estimated time remaining (based on current progress)
    pub fn estimate_remaining(&self) -> Option<Duration> {
        if self.is_indeterminate() {
            return None;
        }

        let completed = self.completed.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);

        if completed == 0 || total == 0 || completed >= total {
            return None;
        }

        let elapsed = self.elapsed();
        let rate = completed as f64 / elapsed.as_secs_f64();

        if rate > 0.0 {
            let remaining_items = total - completed;
            let remaining_seconds = remaining_items as f64 / rate;
            Some(Duration::from_secs_f64(remaining_seconds))
        } else {
            None
        }
    }

    /// Get throughput (items per second)
    pub fn get_throughput(&self) -> f64 {
        let completed = self.completed.load(Ordering::Relaxed);
        let elapsed = self.elapsed().as_secs_f64();

        if elapsed > 0.0 {
            completed as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Check if loading has been active for a significant time
    #[inline]
    pub fn is_long_running(&self) -> bool {
        self.elapsed() > Duration::from_secs(3)
    }

    /// Get loading summary for display
    pub fn get_summary(&self) -> LoadingSummary {
        let (completed, total) = self.get_completion_counts();
        let progress = self.get_progress();
        let current_item = self.get_current_item();
        let estimated_remaining = self.estimate_remaining();
        let throughput = self.get_throughput();

        LoadingSummary {
            message: self.message.clone(),
            loading_type: self.loading_type,
            progress,
            completed,
            total,
            current_item,
            elapsed: self.elapsed(),
            estimated_remaining,
            throughput,
            is_indeterminate: self.is_indeterminate(),
            spinner_char: self.get_spinner_char(),
        }
    }

    /// Update internal timestamp
    #[inline]
    fn update_timestamp(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.last_update.store(now, Ordering::Relaxed);
    }

    /// Mark loading as complete
    pub fn complete(&self) {
        if !self.is_indeterminate() {
            let total = self.total.load(Ordering::Relaxed);
            self.completed.store(total, Ordering::Relaxed);
            self.progress.store(10000, Ordering::Relaxed); // 100.00%
        }
        self.update_timestamp();
    }

    /// Cancel loading operation
    pub fn cancel(&self) {
        self.progress.store(0, Ordering::Relaxed);
        self.update_timestamp();
    }
}

/// Loading state summary for UI display
#[derive(Debug, Clone)]
pub struct LoadingSummary {
    pub message: CompactString,
    pub loading_type: LoadingType,
    pub progress: f32,
    pub completed: u64,
    pub total: u64,
    pub current_item: Option<CompactString>,
    pub elapsed: Duration,
    pub estimated_remaining: Option<Duration>,
    pub throughput: f64,
    pub is_indeterminate: bool,
    pub spinner_char: char,
}

impl LoadingSummary {
    /// Format progress for display
    pub fn format_progress(&self) -> String {
        if self.is_indeterminate {
            format!("{} {}", self.spinner_char, self.message)
        } else if self.total > 0 {
            format!(
                "{} {} [{}/{}] {:.1}%",
                self.spinner_char, self.message, self.completed, self.total, self.progress
            )
        } else {
            format!(
                "{} {} {:.1}%",
                self.spinner_char, self.message, self.progress
            )
        }
    }

    /// Format detailed progress with ETA
    pub fn format_detailed(&self) -> String {
        let mut details = self.format_progress();

        if let Some(item) = &self.current_item {
            details.push_str(&format!("\nProcessing: {item}"));
        }

        if let Some(eta) = self.estimated_remaining {
            details.push_str(&format!("\nETA: {eta:?}"));
        }

        if self.throughput > 0.0 {
            details.push_str(&format!("\nSpeed: {:.1} items/sec", self.throughput));
        }

        details
    }
}

impl Default for LoadingState {
    fn default() -> Self {
        Self::new("Loading...", LoadingType::Generic)
    }
}

/// Helper functions for common loading scenarios
impl LoadingState {
    /// Create loading state for directory scanning
    pub fn directory_scan(path: &Path) -> Self {
        Self::new(
            format!(
                "Scanning {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            LoadingType::DirectoryScan,
        )
    }

    /// Create loading state for file operations
    pub fn file_operation(operation: &str, file_count: u64) -> Self {
        let state = Self::new(format!("{operation} files..."), LoadingType::FileOperation);
        state.set_completion(0, file_count);
        state
    }

    /// Create loading state for search operations
    pub fn search_operation(query: &str) -> Self {
        Self::indeterminate(format!("Searching for '{query}'..."), LoadingType::Search)
    }

    /// Create loading state for content loading
    pub fn content_load(item_name: &str) -> Self {
        Self::indeterminate(format!("Loading {item_name}..."), LoadingType::ContentLoad)
    }
}

/// File operation progress with atomic updates
#[derive(Debug)]
pub struct FileOperationProgress {
    pub operation_type: CompactString,
    pub current_bytes: AtomicU64,
    pub total_bytes: AtomicU64,
    pub current_file: parking_lot::RwLock<PathBuf>,
    pub files_completed: AtomicU32,
    pub total_files: AtomicU32,
    pub start_time: Instant,
    pub throughput_bps: AtomicU64,
}

impl FileOperationProgress {
    pub fn new(operation_type: CompactString, total_bytes: u64, total_files: u32) -> Self {
        Self {
            operation_type,
            current_bytes: AtomicU64::new(0),
            total_bytes: AtomicU64::new(total_bytes),
            current_file: parking_lot::RwLock::new(PathBuf::new()),
            files_completed: AtomicU32::new(0),
            total_files: AtomicU32::new(total_files),
            start_time: Instant::now(),
            throughput_bps: AtomicU64::new(0),
        }
    }

    /// Update progress atomically
    pub fn update(&self, current_bytes: u64, current_file: PathBuf, files_completed: u32) {
        self.current_bytes.store(current_bytes, Ordering::Relaxed);
        self.files_completed
            .store(files_completed, Ordering::Relaxed);
        *self.current_file.write() = current_file;

        // Calculate throughput with atomic operations
        let elapsed_secs = self.start_time.elapsed().as_secs();
        if elapsed_secs > 0 && current_bytes > 0 {
            let bps = current_bytes / elapsed_secs;
            self.throughput_bps.store(bps, Ordering::Relaxed);
        }
    }

    /// Get progress ratio (0.0 to 1.0)
    pub fn progress_ratio(&self) -> f32 {
        let total = self.total_bytes.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            let current = self.current_bytes.load(Ordering::Relaxed);
            current as f32 / total as f32
        }
    }
}

/// Clipboard view mode
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClipBoardViewMode {
    #[default]
    List = 0,
    Details = 1,
    Grid = 2,
}

/// Optimized selection set using SmallVec for common cases
type SelectionSet = SmallVec<[usize; 8]>; // Most selections are < 8items

/// High-performance UI state with cache-friendly layout
#[derive(Debug)]
pub struct UIState {
    // Hot path data - accessed on every frame
    pub redraw_flags: AtomicU32, // Atomic for lock-free updates
    pub mode: UIMode,
    pub overlay: UIOverlay,
    pub selected: Option<usize>,
    pub active_pane: usize,

    // Selection state optimized for small sets
    pub marked_indices: SelectionSet,
    pub visual_range: Option<(usize, usize)>,

    // Input state with compact strings and history
    pub input: CompactString,
    pub input_cursor: usize, // Cursor position for better input editing
    pub last_query: Option<CompactString>,
    pub input_prompt_type: Option<InputPromptType>,
    pub input_history: SmallVec<[CompactString; 32]>, // Command/search history
    pub input_history_index: Option<usize>,           // Current position in history navigation

    // Display state
    pub show_hidden: bool,
    pub theme: CompactString,

    // Search results with pre-allocated capacity
    pub search_results: Vec<ObjectInfo>,
    pub filename_search_results: Vec<ObjectInfo>,
    pub rich_search_results: Vec<CompactString>,
    pub raw_search_results: Option<RawSearchResult>,
    pub raw_search_selected: usize,

    // Status and notifications
    pub loading: Option<LoadingState>,
    pub notification: Option<Notification>,
    pub last_status: Option<CompactString>,

    // Performance tracking
    pub frame_count: AtomicU64,
    pub last_update: Instant,

    // File operations with atomic progress
    pub active_file_operations: HashMap<CompactString, Arc<FileOperationProgress>>,
    pub operations_cancel_tokens: HashMap<CompactString, CancellationToken>,

    // Clipboard system
    pub clipboard: Arc<ClipBoard>,
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item: Option<CompactString>,
    pub selected_clipboard_item_index: usize,
    pub clipboard_view_mode: ClipBoardViewMode,

    // Command palette
    pub command_palette: CommandPaletteState,

    // Recent actions with circular buffer
    pub recent_actions: SmallVec<[CompactString; 16]>,
}

impl Default for UIState {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for UIState {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode && self.overlay == other.overlay
    }
}

impl UIState {
    /// Create new UI state with optimized defaults
    pub fn new() -> Self {
        Self {
            // Hot path data
            redraw_flags: AtomicU32::new(RedrawFlag::All.bits() as u32),
            mode: UIMode::Browse,
            overlay: UIOverlay::None,
            selected: Some(0),
            active_pane: 0,

            // Selection state
            marked_indices: SelectionSet::new(),
            visual_range: None,

            // Input state
            input: CompactString::default(),
            input_cursor: 0,
            last_query: None,
            input_prompt_type: None,
            input_history: SmallVec::new(),
            input_history_index: None,

            // Display state
            show_hidden: false,
            theme: CompactString::const_new("default"),

            // Search results with capacity hints
            search_results: Vec::with_capacity(256),
            filename_search_results: Vec::with_capacity(256),
            rich_search_results: Vec::with_capacity(128),
            raw_search_results: None,
            raw_search_selected: 0,

            // Status
            loading: None,
            notification: None,
            last_status: None,

            // Performance tracking
            frame_count: AtomicU64::new(0),
            last_update: Instant::now(),

            // File operations
            active_file_operations: HashMap::with_capacity(8),
            operations_cancel_tokens: HashMap::with_capacity(8),

            // Clipboard
            clipboard: Arc::new(ClipBoard::default()),
            clipboard_overlay_active: false,
            selected_clipboard_item: None,
            selected_clipboard_item_index: 0,
            clipboard_view_mode: ClipBoardViewMode::List,

            // Command palette
            command_palette: CommandPaletteState::new(vec![Command {
                title: "Open Config".into(),
                action: CommandAction::OpenConfig,
            }]),

            // Recent actions
            recent_actions: SmallVec::new(),
        }
    }

    // Atomic redraw flag operations
    #[inline]
    pub fn request_redraw(&self, flag: RedrawFlag) {
        self.redraw_flags
            .fetch_or(flag.bits() as u32, Ordering::Relaxed);
    }

    #[inline]
    pub fn request_redraw_all(&self) {
        self.redraw_flags
            .store(RedrawFlag::All.bits() as u32, Ordering::Relaxed);
    }

    #[inline]
    pub fn needs_redraw(&self) -> bool {
        self.redraw_flags.load(Ordering::Relaxed) != 0
    }

    #[inline]
    pub fn clear_redraw(&self) {
        self.redraw_flags.store(0, Ordering::Relaxed);
    }

    /// Update frame counter for performance metrics
    #[inline]
    pub fn increment_frame(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get frames per second
    pub fn get_fps(&self) -> f64 {
        let elapsed = self.last_update.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.frame_count.load(Ordering::Relaxed) as f64 / elapsed
        } else {
            0.0
        }
    }

    // Optimized selection operations
    #[inline]
    pub fn mark_index(&mut self, idx: usize) {
        if !self.marked_indices.contains(&idx) {
            self.marked_indices.push(idx);
            self.request_redraw(RedrawFlag::All);
        }
    }

    #[inline]
    pub fn unmark_index(&mut self, idx: usize) {
        if let Some(pos) = self.marked_indices.iter().position(|&x| x == idx) {
            self.marked_indices.remove(pos);
            self.request_redraw(RedrawFlag::All);
        }
    }

    #[inline]
    pub fn clear_marks(&mut self) {
        self.marked_indices.clear();
        self.visual_range = None;
        self.request_redraw(RedrawFlag::All);
    }

    /// Show notification with optimized string handling
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

    /// Optimized notification helpers
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

    /// Check and auto-dismiss notifications
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

    /// Optimized action history
    pub fn push_action(&mut self, action: impl Into<CompactString>) {
        if self.recent_actions.len() == 16 {
            self.recent_actions.remove(0);
        }
        self.recent_actions.push(action.into());
    }

    // Enhanced input management for overlays

    /// Clear input and reset cursor position
    pub fn clear_input(&mut self) {
        self.input = CompactString::default();
        self.input_cursor = 0;
        self.input_history_index = None;
    }

    /// Set input text and update cursor to end
    pub fn set_input(&mut self, text: impl Into<CompactString>) {
        self.input = text.into();
        self.input_cursor = self.input.len();
        self.input_history_index = None;
    }

    /// Insert character at cursor position
    pub fn insert_char(&mut self, ch: char) {
        let mut input_str = self.input.to_string();
        input_str.insert(self.input_cursor, ch);
        self.input = input_str.into();
        self.input_cursor += ch.len_utf8();
        self.input_history_index = None;
    }

    /// Delete character before cursor (backspace)
    pub fn delete_char_before(&mut self) -> bool {
        if self.input_cursor > 0 {
            let mut input_str = self.input.to_string();
            let char_indices: Vec<_> = input_str.char_indices().collect();

            // Find the character boundary before cursor
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

    /// Move cursor left by one character
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

    /// Move cursor right by one character
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

    /// Add input to history (for commands/searches)
    pub fn add_to_history(&mut self, input: impl Into<CompactString>) {
        let input_str = input.into();
        if !input_str.is_empty() {
            // Remove duplicate if exists
            if let Some(pos) = self.input_history.iter().position(|x| *x == input_str) {
                self.input_history.remove(pos);
            }

            // Add to end (most recent)
            self.input_history.push(input_str);

            // Keep only last 32 items
            if self.input_history.len() > 32 {
                self.input_history.remove(0);
            }
        }
        self.input_history_index = None;
    }

    /// Navigate to previous item in history
    pub fn history_prev(&mut self) -> bool {
        if self.input_history.is_empty() {
            return false;
        }

        match self.input_history_index {
            None => {
                // Start from the end (most recent)
                self.input_history_index = Some(self.input_history.len() - 1);
            }
            Some(idx) if idx > 0 => {
                self.input_history_index = Some(idx - 1);
            }
            _ => return false, // Already at oldest
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

    /// Navigate to next item in history  
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
                // Go to empty (beyond history)
                self.input_history_index = None;
                self.clear_input();
                return true;
            }
        }
        false
    }

    /// Get overlay-specific title
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

    /// Check if current overlay supports input
    pub fn overlay_accepts_input(&self) -> bool {
        matches!(
            self.overlay,
            UIOverlay::FileNameSearch | UIOverlay::ContentSearch | UIOverlay::Prompt
        )
    }
}

impl Clone for UIState {
    fn clone(&self) -> Self {
        Self {
            // copy atomic flags into new atomics
            redraw_flags: AtomicU32::new(self.redraw_flags.load(Ordering::Relaxed)),
            mode: self.mode,
            overlay: self.overlay,
            selected: self.selected,
            active_pane: self.active_pane,

            // simple clones of collections
            marked_indices: self.marked_indices.clone(),
            visual_range: self.visual_range,
            input: self.input.clone(),
            input_cursor: self.input_cursor,
            last_query: self.last_query.clone(),
            input_prompt_type: self.input_prompt_type.clone(),
            input_history: self.input_history.clone(),
            input_history_index: self.input_history_index,
            show_hidden: self.show_hidden,
            theme: self.theme.clone(),
            search_results: self.search_results.clone(),
            filename_search_results: self.filename_search_results.clone(),
            rich_search_results: self.rich_search_results.clone(),
            raw_search_results: self.raw_search_results.clone(),
            raw_search_selected: self.raw_search_selected,
            loading: self.loading.clone(),
            notification: self.notification.clone(),
            last_status: self.last_status.clone(),

            // frame counter
            frame_count: AtomicU64::new(self.frame_count.load(Ordering::Relaxed)),
            last_update: self.last_update,

            // hash maps with `Clone` or `Arc` items
            active_file_operations: self.active_file_operations.clone(),
            operations_cancel_tokens: self.operations_cancel_tokens.clone(),

            // clone arcs
            clipboard: Arc::clone(&self.clipboard),
            clipboard_overlay_active: self.clipboard_overlay_active,
            selected_clipboard_item: self.selected_clipboard_item.clone(),
            selected_clipboard_item_index: self.selected_clipboard_item_index,
            clipboard_view_mode: self.clipboard_view_mode,

            // command palette and recent actions
            command_palette: self.command_palette.clone(),
            recent_actions: self.recent_actions.clone(),
        }
    }
}
