//! src/model/fs_state.rs
//! ============================================================================
//! # FSState: Advanced Filesystem State for Power Users
//!
//! Manages current working directories (multi-pane), directory entries, filters/sorts,
//! scan/loading/error state, batch op status, and history for the session.

use ratatui::widgets::TableState;

use crate::fs::object_info::ObjectInfo;
use std::collections::{HashSet, VecDeque};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryFilter {
    All,
    FilesOnly,
    DirsOnly,
    Extension(String),
    Pattern(String),
    Custom(String), // plugin/user script
}

/// Single pane state (e.g., for dual/multi-pane UI)
#[derive(Debug, Clone)]
pub struct PaneState {
    /// The working directory for this pane.
    pub cwd: PathBuf,

    /// The directory contents as ObjectInfo snapshot.
    pub entries: Vec<ObjectInfo>,

    /// Selected index in entries.
    pub selected: Option<usize>,

    /// Focused (previewed) entry.
    pub focused: Option<ObjectInfo>,

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
}

impl PaneState {
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
    pub recent_dirs: VecDeque<PathBuf>,
    pub favorite_dirs: HashSet<PathBuf>,
}

/// The type of the filesystem object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectType {
    Dir,
    File,
    Symlink,
    // Extendable: Add Archive, Image, Video, Custom, etc.
    Other(String),
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectType::Dir => write!(f, "Dir"),
            ObjectType::File => write!(f, "File"),
            ObjectType::Symlink => write!(f, "Symlink"),
            ObjectType::Other(ext) => write!(f, "{ext}"),
        }
    }
}

impl ObjectType {
    // You may want to move this logic to ObjectInfo itself, but shown here for clarity.
    pub fn object_type(obj: &crate::fs::object_info::ObjectInfo) -> ObjectType {
        if obj.is_dir {
            ObjectType::Dir
        } else if obj.is_symlink {
            ObjectType::Symlink
        } else if let Some(ext) = obj.extension.as_deref() {
            ObjectType::Other(ext.to_ascii_uppercase())
        } else {
            ObjectType::File
        }
    }
}

impl FSState {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            panes: vec![PaneState::new(cwd)],
            active_pane: 0,
            batch_op_status: None,
            recent_dirs: VecDeque::with_capacity(32),
            favorite_dirs: HashSet::new(),
        }
    }

    pub fn set_active_pane(&mut self, idx: usize) {
        if idx < self.panes.len() {
            self.active_pane = idx;
        }
    }

    pub fn add_recent_dir(&mut self, path: PathBuf) {
        if self.recent_dirs.len() == 32 {
            self.recent_dirs.pop_front();
        }
        self.recent_dirs.push_back(path);
    }

    pub fn add_favorite(&mut self, path: PathBuf) {
        self.favorite_dirs.insert(path);
    }

    pub fn remove_favorite(&mut self, path: &PathBuf) {
        self.favorite_dirs.remove(path);
    }

    // More: sorting, filtering, pane management, etc.
}

impl Default for FSState {
    fn default() -> Self {
        FSState::new(PathBuf::from("."))
    }
}
