//! src/view/components/notification_overlay.rs
use crate::model::ui_state::{Notification, NotificationLevel};
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tracing::{debug, info, instrument, trace, warn};

pub struct OptimizedNotificationOverlay;

impl OptimizedNotificationOverlay {
    pub fn new() -> Self {
        debug!(
            target: "fsm_core::view::components::notification_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "OptimizedNotificationOverlay",
            message = "Creating new OptimizedNotificationOverlay component"
        );
        Self
    }

    #[instrument(
        level = "info",
        skip_all,
        fields(
            marker = "NOTIFICATION_DISPLAYED",
            operation_type = "notification_render",
            level = ?notification.level,
            message_len = notification.message.len(),
            auto_dismiss_ms = ?notification.auto_dismiss_ms,
            area_width = area.width,
            area_height = area.height,
            message = "Notification overlay render initiated"
        )
    )]
    pub fn render_notification(
        &self,
        frame: &mut Frame<'_>,
        notification: &Notification,
        area: Rect,
    ) {
        let render_start = std::time::Instant::now();

        info!(
            target: "fsm_core::view::components::notification_overlay",
            level = ?notification.level,
            message_len = notification.message.len(),
            auto_dismiss_ms = ?notification.auto_dismiss_ms,
            area_width = area.width,
            area_height = area.height,
            "Rendering notification overlay"
        );

        frame.render_widget(Clear, area);

        let (border_style, title, icon) = match notification.level {
            NotificationLevel::Info => (Style::default().fg(theme::CYAN), "Info", "ℹ"),
            NotificationLevel::Warning => (Style::default().fg(theme::YELLOW), "Warning", "⚠"),
            NotificationLevel::Error => (Style::default().fg(theme::RED), "Error", "✕"),
            NotificationLevel::Success => (Style::default().fg(theme::GREEN), "Success", "✓"),
        };

        trace!(
            target: "fsm_core::view::components::notification_overlay",
            level = ?notification.level,
            title = title,
            icon = icon,
            "Selected notification style based on level"
        );

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

        debug!(
            target: "fsm_core::view::components::notification_overlay",
            message_area_width = layout[0].width,
            message_area_height = layout[0].height,
            dismiss_area_width = layout[1].width,
            dismiss_area_height = layout[1].height,
            "Layout areas calculated for notification content"
        );

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

        trace!(
            target: "fsm_core::view::components::notification_overlay",
            dismiss_text = dismiss_text,
            has_auto_dismiss = notification.auto_dismiss_ms.is_some(),
            "Generated dismiss text for notification"
        );

        let dismiss = Paragraph::new(dismiss_text)
            .style(
                Style::default()
                    .fg(theme::COMMENT)
                    .add_modifier(Modifier::ITALIC),
            )
            .alignment(Alignment::Center);
        frame.render_widget(dismiss, layout[1]);

        let render_time_us = render_start.elapsed().as_micros();
        info!(
            target: "fsm_core::view::components::notification_overlay",
            marker = "UI_RENDER_COMPLETE",
            operation_type = "notification_render",
            render_time_us = render_time_us,
            level = ?notification.level,
            message_len = notification.message.len(),
            total_area = format!("{}x{}", area.width, area.height),
            "Notification overlay render completed"
        );

        if render_time_us > 3000 {
            warn!(
                target: "fsm_core::view::components::notification_overlay",
                marker = "UI_RENDER_SLOW",
                render_time_us = render_time_us,
                level = ?notification.level,
                message_len = notification.message.len(),
                area_size = format!("{}x{}", area.width, area.height),
                "Slow notification overlay render detected"
            );
        }

        // performance characteristics tracking
        match notification.level {
            NotificationLevel::Error => {
                debug!(
                    target: "fsm_core::view::components::notification_overlay",
                    marker = "NOTIFICATION_ERROR",
                    message = %notification.message,
                    render_time_us = render_time_us,
                    "Error notification displayed to user"
                );
            }
            NotificationLevel::Warning => {
                debug!(
                    target: "fsm_core::view::components::notification_overlay",
                    marker = "NOTIFICATION_WARNING",
                    message = %notification.message,
                    render_time_us = render_time_us,
                    "Warning notification displayed to user"
                );
            }
            _ => {}
        }
    }
}

impl Default for OptimizedNotificationOverlay {
    fn default() -> Self {
        Self::new()
    }
}
