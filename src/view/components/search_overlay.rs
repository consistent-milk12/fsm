//! src/view/components/search_overlay.rs
//! ============================================================================
//! # SearchOverlay: Modal Full-Text Search/Input (ratatui v0.24+)
//!
//! - Displays a search input field and optional live match count
//! - Integrates with UIState's input buffer and last_query
//! - Themed, keyboard-centric, and visually distinct

use crate::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct SearchOverlay;

impl SearchOverlay {
    /// Render the search overlay as a modal, centered on the screen.
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let input: &String = &app.ui.input;
        let last_query: &str = app.ui.last_query.as_deref().unwrap_or("");
        // Optionally, display a count of live matches if your app supports it
        // (replace `app.ui.search_results_count` with your actual state field)
        let match_count: Option<usize> = None; // Option<usize>

        let mut lines = vec![Line::from(vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(input),
        ])];

        if !last_query.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Last: ", Style::default().fg(Color::Gray)),
                Span::raw(last_query),
            ]));
        }

        if let Some(count) = match_count {
            lines.push(Line::from(vec![
                Span::styled("Matches: ", Style::default().fg(Color::Cyan)),
                Span::raw(count.to_string()),
            ]));
        }

        // Centered overlay (e.g., 50% width, 18% height for a small search modal)
        let overlay_area = Self::centered_rect(50, 18, area);
        frame.render_widget(Clear, overlay_area);

        let para = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title("Find in files")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(para, overlay_area);
    }

    /// Centers a rectangle of the given percent width/height inside area.
    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vertical[1]);

        horizontal[1]
    }
}
