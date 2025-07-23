//! src/view/ui.rs
//! ============================================================================
//! # View: TUI Render Orchestrator (ratatui v0.24+)
//!
//! Uses the simplified `Frame<'_>` API (no backend generic).
//! Each draw cycle refreshes file list, status, and overlays.

//! src/view/ui.rs
//! ============================================================================
//! # View: TUI Render Orchestrator (ratatui v0.24+)
//!
//! Uses the simplified `Frame<'_>` API (no backend generic).
//! Each draw cycle refreshes file list, status, and overlays.

use crate::AppState;
use crate::model::ui_state::UIOverlay;
use crate::view::components::{
    command_palette::CommandPalette, help_overlay::HelpOverlay, loading_overlay::LoadingOverlay,
    object_table::ObjectTable, prompt_bar::PromptBar, search_overlay::SearchOverlay,
    status_bar::StatusBar,
};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

pub struct View;

impl View {
    /// Draws the full UI for one frame.
    pub fn redraw(frame: &mut Frame<'_>, app: &AppState) {
        // The main object table's block will act as the background
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),   // Main content area
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

        ObjectTable::render(frame, app, main_layout[0]);
        StatusBar::render(frame, app, main_layout[1]);

        // Overlays are rendered on top of the main UI
        if app.ui.overlay != UIOverlay::None {
            let overlay_area = frame.area();
            match app.ui.overlay {
                UIOverlay::Help => HelpOverlay::render(frame, app, overlay_area),
                UIOverlay::Search => SearchOverlay::render(frame, app, overlay_area),
                UIOverlay::Loading => LoadingOverlay::render(frame, app, overlay_area),
                UIOverlay::Prompt => PromptBar::render(frame, app, overlay_area),
                UIOverlay::CommandPalette => CommandPalette::render(frame, app, overlay_area),
                _ => {}
            }
        }
    }
}
