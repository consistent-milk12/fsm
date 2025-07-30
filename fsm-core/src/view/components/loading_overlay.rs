//! src/view/components/loading_overlay.rs
//! ============================================================
//! Lightweight, lock-free loading overlay.  It takes a snapshot
//! of the atomic counters in `LoadingState` and never touches
//! shared state again while painting.

use crate::model::ui_state::LoadingState;
use crate::view::theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};
use tracing::{debug, info, instrument, trace, warn};

/// Simple progress overlay
pub struct OptimizedLoadingOverlay;

impl OptimizedLoadingOverlay {
    /// ctor
    pub fn new() -> Self {
        debug!(
            target: "fsm_core::view::components::loading_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "OptimizedLoadingOverlay",
            message = "Creating new OptimizedLoadingOverlay component"
        );
        Self
    }

    /// Draw a determinate / indeterminate loading box.
    ///
    /// * `loading` – immutable `LoadingState` snapshot from the
    /// * `loading` – immutable `LoadingState` snapshot from the
    ///   renderer (no atomics inside).  
    /// * `rect`    – screen rectangle where the overlay is drawn.
    #[instrument(
        level = "trace",
        skip_all,
        fields(
            marker = "UI_LOADING_OVERLAY_RENDERED",
            operation_type = "loading_overlay_render",
            progress = loading.progress(),
            determinate = loading.progress() > 0.0,
            elapsed_ms = loading.start_time.elapsed().as_millis(),
            area_width = rect.width,
            area_height = rect.height,
            message = "Loading overlay render initiated"
        )
    )]
    pub fn render_progress(&self, frame: &mut Frame<'_>, loading: &LoadingState, rect: Rect) {
        let render_start = std::time::Instant::now();
        info!(
            target: "fsm_core::view::components::loading_overlay",
            marker = "UI_LOADING_OVERLAY_RENDERED",
            operation_type = "loading_overlay_render",
            progress = loading.progress(),
            determinate = loading.progress() > 0.0,
            elapsed_ms = loading.start_time.elapsed().as_millis(),
            area_width = rect.width,
            area_height = rect.height,
            message = "Loading overlay render initiated"
        );

        // -----------------------------------------------------
        // Derive percentage from the compact fixed-point field
        // -----------------------------------------------------
        let pct = loading.progress(); // 0.00 - 100.00
        let determinate = pct > 0.0;

        // elapsed time for info line
        let elapsed = loading.start_time.elapsed();

        trace!(
            target: "fsm_core::view::components::loading_overlay",
            progress = pct,
            determinate = determinate,
            elapsed_ms = elapsed.as_millis(),
            area_width = rect.width,
            area_height = rect.height,
            "Starting loading overlay render"
        );

        // -----------------------------------------------------
        // Clear background & chrome
        // -----------------------------------------------------
        frame.render_widget(Clear, rect);

        let chrome = Block::default()
            .title(" Loading ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND));

        // -----------------------------------------------------
        // Determinate gauge  (progress > 0%)
        // -----------------------------------------------------
        if determinate {
            debug!(
                target: "fsm_core::view::components::loading_overlay",
                progress = pct,
                message = %loading.message,
                elapsed_ms = elapsed.as_millis(),
                "Rendering determinate loading gauge"
            );

            let gauge = Gauge::default()
                .block(chrome)
                .gauge_style(Style::default().fg(theme::PINK).bg(theme::CURRENT_LINE))
                .percent(pct as u16)
                .label(format!("{pct:.1}%"));

            frame.render_widget(gauge, rect);

            // message + elapsed under the gauge --------------
            let inner = rect.inner(Margin {
                vertical: 2,
                horizontal: 2,
            });
            let lines = vec![
                Line::from(Span::styled(
                    format!("{} • {pct:.1}% complete", loading.message),
                    Style::default()
                        .fg(theme::YELLOW)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("Elapsed: {elapsed:.0?}")),
            ];

            frame.render_widget(
                Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
                inner,
            );
        }
        // -----------------------------------------------------
        // Indeterminate (0 %) – just text & elapsed timer
        // -----------------------------------------------------
        else {
            debug!(
                target: "fsm_core::view::components::loading_overlay",
                message = %loading.message,
                elapsed_ms = elapsed.as_millis(),
                "Rendering indeterminate loading display"
            );

            let text = Text::from(vec![
                Line::from(Span::styled(
                    &*loading.message,
                    Style::default()
                        .fg(theme::YELLOW)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("Elapsed: {elapsed:.0?}")),
            ]);

            frame.render_widget(
                Paragraph::new(text)
                    .block(chrome)
                    .alignment(Alignment::Center),
                rect,
            );
        }

        let render_time_us = render_start.elapsed().as_micros();
        trace!(
            target: "fsm_core::view::components::loading_overlay",
            render_time_us = render_time_us,
            progress = pct,
            determinate = determinate,
            message_len = loading.message.len(),
            elapsed_ms = elapsed.as_millis(),
            "Loading overlay render completed"
        );

        if render_time_us > 5000 {
            warn!(
                target: "fsm_core::view::components::loading_overlay",
                marker = "UI_RENDER_SLOW",
                render_time_us = render_time_us,
                progress = pct,
                area_size = format!("{}x{}", rect.width, rect.height),
                "Slow loading overlay render detected"
            );
        }
    }
}

impl Default for OptimizedLoadingOverlay {
    fn default() -> Self {
        Self::new()
    }
}
