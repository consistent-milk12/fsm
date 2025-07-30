//! src/view/components/notification_overlay.rs

use crate::model::ui_state::{Notification, NotificationLevel};
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::rc::Rc;
use std::time::Instant;
use tracing::{debug, info, trace, warn};

/// Component for drawing notification overlays with instrumentation.
pub struct OptimizedNotificationOverlay;

impl OptimizedNotificationOverlay {
    /// Create a new notification overlay component.
    ///  
    /// Logs initialization for TSV tracing.
    pub fn new() -> Self {
        // Emit component-init marker for UI component setup
        debug!(
            target = "fsm_core::view::components::notification_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "OptimizedNotificationOverlay",
            message = "Creating new OptimizedNotificationOverlay component"
        );
        Self
    }

    /// Render the notification overlay in the given frame and area.
    ///  
    /// Emits explicit markers for display, render timing, and dismissal.
    pub fn render_notification(
        &self,
        frame: &mut Frame<'_>,
        notification: &Notification,
        area: Rect,
    ) {
        // Log the moment the notification becomes visible
        info!(
            marker = "NOTIFICATION_DISPLAYED",
            operation_type = "notification_render",
            level = ?notification.level,
            message = %notification.message,
            auto_dismiss_ms = ?notification.auto_dismiss_ms,
            "Notification overlay displayed"
        );

        // Capture start time for render duration measurement
        let render_start: Instant = Instant::now();

        // Clear the widget area before drawing the overlay
        frame.render_widget(Clear, area);

        // Determine border style, title, and icon based on severity level
        let (border_style, title, icon) = match notification.level {
            NotificationLevel::Info => (Style::default().fg(theme::CYAN), "Info", "ℹ"),
            NotificationLevel::Warning => (Style::default().fg(theme::YELLOW), "Warning", "⚠"),
            NotificationLevel::Error => (Style::default().fg(theme::RED), "Error", "✕"),
            NotificationLevel::Success => (Style::default().fg(theme::GREEN), "Success", "✓"),
        };

        // Trace selected style details for diagnostics
        trace!(
            target = "fsm_core::view::components::notification_overlay",
            level = ?notification.level,
            title = title,
            icon = icon,
            "Selected notification style based on level"
        );

        // Create and render the styled block for the overlay
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {icon} {title} "))
            .title_style(border_style.bold())
            .border_style(border_style)
            .style(Style::default().bg(theme::BACKGROUND));
        frame.render_widget(&block, area);

        // Split the inner area for message and dismiss text
        let inner_area: Rect = block.inner(area);
        let layout: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .split(inner_area);

        // Log layout computation details for diagnostics
        debug!(
            target = "fsm_core::view::components::notification_overlay",
            message_area_width = layout[0].width,
            message_area_height = layout[0].height,
            dismiss_area_width = layout[1].width,
            dismiss_area_height = layout[1].height,
            "Layout areas calculated for notification content"
        );

        // Render the notification message text
        let message: Paragraph<'_> = Paragraph::new(notification.message.as_str())
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left);
        frame.render_widget(message, layout[0]);

        // Prepare the dismiss instructions text
        let dismiss_text: &'static str = if notification.auto_dismiss_ms.is_some() {
            "Auto-dismissing... Press any key to dismiss"
        } else {
            "Press any key to dismiss"
        };

        // Trace dismiss text generation details
        trace!(
            target = "fsm_core::view::components::notification_overlay",
            dismiss_text = dismiss_text,
            has_auto_dismiss = notification.auto_dismiss_ms.is_some(),
            "Generated dismiss text for notification"
        );

        // Render the dismiss instructions
        let dismiss: Paragraph<'_> = Paragraph::new(dismiss_text)
            .style(
                Style::default()
                    .fg(theme::COMMENT)
                    .add_modifier(Modifier::ITALIC),
            )
            .alignment(Alignment::Center);

        frame.render_widget(dismiss, layout[1]);

        // Compute total render duration in microseconds
        let render_time_us: u128 = render_start.elapsed().as_micros();

        // Emit render-complete marker with timing and metadata
        info!(
            target = "fsm_core::view::components::notification_overlay",
            marker = "UI_RENDER_COMPLETE",
            operation_type = "notification_render",
            render_time_us = render_time_us,
            level = ?notification.level,
            message_len = notification.message.len(),
            total_area = format!("{}x{}", area.width, area.height),
            "Notification overlay render completed"
        );

        // Warn if rendering is slower than threshold
        if render_time_us > 3000 {
            warn!(
                target = "fsm_core::view::components::notification_overlay",
                marker = "UI_RENDER_SLOW",
                render_time_us = render_time_us,
                level = ?notification.level,
                message_len = notification.message.len(),
                area_size = format!("{}x{}", area.width, area.height),
                "Slow notification overlay render detected"
            );
        }

        // Log level-specific debug markers for performance tracking
        match notification.level {
            NotificationLevel::Error => {
                debug!(
                    target = "fsm_core::view::components::notification_overlay",
                    marker = "NOTIFICATION_ERROR",
                    message = %notification.message,
                    render_time_us = render_time_us,
                    "Error notification displayed to user"
                );
            }

            NotificationLevel::Warning => {
                debug!(
                    target = "fsm_core::view::components::notification_overlay",
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
    /// Provide a default constructor for UI component registry.
    fn default() -> Self {
        Self::new()
    }
}
