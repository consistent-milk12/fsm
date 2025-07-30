//! src/view/components/error_overlay.rs
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use tracing::{debug, info, instrument};

pub struct ErrorOverlay {
    message: String,
}

impl ErrorOverlay {
    pub fn new(message: String) -> Self {
        debug!(
            target: "fsm_core::view::components::error_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "ErrorOverlay",
            message = "Creating new ErrorOverlay component"
        );
        Self { message }
    }

    #[instrument(
        level = "info",
        skip_all,
        fields(
            marker = "ERROR_RENDER",
            operation_type = "error_overlay_render",
            message_len = self.message.len(),
            area_width = area.width,
            area_height = area.height,
            message = "Error overlay render initiated"
        )
    )]
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let render_start = std::time::Instant::now();
        info!(
            target: "fsm_core::view::components::error_overlay",
            marker = "ERROR_RENDER",
            operation_type = "error_overlay_render",
            message_len = self.message.len(),
            area_width = area.width,
            area_height = area.height,
            message = "Error overlay render initiated"
        );

        let block = Block::default()
            .title(" Error ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = Paragraph::new(self.message.as_str())
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(Clear, area);
        frame.render_widget(text, area);

        let render_time_us = render_start.elapsed().as_micros();
        info!(
            target: "fsm_core::view::components::error_overlay",
            marker = "UI_RENDER_COMPLETE",
            operation_type = "error_overlay_render",
            render_time_us = render_time_us,
            message_len = self.message.len(),
            area_width = area.width,
            area_height = area.height,
            message = "Error overlay render completed"
        );
    }
}
