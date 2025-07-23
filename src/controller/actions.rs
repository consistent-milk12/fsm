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

/// Represents a high-level action that the application can perform.
/// This abstracts away raw terminal events into meaningful commands.
#[derive(Debug, Clone, PartialEq)]
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
    /// Toggle the command palette visibility.
    ToggleCommandPalette,
    /// Toggle showing hidden files.
    ToggleShowHidden,
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
    /// Enter selected directory or open file.
    EnterSelected,
    /// Go to parent directory.
    GoToParent,
    /// Updates an ObjectInfo in the state (e.g., from a background task).
    UpdateObjectInfo {
        parent_dir: PathBuf,
        info: ObjectInfo,
    },
    // Add more actions as the application grows, e.g.:
    // ChangeDirectory(PathBuf),
    // SelectEntry,
    // MarkEntry(PathBuf),
    // CopyMarked,
    // DeleteMarked,
}
