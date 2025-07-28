//! FSState: Enhanced filesystem state with action integration

use crate::fs::object_info::ObjectInfo;
use compact_str::CompactString;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// Enhanced sort modes with performance hints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntrySort {
    NameAsc = 0,
    NameDesc = 1,
    SizeAsc = 2,
    SizeDesc = 3,
    ModifiedAsc = 4,
    ModifiedDesc = 5,
    TypeAsc = 6,
    TypeDesc = 7,
}

impl Default for EntrySort {
    fn default() -> Self {
        Self::NameAsc
    }
}

/// Enhanced filter modes with regex support
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryFilter {
    All,
    FilesOnly,
    DirsOnly,
    Extension(CompactString),
    Pattern(CompactString),
    Regex(CompactString),
    SizeRange(u64, u64),
    DateRange(u64, u64),
    Hidden(bool),
    Marked,
}

impl Default for EntryFilter {
    fn default() -> Self {
        Self::All
    }
}

/// Optimized pane state with action integration
#[derive(Debug)]
pub struct PaneState {
    // Core directory state
    pub cwd: PathBuf,
    pub entries: Vec<ObjectInfo>,

    // Atomic selection state
    pub selected: AtomicUsize,
    pub scroll_offset: AtomicUsize,
    pub viewport_height: AtomicUsize,

    // Loading and operation state
    pub is_loading: AtomicBool,
    pub is_incremental_loading: AtomicBool,
    pub operation_in_progress: AtomicBool,

    // Sorting and filtering
    pub sort: EntrySort,
    pub filter: EntryFilter,
    pub show_hidden: AtomicBool,

    // Error state
    pub last_error: Option<CompactString>,

    // Incremental loading
    pub incremental_entries: parking_lot::RwLock<Vec<ObjectInfo>>,
    pub expected_entries: AtomicUsize,

    // Performance metrics
    pub entries_loaded: AtomicUsize,
    pub last_scan_duration: AtomicU64,
    pub last_sort_duration: AtomicU64,

    // Search state integration
    pub search_results: Vec<ObjectInfo>,
    pub search_query: Option<CompactString>,
    pub search_mode: SearchMode,

    // Selection state
    pub marked_entries: HashMap<PathBuf, Instant>,
    pub clipboard_selection: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    None,
    FileName,
    Content,
    Advanced,
}

impl PaneState {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            entries: Vec::with_capacity(512),
            selected: AtomicUsize::new(0),
            scroll_offset: AtomicUsize::new(0),
            viewport_height: AtomicUsize::new(20),
            is_loading: AtomicBool::new(false),
            is_incremental_loading: AtomicBool::new(false),
            operation_in_progress: AtomicBool::new(false),
            sort: EntrySort::NameAsc,
            filter: EntryFilter::All,
            show_hidden: AtomicBool::new(false),
            last_error: None,
            incremental_entries: parking_lot::RwLock::new(Vec::with_capacity(512)),
            expected_entries: AtomicUsize::new(0),
            entries_loaded: AtomicUsize::new(0),
            last_scan_duration: AtomicU64::new(0),
            last_sort_duration: AtomicU64::new(0),
            search_results: Vec::new(),
            search_query: None,
            search_mode: SearchMode::None,
            marked_entries: HashMap::new(),
            clipboard_selection: Vec::new(),
        }
    }

    /// Enhanced set_entries with action support
    pub fn set_entries(&mut self, mut entries: Vec<ObjectInfo>) {
        let start = Instant::now();

        // Apply filter
        self.apply_filter(&mut entries);

        // SIMD-optimized sorting
        let sort_start = Instant::now();
        self.sort_entries_optimized(&mut entries);
        self.last_sort_duration
            .store(sort_start.elapsed().as_micros() as u64, Ordering::Relaxed);

        self.entries = entries;
        self.selected.store(0, Ordering::Relaxed);
        self.scroll_offset.store(0, Ordering::Relaxed);
        self.entries_loaded
            .store(self.entries.len(), Ordering::Relaxed);

        let duration_us = start.elapsed().as_micros() as u64;
        self.last_scan_duration
            .store(duration_us, Ordering::Relaxed);
    }

    /// Action-compatible selection movement
    pub fn move_selection_up(&self) -> bool {
        let current = self.selected.load(Ordering::Relaxed);
        if current > 0 {
            let new_selected = current - 1;
            self.selected.store(new_selected, Ordering::Relaxed);
            self.adjust_scroll_for_selection(new_selected);
            true
        } else {
            false
        }
    }

    pub fn move_selection_down(&self) -> bool {
        let current = self.selected.load(Ordering::Relaxed);
        if current + 1 < self.entries.len() {
            let new_selected = current + 1;
            self.selected.store(new_selected, Ordering::Relaxed);
            self.adjust_scroll_for_selection(new_selected);
            true
        } else {
            false
        }
    }

    /// Enhanced navigation methods
    pub fn select_first(&self) {
        if !self.entries.is_empty() {
            self.selected.store(0, Ordering::Relaxed);
            self.scroll_offset.store(0, Ordering::Relaxed);
        }
    }

    pub fn select_last(&self) {
        if !self.entries.is_empty() {
            let last_idx = self.entries.len() - 1;
            self.selected.store(last_idx, Ordering::Relaxed);
            let viewport_height = self.viewport_height.load(Ordering::Relaxed);
            let new_scroll = last_idx.saturating_sub(viewport_height - 1);
            self.scroll_offset.store(new_scroll, Ordering::Relaxed);
        }
    }

    pub fn page_up(&self) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current = self.selected.load(Ordering::Relaxed);
        let new_selected = current.saturating_sub(viewport_height);
        self.selected.store(new_selected, Ordering::Relaxed);
        self.adjust_scroll_for_selection(new_selected);
    }

    pub fn page_down(&self) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current = self.selected.load(Ordering::Relaxed);
        let new_selected = (current + viewport_height).min(self.entries.len().saturating_sub(1));
        self.selected.store(new_selected, Ordering::Relaxed);
        self.adjust_scroll_for_selection(new_selected);
    }

    pub fn mark_selected(&self) -> bool {
        let selected_idx: usize = self.selected.load(Ordering::Relaxed);

        if self.entries.get(selected_idx).is_some() {
            // Return the path to mark, don't modify here
            true
        } else {
            false
        }
    }

    pub fn unmark_selected(&mut self) {
        if let Some(entry) = self.clone().selected_entry() {
            self.marked_entries.remove(&entry.path);
        }
    }

    pub fn is_marked(&self, path: &PathBuf) -> bool {
        self.marked_entries.contains_key(path)
    }

    pub fn clear_marks(&mut self) {
        self.marked_entries.clear();
    }

    pub fn get_selected_path(&self) -> Option<PathBuf> {
        let selected_idx = self.selected.load(Ordering::Relaxed);
        self.entries.get(selected_idx).map(|e| e.path.clone())
    }

    pub fn get_marked_paths(&self) -> Vec<PathBuf> {
        self.marked_entries.keys().cloned().collect()
    }

    /// Search integration
    pub fn set_search_results(&mut self, results: Vec<ObjectInfo>, query: CompactString) {
        self.search_results = results;
        self.search_query = Some(query);
        self.search_mode = SearchMode::FileName;
    }

    pub fn clear_search(&mut self) {
        self.search_results.clear();
        self.search_query = None;
        self.search_mode = SearchMode::None;
    }

    /// Enhanced sorting with branch prediction hints
    fn sort_entries_optimized(&self, entries: &mut [ObjectInfo]) {
        match self.sort {
            EntrySort::NameAsc => {
                entries.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.cmp(&b.name),
                });
            }
            EntrySort::NameDesc => {
                entries.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.name.cmp(&a.name),
                });
            }
            EntrySort::SizeAsc => {
                entries.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.size.cmp(&b.size),
                });
            }
            EntrySort::SizeDesc => {
                entries.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.size.cmp(&a.size),
                });
            }
            EntrySort::ModifiedAsc => entries.sort_unstable_by_key(|e| e.modified),
            EntrySort::ModifiedDesc => entries.sort_unstable_by(|a, b| b.modified.cmp(&a.modified)),
            EntrySort::TypeAsc => {
                entries.sort_unstable_by(|a, b| {
                    a.extension
                        .cmp(&b.extension)
                        .then_with(|| a.name.cmp(&b.name))
                });
            }
            EntrySort::TypeDesc => {
                entries.sort_unstable_by(|a, b| {
                    b.extension
                        .cmp(&a.extension)
                        .then_with(|| a.name.cmp(&b.name))
                });
            }
        }
    }

    /// Enhanced filtering with regex support
    fn apply_filter(&self, entries: &mut Vec<ObjectInfo>) {
        let show_hidden = self.show_hidden.load(Ordering::Relaxed);

        match &self.filter {
            EntryFilter::All => {
                if !show_hidden {
                    entries.retain(|e| !e.name.starts_with('.') || e.name == "..");
                }
            }
            EntryFilter::FilesOnly => {
                entries.retain(|e| !e.is_dir && (show_hidden || !e.name.starts_with('.')));
            }
            EntryFilter::DirsOnly => {
                entries.retain(|e| e.is_dir && (show_hidden || !e.name.starts_with('.')));
            }
            EntryFilter::Extension(ext) => {
                entries.retain(|e| {
                    (e.extension.as_ref() == Some(ext)) && (show_hidden || !e.name.starts_with('.'))
                });
            }
            EntryFilter::Pattern(pattern) => {
                entries.retain(|e| {
                    e.name.contains(pattern.as_str()) && (show_hidden || !e.name.starts_with('.'))
                });
            }
            EntryFilter::Regex(pattern) => {
                if let Ok(regex) = regex::Regex::new(pattern) {
                    entries.retain(|e| {
                        regex.is_match(&e.name) && (show_hidden || !e.name.starts_with('.'))
                    });
                }
            }
            EntryFilter::Hidden(hidden) => {
                entries.retain(|e| e.name.starts_with('.') == *hidden);
            }
            EntryFilter::Marked => {
                entries.retain(|e| self.marked_entries.contains_key(&e.path));
            }
            _ => {}
        }
    }

    /// Performance helpers
    pub fn selected_entry(&self) -> Option<&ObjectInfo> {
        let idx = self.selected.load(Ordering::Relaxed);
        self.entries.get(idx)
    }

    fn adjust_scroll_for_selection(&self, selected: usize) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current_scroll = self.scroll_offset.load(Ordering::Relaxed);

        let new_scroll = if selected < current_scroll {
            selected
        } else if selected >= current_scroll + viewport_height {
            selected.saturating_sub(viewport_height - 1)
        } else {
            current_scroll
        };

        if new_scroll != current_scroll {
            self.scroll_offset.store(new_scroll, Ordering::Relaxed);
        }
    }
}

/// Enhanced filesystem state with action integration
#[derive(Debug, Clone)]
pub struct FSState {
    pub panes: SmallVec<[PaneState; 2]>,
    pub active_pane: usize,
    pub recent_dirs: VecDeque<PathBuf>,
    pub favorite_dirs: HashSet<PathBuf>,
    pub bookmarks: HashMap<char, PathBuf>,
    pub batch_op_status: Option<CompactString>,

    // Navigation history
    pub history: VecDeque<PathBuf>,
    pub history_index: usize,

    // Global operation tracking
    pub active_operations: HashMap<CompactString, OperationStatus>,
}

#[derive(Debug, Clone)]
pub struct OperationStatus {
    pub operation_type: CompactString,
    pub progress: f32,
    pub message: Option<CompactString>,
    pub started_at: Instant,
}

impl FSState {
    pub fn new(cwd: PathBuf) -> Self {
        let mut panes = SmallVec::new();
        panes.push(PaneState::new(cwd.clone()));

        let mut history = VecDeque::with_capacity(64);
        history.push_back(cwd);

        Self {
            panes,
            active_pane: 0,
            recent_dirs: VecDeque::with_capacity(32),
            favorite_dirs: HashSet::with_capacity(16),
            bookmarks: HashMap::with_capacity(26),
            batch_op_status: None,
            history,
            history_index: 0,
            active_operations: HashMap::new(),
        }
    }

    #[inline]
    pub fn active_pane(&self) -> &PaneState {
        &self.panes[self.active_pane]
    }

    #[inline]
    pub fn active_pane_mut(&mut self) -> &mut PaneState {
        &mut self.panes[self.active_pane]
    }

    /// Enhanced navigation with history
    pub fn navigate_to(&mut self, path: PathBuf) {
        self.add_to_history(path.clone());
        self.active_pane_mut().cwd = path;
    }

    pub fn navigate_back(&mut self) -> Option<PathBuf> {
        if self.history_index > 0 {
            self.history_index -= 1;
            self.history.get(self.history_index).cloned()
        } else {
            None
        }
    }

    pub fn navigate_forward(&mut self) -> Option<PathBuf> {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            self.history.get(self.history_index).cloned()
        } else {
            None
        }
    }

    fn add_to_history(&mut self, path: PathBuf) {
        // Truncate forward history if we're not at the end
        self.history.truncate(self.history_index + 1);

        // Add new path
        self.history.push_back(path.clone());
        self.history_index = self.history.len() - 1;

        // Maintain capacity
        if self.history.len() > 64 {
            self.history.pop_front();
            self.history_index = self.history_index.saturating_sub(1);
        }

        // Add to recent directories
        self.add_recent_dir(path);
    }

    /// Bookmark management
    pub fn add_bookmark(&mut self, key: char, path: PathBuf) {
        self.bookmarks.insert(key, path);
    }

    pub fn get_bookmark(&self, key: char) -> Option<&PathBuf> {
        self.bookmarks.get(&key)
    }

    pub fn remove_bookmark(&mut self, key: char) -> Option<PathBuf> {
        self.bookmarks.remove(&key)
    }

    /// Recent directories with LRU
    pub fn add_recent_dir(&mut self, path: PathBuf) {
        if let Some(pos) = self.recent_dirs.iter().position(|p| p == &path) {
            self.recent_dirs.remove(pos);
        }

        self.recent_dirs.push_front(path);

        if self.recent_dirs.len() > 32 {
            self.recent_dirs.pop_back();
        }
    }

    /// Favorites management
    #[inline]
    pub fn add_favorite(&mut self, path: PathBuf) {
        self.favorite_dirs.insert(path);
    }

    #[inline]
    pub fn remove_favorite(&mut self, path: &PathBuf) {
        self.favorite_dirs.remove(path);
    }

    #[inline]
    pub fn is_favorite(&self, path: &PathBuf) -> bool {
        self.favorite_dirs.contains(path)
    }

    /// Operation tracking
    pub fn start_operation(&mut self, id: CompactString, op_type: CompactString) {
        self.active_operations.insert(
            id,
            OperationStatus {
                operation_type: op_type,
                progress: 0.0,
                message: None,
                started_at: Instant::now(),
            },
        );
    }

    pub fn update_operation(&mut self, id: &str, progress: f32, message: Option<CompactString>) {
        if let Some(op) = self.active_operations.get_mut(id) {
            op.progress = progress.clamp(0.0, 1.0);
            op.message = message;
        }
    }

    pub fn complete_operation(&mut self, id: &str) {
        self.active_operations.remove(id);
    }

    /// Get current selection path
    pub fn get_selected_path(&self) -> Option<PathBuf> {
        self.active_pane()
            .selected_entry()
            .map(|entry| entry.path.clone())
    }

    /// Get multiple selected paths (marked entries)
    pub fn get_selected_paths(&self) -> Vec<PathBuf> {
        let active_pane = self.active_pane();
        if active_pane.marked_entries.is_empty() {
            // If nothing marked, return current selection
            self.get_selected_path().into_iter().collect()
        } else {
            active_pane.get_marked_paths()
        }
    }

    /// Check if any operations are in progress
    pub fn has_active_operations(&self) -> bool {
        !self.active_operations.is_empty()
    }

    /// Get operation progress summary
    pub fn get_operation_summary(&self) -> Option<(f32, usize)> {
        if self.active_operations.is_empty() {
            None
        } else {
            let total_progress: f32 = self.active_operations.values().map(|op| op.progress).sum();
            let avg_progress = total_progress / self.active_operations.len() as f32;
            Some((avg_progress, self.active_operations.len()))
        }
    }
}

impl Default for FSState {
    fn default() -> Self {
        Self::new(PathBuf::from("."))
    }
}

impl Clone for PaneState {
    fn clone(&self) -> Self {
        Self {
            cwd: self.cwd.clone(),
            entries: self.entries.clone(),
            selected: AtomicUsize::new(self.selected.load(Ordering::Relaxed)),
            scroll_offset: AtomicUsize::new(self.scroll_offset.load(Ordering::Relaxed)),
            viewport_height: AtomicUsize::new(self.viewport_height.load(Ordering::Relaxed)),
            is_loading: AtomicBool::new(self.is_loading.load(Ordering::Relaxed)),
            is_incremental_loading: AtomicBool::new(
                self.is_incremental_loading.load(Ordering::Relaxed),
            ),
            operation_in_progress: AtomicBool::new(
                self.operation_in_progress.load(Ordering::Relaxed),
            ),
            sort: self.sort,
            filter: self.filter.clone(),
            show_hidden: AtomicBool::new(self.show_hidden.load(Ordering::Relaxed)),
            last_error: self.last_error.clone(),
            incremental_entries: parking_lot::RwLock::new(self.incremental_entries.read().clone()),
            expected_entries: AtomicUsize::new(self.expected_entries.load(Ordering::Relaxed)),
            entries_loaded: AtomicUsize::new(self.entries_loaded.load(Ordering::Relaxed)),
            last_scan_duration: AtomicU64::new(self.last_scan_duration.load(Ordering::Relaxed)),
            last_sort_duration: AtomicU64::new(self.last_sort_duration.load(Ordering::Relaxed)),
            search_results: self.search_results.clone(),
            search_query: self.search_query.clone(),
            search_mode: self.search_mode,
            marked_entries: self.marked_entries.clone(),
            clipboard_selection: self.clipboard_selection.clone(),
        }
    }
}
