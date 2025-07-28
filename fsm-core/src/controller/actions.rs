//! Enhanced Actions with comprehensive clipboard and file operations support

use crate::fs::object_info::ObjectInfo;
use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;

use super::TaskResult;

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
    // Clipboard operations
    PasteDestination,
}

/// Unique identifier for tracking file operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationId(pub String);

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

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a high-level action that the application can perform.
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

    /// Simulate a loading state (for demo/testing).
    SimulateLoading,

    /// An internal tick event for periodic updates.
    Tick,

    /// A result from a background task.
    TaskResult(TaskResult),

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

    /// Update task status
    UpdateTaskStatus {
        task_id: u64,
        completed: bool,
    },

    // ===== Clipboard Operations =====
    /// Copy selected item(s) to clipboard
    Copy(PathBuf),

    /// Cut selected item(s) to clipboard (move operation)
    Cut(PathBuf),

    /// Copy multiple items to clipboard
    CopyMultiple(Vec<PathBuf>),

    /// Cut multiple items to clipboard
    CutMultiple(Vec<PathBuf>),

    /// Paste from clipboard to current directory
    Paste,

    /// Paste from clipboard to specified directory
    PasteToDirectory(PathBuf),

    /// Toggle clipboard overlay
    ToggleClipboard,

    /// Navigate clipboard selection up
    ClipboardUp,

    /// Navigate clipboard selection down
    ClipboardDown,

    /// Select clipboard item by index
    SelectClipboardItem(usize),

    /// Remove item from clipboard
    RemoveFromClipboard(u64), // clipboard item ID

    /// Clear entire clipboard
    ClearClipboard,

    /// Paste selected clipboard item
    PasteClipboardItem {
        item_id: u64,
        destination: PathBuf,
    },

    /// Show clipboard item details
    ShowClipboardItemDetails(u64),

    /// Execute clipboard paste operation
    ExecuteClipboardPaste {
        operation_id: OperationId,
        item_ids: Vec<u64>,
        destination: PathBuf,
    },

    /// Clipboard operation progress
    ClipboardOperationProgress {
        operation_id: OperationId,
        completed_items: u32,
        total_items: u32,
        current_item: String,
    },

    /// Clipboard operation completed
    ClipboardOperationComplete {
        operation_id: OperationId,
        items_processed: u32,
    },

    /// Clipboard operation failed
    ClipboardOperationError {
        operation_id: OperationId,
        item_id: Option<u64>,
        error: String,
    },

    // ===== Enhanced Navigation =====
    /// Bookmark current directory
    BookmarkDirectory,

    /// Show bookmarks overlay
    ShowBookmarks,

    /// Navigate to bookmark
    GoToBookmark(usize),

    /// Remove bookmark
    RemoveBookmark(usize),

    /// Show recent directories
    ShowRecentDirectories,

    /// Navigate to recent directory
    GoToRecentDirectory(usize),

    /// Add directory to navigation history
    AddToHistory(PathBuf),

    /// Go back in navigation history
    NavigateBack,

    /// Go forward in navigation history
    NavigateForward,

    // ===== Enhanced Search =====
    /// Navigate to next search result
    NextSearchResult,

    /// Navigate to previous search result
    PreviousSearchResult,

    /// Advanced search with options
    AdvancedSearch {
        pattern: String,
        case_sensitive: bool,
        regex: bool,
        include_hidden: bool,
        file_types: Vec<String>,
    },

    /// Search in specific directory
    SearchInDirectory {
        directory: PathBuf,
        pattern: String,
    },

    /// Cancel current search operation
    CancelSearch,

    /// Search result selected
    SelectSearchResult(usize),

    /// Jump to search result in file
    JumpToSearchResult {
        path: PathBuf,
        line_number: Option<u32>,
        column: Option<u32>,
    },

    // ===== Task Management =====
    /// Show running tasks overlay
    ShowRunningTasks,

    /// Cancel specific task
    CancelTask(u64),

    /// Pause/resume task
    ToggleTaskPause(u64),

    /// Show task details
    ShowTaskDetails(u64),

    /// Set task priority
    SetTaskPriority {
        task_id: u64,
        priority: i8, // -10 to 10
    },

    // System events
    TerminalResize(u16, u16),

    // Input handling
    InputCharacter(char),
}

impl Action {
    /// Check if action requires async processing
    pub fn is_async(&self) -> bool {
        matches!(
            self,
            Action::Copy(_)
                | Action::Cut(_)
                | Action::CopyMultiple(_)
                | Action::CutMultiple(_)
                | Action::Paste
                | Action::PasteToDirectory(_)
                | Action::ExecuteClipboardPaste { .. }
                | Action::ClearClipboard
                | Action::ContentSearch(_)
                | Action::DirectContentSearch(_)
                | Action::AdvancedSearch { .. }
                | Action::ExecuteCopy { .. }
                | Action::ExecuteMove { .. }
                | Action::ExecuteRename { .. }
        )
    }

    /// Get operation priority (lower number = higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Action::Quit => 0,
            Action::Key(_) | Action::Mouse(_) => 1,
            Action::MoveSelectionUp | Action::MoveSelectionDown => 2,
            Action::ToggleClipboard | Action::ClipboardUp | Action::ClipboardDown => 3,
            Action::Copy(_) | Action::Cut(_) => 4,
            Action::Paste | Action::PasteToDirectory(_) => 5,
            Action::EnterSelected | Action::GoToParent => 6,
            Action::ContentSearch(_) | Action::FileNameSearch(_) => 7,
            Action::ExecuteCopy { .. } | Action::ExecuteMove { .. } => 8,
            Action::FileOperationProgress { .. } => 9,
            _ => 10,
        }
    }

    /// Check if action modifies filesystem
    pub fn modifies_filesystem(&self) -> bool {
        matches!(
            self,
            Action::CreateFileWithName(_)
                | Action::CreateDirectoryWithName(_)
                | Action::Delete
                | Action::ExecuteCopy { .. }
                | Action::ExecuteMove { .. }
                | Action::ExecuteRename { .. }
                | Action::Paste
                | Action::PasteToDirectory(_)
                | Action::ExecuteClipboardPaste { .. }
        )
    }

    /// Get human-readable action description
    pub fn description(&self) -> &'static str {
        match self {
            Action::Copy(_) => "Copy to clipboard",
            Action::Cut(_) => "Cut to clipboard",
            Action::Paste => "Paste from clipboard",
            Action::ToggleClipboard => "Toggle clipboard overlay",
            Action::ClearClipboard => "Clear clipboard",
            Action::ExecuteCopy { .. } => "Executing copy operation",
            Action::ExecuteMove { .. } => "Executing move operation",
            Action::FileOperationProgress { .. } => "File operation in progress",
            Action::ClipboardOperationProgress { .. } => "Clipboard operation in progress",
            Action::Quit => "Quit application",
            Action::EnterSelected => "Enter selected item",
            Action::GoToParent => "Go to parent directory",
            Action::Delete => "Delete selected item",
            Action::ToggleHelp => "Toggle help overlay",
            Action::ReloadDirectory => "Reload directory",
            Action::NextSearchResult => "Next search result",
            Action::PreviousSearchResult => "Previous search result",
            _ => "Unknown action",
        }
    }
}
