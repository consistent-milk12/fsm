//! src/model/ui_state.rs
//! ============================================================================
//! # UIState: Power-User UI and Interaction State
//!
//! Tracks all ephemeral and persistent UI state for the file manager, including
//! selection, multi-selection, overlays, hotkeys, panes, quick actions, and more.

use std::collections::HashSet;

use crate::model::command_palette::CommandPaletteState;

/// UI focus/mode (e.g., for keyboard-driven workflows)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UIMode {
    Browse,
    Visual, // for multi-select/range
    Search,
    Prompt,
    HelpOverlay,
    BatchOp, // show/cancel batch operation
    Scripting, // for scripting/plugins
             // ... extend as needed
}

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
    // ... extend as needed
}

/// All advanced UI state: selections, overlays, hotkeys, panes, themes, etc.
#[derive(Debug, Clone)]
pub struct UIState {
    /// The selected entry in the current pane.
    pub selected: Option<usize>,
    /// Set of multi-selected entry indices in the current pane.
    pub marked_indices: HashSet<usize>,
    /// If in range/visual mode: (start, end) of selection.
    pub visual_range: Option<(usize, usize)>,
    /// UI mode (browse, visual, search, scripting, etc).
    pub mode: UIMode,
    /// Current overlay, if any.
    pub overlay: UIOverlay,
    /// User input buffer (prompt, search, command, etc).
    pub input: String,
    /// Last search/filter string.
    pub last_query: Option<String>,
    /// Show hidden files flag.
    pub show_hidden: bool,
    /// Current theme.
    pub theme: String,
    /// Index of currently active pane.
    pub active_pane: usize,
    /// (Optional) Recently used quick actions.
    pub recent_actions: Vec<String>,
    pub command_palette: CommandPaletteState,
    pub show_help: bool,
}

impl UIState {
    pub fn new() -> Self {
        UIState {
            selected: Some(0),
            marked_indices: HashSet::new(),
            visual_range: None,
            mode: UIMode::Browse,
            overlay: UIOverlay::None,
            last_query: None,
            input: String::new(),
            show_hidden: false,
            theme: "default".to_string(),
            active_pane: 0,
            recent_actions: Vec::with_capacity(16),
            command_palette: CommandPaletteState::default(),
            show_help: false,
        }
    }

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

    pub fn set_mode(&mut self, mode: UIMode) {
        self.mode = mode;
    }

    pub fn set_overlay(&mut self, overlay: UIOverlay) {
        self.overlay = overlay;
    }

    pub fn set_input(&mut self, s: impl Into<String>) {
        self.input = s.into();
    }

    pub fn set_last_query(&mut self, query: Option<String>) {
        self.last_query = query;
    }

    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
    }

    pub fn set_theme(&mut self, theme: impl Into<String>) {
        self.theme = theme.into();
    }

    pub fn set_active_pane(&mut self, pane: usize) {
        self.active_pane = pane;
    }

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
