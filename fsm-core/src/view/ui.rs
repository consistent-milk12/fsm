//!
//! Filepath: src/view/ui.rs
//! Caller File: src/main.rs
//!

use std::rc::Rc;

use crate::model::ui_state::UIOverlay;
use crate::{
    AppState, ContentSearchOverlay, FileNameSearchOverlay, FileOperationsOverlay, HelpOverlay,
    InputPromptOverlay, LoadingOverlay, NotificationOverlay, ObjectTable, SearchOverlay,
    SearchResultsOverlay, StatusBar,
};

use ratatui::layout::Rect;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

pub struct View;

impl View {
    /// Draws the full UI for one frame.
    pub fn redraw(frame: &mut Frame<'_>, app: &mut AppState) {
        // The main object table's block will act as the background
        let main_layout: Rc<[Rect]> = Layout::default()
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
            let overlay_area: Rect = frame.area();
            match app.ui.overlay {
                UIOverlay::Help => HelpOverlay::render(frame, app, overlay_area),

                UIOverlay::Search => SearchOverlay::render(frame, app, overlay_area),

                UIOverlay::FileNameSearch => {
                    FileNameSearchOverlay::render(frame, app, overlay_area)
                }

                UIOverlay::ContentSearch => ContentSearchOverlay::render(frame, app, overlay_area),

                UIOverlay::SearchResults => SearchResultsOverlay::render(frame, app, overlay_area),

                UIOverlay::Loading => LoadingOverlay::render(frame, app, overlay_area),

                UIOverlay::Prompt => InputPromptOverlay::render(frame, app, overlay_area),

                _ => {}
            }
        }

        // Render file operations progress overlay if operations are active
        if !app.ui.active_file_operations.is_empty() {
            let overlay_area = Self::calculate_progress_overlay_area(
                frame.area(),
                app.ui.active_file_operations.len(),
            );

            FileOperationsOverlay::render(frame, overlay_area, &app.ui.active_file_operations);
        }

        // Always render notifications on top of everything
        if app.ui.notification.is_some() {
            NotificationOverlay::render(frame, app, frame.area());
        }
    }

    /// Calculate overlay position - bottom of screen, above status bar
    fn calculate_progress_overlay_area(screen_size: Rect, operation_count: usize) -> Rect {
        let height: u16 = (operation_count + 3 + 2) as u16;
        let max_height: u16 = screen_size.height / 3;
        let overlay_height: u16 = std::cmp::min(height, max_height);

        Rect {
            x: 1,
            y: screen_size.height.saturating_sub(overlay_height + 2),
            width: screen_size.width.saturating_sub(2),
            height: overlay_height,
        }
    }
}
