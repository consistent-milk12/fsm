//! src/controller/actions.rs
//! ============================================================================
//! # Actions: Centralized Application Commands
//!
//! Defines the `Action` enum, which represents all possible user inputs and
//! internal events that the application can respond to. This provides a single,
//! clear interface for the `Controller` to process.

use crate::fs::object_info::ObjectInfo;
use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;

/// Type of input prompt to show
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPromptType {
    CreateFile,
    CreateDirectory,
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

    /// Perform a file name search (instant).
    FileNameSearch(String),

    /// Perform a content search (ripgrep).
    ContentSearch(String),

    /// Direct content search with pattern (no overlay).
    DirectContentSearch(String),

    /// Toggle showing hidden files.
    ToggleShowHidden,

    /// Show search results.
    ShowSearchResults(Vec<crate::fs::object_info::ObjectInfo>),

    /// Show filename search results.
    ShowFilenameSearchResults(Vec<crate::fs::object_info::ObjectInfo>),

    /// Show rich content search results with line numbers and context (deprecated).
    ShowRichSearchResults(Vec<String>),

    /// Show raw ripgrep search results.
    ShowRawSearchResults(crate::tasks::search_task::RawSearchResult),

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

    /// Enter selected directory or open file.
    EnterSelected,

    /// Go to parent directory.
    GoToParent,

    Delete,

    CreateFile,

    CreateDirectory,

    CreateFileWithName(String),

    CreateDirectoryWithName(String),

    Sort(String),

    Filter(String),

    /// Updates an ObjectInfo in the state (e.g., from a background task).
    UpdateObjectInfo {
        parent_dir: PathBuf,
        info: ObjectInfo,
    },

    /// Handle streaming directory scan updates
    DirectoryScanUpdate {
        path: PathBuf,
        update: crate::fs::dir_scanner::ScanUpdate,
    },

    /// No operation. Used when an event is consumed but no state change is needed.
    NoOp,

    /// Close the currently active overlay.
    CloseOverlay,

    /// Reload the current directory.
    ReloadDirectory,

    /// Open a file with external editor.
    OpenFile(PathBuf),
    /// Show input prompt for file/directory creation.
    ShowInputPrompt(InputPromptType),
    /// Submit input prompt with user input.
    SubmitInputPrompt(String),
}
