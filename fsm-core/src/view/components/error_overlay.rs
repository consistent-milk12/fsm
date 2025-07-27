//! src/view/components/error_overlay.rs
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct ErrorOverlay {
    message: String,
}

impl ErrorOverlay {
    pub fn new(message: String) -> Self {
        Self { message }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title(" Error ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = Paragraph::new(self.message.as_str())
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(Clear, area);
        frame.render_widget(text, area);
    }
}
