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

use std::collections::HashSet;

use crate::controller::actions::InputPromptType;
use crate::fs::object_info::ObjectInfo;
use crate::model::command_palette::{Command, CommandAction, CommandPaletteState};
use crate::tasks::search_task::RawSearchResult;

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
            Self::Main => 0b00000001,         // Main file listing
            Self::StatusBar => 0b00000010,    // Status/footer bar
            Self::Overlay => 0b00000100,      // Active overlay/modal
            Self::Notification => 0b00001000, // Notification area
            Self::Command => 0b00010000,      // Command input area
            Self::Sidebar => 0b00100000,      // Left sidebar/pane
            Self::Preview => 0b01000000,      // Preview pane
            Self::All => 0b01111111,          // Full UI redraw
        }
    }
}

// UI modes for keyboard-driven workflows, selections, and plugins
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UIMode {
    Browse,
    Visual, // for multi-select/range
    Search,
    Prompt,
    Command,   // vim-style command input mode
    Scripting, // for scripting/plugins
    BatchOp,   // show/cancel batch operation
}

// All overlays (mutually exclusive modals)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UIOverlay {
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
    pub timestamp: std::time::Instant,
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

/// Main UI state structure
#[derive(Debug, Clone, PartialEq)]
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
}

impl Default for UIState {
    fn default() -> Self {
        Self::new()
    }
}
