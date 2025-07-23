//! src/view/components/search_overlay.rs
//! ============================================================================
//! # SearchOverlay: Modal Full-Text Search/Input (ratatui v0.24+)
//!
//! - Displays a search input field and optional live match count
//! - Integrates with UIState's input buffer and last_query
//! - Themed, keyboard-centric, and visually distinct

use crate::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct SearchOverlay;

impl SearchOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let input = &app.ui.input;
        let last_query = app.ui.last_query.as_deref().unwrap_or("");
        let match_count: Option<usize> = None; // Placeholder

        let mut lines = vec![Line::from(vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(input),
        ])];

        if !last_query.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Last: ", Style::default().fg(theme::COMMENT)),
                Span::raw(last_query),
            ]));
        }

        if let Some(count) = match_count {
            lines.push(Line::from(vec![
                Span::styled("Matches: ", Style::default().fg(theme::CYAN)),
                Span::raw(count.to_string()),
            ]));
        }

        let overlay_area = Self::centered_rect(50, 18, area);
        frame.render_widget(Clear, overlay_area);

        let para = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title("Find in files")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(para, overlay_area);
    }

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
