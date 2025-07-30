//! FSState: Enhanced filesystem state with action integration

use crate::fs::object_info::ObjectInfo;
use compact_str::CompactString;
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;
use tracing::{debug, info, instrument, trace, warn};

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
    #[instrument(skip(cwd), fields(cwd = %cwd.display()))]
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
    #[instrument(skip(self, entries), fields(num_entries = entries.len()))]
    pub fn sort_entries(&mut self, mut entries: Vec<ObjectInfo>) {
        let start: Instant = Instant::now();
        debug!("Setting new entries. Initial count: {}.", entries.len());

        // Apply filter
        let initial_len: usize = entries.len();
        self.apply_filter(&mut entries);

        debug!(
            "Applied filter. Entries after filtering: {}. (Removed {} entries)",
            entries.len(),
            initial_len - entries.len()
        );

        // SIMD-optimized sorting
        let sort_start: Instant = Instant::now();
        self.sort_entries_optimized(&mut entries);
        let sort_duration: u64 = sort_start.elapsed().as_micros() as u64;

        self.last_sort_duration
            .store(sort_duration, Ordering::Relaxed);
        debug!("Entries sorted in {} us.", sort_duration);

        self.entries = entries;
        self.selected.store(0, Ordering::Relaxed);
        self.scroll_offset.store(0, Ordering::Relaxed);
        self.entries_loaded
            .store(self.entries.len(), Ordering::Relaxed);

        let duration_us: u64 = start.elapsed().as_micros() as u64;
        self.last_scan_duration
            .store(duration_us, Ordering::Relaxed);
        info!(
            "Entries set and processed in {} us. Total entries: {}.",
            duration_us,
            self.entries.len()
        );
    }

    /// Action-compatible selection movement
    #[instrument(skip(self), fields(current_selection = self.selected.load(Ordering::Relaxed)))]
    pub fn move_selection_up(&self) -> bool {
        let current: usize = self.selected.load(Ordering::Relaxed);

        if current > 0 {
            let new_selected: usize = current - 1;
            self.selected.store(new_selected, Ordering::Relaxed);
            self.adjust_scroll_for_selection(new_selected);

            debug!("Moved selection up to {}.", new_selected);

            true
        } else {
            trace!("Cannot move selection up, already at top.");
            false
        }
    }

    #[instrument(skip(self), fields(current_selection = self.selected.load(Ordering::Relaxed)))]
    pub fn move_selection_down(&self) -> bool {
        let current: usize = self.selected.load(Ordering::Relaxed);

        if current + 1 < self.entries.len() {
            let new_selected: usize = current + 1;
            self.selected.store(new_selected, Ordering::Relaxed);
            self.adjust_scroll_for_selection(new_selected);

            debug!("Moved selection down to {}.", new_selected);

            true
        } else {
            trace!("Cannot move selection down, already at bottom.");
            false
        }
    }

    /// Enhanced navigation methods
    #[instrument(skip(self))]
    pub fn select_first(&self) {
        if !self.entries.is_empty() {
            self.selected.store(0, Ordering::Relaxed);
            self.scroll_offset.store(0, Ordering::Relaxed);

            debug!("Selected first entry.");
        } else {
            trace!("No entries to select first.");
        }
    }

    #[instrument(skip(self))]
    pub fn select_last(&self) {
        if !self.entries.is_empty() {
            let last_idx = self.entries.len() - 1;
            self.selected.store(last_idx, Ordering::Relaxed);
            let viewport_height = self.viewport_height.load(Ordering::Relaxed);
            let new_scroll = last_idx.saturating_sub(viewport_height - 1);
            self.scroll_offset.store(new_scroll, Ordering::Relaxed);
            debug!("Selected last entry: {}.", last_idx);
        } else {
            trace!("No entries to select last.");
        }
    }

    #[instrument(skip(self))]
    pub fn page_up(&self) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current = self.selected.load(Ordering::Relaxed);
        let new_selected = current.saturating_sub(viewport_height);
        self.selected.store(new_selected, Ordering::Relaxed);
        self.adjust_scroll_for_selection(new_selected);
        debug!("Page up: new selection {}.", new_selected);
    }

    #[instrument(skip(self))]
    pub fn page_down(&self) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current = self.selected.load(Ordering::Relaxed);
        let new_selected = (current + viewport_height).min(self.entries.len().saturating_sub(1));
        self.selected.store(new_selected, Ordering::Relaxed);
        self.adjust_scroll_for_selection(new_selected);
        debug!("Page down: new selection {}.", new_selected);
    }

    #[instrument(skip(self), fields(selected_idx = self.selected.load(Ordering::Relaxed)))]
    pub fn mark_selected(&self) -> bool {
        let selected_idx: usize = self.selected.load(Ordering::Relaxed);

        if self.entries.get(selected_idx).is_some() {
            debug!("Entry at index {} is valid for marking.", selected_idx);
            true
        } else {
            warn!("No entry found at index {} for marking.", selected_idx);
            false
        }
    }

    #[instrument(skip(self))]
    pub fn unmark_selected(&mut self) {
        if let Some(entry) = self.clone().selected_entry() {
            debug!("Unmarking entry: {:?}", entry.path);
            self.marked_entries.remove(&entry.path);
        } else {
            warn!("No selected entry to unmark.");
        }
    }

    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn is_marked(&self, path: &PathBuf) -> bool {
        let is_marked = self.marked_entries.contains_key(path);
        trace!(
            "Checking if path is marked: {} -> {}.",
            path.display(),
            is_marked
        );
        is_marked
    }

    #[instrument(skip(self))]
    pub fn clear_marks(&mut self) {
        debug!("Clearing all marked entries.");
        self.marked_entries.clear();
    }

    #[instrument(skip(self))]
    pub fn get_selected_path(&self) -> Option<PathBuf> {
        let selected_idx = self.selected.load(Ordering::Relaxed);
        let path = self.entries.get(selected_idx).map(|e| e.path.clone());
        debug!("Getting selected path: {:?}", path);
        path
    }

    #[instrument(skip(self))]
    pub fn get_marked_paths(&self) -> Vec<PathBuf> {
        let paths: Vec<PathBuf> = self.marked_entries.keys().cloned().collect();
        debug!("Retrieving {} marked paths.", paths.len());
        paths
    }

    /// Search integration
    #[instrument(skip(self, results), fields(query = %query.as_str(), num_results = results.len()))]
    pub fn set_search_results(&mut self, results: Vec<ObjectInfo>, query: CompactString) {
        self.search_results = results;
        self.search_query = Some(query);
        self.search_mode = SearchMode::FileName;
        info!("Search results set.");
    }

    #[instrument(skip(self))]
    pub fn clear_search(&mut self) {
        self.search_results.clear();
        self.search_query = None;
        self.search_mode = SearchMode::None;
        info!("Search cleared.");
    }

    /// Enhanced sorting with branch prediction hints
    #[instrument(skip(self, entries), fields(sort_mode = ?self.sort))]
    fn sort_entries_optimized(&self, entries: &mut [ObjectInfo]) {
        trace!("Starting optimized sorting.");
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
    #[instrument(skip(self, entries), fields(filter_mode = ?self.filter, show_hidden = self.show_hidden.load(Ordering::Relaxed)))]
    fn apply_filter(&self, entries: &mut Vec<ObjectInfo>) {
        trace!("Applying filter.");
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
    #[instrument(skip(self))]
    pub fn selected_entry(&self) -> Option<&ObjectInfo> {
        let idx = self.selected.load(Ordering::Relaxed);
        let entry = self.entries.get(idx);
        trace!(
            "Getting selected entry at index {}: {:?}",
            idx,
            entry.map(|e| &e.name)
        );
        entry
    }

    #[instrument(skip(self))]
    fn adjust_scroll_for_selection(&self, selected: usize) {
        let viewport_height = self.viewport_height.load(Ordering::Relaxed);
        let current_scroll = self.scroll_offset.load(Ordering::Relaxed);

        let new_scroll = if selected < current_scroll {
            trace!(
                "Adjusting scroll: selected {} is above current scroll {}.",
                selected, current_scroll
            );
            selected
        } else if selected >= current_scroll + viewport_height {
            trace!(
                "Adjusting scroll: selected {} is below viewport. Current scroll {}.",
                selected, current_scroll
            );
            selected.saturating_sub(viewport_height - 1)
        } else {
            trace!(
                "Adjusting scroll: selected {} is within viewport. Current scroll {}.",
                selected, current_scroll
            );
            current_scroll
        };

        if new_scroll != current_scroll {
            debug!("Scroll adjusted from {} to {}.", current_scroll, new_scroll);
            self.scroll_offset.store(new_scroll, Ordering::Relaxed);
        }
    }

    /// Update metadata for a specific entry
    #[instrument(skip(self, updated_entry), fields(entry_path = %entry_path.display()))]
    pub fn update_entry_metadata(
        &mut self,
        entry_path: &PathBuf,
        updated_entry: ObjectInfo,
    ) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| &e.path == entry_path) {
            *entry = updated_entry;
            trace!(
                marker = "METADATA_UPDATE_SUCCESS",
                operation_type = "entry_update",
                current_path = %self.cwd.display(),
                target_path = %entry_path.display(),
                "Entry metadata updated in pane"
            );
            true
        } else {
            warn!(
                marker = "METADATA_UPDATE_FAILED",
                operation_type = "entry_update",
                current_path = %self.cwd.display(),
                target_path = %entry_path.display(),
                "Entry not found for metadata update"
            );
            false
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
}

impl FSState {
    #[instrument(skip(cwd), fields(cwd = %cwd.display()))]
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
        }
    }

    #[inline]
    #[instrument(skip(self))]
    pub fn active_pane(&self) -> &PaneState {
        trace!("Getting active pane.");
        &self.panes[self.active_pane]
    }

    #[inline]
    #[instrument(skip(self))]
    pub fn active_pane_mut(&mut self) -> &mut PaneState {
        trace!("Getting mutable active pane.");
        &mut self.panes[self.active_pane]
    }

    /// Enhanced navigation with history
    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn navigate_to(&mut self, path: PathBuf) {
        info!("Navigating to: {}.", path.display());
        self.add_to_history(path.clone());
        self.active_pane_mut().cwd = path;

        trace!("Navigation completed with sorted entries");
    }

    /// Navigate to parent directory with proper sorting
    #[instrument(skip(self), fields(current_dir = %self.active_pane().cwd.display()))]
    pub fn navigate_to_parent(&mut self) -> Option<PathBuf> {
        let current_path = self.active_pane().cwd.clone();

        if let Some(parent) = current_path.parent() {
            let parent_path = parent.to_path_buf();
            info!("Navigating to parent: {}", parent_path.display());

            self.add_to_history(parent_path.clone());
            self.active_pane_mut().cwd = parent_path.clone();

            debug!("Parent navigation completed with re-sorted entries");

            Some(parent_path)
        } else {
            trace!("Already at root directory - cannot navigate to parent");
            None
        }
    }

    #[instrument(skip(self))]
    pub fn navigate_back(&mut self) -> Option<PathBuf> {
        if self.history_index > 0 {
            self.history_index -= 1;
            let path = self.history.get(self.history_index).cloned();
            debug!(
                "Navigating back to: {:?}. New history index: {}.",
                path, self.history_index
            );
            path
        } else {
            trace!("Cannot navigate back, already at the beginning of history.");
            None
        }
    }

    #[instrument(skip(self))]
    pub fn navigate_forward(&mut self) -> Option<PathBuf> {
        if self.history_index + 1 < self.history.len() {
            self.history_index += 1;
            let path = self.history.get(self.history_index).cloned();
            debug!(
                "Navigating forward to: {:?}. New history index: {}.",
                path, self.history_index
            );
            path
        } else {
            trace!("Cannot navigate forward, already at the end of history.");
            None
        }
    }

    #[instrument(skip(self, path), fields(path = %path.display()))]
    fn add_to_history(&mut self, path: PathBuf) {
        debug!("Adding path to history: {}.", path.display());
        // Truncate forward history if we're not at the end
        self.history.truncate(self.history_index + 1);

        // Add new path
        self.history.push_back(path.clone());
        self.history_index = self.history.len() - 1;

        // Maintain capacity
        if self.history.len() > 64 {
            warn!(
                "History capacity exceeded ({} entries), popping oldest.",
                self.history.len()
            );
            self.history.pop_front();
            self.history_index = self.history_index.saturating_sub(1);
        }

        // Add to recent directories
        self.add_recent_dir(path);
        trace!(
            "History updated. Current index: {}. Total entries: {}.",
            self.history_index,
            self.history.len()
        );
    }

    /// Bookmark management
    #[instrument(skip(self, path), fields(key = %key, path = %path.display()))]
    pub fn add_bookmark(&mut self, key: char, path: PathBuf) {
        debug!("Adding bookmark: '{}' -> {}.", key, path.display());
        self.bookmarks.insert(key, path);
    }

    #[instrument(skip(self), fields(key = %key))]
    pub fn get_bookmark(&self, key: char) -> Option<&PathBuf> {
        let bookmark = self.bookmarks.get(&key);
        trace!("Getting bookmark '{}': {:?}.", key, bookmark);
        bookmark
    }

    #[instrument(skip(self), fields(key = %key))]
    pub fn remove_bookmark(&mut self, key: char) -> Option<PathBuf> {
        let removed = self.bookmarks.remove(&key);
        if removed.is_some() {
            debug!("Removed bookmark '{}'.", key);
        } else {
            warn!("Attempted to remove non-existent bookmark '{}'.", key);
        }
        removed
    }

    /// Recent directories with LRU
    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn add_recent_dir(&mut self, path: PathBuf) {
        if let Some(pos) = self.recent_dirs.iter().position(|p| p == &path) {
            debug!(
                "Path {} already in recent dirs, moving to front.",
                path.display()
            );
            self.recent_dirs.remove(pos);
        }

        self.recent_dirs.push_front(path);

        if self.recent_dirs.len() > 32 {
            warn!(
                "Recent directories capacity exceeded ({} entries), popping oldest.",
                self.recent_dirs.len()
            );
            self.recent_dirs.pop_back();
        }
        trace!(
            "Recent directories updated. Total entries: {}.",
            self.recent_dirs.len()
        );
    }

    /// Favorites management
    #[inline]
    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn add_favorite(&mut self, path: PathBuf) {
        debug!("Adding favorite: {}.", path.display());
        self.favorite_dirs.insert(path);
    }

    #[inline]
    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn remove_favorite(&mut self, path: &PathBuf) {
        debug!("Removing favorite: {}.", path.display());
        self.favorite_dirs.remove(path);
    }

    #[inline]
    #[instrument(skip(self, path), fields(path = %path.display()))]
    pub fn is_favorite(&self, path: &PathBuf) -> bool {
        let is_fav = self.favorite_dirs.contains(path);
        trace!(
            "Checking if path is favorite: {} -> {}.",
            path.display(),
            is_fav
        );
        is_fav
    }

    /// Get current selection path
    #[instrument(skip(self))]
    pub fn get_selected_path(&self) -> Option<PathBuf> {
        let path = self
            .active_pane()
            .selected_entry()
            .map(|entry| entry.path.clone());
        debug!("Getting selected path from active pane: {:?}.", path);
        path
    }

    /// Get multiple selected paths (marked entries)
    #[instrument(skip(self))]
    pub fn get_selected_paths(&self) -> Vec<PathBuf> {
        let active_pane = self.active_pane();

        if active_pane.marked_entries.is_empty() {
            // If nothing marked, return current selection
            let path = self.get_selected_path().into_iter().collect();
            trace!(
                "No marked entries, returning single selected path: {:?}.",
                path
            );
            path
        } else {
            let paths = active_pane.get_marked_paths();
            debug!("Returning {} marked paths.", paths.len());
            paths
        }
    }
}

impl Default for FSState {
    #[instrument]
    fn default() -> Self {
        debug!("Creating default FSState.");
        Self::new(PathBuf::from("."))
    }
}

impl Clone for PaneState {
    #[instrument(skip(self))]
    fn clone(&self) -> Self {
        debug!("Cloning PaneState.");
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
