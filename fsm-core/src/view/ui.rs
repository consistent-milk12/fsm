
//! src/view/ui.rs - Enhanced UI rendering with premium clipboard overlayintegration
use std::rc::Rc;

use crate::model::ui_state::UIOverlay;
use crate::{
    AppState, ClipboardOverlay, ContentSearchOverlay, FileNameSearchOverlay, FileOperationsOverlay,
    HelpOverlay, InputPromptOverlay, LoadingOverlay, NotificationOverlay, ObjectTable,
    SearchOverlay, SearchResultsOverlay, StatusBar,
};

use ratatui::layout::Rect;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

pub struct View;

impl View {
    /// Enhanced UI rendering with premium clipboard overlay support
    pub fn redraw(frame: &mut Frame<'_>, app: &mut AppState) {
        // Main application layout
        let main_layout: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),   // Main content area
                Constraint::Length(1), // Status bar
            ])
            .split(frame.area());

        // Render core application components
        ObjectTable::render(frame, app, main_layout[0]);
        StatusBar::render(frame, app, main_layout[1]);

        // Render modal overlays (mutually exclusive)
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

        // Render file operations progress overlay (non-modal, can coexist)
        if !app.ui.active_file_operations.is_empty() {
            let progress_area = Self::calculate_progress_overlay_area(
                frame.area(),
                app.ui.active_file_operations.len(),
            );
            FileOperationsOverlay::render(frame, progress_area, &app.ui.active_file_operations);
        }

        // Render premium clipboard overlay (high priority, modal-like)
        if app.ui.clipboard_overlay_active {
            // Calculate premium centered overlay area with generous spacing
            let clipboard_area = Self::calculate_premium_clipboard_area(frame.area());

            // Create a persistent clipboard overlay instance
            // Note: In a real implementation, you might want to store this in AppState
            // to avoid recreating it every frame for better performance
            let mut clipboard_overlay = ClipboardOverlay::new();

            // Render with zero-allocation performance and error handling
            if let Err(e) = futures::executor::block_on(clipboard_overlay.render_zero_alloc(
                frame,
                clipboard_area,
                &app.ui.clipboard,
                app.ui.selected_clipboard_item_index,
            )) {
                // Graceful error handling - log but don't crash UI
                eprintln!("Premium clipboard overlay render error: {}", e);

                // Fallback: Show error notification
                app.ui.show_error(format!("Clipboard overlay error: {}", e));
            }
        }

        // Always render notifications on top (highest priority)
        if app.ui.notification.is_some() {
            NotificationOverlay::render(frame, app, frame.area());
        }
    }

    /// Calculate file operations progress overlay position
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

    /// Calculate premium clipboard overlay area with optimal sizing and centering
    fn calculate_premium_clipboard_area(area: Rect) -> Rect {
        // Premium sizing: larger overlay for better visibility and usability
        let min_width = 100u16; // Minimum width for readability
        let min_height = 25u16; // Minimum height for content

        // Calculate responsive dimensions
        let overlay_width = (area.width * 85 / 100) // 85% of screen width
            .max(min_width)
            .min(area.width.saturating_sub(4)); // Leave 2 chars padding on each side

        let overlay_height = (area.height * 80 / 100) // 80% of screen height
            .max(min_height)
            .min(area.height.saturating_sub(4)); // Leave 2 chars padding top/bottom

        // Perfect centering
        let x = (area.width.saturating_sub(overlay_width)) / 2;
        let y = (area.height.saturating_sub(overlay_height)) / 2;

        Rect {
            x: area.x + x,
            y: area.y + y,
            width: overlay_width,
            height: overlay_height,
        }
    }

    /// Legacy method for backward compatibility - now calls premium version
    #[allow(unused)]
    fn calculate_centered_overlay_area(
        area: Rect,
        width_percent: u16,
        height_percent: u16,
    ) -> Rect {
        let overlay_width = (area.width * width_percent / 100).min(area.width);
        let overlay_height = (area.height * height_percent / 100).min(area.height);

        let x = (area.width.saturating_sub(overlay_width)) / 2;
        let y = (area.height.saturating_sub(overlay_height)) / 2;

        Rect {
            x: area.x + x,
            y: area.y + y,
            width: overlay_width,
            height: overlay_height,
        }
    }
}
