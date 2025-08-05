//! ``src/view/components/status_bar.rs``
//!
//!  # `StatusBar`: Persistent Status/Info/Error Display (ratatui v0.24+)
//!
//! - Renders last error, status message, or hint at bottom of UI
//! - Uses `AppState`'s `last_error` and `last_status` fields
//! - Themed, immediate-mode, power-user friendly

use std::rc::Rc;

use crate::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub struct StatusBar;

impl StatusBar {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let status_block: Block<'_> = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::COMMENT));
        frame.render_widget(status_block, area);

        let (msg, style) = app
            .last_error
            .as_ref()
            .map_or_else(
                || 
                app
                    .ui
                    .last_status
                    .as_ref()
                    .map_or_else(
                        || 
                        ("Ready".to_string(), Style::default().fg(theme::COMMENT)),
                            |status: &String| -> (String, Style) 
                            {(status.clone(), Style::default().fg(theme::GREEN))}), 
                            |err: &String| -> (String, Style) 
                            {(    
                                format!("🔥 Error: {err}"),
                                Style::default().fg(theme::RED).bold(),
                            )}
            );

        let chunks: Rc<[Rect]> = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .margin(0)
            .split(area);

        let left_para: Paragraph<'_> = Paragraph::new(
            Line::from(
                    Span::styled(
                        format!(" {msg} "), style
                    )
                )
            )
            .alignment(Alignment::Left);

        let right_text: String = format!("{} items ", app.fs.active_pane().entries.len());
        let right_para: Paragraph<'_> = Paragraph::new(Line::from(Span::styled(
            right_text,
            Style::default().fg(theme::PURPLE),
        )))
        .alignment(Alignment::Right);

        frame.render_widget(left_para, chunks[0]);
        frame.render_widget(right_para, chunks[1]);
    }
}
