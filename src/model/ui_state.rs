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

use crate::model::command_palette::CommandPaletteState;
use std::collections::HashSet;

// UI modes for keyboard-driven workflows, selections, and plugins
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UIMode {
    Browse,
    Visual, // for multi-select/range
    Search,
    Prompt,
    Scripting, // for scripting/plugins
    BatchOp,   // show/cancel batch operation
}

// All overlays (mutually exclusive modals)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UIOverlay {
    None,
    Help,
    Search,
    Loading,
    Status,
    Prompt,
    Batch,
    Scripting,
    CommandPalette,
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
            command_palette: CommandPaletteState::default(),
            loading: None,
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
    pub fn toggle_command_palette(&mut self) {
        self.overlay = match self.overlay {
            UIOverlay::CommandPalette => UIOverlay::None,
            _ => UIOverlay::CommandPalette,
        };
    }
    pub fn close_all_overlays(&mut self) {
        self.overlay = UIOverlay::None;
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
