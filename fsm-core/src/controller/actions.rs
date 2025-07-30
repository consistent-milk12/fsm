//! Optimized Actions module with render system integration and TSV tracing
use crate::fs::object_info::ObjectInfo;
use crate::fs::utils::ScanUpdate;

use clipr::ClipBoard;
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

#[derive(Clone, Debug)]
pub struct ClipboardStats {
    pub total_items: u64,
    pub copy_items: u64,
    pub move_items: u64,
    pub total_size_bytes: u64,
    pub cache_hit_ratio: f32,
    pub last_updated: SystemTime,
}

impl Default for ClipboardStats {
    fn default() -> Self {
        Self {
            total_items: 0,
            copy_items: 0,
            move_items: 0,
            total_size_bytes: 0,
            cache_hit_ratio: 0.0,
            last_updated: SystemTime::now(),
        }
    }
}

/// clipr operation types for batch processing
#[derive(Clone, Debug)]
pub enum ClipboardOperationType {
    Copy,
    Move,
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

    // Advanced Clipboard Actions
    /// Batch add multiple items to clipboard with specified operation type
    ClipboardAddBatch {
        paths: Vec<PathBuf>,
        operations: ClipboardOperationType,
        operation_id: OperationId,
    },

    /// Paste specific clipboard items by ID to destination
    ClipboardPasteBatch {
        item_ids: Vec<u64>,
        destination: PathBuf,
        operation_id: OperationId,
    },

    /// SIMD-accelerated search through clipboard ietms
    ClipboardSearch {
        pattern: String,
        operation_id: OperationId,
    },

    /// Multi-select clipboard items for batch operations
    ClipboardSelectMultiple {
        item_ids: Vec<u64>,
        operation_id: OperationId,
    },

    /// Remove specific clipboard items by ID
    ClipboardRemoveItems {
        item_ids: Vec<u64>,
        operation_id: OperationId,
    },

    /// Optimized bulk clear using clipr's native clear_all()
    ClipboardOptimizedClear {
        operation_id: OperationId,
    },

    /// Get real-time clipboard statistics
    ClipboardGetStats {
        operation_id: OperationId,
    },

    /// Update clipboard cache for UI refresh
    ClipboardUpdateCache {
        operation_id: OperationId,
    },

    /// Show clipboard search results with highlighting
    ClipboardShowSearchResults {
        /// Item IDs matching search
        results: Vec<u64>,
        pattern: String,
        operation_id: OperationId,
    },

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

            // clipr advanced operations get high priority
            Action::ClipboardAddBatch { .. } | Action::ClipboardPasteBatch { .. } => 6,
            Action::ClipboardSearch { .. } | Action::ClipboardSelectMultiple { .. } => 7,
            Action::ClipboardGetStats { .. } | Action::ClipboardUpdateCache { .. } => 7,

            // Legacy clipboard operations
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
                | Action::ClipboardAddBatch { .. }
                | Action::ClipboardPasteBatch { .. }
                | Action::ClipboardSearch { .. }
                | Action::ClipboardOptimizedClear { .. }
                | Action::ClipboardGetStats { .. }
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
                | Action::ClipboardPasteBatch { .. }
        )
    }

    /// Get human-readable description for TSV logging
    pub fn description(&self) -> &'static str {
        match self {
            Action::TriggerImmediateRender { .. } => {
                "Trigger 
  immediate render"
            }
            Action::HandleRenderError { .. } => "Handle render error",
            Action::UpdateEntryMetadata { .. } => {
                "Update entry 
  metadata"
            }

            // Legacy clipboard actions
            Action::Copy(_) => "Copy to clipboard",
            Action::Cut(_) => "Cut to clipboard",
            Action::Paste => "Paste from clipboard",

            // clipr advanced actions
            Action::ClipboardAddBatch { .. } => {
                "Batch add to 
  clipboard"
            }
            Action::ClipboardPasteBatch { .. } => {
                "Batch paste from 
  clipboard"
            }
            Action::ClipboardSearch { .. } => {
                "Search clipboard 
  items"
            }
            Action::ClipboardSelectMultiple { .. } => {
                "Multi-select 
  clipboard items"
            }
            Action::ClipboardRemoveItems { .. } => {
                "Remove clipboard 
  items"
            }
            Action::ClipboardOptimizedClear { .. } => {
                "Optimized 
  clipboard clear"
            }
            Action::ClipboardGetStats { .. } => {
                "Get clipboard 
  statistics"
            }
            Action::ClipboardUpdateCache { .. } => {
                "Update clipboard 
  cache"
            }
            Action::ClipboardShowSearchResults { .. } => {
                "Show 
  clipboard search results"
            }

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

    /// Get operation ID if available
    pub fn operation_id(&self) -> Option<&OperationId> {
        match self {
            Action::ClipboardAddBatch { operation_id, .. } => Some(operation_id),
            Action::ClipboardPasteBatch { operation_id, .. } => Some(operation_id),
            Action::ClipboardSearch { operation_id, .. } => Some(operation_id),
            Action::ClipboardSelectMultiple { operation_id, .. } => Some(operation_id),
            Action::ClipboardRemoveItems { operation_id, .. } => Some(operation_id),
            Action::ClipboardOptimizedClear { operation_id, .. } => Some(operation_id),
            Action::ClipboardGetStats { operation_id, .. } => Some(operation_id),
            Action::ClipboardUpdateCache { operation_id, .. } => Some(operation_id),
            Action::ClipboardShowSearchResults { operation_id, .. } => Some(operation_id),
            _ => None,
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
                    "TriggerImmediateRender(source={trigger_source:?},
   frame={frame_count})"
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
                    "HandleRenderError(error={error}, 
  frame={frame_count}, source={error_source})"
                )
            }
            Action::UpdateEntryMetadata { entry_path, .. } => {
                write!(f, "UpdateEntryMetadata({})", entry_path.display())
            }
            Action::ClipboardAddBatch {
                paths,
                operations,
                operation_id,
            } => {
                write!(
                    f,
                    "ClipboardAddBatch({} items, {:?}, {})",
                    paths.len(),
                    operations,
                    operation_id
                )
            }

            Action::ClipboardPasteBatch {
                item_ids,
                destination,
                operation_id,
            } => {
                write!(
                    f,
                    "ClipboardPasteBatch({} items, {}, {})",
                    item_ids.len(),
                    destination.display(),
                    operation_id
                )
            }

            Action::ClipboardSearch {
                pattern,
                operation_id,
            } => {
                write!(f, "ClipboardSearch(pattern={}, {})", pattern, operation_id)
            }

            _ => write!(f, "{}", self.description()),
        }
    }
}

unsafe impl Send for Action {}
unsafe impl Sync for Action {}
