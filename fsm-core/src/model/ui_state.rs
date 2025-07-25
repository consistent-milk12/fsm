//! src/model/ui_state.rs
//! ============================================================================
//! # UIState: Power-User UI and Interaction State
//!
//! Tracks all ephemeral and persistent UI state for the file manager, including
//! selection, overlays, modes, panes, themes, quick actions, command palette, and more.
//!
//! - Robust overlay/mode pattern (single active overlay, distinct mode)
//! - Extensible for new overlays/plugins (search, scripting, batch, etc.)
//! - Optimized for immediate-mode TUI, multi-pane and batch ops

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::controller::actions::InputPromptType;
use crate::fs::object_info::ObjectInfo;
use crate::model::command_palette::{Command, CommandAction, CommandPaletteState};
use crate::tasks::search_task::RawSearchResult;

use clipr::ClipBoard;

/// Granular redraw flags for selective UI updates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedrawFlag {
    Main,
    StatusBar,
    Overlay,
    Notification,
    Command,
    Sidebar,
    Preview,
    All,
}

impl RedrawFlag {
    pub const fn bits(self) -> u8 {
        match self {
            Self::Main => 0b0000_0001,         // Main file listing
            Self::StatusBar => 0b0000_0010,    // Status/footer bar
            Self::Overlay => 0b0000_0100,      // Active overlay/modal
            Self::Notification => 0b0000_1000, // Notification area
            Self::Command => 0b0001_0000,      // Command input area
            Self::Sidebar => 0b0010_0000,      // Left sidebar/pane
            Self::Preview => 0b0100_0000,      // Preview pane
            Self::All => 0b0111_1111,          // Full UI redraw
        }
    }
}

// UI modes for keyboard-driven workflows, selections, and plugins
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UIMode {
    #[default]
    Browse,

    Visual, // for multi-select/range

    Search,

    Prompt,

    Command, // vim-style command input mode

    Scripting, // for scripting/plugins

    BatchOp, // show/cancel batch operation
}

// All overlays (mutually exclusive modals)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UIOverlay {
    #[default]
    None,

    Help,

    Search,

    FileNameSearch,

    ContentSearch,

    SearchResults,

    Loading,

    Status,

    Prompt,

    Batch,

    Scripting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    /// Search for file and folder names (fast, local)
    FileName,

    /// Search for content within files using ripgrep (slower, recursive)
    ContentGrep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,

    Warning,

    Error,

    Success,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub message: String,

    pub level: NotificationLevel,

    pub timestamp: Instant,

    pub auto_dismiss_ms: Option<u64>,
}

// Detailed loading state for async/batch operations
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadingState {
    /// Human-readable operation (e.g. "Copying", "Scanning", "Loading Cache")
    pub message: String,

    /// Progress: 0.0–1.0, or None for indeterminate/spinner
    pub progress: Option<f64>,

    /// For animated spinner: increments every tick
    pub spinner_frame: usize,

    /// Optional: current file or item name
    pub current_item: Option<String>,

    /// Optional: total and completed counts for batch ops
    pub completed: Option<u64>,

    pub total: Option<u64>,
}

/// Progress tracking structure for file operations
#[derive(Clone, Debug, PartialEq)]
pub struct FileOperationProgress {
    /// Operation type: "copy", "move", "rename"
    pub operation_type: String,

    /// Bytes processed so far
    pub current_bytes: u64,

    /// Total bytes to process
    pub total_bytes: u64,

    /// Current processing file
    pub current_file: PathBuf,

    /// Files processed
    pub files_completed: u32,

    /// Total files to process
    pub total_files: u32,

    /// Start time for ETA computation
    pub start_time: Instant,

    /// Bytes per second throughput
    pub throughput_bps: Option<u64>,

    /// Estimated completion time
    pub estimated_completion: Option<Instant>,
}

impl FileOperationProgress {
    /// Create new progress tracker
    pub fn new(operation_type: String, total_bytes: u64, total_files: u32) -> Self {
        Self {
            operation_type,
            current_bytes: 0,
            total_bytes,
            current_file: PathBuf::new(),
            files_completed: 0,
            total_files,
            start_time: Instant::now(),
            throughput_bps: None,
            estimated_completion: None,
        }
    }

    /// Update progress and calculate throughput/ETA
    pub fn update(&mut self, current_bytes: u64, current_file: PathBuf, files_completed: u32) {
        self.current_bytes = current_bytes;
        self.current_file = current_file;
        self.files_completed = files_completed;

        // Calculate throughput
        let elapsed: Duration = self.start_time.elapsed();

        if elapsed.as_secs() > 0 && current_bytes > 0 {
            self.throughput_bps = Some(current_bytes / elapsed.as_secs());

            // Estimate completion time
            if let Some(bps) = self.throughput_bps
                && bps > 0
            {
                let rem_bytes: u64 = self.total_bytes.saturating_sub(current_bytes);

                // Approx. seconds needed to finish
                let rem_secs: u64 = rem_bytes / bps;

                let now: Instant = Instant::now();
                let rem_time: Duration = Duration::from_secs(rem_secs);
                let eta: Instant = now + rem_time;

                self.estimated_completion = Some(eta);
            }
        }
    }

    /// Get progress percentage (0.0 to 1.0)
    pub fn progress_ratio(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.current_bytes as f64 / self.total_bytes as f64
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ClipBoardViewMode {
    /// Standard list view
    #[default]
    List,

    /// Detailed view with metadata
    Details,

    /// Grid layout for many items
    Grid,
}

/// Main UI state structure
#[derive(Debug, Default, Clone)]
pub struct UIState {
    pub redraw_flags: u8,
    // --- Selection and Navigation State ---
    /// Current selected entry in the active pane.
    pub selected: Option<usize>,

    /// Multi-selection (indices) for batch ops in current pane.
    pub marked_indices: HashSet<usize>,

    /// Visual/range selection, if active: (start, end)
    pub visual_range: Option<(usize, usize)>,

    /// Index of active pane.
    pub active_pane: usize,

    // --- Mode and Overlay State ---
    /// High-level UI mode (browse, search, scripting, etc).
    pub mode: UIMode,

    /// Currently active overlay/modal.
    pub overlay: UIOverlay,

    // --- Input and Interaction State ---
    /// User input buffer (prompt/search/command).
    pub input: String,

    /// Last search/filter query.
    pub last_query: Option<String>,

    /// Command palette modal state.
    pub command_palette: CommandPaletteState,

    /// Type of input prompt currently active.
    pub input_prompt_type: Option<InputPromptType>,

    // --- Visual and Display State ---
    /// Show hidden files flag.
    pub show_hidden: bool,

    /// Current theme (theme name).
    pub theme: String,

    // --- Search Results State (moved from AppState) ---
    /// Generic search results for file listing
    pub search_results: Vec<ObjectInfo>,

    /// Filename-specific search results  
    pub filename_search_results: Vec<ObjectInfo>,

    /// Rich text search results (formatted strings)
    pub rich_search_results: Vec<String>,

    /// Raw search results from ripgrep
    pub raw_search_results: Option<RawSearchResult>,

    /// Currently selected index in raw search results
    pub raw_search_selected: usize,

    // --- Feedback and Status State (consolidated) ---
    /// Current loading overlay state (if active).
    pub loading: Option<LoadingState>,

    /// Current notification (if any).
    pub notification: Option<Notification>,

    /// Last status/info message
    pub last_status: Option<String>,

    /// Recent quick actions (for palette/undo).
    pub recent_actions: Vec<String>,

    /// Track active file operations with progress
    pub active_file_operations: HashMap<String, FileOperationProgress>,

    /// Track cancellation tokens for active operations
    pub operations_cancel_tokens: HashMap<String, CancellationToken>,

    /// Integrated clipboard system
    pub clipboard: Arc<ClipBoard>,

    /// Clipboard overlay state
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item: Option<String>,
    pub selected_clipboard_item_index: usize,
    pub clipboard_view_mode: ClipBoardViewMode,
}

impl PartialEq for UIState {
    fn eq(&self, other: &Self) -> bool {
        self.mode.eq(&other.mode)
    }
}

impl UIState {
    /// Construct a new UI state with default values.
    pub fn new() -> Self {
        Self {
            redraw_flags: RedrawFlag::All.bits(),
            // Selection and Navigation State
            selected: Some(0),
            marked_indices: HashSet::new(),
            visual_range: None,
            active_pane: 0,

            // Mode and Overlay State
            mode: UIMode::Browse,
            overlay: UIOverlay::None,

            // Input and Interaction State
            input: String::new(),
            last_query: None,
            command_palette: CommandPaletteState::new(vec![
                // Only keep essential commands that don't have short aliases
                Command {
                    title: "Open Config".to_string(),
                    action: CommandAction::OpenConfig,
                },
            ]),
            input_prompt_type: None,

            // Visual and Display State
            show_hidden: false,
            theme: "default".to_string(),

            // Search Results State
            search_results: Vec::new(),
            filename_search_results: Vec::new(),
            rich_search_results: Vec::new(),
            raw_search_results: None,
            raw_search_selected: 0,

            // Feedback and Status State
            loading: None,
            notification: None,
            last_status: None,
            recent_actions: Vec::with_capacity(16),

            // File operation tracker
            active_file_operations: HashMap::new(),

            // Operation cancel tracker
            operations_cancel_tokens: HashMap::new(),

            // Clipboard Flag
            clipboard: Arc::new(ClipBoard::default()),
            clipboard_overlay_active: false,
            selected_clipboard_item: None,
            selected_clipboard_item_index: 0,
            clipboard_view_mode: ClipBoardViewMode::default(),
        }
    }

    // --- Selection/marking ---
    pub fn set_selected(&mut self, idx: Option<usize>) {
        self.selected = idx;
        self.request_redraw(RedrawFlag::Main);
    }

    pub fn mark_index(&mut self, idx: usize) {
        self.marked_indices.insert(idx);
        self.request_redraw(RedrawFlag::Main);
    }

    pub fn unmark_index(&mut self, idx: usize) {
        self.marked_indices.remove(&idx);
        self.request_redraw(RedrawFlag::Main);
    }

    pub fn clear_marks(&mut self) {
        self.marked_indices.clear();
        self.visual_range = None;
        self.request_redraw(RedrawFlag::Main);
    }

    pub fn set_visual_range(&mut self, start: usize, end: usize) {
        self.visual_range = Some((start, end));
        self.request_redraw(RedrawFlag::Main);
    }

    pub fn move_selection_up<T>(&mut self, entries: &[T]) {
        if !entries.is_empty() {
            let new_selected = self.selected.map_or(0, |s| s.saturating_sub(1));
            self.selected = Some(new_selected);
            self.request_redraw(RedrawFlag::Main);
        }
    }

    pub fn move_selection_down<T>(&mut self, entries: &[T]) {
        if !entries.is_empty() {
            let new_selected = self
                .selected
                .map_or(0, |s| s.saturating_add(1).min(entries.len() - 1));
            self.selected = Some(new_selected);
            self.request_redraw(RedrawFlag::Main);
        }
    }

    // --- Modes/overlay management ---
    pub fn set_mode(&mut self, mode: UIMode) {
        self.mode = mode;
        self.request_redraw_all();
    }

    pub fn set_overlay(&mut self, overlay: UIOverlay) {
        self.overlay = overlay;
        self.request_redraw_all();
    }

    pub fn toggle_help_overlay(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::Help => UIOverlay::None,
            _ => UIOverlay::Help,
        };

        self.request_redraw_all();
    }

    /// Enter vim-style command mode
    pub fn enter_command_mode(&mut self) {
        self.mode = UIMode::Command;
        self.input.clear();
        self.command_palette.input.clear();
        self.command_palette.update_filter();
    }

    /// Exit command mode and return to browse mode
    pub fn exit_command_mode(&mut self) {
        self.mode = UIMode::Browse;
        self.input.clear();
        self.command_palette.input.clear();

        // Reset completion state to prevent stale completions
        self.command_palette.hide_completions();
        self.command_palette.completions.clear();
        self.command_palette.completion_index = 0;
    }

    /// Check if currently in command input mode
    pub fn is_in_command_mode(&self) -> bool {
        self.mode == UIMode::Command
    }

    pub fn toggle_filename_search_overlay(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::FileNameSearch => UIOverlay::None,
            _ => UIOverlay::FileNameSearch,
        };
        if self.overlay == UIOverlay::FileNameSearch {
            self.input.clear();
        }
        self.request_redraw(RedrawFlag::Overlay);
    }

    pub fn toggle_content_search_overlay(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::ContentSearch => UIOverlay::None,
            _ => UIOverlay::ContentSearch,
        };
        if self.overlay == UIOverlay::ContentSearch {
            self.input.clear();
        }
        self.request_redraw(RedrawFlag::Overlay);
    }
    pub fn close_all_overlays(&mut self) {
        self.overlay = UIOverlay::None;
        self.request_redraw(RedrawFlag::Overlay);
    }

    /// Show a notification with auto-dismiss
    pub fn show_notification(
        &mut self,
        message: String,
        level: NotificationLevel,
        auto_dismiss_ms: Option<u64>,
    ) {
        self.notification = Some(Notification {
            message,
            level,
            timestamp: std::time::Instant::now(),
            auto_dismiss_ms,
        });
        self.request_redraw(RedrawFlag::Notification);
    }

    /// Show an info notification
    pub fn show_info(&mut self, message: String) {
        self.show_notification(message, NotificationLevel::Info, Some(3000));
    }

    /// Show a warning notification  
    pub fn show_warning(&mut self, message: String) {
        self.show_notification(message, NotificationLevel::Warning, Some(5000));
    }

    /// Show an error notification
    pub fn show_error(&mut self, message: String) {
        self.show_notification(message, NotificationLevel::Error, None); // No auto-dismiss for errors
    }

    /// Show a success notification
    pub fn show_success(&mut self, message: String) {
        self.show_notification(message, NotificationLevel::Success, Some(2000));
    }

    /// Dismiss the current notification
    pub fn dismiss_notification(&mut self) {
        self.notification = None;
        self.request_redraw(RedrawFlag::Notification);
    }

    /// Check if notification should auto-dismiss and do so if needed
    pub fn update_notification(&mut self) -> bool {
        if let Some(notification) = &self.notification
            && let Some(auto_dismiss_ms) = notification.auto_dismiss_ms
            && notification.timestamp.elapsed().as_millis() > auto_dismiss_ms as u128
        {
            self.notification = None;
            self.request_redraw(RedrawFlag::Notification);
            return true; // Notification was dismissed
        }
        false
    }

    // --- Redraw and State Management ---
    /// Mark specific UI components for redraw.
    pub fn request_redraw(&mut self, flag: RedrawFlag) {
        self.redraw_flags |= flag.bits();
    }

    /// Mark the entire UI for redraw.
    pub fn request_redraw_all(&mut self) {
        self.redraw_flags = RedrawFlag::All.bits();
    }

    /// Check if any redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        self.redraw_flags != 0
    }

    /// Check if a specific component needs redraw.
    pub fn needs_redraw_for(&self, flag: RedrawFlag) -> bool {
        (self.redraw_flags & flag.bits()) != 0
    }

    /// Clear redraw flags (called after rendering).
    pub fn clear_redraw(&mut self) {
        self.redraw_flags = 0;
    }

    /// Clear a specific redraw flag.
    pub fn clear_redraw_for(&mut self, flag: RedrawFlag) {
        self.redraw_flags &= !flag.bits();
    }

    /// Set status message
    pub fn set_status(&mut self, status: Option<String>) {
        self.last_status = status;
        self.request_redraw(RedrawFlag::StatusBar);
    }

    /// Clear search results
    pub fn clear_search_results(&mut self) {
        self.search_results.clear();
        self.filename_search_results.clear();
        self.rich_search_results.clear();
        self.raw_search_results = None;
        self.raw_search_selected = 0;
        self.request_redraw(RedrawFlag::Main);
    }

    /// Show input prompt for the given type
    pub fn show_input_prompt(&mut self, prompt_type: InputPromptType) {
        self.input_prompt_type = Some(prompt_type);
        self.overlay = UIOverlay::Prompt;
        self.input.clear();
        self.request_redraw(RedrawFlag::Overlay);
    }

    /// Hide input prompt
    pub fn hide_input_prompt(&mut self) {
        self.input_prompt_type = None;
        self.overlay = UIOverlay::None;
        self.input.clear();
        self.request_redraw(RedrawFlag::Overlay);
    }

    // --- Input/query ---
    pub fn set_input(&mut self, s: impl Into<String>) {
        self.input = s.into();
        self.request_redraw(RedrawFlag::Overlay);
    }
    pub fn set_last_query(&mut self, query: Option<String>) {
        self.last_query = query;
        self.request_redraw(RedrawFlag::Overlay);
    }

    // --- UI toggles/theme/panes ---
    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.request_redraw(RedrawFlag::Main);
    }
    pub fn set_theme(&mut self, theme: impl Into<String>) {
        self.theme = theme.into();
        self.request_redraw_all();
    }
    pub fn set_active_pane(&mut self, pane: usize) {
        self.active_pane = pane;
        self.request_redraw_all();
    }

    // --- Quick actions ---
    pub fn push_action(&mut self, action: impl Into<String>) {
        if self.recent_actions.len() == 16 {
            self.recent_actions.remove(0);
        }
        self.recent_actions.push(action.into());
    }

    /// Store cancellation token for operation
    pub fn store_cancel_token(&mut self, operation_id: String, token: CancellationToken) {
        self.operations_cancel_tokens.insert(operation_id, token);
    }

    /// Cancel all active file operations
    pub fn cancel_all_operations(&mut self) -> usize {
        let count: usize = self.operations_cancel_tokens.len();

        // Cancel all tokens
        for token in self.operations_cancel_tokens.values() {
            token.cancel();
        }

        // Clear tracking data
        self.operations_cancel_tokens.clear();
        self.active_file_operations.clear();

        count
    }

    /// Remove completed/cancelled operation
    pub fn remove_operation(&mut self, operation_id: &str) {
        self.operations_cancel_tokens.remove(operation_id);

        self.active_file_operations.remove(operation_id);
    }

    pub fn toggle_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = !self.clipboard_overlay_active;

        if !self.clipboard_overlay_active {
            self.selected_clipboard_item = None;
            self.selected_clipboard_item_index = 0;
        }
    }

    pub fn show_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = true;
        self.request_redraw(RedrawFlag::Overlay);
    }

    pub fn close_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = false;
        self.selected_clipboard_item = None;
        self.selected_clipboard_item_index = 0;
        self.request_redraw(RedrawFlag::Overlay);
    }
}
