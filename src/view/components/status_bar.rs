//! src/view/components/status_bar.rs
//! ============================================================================
//! # StatusBar: Persistent Status/Info/Error Display (ratatui v0.24+)
//!
//! - Renders last error, status message, or hint at bottom of UI
//! - Uses AppState's `last_error` and `last_status` fields
//! - Themed, immediate-mode, power-user friendly

use crate::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};

pub struct StatusBar;

impl StatusBar {
    /// Render the status bar at the given area.
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let (msg, style) = if let Some(ref err) = app.last_error {
            (
                format!("Error: {err}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else if let Some(ref status) = app.last_status {
            (status.clone(), Style::default().fg(Color::LightGreen))
        } else {
            ("Ready".to_string(), Style::default().fg(Color::Gray))
        };

        let text = Text::from(Line::from(Span::styled(msg, style)));

        let para = Paragraph::new(text)
            .block(Block::default().borders(Borders::NONE))
            .alignment(ratatui::layout::Alignment::Left);

        frame.render_widget(para, area);
    }
}
