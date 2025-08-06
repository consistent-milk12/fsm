//! src/controller/actions.rs
//! ============================================================================
//! # Actions: Centralized Application Commands
//!
//! Defines the `Action` enum, which represents all possible user inputs and
//! internal events that the application can respond to. This provides a single,
//! clear interface for the `Controller` to process.

use crate::{controller::event_loop::TaskResult, fs::object_info::ObjectInfo, tasks::search_task::RawSearchResult};
use crossterm::event::{KeyEvent, MouseEvent};
use std::{path::PathBuf, sync::Arc};

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

/// Represents a high-level action that the application can perform.
/// This abstracts away raw terminal events into meaningful commands.
#[derive(Debug, Clone)]
pub enum Action {
    /// Batch update `ObjectInfo` entries (reduces event queue saturation)
    BatchUpdateObjectInfo
    {
        parent_dir: Arc<PathBuf>,
        objects: Vec<ObjectInfo>,
    },

    /// Cancel ongoing file operation
    CancelFileOperation {
        operation_id: String,
    },

    /// Close the currently active overlay.
    CloseOverlay,
    
    /// Perform a content search (ripgrep).
    ContentSearch(String),
    
    // File operations
    /// Copy file/directory from source to destination
    Copy {
        source: Arc<PathBuf>,
        dest: Arc<PathBuf>,
    },
    
    CreateDirectory,
    
    CreateDirectoryWithName(String),
    
    CreateFile,
    
    CreateFileWithName(String),
    
    Delete,
    
    /// Direct content search with pattern (no overlay).
    DirectContentSearch(String),
    
    /// Handle streaming directory scan updates
    DirectoryScanUpdate {
        path: Arc<PathBuf>,
        update: crate::fs::dir_scanner::ScanUpdate,
    },
    
    /// Enter vim-style command mode.
    EnterCommandMode,
    
    /// Enter selected directory or open file.
    EnterSelected,
    
    /// Exit command mode.
    ExitCommandMode,
    
    /// Perform a file name search (instant).
    FileNameSearch(String),
    
    Filter(String),
    
    /// Go to parent directory.
    GoToParent,
    
    /// Navigate to specified path.
    GoToPath(String),
    
    /// A keyboard event.
    Key(KeyEvent),
    
    /// A mouse event.
    Mouse(MouseEvent),
    
    /// Move file/directory from source to destination  
    Move {
        source: Arc<PathBuf>,
        dest: Arc<PathBuf>,
    },
    
    /// Move selection down.
    MoveSelectionDown,
    
    /// Move selection up.
    MoveSelectionUp,
    
    /// No operation. Used when an event is consumed but no state change is needed.
    NoOp,
    
    /// Open a file with external editor, optionally jumping to a specific line.
    OpenFile(Arc<PathBuf>, Option<usize>),
    
    /// Page down (move selection down by viewport height).
    PageDown,
    
    /// Page up (move selection up by viewport height).
    PageUp,
    
    /// Quit the application.
    Quit,
    
    /// Reload the current directory.
    ReloadDirectory,
    
    /// Rename file/directory
    Rename {
        source: Arc<PathBuf>,
        new_name: String,
    },
    
    /// Rename selected entry.
    RenameEntry(String),
    
    /// A terminal resize event.
    Resize(u16, u16),
    
    /// Jump to first entry.
    SelectFirst,
    
    /// Jump to last entry.
    SelectLast,
    
    /// Show filename search results.
    ShowFilenameSearchResults(Vec<ObjectInfo>),
    
    /// Show input prompt for file/directory creation.
    ShowInputPrompt(InputPromptType),
    
    /// Show raw ripgrep search results.
    ShowRawSearchResults(RawSearchResult),
    
    /// Show rich content search results with line numbers and context (deprecated).
    ShowRichSearchResults(Vec<String>),
    
    /// Show search results.
    ShowSearchResults(Vec<ObjectInfo>),
    
    /// Simulate a loading state (for demo/testing).
    SimulateLoading,
    
    Sort(String),
    
    /// Submit input prompt with user input.
    SubmitInputPrompt(String),
    
    /// A result from a background task.
    TaskResult(TaskResult),
    
    /// An internal tick event for periodic updates.
    Tick,
    
    /// Toggle the content search overlay.
    ToggleContentSearch,
    
    /// Toggle the file name search overlay.
    ToggleFileNameSearch,
    
    /// Toggle the help overlay visibility.
    ToggleHelp,
    
    /// Toggle showing hidden files.
    ToggleShowHidden,
    
    /// Updates an `ObjectInfo` in the state (e.g., from a background task).
    UpdateObjectInfo {
        parent_dir: Arc<PathBuf>,
        info: ObjectInfo,
    },
}
