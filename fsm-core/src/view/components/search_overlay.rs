//! src/view/components/search_overlay.rs
use crate::model::ui_state::{UIOverlay, UIState};
use crate::view::theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct OptimizedSearchOverlay {
    overlay_type: UIOverlay,
}

impl OptimizedSearchOverlay {
    pub fn new(overlay_type: UIOverlay) -> Self {
        Self { overlay_type }
    }

    pub fn render_with_input(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        let title = match self.overlay_type {
            UIOverlay::Search => " Search ",
            UIOverlay::FileNameSearch => " File Name Search ",
            UIOverlay::ContentSearch => " Content Search ",
            _ => " Search ",
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(ui_state.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(Clear, area);
        frame.render_widget(input_paragraph, area);

        frame.set_cursor(area.x + ui_state.input.len() as u16 + 1, area.y + 1);
    }
}
