//! src/view/components/status_bar.rs
//! ============================================================================
//! # StatusBar: Persistent Status/Info/Error Display (ratatui v0.24+)
//!
//! - Renders last error, status message, or hint at bottom of UI
//! - Uses AppState's `last_error` and `last_status` fields
//! - Themed, immediate-mode, power-user friendly

//! src/view/components/status_bar.rs
//! ============================================================================
//! # StatusBar: Persistent Status/Info/Error Display (ratatui v0.24+)
//!
//! - Renders last error, status message, or hint at bottom of UI
//! - Uses AppState's `last_error` and `last_status` fields
//! - Themed, immediate-mode, power-user friendly

use crate::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use heapless::String;
use core::fmt::Write;

pub struct StatusBar;

impl StatusBar {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let status_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::COMMENT));
        frame.render_widget(status_block, area);

        let mut msg_buffer: String<256> = String::new();
        let style = if let Some(ref err) = app.last_error {
            write!(&mut msg_buffer, "🔥 Error: {}", err).unwrap_or_default();
            Style::default().fg(theme::RED).bold()
        } else if let Some(ref status) = app.ui.last_status {
            write!(&mut msg_buffer, "{}", status).unwrap_or_default();
            Style::default().fg(theme::GREEN)
        } else {
            write!(&mut msg_buffer, "Ready").unwrap_or_default();
            Style::default().fg(theme::COMMENT)
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .margin(0)
            .split(area);

        let left_para = Paragraph::new(Line::from(Span::styled(msg_buffer.as_str(), style)))
            .alignment(Alignment::Left);

        let mut right_spans = vec![];

        // Clipboard stats
        let clipboard_stats = app.ui.clipboard.stats();
        if clipboard_stats.total_items > 0 {
            let mut cb_buffer: String<32> = String::new();
            write!(&mut cb_buffer, "CB:{} ", clipboard_stats.total_items).unwrap_or_default();
            right_spans.push(Span::styled(
                cb_buffer.to_string(),
                Style::default().fg(theme::CYAN),
            ));
        }

        // Performance metrics
        if let Some(key_processor) = &app.key_processor {
            let hit_rate = key_processor.stats.cache_hit_rate();
            let mut hit_buffer: String<32> = String::new();
            write!(&mut hit_buffer, "Hit:{:.1}% ", hit_rate * 100.0).unwrap_or_default();
            right_spans.push(Span::styled(
                hit_buffer.to_string(),
                Style::default().fg(theme::YELLOW),
            ));
        }

        let mut items_buffer: String<32> = String::new();
        write!(&mut items_buffer, "{} items ", app.fs.active_pane().entries.len()).unwrap_or_default();
        right_spans.push(Span::styled(
            items_buffer.to_string(),
            Style::default().fg(theme::PURPLE),
        ));

        let right_para = Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right);

        frame.render_widget(left_para, chunks[0]);
        frame.render_widget(right_para, chunks[1]);
    }
}
