//! src/view/components/filename_search_overlay.rs
//! ============================================================================
//! # FileNameSearchOverlay: Live file/folder name search
//!
//! Provides instant search results for file and folder names in the current directory

use crate::fs::object_info::ObjectInfo;
use crate::view::theme;
use crate::{model::app_state::AppState, view::icons};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

pub struct FileNameSearchOverlay;

impl FileNameSearchOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let overlay_area = Self::centered_rect(70, 60, area);
        frame.render_widget(Clear, overlay_area);

        // Split the overlay into input and results
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input box
                Constraint::Fill(1),   // Results
            ])
            .split(overlay_area);

        // Render input box
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(" File/Folder Search ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(app.ui.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, layout[0]);

        // Show cursor
        frame.set_cursor_position((layout[0].x + app.ui.input.len() as u16 + 1, layout[0].y + 1));

        // Render search results
        Self::render_search_results(frame, app, layout[1]);

        // Render help text at bottom
        let help_text = "Type to search • Enter to open • Esc to close";
        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        let help_area = Rect {
            x: overlay_area.x,
            y: overlay_area.y + overlay_area.height,
            width: overlay_area.width,
            height: 1,
        };

        if help_area.y < area.height {
            frame.render_widget(help_paragraph, help_area);
        }
    }

    fn render_search_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        // Check if there's an active filename search task
        let is_searching = app
            .tasks
            .values()
            .any(|task| task.description.contains("Filename search") && !task.is_completed);

        // Show loading state if searching
        if is_searching {
            let loading_text = format!("Searching filenames for '{}'...", app.ui.input);
            let loading = Paragraph::new(loading_text)
                .style(Style::default().fg(theme::CYAN))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::CYAN))
                        .style(Style::default().bg(theme::BACKGROUND)),
                );
            frame.render_widget(loading, area);
            return;
        }

        // Use recursive search results if available, otherwise fallback to local filtering
        let entries_to_display: Vec<_> = if !app.filename_search_results.is_empty() {
            app.filename_search_results.iter().collect()
        } else {
            // Fallback to local filtering for immediate feedback while typing
            let search_term = app.ui.input.to_lowercase();
            if search_term.is_empty() {
                app.fs.active_pane().entries.iter().collect()
            } else {
                app.fs
                    .active_pane()
                    .entries
                    .iter()
                    .filter(|entry| entry.name.to_lowercase().contains(&search_term))
                    .collect()
            }
        };

        if entries_to_display.is_empty() {
            let message = if app.ui.input.is_empty() {
                "Type to search for files and folders recursively"
            } else {
                "No matching files or folders found"
            };
            let no_results = Paragraph::new(message)
                .style(Style::default().fg(theme::COMMENT))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::COMMENT))
                        .style(Style::default().bg(theme::BACKGROUND)),
                );
            frame.render_widget(no_results, area);
            return;
        }

        // Create list items
        let list_items: Vec<ListItem> = entries_to_display
            .iter()
            .map(|entry: &&ObjectInfo| {
                let icon = if entry.is_dir {
                    icons::FOLDER_ICON
                } else if entry.is_symlink {
                    "🔗"
                } else {
                    icons::FILE_ICON
                };

                // Show full path for recursive results, just name for local
                let display_name = if !app.filename_search_results.is_empty() {
                    // Show relative path for recursive search results
                    let current_dir = &app.fs.active_pane().cwd;
                    if let Ok(relative) = entry.path.strip_prefix(current_dir) {
                        relative.to_string_lossy().to_string()
                    } else {
                        entry.path.to_string_lossy().to_string()
                    }
                } else {
                    entry.name.clone()
                };

                // Highlight matching parts
                let display_text = format!("{icon} {display_name}");

                ListItem::new(display_text).style(if entry.is_dir {
                    Style::default().fg(theme::CYAN)
                } else {
                    Style::default().fg(theme::FOREGROUND)
                })
            })
            .collect();

        let title = if !app.filename_search_results.is_empty() {
            format!(" {} Results (Recursive) ", entries_to_display.len())
        } else {
            format!(" {} Results (Local) ", entries_to_display.len())
        };

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        let list = List::new(list_items)
            .block(results_block)
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .add_modifier(Modifier::BOLD),
            );

        let mut list_state = ListState::default();
        list_state.select(app.ui.selected);

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}
