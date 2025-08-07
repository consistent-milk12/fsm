//! ``src/view/components/prompt_bar.rs``
//! ============================================================================
//! # `PromptBar`: Modal User Input (Immediate-Mode, ratatui v0.24+)
//!
//! - Unified single-line input for search, commands, rename, etc.
//! - Uses `UIState`'s input buffer
//! - Always centered, visually distinct, themable

use crate::model::shared_state::SharedState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct PromptBar;

impl PromptBar {
    pub fn render(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let (prompt_text, prompt_label) = {
            let ui_guard = shared_state.lock_ui();
            let prompt_text = ui_guard.input.clone();
            let prompt_label = match ui_guard.overlay {
                crate::model::ui_state::UIOverlay::Prompt => "Command:",
                crate::model::ui_state::UIOverlay::Search => "Search:",
                _ => "Input:",
            };
            (prompt_text, prompt_label)
        };

        let overlay_area = Self::centered_rect(40, 12, area);
        frame.render_widget(Clear, overlay_area);

        let paragraph = Paragraph::new(Text::from(Span::raw(prompt_text)))
            .block(
                Block::default()
                    .title(prompt_label)
                    .title_alignment(Alignment::Left)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, overlay_area);
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
