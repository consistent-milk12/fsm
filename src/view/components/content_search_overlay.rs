//! src/view/components/content_search_overlay.rs
//! ============================================================================
//! # ContentSearchOverlay: Search content within files using ripgrep
//!
//! Provides a search interface for finding patterns in file contents

use crate::view::theme;
use crate::{model::app_state::AppState, view::icons};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

pub struct ContentSearchOverlay;

impl ContentSearchOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let overlay_area = Self::centered_rect(80, 70, area);
        frame.render_widget(Clear, overlay_area);

        // Split the overlay into input and results
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input box
                Constraint::Fill(1),   // Results or loading
            ])
            .split(overlay_area);

        // Render input box
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(" Content Search (ripgrep) ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::YELLOW))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(app.ui.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, layout[0]);

        // Show cursor
        frame.set_cursor_position((layout[0].x + app.ui.input.len() as u16 + 1, layout[0].y + 1));

        // Render search results or status
        Self::render_content_results(frame, app, layout[1]);

        // Render help text at bottom
        let help_text = "Type pattern • Enter to search and jump to file • Esc to close";
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

    fn render_content_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        // Check if there's an active search task
        let is_searching = app
            .tasks
            .values()
            .any(|task| task.description.contains("Content search") && !task.is_completed);

        // Show loading state if searching
        if is_searching {
            let loading_text = format!("Searching for '{}'...", app.ui.input);
            let loading = Paragraph::new(loading_text)
                .style(Style::default().fg(theme::YELLOW))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::YELLOW))
                        .style(Style::default().bg(theme::BACKGROUND)),
                );
            frame.render_widget(loading, area);
            return;
        }

        // Check if we have raw search results first (preferred)
        if let Some(ref raw_results) = app.raw_search_results
            && !raw_results.lines.is_empty()
        {
            Self::render_raw_results(frame, app, area);
            return;
        }

        // Check if we have rich search results, then fallback to simple results
        if !app.rich_search_results.is_empty() {
            Self::render_rich_results(frame, app, area);
            return;
        }

        // Check if we have simple search results
        if app.search_results.is_empty() {
            let status_text = if app.ui.input.is_empty() {
                "Enter a search pattern to find content in files"
            } else if app.ui.last_query.is_some() {
                "No files found with that pattern"
            } else {
                "Press Enter to search for this pattern in files"
            };

            let status = Paragraph::new(status_text)
                .style(Style::default().fg(theme::COMMENT))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::COMMENT))
                        .style(Style::default().bg(theme::BACKGROUND)),
                );
            frame.render_widget(status, area);
            return;
        }

        // Create list items from search results
        let list_items: Vec<ListItem> = app
            .search_results
            .iter()
            .map(|entry| {
                let file_name = entry
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let dir_path = entry
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string());

                let display_text = format!("{} {} ({})", icons::FILE_ICON, file_name, dir_path);

                ListItem::new(display_text).style(Style::default().fg(theme::FOREGROUND))
            })
            .collect();

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} Files Found ", app.search_results.len()))
            .border_style(Style::default().fg(theme::YELLOW))
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

    fn render_rich_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        // Simple display of rich search results (deprecated - using raw results now)
        let list_items: Vec<ListItem> = app
            .rich_search_results
            .iter()
            .map(|line| ListItem::new(line.as_str()).style(Style::default().fg(theme::FOREGROUND)))
            .collect();

        let title = format!(" {} Rich Results ", app.rich_search_results.len());

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::YELLOW))
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

    fn render_raw_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        if let Some(ref raw_results) = app.raw_search_results {
            // Create list items from parsed ANSI text (preserving colors)
            let list_items: Vec<ListItem> = raw_results
                .parsed_lines
                .iter()
                .map(|parsed_text| {
                    // Use the parsed text with ANSI colors preserved
                    ListItem::new(parsed_text.clone())
                })
                .collect();

            let title = format!(" {} Lines Found ", raw_results.total_matches);

            let results_block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme::YELLOW))
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
