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

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Clear,
};

// --- Core application state and overlay enum ---
use crate::AppState;
use crate::model::ui_state::UIOverlay;

// --- All UI components (modal overlays and widgets) ---
use crate::view::components::{
    help_overlay::HelpOverlay, loading_overlay::LoadingOverlay, object_table::ObjectTable,
    prompt_bar::PromptBar, search_overlay::SearchOverlay, status_bar::StatusBar,
};

pub struct View;

impl View {
    /// Draws the full UI for one frame; to be called in the `terminal.draw(|frame| ...)` callback.
    pub fn redraw(frame: &mut Frame<'_>, app: &AppState) {
        let chunks: Vec<Rect> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(1)])
            .split(frame.area())
            .to_vec();

        // Render primary file list area
        ObjectTable::render(frame, app, chunks[0]);

        // Render the bottom status bar
        StatusBar::render(frame, app, chunks[1]);

        // Render overlay (if active) in centered box
        if let Some(area) = Self::overlay_area(app.ui.overlay, frame.area()) {
            // Optional: Clear background behind overlay
            frame.render_widget(Clear, area);

            match app.ui.overlay {
                UIOverlay::Help => HelpOverlay::render(frame, app, area),
                UIOverlay::Search => SearchOverlay::render(frame, app, area),
                UIOverlay::Loading => LoadingOverlay::render(frame, app, area),
                UIOverlay::Prompt => PromptBar::render(frame, app, area),
                UIOverlay::Status => StatusBar::render(frame, app, area),
                UIOverlay::None => {}
                UIOverlay::Batch => todo!(),
                UIOverlay::Scripting => todo!(),
                UIOverlay::CommandPalette => todo!(),
            }
        }
    }

    /// Calculate a centered overlay rectangle when needed.
    fn overlay_area(overlay: UIOverlay, full: Rect) -> Option<Rect> {
        match overlay {
            UIOverlay::None => None,
            _ => {
                let w: u16 = (full.width * 80 / 100).max(10);
                let h: u16 = (full.height * 60 / 100).max(5);
                let x: u16 = full.x + (full.width - w) / 2;
                let y: u16 = full.y + (full.height - h) / 2;
                Some(Rect::new(x, y, w, h))
            }
        }
    }
}
