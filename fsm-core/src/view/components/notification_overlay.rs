//! src/view/components/notification_overlay.rs
//! ============================================================================
//! # Notification Overlay: User-Friendly Error and Info Messages
//!
//! Displays dismissible notifications with different severity levels

use crate::model::ui_state::NotificationLevel;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct NotificationOverlay;

impl NotificationOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &crate::AppState, area: Rect) {
        if let Some(notification) = &app.ui.notification {
            // Create a smaller overlay area in the top-right corner
            let notification_width = 60;
            let notification_height = 6;

            let x = area.width.saturating_sub(notification_width + 2);
            let y = 1;

            let notification_area = Rect {
                x,
                y,
                width: notification_width.min(area.width),
                height: notification_height.min(area.height),
            };

            // Clear the background
            frame.render_widget(Clear, notification_area);

            let (border_style, title, icon) = match notification.level {
                NotificationLevel::Info => (Style::default().fg(theme::CYAN), "Info", "ℹ"),
                NotificationLevel::Warning => (Style::default().fg(theme::YELLOW), "Warning", "⚠"),
                NotificationLevel::Error => (Style::default().fg(theme::RED), "Error", "✕"),
                NotificationLevel::Success => (Style::default().fg(theme::GREEN), "Success", "✓"),
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" {icon} {title} "))
                .title_style(border_style.bold())
                .border_style(border_style)
                .style(Style::default().bg(theme::BACKGROUND));

            let inner_area = block.inner(notification_area);
            frame.render_widget(block, notification_area);

            // Split inner area for message and dismiss instructions
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Fill(1),   // Message
                    Constraint::Length(1), // Dismiss instruction
                ])
                .split(inner_area);

            // Render message
            let message = Paragraph::new(notification.message.as_str())
                .style(Style::default().fg(theme::FOREGROUND))
                .wrap(Wrap { trim: true })
                .alignment(Alignment::Left);
            frame.render_widget(message, layout[0]);

            // Render dismiss instruction
            let dismiss_text = if notification.auto_dismiss_ms.is_some() {
                "Auto-dismissing... Press any key to dismiss"
            } else {
                "Press any key to dismiss"
            };

            let dismiss = Paragraph::new(dismiss_text)
                .style(
                    Style::default()
                        .fg(theme::COMMENT)
                        .add_modifier(Modifier::ITALIC),
                )
                .alignment(Alignment::Center);
            frame.render_widget(dismiss, layout[1]);
        }
    }
}
