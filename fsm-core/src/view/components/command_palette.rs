// src/view/components/command_palette.rs

use crate::model::app_state::AppState;
use ratatui::{Frame, layout::Rect};

pub struct CommandPalette;

impl CommandPalette {
    pub const fn render(_frame: &mut Frame<'_>, _app: &AppState, _area: Rect) {
        // CommandPalette is no longer used as overlay - kept for compatibility
        // Unused - all functionality moved to inline command mode
    }
}
