//!
//! Filepath: src/view/ui.rs
//! Caller File: src/main.rs
//!

use std::rc::Rc;

use crate::model::ui_state::{Component, UIOverlay};
use crate::{
    ContentSearchOverlay, FileNameSearchOverlay, FileOperationsOverlay, HelpOverlay,
    InputPromptOverlay, LoadingOverlay, NotificationOverlay, ObjectTable, SearchOverlay,
    SearchResultsOverlay, StatusBar, model::shared_state::SharedState,
};

use ratatui::layout::Rect;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use tracing::{instrument, span::EnteredSpan};

pub struct View;

impl View {
    /// Draws the full UI for one frame, only rendering components marked as dirty.
    #[instrument(skip_all, fields(operation_type = "ui_redraw"))]
    pub fn redraw(frame: &mut Frame<'_>, shared_state: &SharedState) {
        let area: Rect = frame.area();

        let (content_constraint, status_constraint) = match area.height {
            0 => return,

            1 => (Constraint::Fill(1), None),

            _ => (Constraint::Fill(1), Some(Constraint::Length(1))),
        };

        let mut constraints: Vec<Constraint> = vec![content_constraint];

        if let Some(status) = status_constraint {
            constraints.push(status);
        }

        let main_layout: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        if !main_layout.is_empty() && main_layout[0].height > 0 {
            let ui_guard = shared_state.lock_ui();
            if ui_guard.is_dirty_component(Component::Main) {
                drop(ui_guard); // Release lock before rendering
                ObjectTable::render(frame, shared_state, main_layout[0]);
            }
        }

        if main_layout.len() > 1 && main_layout[1].height > 0 {
            let ui_guard = shared_state.lock_ui();
            if ui_guard.is_dirty_component(Component::StatusBar) {
                drop(ui_guard); // Release lock before rendering
                StatusBar::render_with_degradation(frame, shared_state, main_layout[1]);
            }
        }

        // Overlays are rendered on top of the main UI only if the overlay component is dirty
        let should_render_overlay = {
            let ui_guard = shared_state.lock_ui();
            ui_guard.is_dirty_component(Component::Overlay)
                && ui_guard.overlay != UIOverlay::None
                && area.height > 2
                && area.width > 20
        };

        if should_render_overlay {
            let _overlay_span: EnteredSpan =
                tracing::info_span!("overlay_render", operation_type = "overlay_render").entered();

            let overlay_area: Rect = frame.area();

            let overlay_type = {
                let ui_guard = shared_state.lock_ui();
                ui_guard.overlay.clone()
            };

            match overlay_type {
                UIOverlay::Help => HelpOverlay::render(frame, shared_state, overlay_area),

                UIOverlay::Search => SearchOverlay::render(frame, shared_state, overlay_area),

                UIOverlay::FileNameSearch => {
                    let mut filename_search_overlay = {
                        let ui_guard = shared_state.lock_ui();
                        ui_guard.filename_search_overlay.clone()
                    };

                    FileNameSearchOverlay::render(
                        &mut filename_search_overlay,
                        frame,
                        shared_state,
                        overlay_area,
                    );
                }

                UIOverlay::ContentSearch => {
                    ContentSearchOverlay::render(frame, shared_state, overlay_area)
                }

                UIOverlay::SearchResults => {
                    SearchResultsOverlay::render(frame, shared_state, overlay_area)
                }

                UIOverlay::Loading => LoadingOverlay::render(frame, shared_state, overlay_area),

                UIOverlay::Prompt => InputPromptOverlay::render(frame, shared_state, overlay_area),

                _ => {}
            }
        }

        // Render file operations progress overlay if operations are active
        let active_file_ops = {
            let ui_guard = shared_state.lock_ui();
            ui_guard.active_file_operations.clone()
        };

        if !active_file_ops.is_empty() {
            let _file_ops_span: EnteredSpan =
                tracing::info_span!("file_ops_render", operation_type = "file_ops_render")
                    .entered();
            let overlay_area =
                Self::calculate_progress_overlay_area(frame.area(), active_file_ops.len());

            FileOperationsOverlay::render(frame, overlay_area, &active_file_ops);
        }

        // Always render notifications on top of everything if the notification component is dirty
        let should_render_notification = {
            let ui_guard = shared_state.lock_ui();
            ui_guard.is_dirty_component(Component::Notification) && ui_guard.notification.is_some()
        };

        if should_render_notification {
            let _notification_span = tracing::info_span!(
                "notification_render",
                operation_type = "notification_render"
            )
            .entered();
            NotificationOverlay::render(frame, shared_state, frame.area());
        }
    }

    #[allow(clippy::cast_possible_truncation)]
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
