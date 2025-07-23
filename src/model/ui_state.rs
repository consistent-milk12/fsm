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

use crate::model::command_palette::{Command, CommandAction, CommandPaletteState};

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

#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NotificationLevel,
    pub timestamp: std::time::Instant,
    pub auto_dismiss_ms: Option<u64>,
}

// Detailed loading state for async/batch operations
#[derive(Clone, Debug, Default)]
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

/// Complete, extensible UI and interaction state.
#[derive(Debug, Clone)]
pub struct UIState {
    /// Current selected entry in the active pane.
    pub selected: Option<usize>,
    /// Multi-selection (indices) for batch ops in current pane.
    pub marked_indices: HashSet<usize>,
    /// Visual/range selection, if active: (start, end)
    pub visual_range: Option<(usize, usize)>,
    /// High-level UI mode (browse, search, scripting, etc).
    pub mode: UIMode,
    /// Currently active overlay/modal.
    pub overlay: UIOverlay,
    /// User input buffer (prompt/search/command).
    pub input: String,
    /// Last search/filter query.
    pub last_query: Option<String>,
    /// Show hidden files flag.
    pub show_hidden: bool,
    /// Current theme (theme name).
    pub theme: String,
    /// Index of active pane.
    pub active_pane: usize,
    /// Recent quick actions (for palette/undo).
    pub recent_actions: Vec<String>,
    /// Command palette modal state.
    pub command_palette: CommandPaletteState,
    /// Current loading overlay state (if active).
    pub loading: Option<LoadingState>,
    /// Current notification (if any).
    pub notification: Option<Notification>,
}

impl UIState {
    /// Construct a new UI state with default values.
    pub fn new() -> Self {
        Self {
            selected: Some(0),
            marked_indices: HashSet::new(),
            visual_range: None,
            mode: UIMode::Browse,
            overlay: UIOverlay::None,
            input: String::new(),
            last_query: None,
            show_hidden: false,
            theme: "default".to_string(),
            active_pane: 0,
            recent_actions: Vec::with_capacity(16),
            command_palette: CommandPaletteState::new(vec![
                Command {
                    title: "Open Config".to_string(),
                    action: CommandAction::OpenConfig,
                },
                Command {
                    title: "Reload Directory".to_string(),
                    action: CommandAction::Reload,
                },
                Command {
                    title: "New File".to_string(),
                    action: CommandAction::NewFile,
                },
                Command {
                    title: "New Folder".to_string(),
                    action: CommandAction::NewFolder,
                },
                Command {
                    title: "Search File Content".to_string(),
                    action: CommandAction::SearchContent,
                },
            ]),
            loading: None,
            notification: None,
        }
    }

    // --- Selection/marking ---
    pub fn set_selected(&mut self, idx: Option<usize>) {
        self.selected = idx;
    }
    pub fn mark_index(&mut self, idx: usize) {
        self.marked_indices.insert(idx);
    }
    pub fn unmark_index(&mut self, idx: usize) {
        self.marked_indices.remove(&idx);
    }
    pub fn clear_marks(&mut self) {
        self.marked_indices.clear();
        self.visual_range = None;
    }
    pub fn set_visual_range(&mut self, start: usize, end: usize) {
        self.visual_range = Some((start, end));
    }

    pub fn move_selection_up<T>(&mut self, entries: &[T]) {
        if !entries.is_empty() {
            let new_selected = self.selected.map_or(0, |s| s.saturating_sub(1));
            self.selected = Some(new_selected);
        }
    }

    pub fn move_selection_down<T>(&mut self, entries: &[T]) {
        if !entries.is_empty() {
            let new_selected = self
                .selected
                .map_or(0, |s| s.saturating_add(1).min(entries.len() - 1));
            self.selected = Some(new_selected);
        }
    }

    // --- Modes/overlay management ---
    pub fn set_mode(&mut self, mode: UIMode) {
        self.mode = mode;
    }
    pub fn set_overlay(&mut self, overlay: UIOverlay) {
        self.overlay = overlay;
    }
    pub fn toggle_help_overlay(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::Help => UIOverlay::None,
            _ => UIOverlay::Help,
        };
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
    }

    pub fn toggle_content_search_overlay(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::ContentSearch => UIOverlay::None,
            _ => UIOverlay::ContentSearch,
        };
        if self.overlay == UIOverlay::ContentSearch {
            self.input.clear();
        }
    }
    pub fn close_all_overlays(&mut self) {
        self.overlay = UIOverlay::None;
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
    }

    /// Check if notification should auto-dismiss and do so if needed
    pub fn update_notification(&mut self) -> bool {
        if let Some(notification) = &self.notification {
            if let Some(auto_dismiss_ms) = notification.auto_dismiss_ms {
                if notification.timestamp.elapsed().as_millis() > auto_dismiss_ms as u128 {
                    self.notification = None;
                    return true; // Notification was dismissed
                }
            }
        }
        false
    }

    // --- Input/query ---
    pub fn set_input(&mut self, s: impl Into<String>) {
        self.input = s.into();
    }
    pub fn set_last_query(&mut self, query: Option<String>) {
        self.last_query = query;
    }

    // --- UI toggles/theme/panes ---
    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
    }
    pub fn set_theme(&mut self, theme: impl Into<String>) {
        self.theme = theme.into();
    }
    pub fn set_active_pane(&mut self, pane: usize) {
        self.active_pane = pane;
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
