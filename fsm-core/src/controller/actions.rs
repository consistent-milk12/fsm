//! Optimized Actions module with render system integration and TSV tracing
use crate::fs::object_info::ObjectInfo;
use crate::fs::utils::ScanUpdate;

use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;
use std::time::SystemTime;

use super::TaskResult;

/// Input prompt types for user interactions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPromptType {
    CreateFile,
    CreateDirectory,
    Rename,
    Search,
    GoToPath,
    Custom(String),
}

/// Unique identifier for tracking operations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationId(pub String);

impl OperationId {
    pub fn new() -> Self {
        Self(nanoid::nanoid!())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_id_str(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// System monitoring data
#[derive(Debug, Clone, Default)]
pub struct SystemData {
    pub cpu_usage: f32,
    pub mem_usage: u64,
    pub total_mem: u64,
    pub swap_usage: u64,
    pub total_swap: u64,
}

/// Process data for monitoring
#[derive(Debug, Clone)]
pub struct ProcessData {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
    pub status: String,
}

/// Render trigger sources for performance tracking
#[derive(Clone, Debug)]
pub enum RenderTrigger {
    MetadataUpdate,
    StateChange,
    UserInteraction,
    SystemEvent,
}

/// Core application actions with clean architecture compliance
#[derive(Debug, Clone)]
pub enum Action {
    // ===== Core Input Events =====
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    TerminalResize(u16, u16),
    InputCharacter(char),

    // ===== Application Control =====
    Quit,
    Tick,
    NoOp,

    // ===== UI Control =====
    ToggleHelp,
    CloseOverlay,

    // ===== Navigation =====
    MoveSelectionUp,
    MoveSelectionDown,
    PageUp,
    PageDown,
    SelectFirst,
    SelectLast,
    SelectIndex(usize),
    EnterSelected,
    GoToParent,
    GoToPath(String),
    ReloadDirectory,

    // ===== File Operations =====
    CreateFile,
    CreateDirectory,
    CreateFileWithName(String),
    CreateDirectoryWithName(String),
    Delete,
    RenameEntry(String),
    ToggleShowHidden,

    // ===== Search =====
    EnterCommandMode,
    ToggleFileNameSearch,
    ToggleContentSearch,
    FileNameSearch(String),
    ContentSearch(String),
    ShowSearchResults(Vec<ObjectInfo>),
    NextSearchResult,
    PreviousSearchResult,
    ExitCommandMode,

    // ===== Clipboard =====
    ClipboardUp,
    ClipboardDown,
    Copy(PathBuf),
    Cut(PathBuf),
    SelectClipboardItem(usize),
    RemoveFromClipboard(usize),
    Paste,
    ToggleClipboard,
    ClearClipboard,

    // ===== System Monitoring =====
    ToggleSystemMonitor,
    SystemMonitorUpdate(SystemData),
    ToggleProcessMonitor,
    ProcessMonitorUpdate(Vec<ProcessData>),
    KillProcess(u32),

    // ===== Metadata Updates =====
    UpdateEntryMetadata {
        directory_path: PathBuf,
        entry_path: PathBuf,
        updated_entry: ObjectInfo,
    },

    // ===== Render System Integration =====
    TriggerImmediateRender {
        trigger_source: RenderTrigger,
        frame_count: u64,
        timestamp: SystemTime,
    },

    HandleRenderError {
        error: String,
        frame_count: u64,
        error_source: String,
        recovery_action: Option<Box<Action>>,
        timestamp: SystemTime,
    },

    // ===== Internal Actions =====
    TaskResult(TaskResult),
    ShowInputPrompt(InputPromptType),
    SubmitInputPrompt(String),
    UpdateInput(String),
    DirectoryScanUpdate {
        path: PathBuf,
        update: ScanUpdate,
    },
}

impl Action {
    /// Get operation priority (lower = higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Action::Quit => 0,
            Action::HandleRenderError { .. } => 1,
            Action::Key(_) | Action::Mouse(_) => 2,
            Action::TriggerImmediateRender { .. } => 3,
            Action::MoveSelectionUp | Action::MoveSelectionDown => 4,
            Action::EnterSelected | Action::GoToParent => 5,
            Action::UpdateEntryMetadata { .. } => 6,
            Action::Copy(_) | Action::Cut(_) | Action::Paste => 7,
            Action::ContentSearch(_) | Action::FileNameSearch(_) => 8,
            Action::SystemMonitorUpdate(_) | Action::ProcessMonitorUpdate(_) => 9,
            _ => 10,
        }
    }

    /// Check if action requires async processing
    pub fn is_async(&self) -> bool {
        matches!(
            self,
            Action::Copy(_)
                | Action::Cut(_)
                | Action::Paste
                | Action::ContentSearch(_)
                | Action::CreateFileWithName(_)
                | Action::CreateDirectoryWithName(_)
                | Action::Delete
                | Action::RenameEntry(_)
        )
    }

    /// Check if action modifies filesystem
    pub fn modifies_filesystem(&self) -> bool {
        matches!(
            self,
            Action::CreateFileWithName(_)
                | Action::CreateDirectoryWithName(_)
                | Action::Delete
                | Action::RenameEntry(_)
                | Action::Paste
        )
    }

    /// Get human-readable description for TSV logging
    pub fn description(&self) -> &'static str {
        match self {
            Action::TriggerImmediateRender { .. } => "Trigger immediate render",
            Action::HandleRenderError { .. } => "Handle render error",
            Action::UpdateEntryMetadata { .. } => "Update entry metadata",
            Action::Copy(_) => "Copy to clipboard",
            Action::Cut(_) => "Cut to clipboard",
            Action::Paste => "Paste from clipboard",
            Action::EnterSelected => "Enter selected item",
            Action::GoToParent => "Go to parent directory",
            Action::Delete => "Delete selected item",
            Action::Quit => "Quit application",
            Action::ReloadDirectory => "Reload directory",
            Action::ContentSearch(_) => "Content search",
            Action::FileNameSearch(_) => "Filename search",
            _ => "Unknown action",
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::TriggerImmediateRender {
                trigger_source,
                frame_count,
                ..
            } => {
                write!(
                    f,
                    "TriggerImmediateRender(source={trigger_source:?}, frame={frame_count})"
                )
            }
            Action::HandleRenderError {
                error,
                frame_count,
                error_source,
                ..
            } => {
                write!(
                    f,
                    "HandleRenderError(error={error}, frame={frame_count}, source={error_source})"
                )
            }
            Action::UpdateEntryMetadata { entry_path, .. } => {
                write!(f, "UpdateEntryMetadata({})", entry_path.display())
            }
            _ => write!(f, "{}", self.description()),
        }
    }
}

unsafe impl Send for Action {}
unsafe impl Sync for Action {}
