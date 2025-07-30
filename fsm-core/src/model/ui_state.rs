//! src/model/ui_state.rs
//! ============================================================
//! Enhanced UIState with clipr multi-selection, SIMD search, and real-time statistics
//! Optimized for high-performance clipboard operations with TSV tracing

// Change logs:
//   1. Dropped the lock guard early in start_clipboard_search() before
//   calling add_clipboard_search_to_history()
//   2. Added Debug trait bound for the operation_type parameter in
//   start_clipboard_batch_operation()
//   3. Fixed the ArcSwap clone by properly cloning the Arc contents
//   4. Extracted values before using them in
//   update_clipboard_batch_progress() to avoid borrowing conflicts

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use compact_str::CompactString;
use smallvec::SmallVec;

use clipr::{ClipBoardConfig, ClipBoardItem, ClipBoardStats, clipboard::ClipBoard};

use crate::AppError;
use crate::controller::actions::{ClipboardStats, InputPromptType, OperationId};
use crate::model::fs_state::SearchMode;

use tracing::{debug, info, instrument, trace};

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
    // clipr enhanced modes
    ClipboardMultiSelect,
    ClipboardSearch,
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
    ClipBoard,
    // clipr enhanced overlays
    ClipboardSearch,
    ClipboardStats,
    ClipboardBatchOperation,
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
// clipr enhanced data structures
// ------------------------------------------------------------

/// clipboard search state with SIMD acceleration support
#[derive(Clone, Debug)]
pub struct ClipboardSearchState {
    pub pattern: CompactString,
    pub results: Vec<u64>, // Item IDs matching search
    pub selected_result_idx: usize,
    pub search_started_at: Instant,
    pub search_duration_us: u64,
    pub use_simd: bool, // Enable SIMD acceleration for large clipboards
    pub case_sensitive: bool,
    pub regex_mode: bool,
}

impl Default for ClipboardSearchState {
    fn default() -> Self {
        Self {
            pattern: CompactString::new(""),
            results: Vec::new(),
            selected_result_idx: 0,
            search_started_at: Instant::now(),
            search_duration_us: 0,
            use_simd: true, // Default to SIMD for performance
            case_sensitive: false,
            regex_mode: false,
        }
    }
}

impl ClipboardSearchState {
    pub fn clear(&mut self) {
        self.pattern.clear();
        self.results.clear();
        self.selected_result_idx = 0;
        self.search_duration_us = 0;
    }

    pub fn set_results(&mut self, results: Vec<u64>, duration_us: u64) {
        self.results = results;
        self.selected_result_idx = 0;
        self.search_duration_us = duration_us;
    }

    pub fn next_result(&mut self) {
        if !self.results.is_empty() && self.selected_result_idx < self.results.len() - 1 {
            self.selected_result_idx += 1;
        }
    }

    pub fn prev_result(&mut self) {
        if self.selected_result_idx > 0 {
            self.selected_result_idx -= 1;
        }
    }

    pub fn selected_item_id(&self) -> Option<u64> {
        self.results.get(self.selected_result_idx).copied()
    }
}

/// clipboard batch operation progress tracking
#[derive(Clone, Debug)]
pub struct ClipboardBatchProgress {
    pub operation_id: OperationId,
    pub operation_type: CompactString, // "batch_add", "batch_paste", "batch_remove"
    pub total_items: usize,
    pub completed_items: usize,
    pub failed_items: usize,
    pub started_at: Instant,
    pub estimated_completion: Option<Instant>,
    pub current_item_name: Option<CompactString>,
    pub bytes_processed: u64,
    pub bytes_total: u64,
}

impl ClipboardBatchProgress {
    pub fn new(
        operation_id: OperationId,
        operation_type: impl Into<CompactString>,
        total_items: usize,
    ) -> Self {
        Self {
            operation_id,
            operation_type: operation_type.into(),
            total_items,
            completed_items: 0,
            failed_items: 0,
            started_at: Instant::now(),
            estimated_completion: None,
            current_item_name: None,
            bytes_processed: 0,
            bytes_total: 0,
        }
    }

    pub fn progress_ratio(&self) -> f32 {
        if self.total_items == 0 {
            return 1.0;
        }
        (self.completed_items + self.failed_items) as f32 / self.total_items as f32
    }

    pub fn update_progress(
        &mut self,
        completed: usize,
        failed: usize,
        current_item: Option<impl Into<CompactString>>,
    ) {
        self.completed_items = completed;
        self.failed_items = failed;
        self.current_item_name = current_item.map(|s| s.into());

        // Calculate estimated completion time
        let elapsed = self.started_at.elapsed();
        let progress = self.progress_ratio();
        if progress > 0.0 && progress < 1.0 {
            let total_estimated = elapsed.as_secs_f64() / progress as f64;
            let remaining = total_estimated - elapsed.as_secs_f64();
            self.estimated_completion = Some(Instant::now() + Duration::from_secs_f64(remaining));
        }
    }

    pub fn is_complete(&self) -> bool {
        self.completed_items + self.failed_items >= self.total_items
    }
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
//                    Enhanced UIState with clipr
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

    // Legacy clipboard for backward compatibility
    pub clipboard: ClipBoard,
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item_idx: usize,

    // System monitoring
    pub show_process_monitor: bool,
    pub selected_process_idx: usize,

    // ===== clipr Enhanced Features =====
    /// Multi-selection support with HashSet for O(1) lookups
    pub selected_clipboard_items: Arc<RwLock<HashSet<u64>>>,

    /// SIMD-accelerated clipboard search state
    pub clipboard_search_state: Arc<RwLock<ClipboardSearchState>>,

    /// Real-time clipboard statistics cache for instant UI updates
    pub clipboard_stats_cache: ArcSwap<Option<ClipboardStats>>,

    /// Active batch operation progress tracking
    pub clipboard_batch_progress: Option<ClipboardBatchProgress>,

    /// Search pattern history for autocomplete and recent suggestions
    pub clipboard_search_history: SmallVec<[CompactString; 16]>,

    /// Performance metrics for UI display
    pub clipboard_performance_metrics: Arc<RwLock<ClipboardPerformanceMetrics>>,
}

/// Real-time clipboard performance metrics for UI display
#[derive(Debug, Clone)]
pub struct ClipboardPerformanceMetrics {
    pub last_add_duration_us: u64,
    pub last_search_duration_us: u64,
    pub last_paste_duration_us: u64,
    pub average_add_duration_us: u64,
    pub average_search_duration_us: u64,
    pub total_operations: u64,
    pub cache_hit_ratio: f32,
    pub simd_acceleration_enabled: bool,
    pub last_updated: Instant,
}

impl Default for ClipboardPerformanceMetrics {
    fn default() -> Self {
        Self {
            last_add_duration_us: 0,
            last_search_duration_us: 0,
            last_paste_duration_us: 0,
            average_add_duration_us: 0,
            average_search_duration_us: 0,
            total_operations: 0,
            cache_hit_ratio: 0.0,
            simd_acceleration_enabled: true,
            last_updated: Instant::now(),
        }
    }
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

            // clipr enhanced features with optimized defaults
            selected_clipboard_items: Arc::new(RwLock::new(HashSet::with_capacity(64))),
            clipboard_search_state: Arc::new(RwLock::new(ClipboardSearchState::default())),
            clipboard_stats_cache: ArcSwap::new(Arc::new(None)),
            clipboard_batch_progress: None,
            clipboard_search_history: SmallVec::new(),
            clipboard_performance_metrics: Arc::new(RwLock::new(
                ClipboardPerformanceMetrics::default(),
            )),
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
// clipr Enhanced Multi-Selection Management
// ------------------------------------------------------------
impl UIState {
    /// Add item to multi-selection with atomic safety
    #[instrument(level = "debug", skip(self))]
    pub fn select_clipboard_item(&self, item_id: u64) -> bool {
        if let Ok(mut selected) = self.selected_clipboard_items.write() {
            let was_new = selected.insert(item_id);
            if was_new {
                self.request_redraw(RedrawFlag::Overlay);
                info!(
                    marker = "CLIPBOARD_ITEM_SELECTED",
                    operation_type = "clipboard_selection",
                    item_id = item_id,
                    total_selected = selected.len(),
                    "Clipboard item added to selection"
                );
            }
            return was_new;
        }
        false
    }

    /// Remove item from multi-selection
    #[instrument(level = "debug", skip(self))]
    pub fn deselect_clipboard_item(&self, item_id: u64) -> bool {
        if let Ok(mut selected) = self.selected_clipboard_items.write() {
            let was_removed = selected.remove(&item_id);
            if was_removed {
                self.request_redraw(RedrawFlag::Overlay);
                info!(
                    marker = "CLIPBOARD_ITEM_DESELECTED",
                    operation_type = "clipboard_selection",
                    item_id = item_id,
                    total_selected = selected.len(),
                    "Clipboard item removed from selection"
                );
            }
            return was_removed;
        }
        false
    }

    /// Toggle item selection state
    #[instrument(level = "debug", skip(self))]
    pub fn toggle_clipboard_item_selection(&self, item_id: u64) -> bool {
        if let Ok(selected) = self.selected_clipboard_items.read() {
            if selected.contains(&item_id) {
                drop(selected);
                self.deselect_clipboard_item(item_id)
            } else {
                drop(selected);
                self.select_clipboard_item(item_id)
            }
        } else {
            false
        }
    }

    /// Clear all selections
    #[instrument(level = "debug", skip(self))]
    pub fn clear_clipboard_selection(&self) {
        if let Ok(mut selected) = self.selected_clipboard_items.write() {
            let count = selected.len();
            selected.clear();
            if count > 0 {
                self.request_redraw(RedrawFlag::Overlay);
                info!(
                    marker = "CLIPBOARD_SELECTION_CLEARED",
                    operation_type = "clipboard_selection",
                    cleared_count = count,
                    "All clipboard selections cleared"
                );
            }
        }
    }

    /// Get selected item IDs as Vec for batch operations
    #[instrument(level = "trace", skip(self))]
    pub fn get_selected_clipboard_items(&self) -> Vec<u64> {
        self.selected_clipboard_items
            .read()
            .map(|selected| selected.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Check if item is selected
    #[instrument(level = "trace", skip(self))]
    pub fn is_clipboard_item_selected(&self, item_id: u64) -> bool {
        self.selected_clipboard_items
            .read()
            .map(|selected| selected.contains(&item_id))
            .unwrap_or(false)
    }

    /// Get selection count for UI display
    #[instrument(level = "trace", skip(self))]
    pub fn clipboard_selection_count(&self) -> usize {
        self.selected_clipboard_items
            .read()
            .map(|selected| selected.len())
            .unwrap_or(0)
    }
}

// ------------------------------------------------------------
// clipr Enhanced Search Management
// ------------------------------------------------------------
impl UIState {
    /// Start clipboard search with SIMD acceleration
    #[instrument(level = "info", skip(self, pattern))]
    pub fn start_clipboard_search(&mut self, pattern: impl Into<CompactString>, use_simd: bool) {
        let pattern_str = pattern.into();

        if let Ok(mut search_state) = self.clipboard_search_state.write() {
            search_state.pattern = pattern_str.clone();
            search_state.use_simd = use_simd;
            search_state.search_started_at = Instant::now();
            search_state.results.clear();
            search_state.selected_result_idx = 0;
        } // Drop the search_state guard here

        // Add to search history after dropping the guard
        self.add_clipboard_search_to_history(pattern_str.clone());

        self.request_redraw(RedrawFlag::Overlay);

        info!(
            marker = "CLIPBOARD_SEARCH_STARTED",
            operation_type = "clipboard_search",
            pattern = %pattern_str,
            use_simd = use_simd,
            "Clipboard search initiated"
        );
    }

    /// Update search results with performance tracking
    #[instrument(level = "info", skip(self, results))]
    pub fn update_clipboard_search_results(&self, results: Vec<u64>, duration_us: u64) {
        if let Ok(mut search_state) = self.clipboard_search_state.write() {
            search_state.set_results(results.clone(), duration_us);

            // Update performance metrics
            if let Ok(mut metrics) = self.clipboard_performance_metrics.write() {
                metrics.last_search_duration_us = duration_us;
                metrics.average_search_duration_us =
                    (metrics.average_search_duration_us + duration_us) / 2;
                metrics.last_updated = Instant::now();
            }

            self.request_redraw(RedrawFlag::Overlay);

            info!(
                marker = "CLIPBOARD_SEARCH_RESULTS_UPDATED",
                operation_type = "clipboard_search",
                results_count = results.len(),
                duration_us = duration_us,
                "Clipboard search results updated"
            );
        }
    }

    /// Clear search state
    #[instrument(level = "debug", skip(self))]
    pub fn clear_clipboard_search(&self) {
        if let Ok(mut search_state) = self.clipboard_search_state.write() {
            search_state.clear();
            self.request_redraw(RedrawFlag::Overlay);

            debug!(
                marker = "CLIPBOARD_SEARCH_CLEARED",
                operation_type = "clipboard_search",
                "Clipboard search state cleared"
            );
        }
    }

    /// Navigate search results
    #[instrument(level = "trace", skip(self))]
    pub fn next_clipboard_search_result(&self) {
        if let Ok(mut search_state) = self.clipboard_search_state.write() {
            search_state.next_result();
            self.request_redraw(RedrawFlag::Overlay);
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn prev_clipboard_search_result(&self) {
        if let Ok(mut search_state) = self.clipboard_search_state.write() {
            search_state.prev_result();
            self.request_redraw(RedrawFlag::Overlay);
        }
    }

    /// Get current search state for UI rendering
    #[instrument(level = "trace", skip(self))]
    pub fn get_clipboard_search_state(&self) -> Option<ClipboardSearchState> {
        self.clipboard_search_state
            .read()
            .ok()
            .map(|state| state.clone())
    }

    /// Add search pattern to history with deduplication
    fn add_clipboard_search_to_history(&mut self, pattern: CompactString) {
        // Remove existing pattern if present
        if let Some(pos) = self
            .clipboard_search_history
            .iter()
            .position(|p| p == &pattern)
        {
            self.clipboard_search_history.remove(pos);
        }

        // Add to front
        self.clipboard_search_history.insert(0, pattern);

        // Limit history size
        if self.clipboard_search_history.len() > 16 {
            self.clipboard_search_history.truncate(16);
        }
    }
}

// ------------------------------------------------------------
// clipr Statistics and Performance Management
// ------------------------------------------------------------
impl UIState {
    /// Update clipboard statistics cache for instant UI access
    #[instrument(level = "debug", skip(self, stats))]
    pub fn update_clipboard_stats_cache(&self, stats: ClipboardStats) {
        self.clipboard_stats_cache.store(Arc::new(Some(stats)));
        self.request_redraw(RedrawFlag::StatusBar);

        debug!(
            marker = "CLIPBOARD_STATS_CACHE_UPDATED",
            operation_type = "clipboard_stats",
            "Clipboard statistics cache updated"
        );
    }

    /// Get cached clipboard statistics for UI display
    #[instrument(level = "trace", skip(self))]
    pub fn get_clipboard_stats_cache(&self) -> Option<ClipboardStats> {
        self.clipboard_stats_cache.load().as_ref().clone()
    }

    /// Update performance metrics
    #[instrument(level = "debug", skip(self))]
    pub fn update_clipboard_performance_metrics(
        &self,
        operation_type: &str,
        duration_us: u64,
        simd_enabled: bool,
    ) {
        if let Ok(mut metrics) = self.clipboard_performance_metrics.write() {
            match operation_type {
                "add" => {
                    metrics.last_add_duration_us = duration_us;
                    metrics.average_add_duration_us =
                        (metrics.average_add_duration_us + duration_us) / 2;
                }
                "search" => {
                    metrics.last_search_duration_us = duration_us;
                    metrics.average_search_duration_us =
                        (metrics.average_search_duration_us + duration_us) / 2;
                }
                "paste" => {
                    metrics.last_paste_duration_us = duration_us;
                }
                _ => {}
            }

            metrics.total_operations += 1;
            metrics.simd_acceleration_enabled = simd_enabled;
            metrics.last_updated = Instant::now();

            debug!(
                marker = "CLIPBOARD_PERFORMANCE_METRICS_UPDATED",
                operation_type = "clipboard_performance",
                operation = operation_type,
                duration_us = duration_us,
                simd_enabled = simd_enabled,
                "Performance metrics updated"
            );
        }
    }

    /// Get performance metrics for UI display
    #[instrument(level = "trace", skip(self))]
    pub fn get_clipboard_performance_metrics(&self) -> Option<ClipboardPerformanceMetrics> {
        self.clipboard_performance_metrics
            .read()
            .ok()
            .map(|m| m.clone())
    }
}

// ------------------------------------------------------------
// clipr Batch Operation Progress Management
// ------------------------------------------------------------
impl UIState {
    /// Start tracking batch operation progress
    #[instrument(level = "info", skip(self, operation_type))]
    pub fn start_clipboard_batch_operation(
        &mut self,
        operation_id: OperationId,
        operation_type: impl Into<CompactString> + std::fmt::Debug,
        total_items: usize,
    ) {
        let progress =
            ClipboardBatchProgress::new(operation_id.clone(), operation_type, total_items);
        self.clipboard_batch_progress = Some(progress);
        self.request_redraw(RedrawFlag::Overlay);

        info!(
            marker = "CLIPBOARD_BATCH_OPERATION_STARTED",
            operation_type = "clipboard_batch_operation",
            operation_id = %operation_id,
            total_items = total_items,
            "Batch clipboard operation started"
        );
    }

    /// Update batch operation progress
    #[instrument(level = "debug", skip(self, current_item))]
    pub fn update_clipboard_batch_progress(
        &mut self,
        completed: usize,
        failed: usize,
        current_item: Option<impl Into<CompactString>>,
    ) {
        if let Some(ref mut progress) = self.clipboard_batch_progress {
            progress.update_progress(completed, failed, current_item);
            let operation_id = progress.operation_id.clone();
            let progress_ratio = progress.progress_ratio();

            self.request_redraw(RedrawFlag::Overlay);

            debug!(
                marker = "CLIPBOARD_BATCH_PROGRESS_UPDATED",
                operation_type = "clipboard_batch_operation",
                operation_id = %operation_id,
                completed = completed,
                failed = failed,
                progress_ratio = progress_ratio,
                "Batch operation progress updated"
            );
        }
    }

    /// Complete batch operation
    #[instrument(level = "info", skip(self))]
    pub fn complete_clipboard_batch_operation(&mut self) {
        if let Some(progress) = self.clipboard_batch_progress.take() {
            self.request_redraw(RedrawFlag::Overlay);

            info!(
                            marker = "CLIPBOARD_BATCH_OPERATION_COMPLETED",
                            operation_type = "clipboard_batch_operation",
                            operation_id = %progress.operation_id,
                            total_items = progress.total_items,
                            completed_items = progress.completed_items,
                            failed_items = progress.failed_items,
                            duration_ms =
            progress.started_at.elapsed().as_millis(),
                            "Batch clipboard operation completed"
                        );
        }
    }

    /// Cancel batch operation
    #[instrument(level = "warn", skip(self))]
    pub fn cancel_clipboard_batch_operation(&mut self) {
        if let Some(progress) = self.clipboard_batch_progress.take() {
            self.request_redraw(RedrawFlag::Overlay);

            info!(
                marker = "CLIPBOARD_BATCH_OPERATION_CANCELLED",
                operation_type = "clipboard_batch_operation",
                operation_id = %progress.operation_id,
                "Batch clipboard operation cancelled"
            );
        }
    }
}

// ------------------------------------------------------------
// Existing methods (unchanged)
// ------------------------------------------------------------
impl UIState {
    // Prompt / command input helpers
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

    // Notification helpers
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
                    notification = ?self.notification.as_ref().map(|n|
        (&n.level, &n.timestamp, &n.auto_dismiss_ms))
                );

        if let Some(n) = &self.notification {
            if let Some(auto_ms) = n.auto_dismiss_ms
                && n.timestamp.elapsed().as_millis() > auto_ms as u128
            {
                debug!(
                    "auto-dismissing notification (level={:?}) after 
  {}ms",
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

    // Legacy clipboard overlay helpers (maintained for compatibility)
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
        self.clear_clipboard_selection();
        self.request_redraw(RedrawFlag::All);
    }

    // Simple FPS metric (UI only – not thread-safe)
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
// Cheap Clone – atomics copied by value with clipr fields
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

            // clipr enhanced fields - Arc clones are cheap
            selected_clipboard_items: Arc::clone(&self.selected_clipboard_items),
            clipboard_search_state: Arc::clone(&self.clipboard_search_state),
            clipboard_stats_cache: ArcSwap::new(Arc::clone(&self.clipboard_stats_cache.load())),
            clipboard_batch_progress: self.clipboard_batch_progress.clone(),
            clipboard_search_history: self.clipboard_search_history.clone(),
            clipboard_performance_metrics: Arc::clone(&self.clipboard_performance_metrics),
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
