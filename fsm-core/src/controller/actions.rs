//! # Actions: Centralized Application Commands
//!
//! Defines the `Action` enum, which represents all possible user inputs and
//! internal events that the application can respond to. This provides a single,
//! clear interface for the `Controller` to process.

use crate::fs::object_info::ObjectInfo;
use crate::tasks::filename_search_task::RawFileSearchResult;
use crate::tasks::search_task::RawSearchResult;
use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;

/// Type of input prompt to show
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPromptType {
    CreateFile,
    CreateDirectory,
    Rename,
    Search,
    GoToPath,
    Custom(String),
    // File operations
    CopyDestination,
    MoveDestination,
    RenameFile,
}

/// Unique identifier for tracking file operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationId(String);

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationId {
    /// Generate a new unique operation ID
    pub fn new() -> Self {
        Self(nanoid::nanoid!())
    }

    /// Create from existing string
    pub fn from_string(id: String) -> Self {
        Self(id)
    }
}

/// Represents a high-level action that the application can perform.
/// This abstracts away raw terminal events into meaningful commands.
#[derive(Debug, Clone)]
pub enum Action {
    /// A keyboard event.
    Key(KeyEvent),

    /// A mouse event.
    Mouse(MouseEvent),

    /// A terminal resize event.
    Resize(u16, u16),

    /// Quit the application.
    Quit,

    /// Toggle the help overlay visibility.
    ToggleHelp,

    /// Enter vim-style command mode.
    EnterCommandMode,

    /// Exit command mode.
    ExitCommandMode,

    /// Toggle the file name search overlay.
    ToggleFileNameSearch,

    /// Toggle the content search overlay.
    ToggleContentSearch,

    /// Toggle the clipboard overlay visibility.
    ToggleClipboardOverlay,

    /// Perform a file name search (instant).
    FileNameSearch(String),

    /// Perform a content search (ripgrep).
    ContentSearch(String),

    /// Direct content search with pattern (no overlay).
    DirectContentSearch(String),

    /// Toggle showing hidden files.
    ToggleShowHidden,

    /// Show search results.
    ShowSearchResults(Vec<ObjectInfo>),

    /// Show filename search results.
    ShowFilenameSearchResults(Vec<ObjectInfo>),

    /// Show rich content search results with line numbers and context.
    ShowRichSearchResults(Vec<String>),

    /// Show raw ripgrep search results.
    ShowRawSearchResults(RawSearchResult),

    ShowRawFileSearchResults(RawFileSearchResult),

    /// Simulate a loading state (for demo/testing).
    SimulateLoading,

    /// An internal tick event for periodic updates.
    Tick,

    /// A result from a background task.
    TaskResult(crate::controller::event_loop::TaskResult),

    /// Move selection up.
    MoveSelectionUp,

    /// Move selection down.
    MoveSelectionDown,

    /// Page up (move selection up by viewport height).
    PageUp,

    /// Page down (move selection down by viewport height).
    PageDown,

    /// Jump to first entry.
    SelectFirst,

    /// Jump to last entry.
    SelectLast,

    /// Select entry by index (0-based)
    SelectIndex(usize),

    /// Enter selected directory or open file.
    EnterSelected,

    /// Go to parent directory.
    GoToParent,

    /// Delete selected item
    Delete,

    /// Show file creation prompt
    CreateFile,

    /// Show directory creation prompt
    CreateDirectory,

    /// Create file with specified name
    CreateFileWithName(String),

    /// Create directory with specified name
    CreateDirectoryWithName(String),

    /// Sort by specified criteria
    Sort(String),

    /// Filter using specified pattern
    Filter(String),

    /// Updates an ObjectInfo in the state
    UpdateObjectInfo {
        parent_dir: PathBuf,
        info: ObjectInfo,
    },

    /// Handle streaming directory scan updates
    DirectoryScanUpdate {
        path: PathBuf,
        update: crate::fs::dir_scanner::ScanUpdate,
    },

    /// No operation
    NoOp,

    /// Close the currently active overlay.
    CloseOverlay,

    /// Reload the current directory.
    ReloadDirectory,

    /// Open a file with external editor
    OpenFile(PathBuf, Option<u32>), // Path + optional line number

    /// Show input prompt for various operations
    ShowInputPrompt(InputPromptType),

    /// Submit input from a prompt
    SubmitInputPrompt(String),

    /// Update input field text
    UpdateInput(String),

    /// Rename selected entry
    RenameEntry(String),

    /// Navigate to specified path
    GoToPath(String),

    // ===== Enhanced File Operations =====
    /// Start file copy operation (shows destination prompt)
    StartCopy {
        source: PathBuf,
    },

    /// Start file move operation (shows destination prompt)
    StartMove {
        source: PathBuf,
    },

    /// Execute copy operation
    ExecuteCopy {
        operation_id: OperationId,
        source: PathBuf,
        destination: PathBuf,
    },

    /// Execute move operation
    ExecuteMove {
        operation_id: OperationId,
        source: PathBuf,
        destination: PathBuf,
    },

    /// Execute rename operation
    ExecuteRename {
        operation_id: OperationId,
        source: PathBuf,
        new_name: String,
    },

    /// File operation progress update
    FileOperationProgress {
        operation_id: OperationId,
        bytes_processed: u64,
        total_bytes: u64,
    },

    /// File operation completed
    FileOperationComplete {
        operation_id: OperationId,
    },

    /// File operation failed
    FileOperationError {
        operation_id: OperationId,
        error: String,
    },

    /// Cancel ongoing file operation
    CancelFileOperation {
        operation_id: OperationId,
    },

    UpdateTaskStatus {
        task_id: u64,
        completed: bool,
    },
}
