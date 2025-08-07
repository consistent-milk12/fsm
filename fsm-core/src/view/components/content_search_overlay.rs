//! ``src/view/components/content_search_overlay.rs``
//!
//! # `ContentSearchOverlay`: Production-quality content search with ripgrep
//!
//! Enhanced search interface with real-time feedback, syntax highlighting,
//! and comprehensive search result display with context and navigation.

use crate::view::theme;
use crate::{model::shared_state::SharedState, view::icons};
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
    pub fn render(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let _render_start = Instant::now();

        // Use larger overlay for production quality
        let overlay_area = Self::centered_rect(85, 80, area);
        frame.render_widget(Clear, overlay_area);

        // Enhanced layout with status bar
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input area
                Constraint::Length(3), // Status area
                Constraint::Length(2), // Help text
                Constraint::Fill(1),   // Results
            ])
            .split(overlay_area);

        // Render components with shared state
        Self::render_search_input(frame, shared_state, layout[0]);
        Self::render_search_status(frame, shared_state, layout[1]);
        Self::render_help_text(frame, shared_state, layout[2]);

        // Choose result renderer based on current data
        let ui_guard = shared_state.lock_ui();
        let has_raw_results = ui_guard.raw_search_results.is_some();
        let has_rich_results = !ui_guard.rich_search_results.is_empty();
        let has_simple_results = !ui_guard.search_results.is_empty();
        drop(ui_guard);

        if has_raw_results {
            Self::render_raw_results(frame, shared_state, layout[3]);
        } else if has_rich_results {
            Self::render_rich_results(frame, shared_state, layout[3]);
        } else if has_simple_results {
            Self::render_content_results(frame, shared_state, layout[3]);
        } else {
            let ui_guard = shared_state.lock_ui();
            if ui_guard.input.is_empty() {
                drop(ui_guard);
                Self::render_empty_state(frame, shared_state, layout[3]);
            } else {
                drop(ui_guard);
                Self::render_loading_state(frame, shared_state, layout[3]);
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    /// Render enhanced search input with visual feedback
    fn render_search_input(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        // Determine input state for visual feedback
        let (input, is_searching, has_raw_results, has_simple_results) = {
            let ui_guard = shared_state.lock_ui();
            (
                ui_guard.input.clone(),
                ui_guard
                    .loading
                    .as_ref()
                    .is_some_and(|loading| loading.message.contains("Searching")),
                ui_guard.raw_search_results.is_some(),
                !ui_guard.search_results.is_empty(),
            )
        };

        let (title, border_color) = if is_searching {
            (" 🔍 Searching... ", theme::YELLOW)
        } else if input.is_empty() {
            (" Content Search (ripgrep) ", theme::CYAN)
        } else if has_raw_results || has_simple_results {
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

        let input_paragraph = Paragraph::new(input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, area);

        // Enhanced cursor positioning with bounds checking
        let cursor_x = (area.x + input.len() as u16 + 1).min(area.x + area.width - 2);
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    /// Render search statistics and current status
    fn render_search_status(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();
        let status_text = if ui_guard.input.is_empty() {
            "Ready to search • Type pattern and press Enter".to_string()
        } else if let Some(ref raw_results) = ui_guard.raw_search_results {
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
                match_count, file_count, ui_guard.input
            )
        } else if !ui_guard.search_results.is_empty() {
            format!(
                "Found {} files matching '{}'",
                ui_guard.search_results.len(),
                ui_guard.input
            )
        } else if ui_guard
            .loading
            .as_ref()
            .is_some_and(|loading| loading.message.contains("Searching"))
        {
            format!("Searching for '{}'...", ui_guard.input)
        } else if ui_guard.last_query.is_some() {
            format!("No matches found for '{}'", ui_guard.input)
        } else {
            format!("Press Enter to search for '{}'", ui_guard.input)
        };

        let status_paragraph = Paragraph::new(status_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(status_paragraph, area);
    }

    /// Render enhanced help text with context
    fn render_help_text(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        let help_text = if ui_guard.input.is_empty() {
            "Type search pattern • Enter to search • Esc to close • Use regex for advanced patterns"
        } else if ui_guard.raw_search_results.is_some() || !ui_guard.search_results.is_empty() {
            "↑↓ Navigate • Enter to open file • Shift+Enter to jump to match • Esc to close"
        } else {
            "Enter to search • Esc to close • Use quotes for exact phrases • Case sensitive by default"
        };

        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(help_paragraph, area);
    }

    fn render_content_results(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        // Check if there's an active search task
        let is_searching = ui_guard
            .loading
            .as_ref()
            .map_or(false, |loading| loading.message.contains("Searching"));

        // Show enhanced loading state if searching
        if is_searching {
            Self::render_loading_state(frame, shared_state, area);
            return;
        }

        // Check if we have raw search results first (preferred)
        if let Some(ref raw_results) = ui_guard.raw_search_results
            && !raw_results.lines.is_empty()
        {
            Self::render_raw_results(frame, shared_state, area);
            return;
        }

        // Check if we have rich search results, then fallback to simple results
        if !ui_guard.rich_search_results.is_empty() {
            Self::render_rich_results(frame, shared_state, area);
            return;
        }

        // Check if we have simple search results
        if ui_guard.search_results.is_empty() {
            Self::render_empty_state(frame, shared_state, area);
            return;
        }

        // Create list items from search results
        let list_items: Vec<ListItem> = ui_guard
            .search_results
            .iter()
            .filter_map(|entry| shared_state.metadata.get_by_id(entry.id))
            .map(|obj_info| {
                let file_name = obj_info
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let dir_path = obj_info
                    .path
                    .parent()
                    .map_or_else(|| ".".to_string(), |p| p.to_string_lossy().to_string());

                let display_text = format!("{} {} ({})", icons::FILE_ICON, file_name, dir_path);

                ListItem::new(display_text).style(Style::default().fg(theme::FOREGROUND))
            })
            .collect();

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} Files Found ", ui_guard.search_results.len()))
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
        list_state.select(ui_guard.selected);

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn render_rich_results(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        // Simple display of rich search results (deprecated - using raw results now)
        let list_items: Vec<ListItem> = ui_guard
            .rich_search_results
            .iter()
            .map(|result| ListItem::new(result.as_str()))
            .collect();

        let title = format!(" {} Rich Results ", ui_guard.rich_search_results.len());
        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::GREEN))
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
        list_state.select(ui_guard.selected);

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn render_raw_results(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        if let Some(ref raw_results) = ui_guard.raw_search_results {
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
            list_state.select(ui_guard.selected);

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
    fn render_loading_state(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner_frame =
            (Instant::now().elapsed().as_millis() / 80) % spinner_chars.len() as u128;
        let spinner = spinner_chars[spinner_frame as usize];

        let loading_lines = vec![
            Line::from(vec![
                Span::styled(format!("{spinner} "), Style::default().fg(theme::YELLOW)),
                Span::styled(
                    format!("Searching for '{}'", ui_guard.input),
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
    fn render_empty_state(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let ui_guard = shared_state.lock_ui();

        let (title, message_lines, border_color) = if ui_guard.input.is_empty() {
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
        } else if ui_guard.last_query.is_some() {
            (
                " No Results Found ",
                vec![
                    Line::from(format!("No matches found for '{}'", ui_guard.input)),
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
                    Line::from(format!("Ready to search for '{}'", ui_guard.input)),
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
