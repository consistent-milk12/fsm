//! src/view/components/notification_overlay.rs
use crate::model::ui_state::{Notification, NotificationLevel};
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct OptimizedNotificationOverlay;

impl OptimizedNotificationOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_notification(
        &self,
        frame: &mut Frame<'_>,
        notification: &Notification,
        area: Rect,
    ) {
        frame.render_widget(Clear, area);

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

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .split(inner_area);

        let message = Paragraph::new(notification.message.as_str())
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left);
        frame.render_widget(message, layout[0]);

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

impl Default for OptimizedNotificationOverlay {
    fn default() -> Self {
        Self::new()
    }
}
