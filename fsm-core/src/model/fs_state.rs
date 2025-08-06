//! ``src/model/fs_state.rs``
//! ============================================================================
//! # `FSState`: Advanced Filesystem State for Power Users
//!
//! Manages current working directories (multi-pane), directory entries, filters/sorts,
//! scan/loading/error state, batch op status, and history for the session.

use ratatui::widgets::TableState;

use crate::model::object_registry::{ObjectId, SortableEntry};
use std::{cmp::Ordering, collections::{HashSet, VecDeque}, sync::Arc};
use std::path::PathBuf;

/// Filter and sort mode for directory views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySort {
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
    ModifiedAsc,
    ModifiedDesc,
    Custom(String), // for plugin/user
}

impl std::fmt::Display for EntrySort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &'_ str = match self {
            Self::NameAsc => "name_asc",
            
            Self::NameDesc => "name_desc",
            
            Self::SizeAsc => "size_asc",
            
            Self::SizeDesc => "size_desc",
            
            Self::ModifiedAsc => "modified_asc",
            
            Self::ModifiedDesc => "modified_desc",
            
            Self::Custom(s) => s,
        };

        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryFilter {
    All,
    FilesOnly,
    DirsOnly,
    Extension(String),
    Pattern(String),
    Custom(String), // plugin/user script
}

impl std::fmt::Display for EntryFilter {
    fn fmt(&'_ self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &'_ str = match self {
            Self::All => "all",
            
            Self::FilesOnly => "files_only",
            
            Self::DirsOnly => "dirs_only",
            
            Self::Extension(s) 
            | Self::Pattern(s) 
            | Self::Custom(s) => s,
        };

        write!(f, "{s}")
    }
}

/// Single pane state (e.g., for dual/multi-pane UI)
#[derive(Debug, Clone)]
pub struct PaneState {
    /// The working directory for this pane.
    pub cwd: PathBuf,

    /// The directory contents as `ObjectInfo` snapshot.
    pub entries: Vec<SortableEntry>,

    /// Selected index in entries.
    pub selected: Option<usize>,

    /// Focused (previewed) entry.
    pub focused: Option<ObjectId>,

    /// True if loading, disables UI actions.
    pub is_loading: bool,

    /// Last error for this pane, if any.
    pub last_error: Option<String>,

    /// Applied sort mode for entries.
    pub sort: EntrySort,

    /// Active filter mode.
    pub filter: EntryFilter,

    /// Table state for ratatui Table widget (selection, scroll).
    pub table_state: TableState,

    /// Virtual scrolling offset (first visible row)
    pub scroll_offset: usize,

    /// Number of visible rows in the viewport
    pub viewport_height: usize,

    /// Incremental loading state
    pub incremental_entries: Vec<SortableEntry>,

    /// Whether we're currently in incremental loading mode
    pub is_incremental_loading: bool,

    /// Total expected entries (if known)
    pub expected_entries: Option<usize>,
}

impl PaneState {
    #[must_use]
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            entries: Vec::new(),
            selected: Some(0),
            focused: None,
            is_loading: false,
            last_error: None,
            sort: EntrySort::NameAsc,
            filter: EntryFilter::All,
            table_state: TableState::default(),
            scroll_offset: 0,
            viewport_height: 20, // Default viewport height
            incremental_entries: Vec::new(),
            is_incremental_loading: false,
            expected_entries: None,
        }
    }

    /// Update the entry list and reset selection for new directory.
    pub fn set_entries(&mut self, entries: Vec<SortableEntry>) {
        self.entries = entries;
        self.selected = Some(0);
        self.table_state.select(Some(0));
    }

    #[must_use]
    /// Get currently selected entry (if any).
    pub fn selected_entry(&self) -> Option<ObjectId> {
        self
            .selected
            .and_then(
                |idx: usize| -> Option<&SortableEntry> 
                {
                    self.entries.get(idx)
                }
            )
            .map(
                |sortable_entry: &SortableEntry| -> u64 
                {
                    sortable_entry.id
                }
            )
    }

    /// Update viewport height when terminal size changes
    pub const fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.saturating_sub(3); // Account for header and border
        self.adjust_scroll();
    }

    /// Get visible entries for virtual scrolling
    #[must_use]
    pub fn visible_entries(&self) -> &[SortableEntry] {
        let start: usize = self.scroll_offset;
        let end: usize = (start + self.viewport_height).min(self.entries.len());

        if start >= end || start >= self.entries.len() {
            &self.entries[0..0]
        } else {
            &self.entries[start..end]
        }
    }

    /// Move selection up and adjust scroll if needed
    pub fn move_selection_up(&mut self) {
        if let Some(selected) = self.selected
            && selected > 0
        {
            self.selected = Some(selected - 1);
            self.adjust_scroll();
            self.table_state
                .select(Some(selected - 1 - self.scroll_offset));
        }
    }

    /// Move selection down and adjust scroll if needed
    pub fn move_selection_down(&mut self) {
        if let Some(selected) = self.selected
            && selected + 1 < self.entries.len()
        {
            self.selected = Some(selected + 1);
            self.adjust_scroll();
            self.table_state
                .select(Some(selected + 1 - self.scroll_offset));
        }
    }

    /// Adjust scroll offset to keep selection visible
    const fn adjust_scroll(&mut self) {
        if let Some(selected) = self.selected {
            // If selection is above viewport, scroll up
            if selected < self.scroll_offset {
                self.scroll_offset = selected;
            }
            // If selection is below viewport, scroll down
            else if selected >= (self.scroll_offset + self.viewport_height) {
                self.scroll_offset = selected.saturating_sub(self.viewport_height - 1);
            }
        }
    }

    /// Jump to top of list
    pub fn select_first(&mut self) {
        if !self.entries.is_empty() {
            self.selected = Some(0);
            self.scroll_offset = 0;
            self.table_state.select(Some(0));
        }
    }

    /// Jump to bottom of list
    pub fn select_last(&mut self) {
        if !self.entries.is_empty() {
            let last_idx = self.entries.len() - 1;
            self.selected = Some(last_idx);
            self.scroll_offset = last_idx.saturating_sub(self.viewport_height - 1);
            self.table_state.select(Some(last_idx - self.scroll_offset));
        }
    }

    /// Page up (move selection up by viewport height)
    pub fn page_up(&mut self) {
        if let Some(selected) = self.selected {
            let new_selected: usize = selected.saturating_sub(self.viewport_height);
            
            self.selected = Some(new_selected);
            self.adjust_scroll();
            
            self.table_state
                .select(Some(new_selected - self.scroll_offset));
        }
    }

    /// Page down (move selection down by viewport height)
    pub fn page_down(&mut self) {
        if let Some(selected) = self.selected {
            let new_selected: usize = (selected + self.viewport_height).min(self.entries.len() - 1);
            
            self.selected = Some(new_selected);
            self.adjust_scroll();
            self.table_state
                .select(Some(new_selected - self.scroll_offset));
        }
    }

    /// Start incremental loading mode
    pub fn start_incremental_loading(&mut self) {
        self.is_incremental_loading = true;
        self.incremental_entries.clear();
        self.expected_entries = None;
        self.is_loading = true;
    }

    /// Batch approach - only sort on completion or chunk boundaries
    pub fn add_incremental_entry(&mut self, entry: SortableEntry) {
        if !self.is_incremental_loading 
        {
            return;
        }

        self.incremental_entries.push(entry);
    }

    /// Complete incremental loading with final sorted entries
    pub fn complete_incremental_loading(&mut self, final_entries: Vec<SortableEntry>) {
        self.is_incremental_loading = false;
        self.is_loading = false;
        self.entries = final_entries;
        self.incremental_entries.clear();

        // Reset selection to first item if we have entries
        if !self.entries.is_empty() {
            self.selected = Some(0);
            self.scroll_offset = 0;
            self.table_state.select(Some(0));
        }
    }

    /// Sort entries in place based on current sort mode
    pub fn sort_entries(&mut self) {
        match self.sort {
            EntrySort::NameAsc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            if a.is_dir && !b.is_dir {
                                Ordering::Less
                            } else if !a.is_dir && b.is_dir {
                                Ordering::Greater
                            } else {
                                a.sort_name_hash.cmp(&b.sort_name_hash)
                            }
                        }
                    );
            }

            EntrySort::NameDesc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            if a.is_dir && !b.is_dir {
                                Ordering::Less
                            } else if !a.is_dir && b.is_dir {
                                Ordering::Greater
                            } else {
                                b.sort_name_hash.cmp(&a.sort_name_hash)
                            }
                        }
                );
            }

            EntrySort::SizeAsc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            a.size.cmp(&b.size)
                        }
                    );
            }
            
            EntrySort::SizeDesc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            b.size.cmp(&a.size)
                        }
                    );
            }
            
            EntrySort::ModifiedAsc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            a.modified.cmp(&b.modified)
                        }
                    );
            }
            
            EntrySort::ModifiedDesc => {
                self
                    .entries
                    .sort_by(
                        |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                        {
                            b.modified.cmp(&a.modified)
                        }
                    );
            }
            
            EntrySort::Custom(_) => {
                // For custom sorting, keep current order for now
            }
        }
    }
}

/// Persistent, advanced FS state for the app/session.
#[derive(Debug, Clone)]
pub struct FSState {
    /// One or more open panes (for dual-pane, etc.).
    pub panes: Vec<PaneState>,
    
    /// Which pane is currently focused.
    pub active_pane: usize,
    
    /// Batch operation progress (for power-user bulk actions).
    pub batch_op_status: Option<String>,
    
    /// Set of favorite/recent directories.
    pub recent_dirs: VecDeque<Arc<PathBuf>>,
    
    pub favorite_dirs: HashSet<Arc<PathBuf>>,
}


impl FSState {
    #[must_use]
    /// Construct FS state with one pane in the given directory.
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            panes: vec![PaneState::new(cwd)],
            active_pane: 0,
            batch_op_status: None,
            recent_dirs: VecDeque::with_capacity(32),
            favorite_dirs: HashSet::new(),
        }
    }

    /// Get the currently active pane as mutable.
    pub fn active_pane_mut(&mut self) -> &mut PaneState {
        &mut self.panes[self.active_pane]
    }

    #[must_use]
    /// Get the currently active pane as immutable.
    pub fn active_pane(&self) -> &PaneState {
        &self.panes[self.active_pane]
    }

    /// Switch focus to a different pane.
    pub const fn set_active_pane(&mut self, idx: usize) {
        if idx < self.panes.len() {
            self.active_pane = idx;
        }
    }

    /// Add a path to recents (evicts oldest if over 32).
    pub fn add_recent_dir(&mut self, path: Arc<PathBuf>) {
        if self.recent_dirs.len() == 32 {
            self.recent_dirs.pop_front();
        }

        self.recent_dirs.push_back(path);
    }

    pub fn add_favorite(&mut self, path: Arc<PathBuf>) {
        self.favorite_dirs.insert(path);
    }

    pub fn remove_favorite(&mut self, path: &Arc<PathBuf>) {
        self.favorite_dirs.remove(path);
    }
}

impl Default for FSState {
    fn default() -> Self {
        Self::new(PathBuf::from("."))
    }
}
