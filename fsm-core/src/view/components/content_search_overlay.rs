//! ``src/view/components/content_search_overlay.rs``
//!
//! # `ContentSearchOverlay`: Production-quality content search with ripgrep
//!
//! Enhanced search interface with real-time feedback, syntax highlighting,
//! and comprehensive search result display with context and navigation.

use crate::view::theme;
use crate::{model::app_state::AppState, view::icons};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::Instant;

pub struct ContentSearchOverlay;

impl ContentSearchOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let render_start = Instant::now();

        // Use larger overlay for production quality
        let overlay_area = Self::centered_rect(85, 80, area);
        frame.render_widget(Clear, overlay_area);

        // Enhanced layout with status bar
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input box
                Constraint::Length(1), // Status/stats bar
                Constraint::Fill(1),   // Results or loading
                Constraint::Length(1), // Help text
            ])
            .split(overlay_area);

        // Render enhanced input box with search state
        Self::render_search_input(frame, app, layout[0]);

        // Render search statistics and status
        Self::render_search_status(frame, app, layout[1]);

        // Render search results or status
        Self::render_content_results(frame, app, layout[2]);

        // Render enhanced help text
        Self::render_help_text(frame, app, layout[3]);

        // Performance monitoring
        let render_time = render_start.elapsed();
        if render_time.as_millis() > 16 {
            tracing::info!(
                "Content search overlay render took {:?} (slow)",
                render_time
            );
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    /// Render enhanced search input with visual feedback
    fn render_search_input(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        // Determine input state for visual feedback
        let is_searching = app
            .tasks
            .values()
            .any(|task| task.description.contains("Content search") && !task.is_completed);

        let (title, border_color) = if is_searching {
            (" 🔍 Searching... ", theme::YELLOW)
        } else if app.ui.input.is_empty() {
            (" Content Search (ripgrep) ", theme::CYAN)
        } else if app.ui.raw_search_results.is_some() || !app.ui.search_results.is_empty() {
            (" Content Search - Results Found ", theme::GREEN)
        } else {
            (" Content Search - Type to Search ", theme::CYAN)
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(app.ui.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, area);

        // Enhanced cursor positioning with bounds checking
        let cursor_x = (area.x + app.ui.input.len() as u16 + 1).min(area.x + area.width - 2);
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    /// Render search statistics and current status
    fn render_search_status(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let status_text = if app.ui.input.is_empty() {
            "Ready to search • Type pattern and press Enter".to_string()
        } else if let Some(ref raw_results) = app.ui.raw_search_results {
            // Count files by looking for lines without colons (file headings in --heading mode)
            // and lines with line numbers (actual matches)
            let file_count = raw_results
                .lines
                .iter()
                .filter(|line| {
                    let clean_line =
                        crate::tasks::search_task::RawSearchResult::strip_ansi_codes(line);
                    !clean_line.trim().is_empty()
                        && !clean_line.contains(':')
                        && !clean_line.starts_with("--")
                })
                .count();
            let match_count = raw_results
                .lines
                .iter()
                .filter(|line| {
                    let clean_line =
                        crate::tasks::search_task::RawSearchResult::strip_ansi_codes(line);
                    let parts: Vec<&str> = clean_line.splitn(2, ':').collect();
                    parts.len() == 2 && parts[0].trim().parse::<u32>().is_ok()
                })
                .count();
            format!(
                "Found {} matches in {} files for '{}'",
                match_count, file_count, app.ui.input
            )
        } else if !app.ui.search_results.is_empty() {
            format!(
                "Found {} files matching '{}'",
                app.ui.search_results.len(),
                app.ui.input
            )
        } else if app
            .tasks
            .values()
            .any(|task| task.description.contains("Content search") && !task.is_completed)
        {
            format!("Searching for '{}'...", app.ui.input)
        } else if app.ui.last_query.is_some() {
            format!("No matches found for '{}'", app.ui.input)
        } else {
            format!("Press Enter to search for '{}'", app.ui.input)
        };

        let status_paragraph = Paragraph::new(status_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(status_paragraph, area);
    }

    /// Render enhanced help text with context
    fn render_help_text(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let help_text = if app.ui.input.is_empty() {
            "Type search pattern • Enter to search • Esc to close • Use regex for advanced patterns"
        } else if app.ui.raw_search_results.is_some() || !app.ui.search_results.is_empty() {
            "↑↓ Navigate • Enter to open file • Shift+Enter to jump to match • Esc to close"
        } else {
            "Enter to search • Esc to close • Use quotes for exact phrases • Case sensitive by default"
        };

        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(help_paragraph, area);
    }

    fn render_content_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        // Check if there's an active search task
        let is_searching = app
            .tasks
            .values()
            .any(|task| task.description.contains("Content search") && !task.is_completed);

        // Show enhanced loading state if searching
        if is_searching {
            Self::render_loading_state(frame, app, area);
            return;
        }

        // Check if we have raw search results first (preferred)
        if let Some(ref raw_results) = app.ui.raw_search_results
            && !raw_results.lines.is_empty()
        {
            Self::render_raw_results(frame, app, area);
            return;
        }

        // Check if we have rich search results, then fallback to simple results
        if !app.ui.rich_search_results.is_empty() {
            Self::render_rich_results(frame, app, area);
            return;
        }

        // Check if we have simple search results
        if app.ui.search_results.is_empty() {
            Self::render_empty_state(frame, app, area);
            return;
        }

        // Create list items from search results
        let list_items: Vec<ListItem> = app
            .ui
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
                    .map_or_else(|| ".".to_string(), |p| p.to_string_lossy().to_string());

                let display_text = format!("{} {} ({})", icons::FILE_ICON, file_name, dir_path);

                ListItem::new(display_text).style(Style::default().fg(theme::FOREGROUND))
            })
            .collect();

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} Files Found ", app.ui.search_results.len()))
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
            .ui
            .rich_search_results
            .iter()
            .map(|line| ListItem::new(line.as_str()).style(Style::default().fg(theme::FOREGROUND)))
            .collect();

        let title = format!(" {} Rich Results ", app.ui.rich_search_results.len());

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
        if let Some(ref raw_results) = app.ui.raw_search_results {
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

    /// Render enhanced loading state with spinner and progress
    fn render_loading_state(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner_frame =
            (Instant::now().elapsed().as_millis() / 80) % spinner_chars.len() as u128;
        let spinner = spinner_chars[spinner_frame as usize];

        let loading_lines = vec![
            Line::from(vec![
                Span::styled(format!("{spinner} "), Style::default().fg(theme::YELLOW)),
                Span::styled(
                    format!("Searching for '{}'", app.ui.input),
                    Style::default().fg(theme::FOREGROUND),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Scanning files with ripgrep...",
                Style::default().fg(theme::COMMENT),
            )),
        ];

        let loading = Paragraph::new(loading_lines)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Searching ")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::YELLOW))
                    .style(Style::default().bg(theme::BACKGROUND)),
            );

        frame.render_widget(loading, area);
    }

    /// Render enhanced empty state with helpful tips
    fn render_empty_state(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let (title, message_lines, border_color) = if app.ui.input.is_empty() {
            (
                " Ready to Search ",
                vec![
                    Line::from("Enter a search pattern to find content in files"),
                    Line::from(""),
                    Line::from("Tips:"),
                    Line::from("• Use regex patterns for advanced matching"),
                    Line::from("• Quote exact phrases: \"hello world\""),
                    Line::from("• Search is case-sensitive by default"),
                    Line::from("• Results show file path and line context"),
                ],
                theme::CYAN,
            )
        } else if app.ui.last_query.is_some() {
            (
                " No Results Found ",
                vec![
                    Line::from(format!("No matches found for '{}'", app.ui.input)),
                    Line::from(""),
                    Line::from("Try:"),
                    Line::from("• Different search terms"),
                    Line::from("• Simpler patterns"),
                    Line::from("• Check spelling and case"),
                ],
                theme::RED,
            )
        } else {
            (
                " Press Enter to Search ",
                vec![
                    Line::from(format!("Ready to search for '{}'", app.ui.input)),
                    Line::from(""),
                    Line::from("Press Enter to start searching"),
                ],
                theme::GREEN,
            )
        };

        let empty_state = Paragraph::new(message_lines)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(theme::BACKGROUND)),
            );

        frame.render_widget(empty_state, area);
    }
}
