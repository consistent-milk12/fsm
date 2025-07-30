//! src/view/components/file_operations_overlay.rs
//! ============================================================
//! Very small overlay that shows **one** stacked progress bar
//! with the *average* completion of all active operations plus
//! the number of parallel jobs.  The heavy per-file bookkeeping
//! now lives in `FSState`, so the renderer only calls
//! `render_summary(..)` when `op_count > 0`.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge},
};
use tracing::{debug, info, instrument, trace, warn};

/// Single-gauge file-operation overlay
pub struct OptimizedFileOperationsOverlay;

impl OptimizedFileOperationsOverlay {
    /// ctor
    pub fn new() -> Self {
        debug!(
            target: "fsm_core::view::components::file_operations_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "OptimizedFileOperationsOverlay",
            message = "Creating new OptimizedFileOperationsOverlay component"
        );
        Self
    }

    /// Draws a single progress bar summarising *all* operations.
    ///
    /// * `avg`   – average progress in the 0.0‒1.0 range  
    /// * `count` – number of active operations
    #[instrument(
        level = "trace",
        skip_all,
        fields(
            marker = "FILE_OPERATION_OVERLAY_RENDER",
            operation_type = "file_operation_overlay_render",
            avg_progress = avg,
            active_operations_count = count,
            area_width = rect.width,
            area_height = rect.height,
            message = "File operations overlay render initiated"
        )
    )]
    pub fn render_summary(&self, frame: &mut Frame<'_>, avg: f32, count: usize, rect: Rect) {
        let render_start = std::time::Instant::now();
        info!(
            target: "fsm_core::view::components::file_operations_overlay",
            marker = "FILE_OPERATION_OVERLAY_RENDER",
            operation_type = "file_operation_overlay_render",
            avg_progress = avg,
            active_operations_count = count,
            area_width = rect.width,
            area_height = rect.height,
            message = "File operations overlay render initiated"
        );

        if count == 0 {
            trace!(
                target: "fsm_core::view::components::file_operations_overlay",
                marker = "FILE_OPERATION_OVERLAY_SKIP",
                message = "Skipping file operations overlay render as no active operations"
            );
            return;
        }

        // percentage with two decimals => u16 [0‒100]
        let pct = (avg.clamp(0.0, 1.0) * 100.0) as u16;

        debug!(
            target: "fsm_core::view::components::file_operations_overlay",
            percentage = pct,
            "Calculated percentage for gauge"
        );

        // colourful gauge --------------------------------------------------
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {count} active operation(s) ")),
            )
            .gauge_style(
                Style::default()
                    .fg(Color::Rgb(80, 180, 255))
                    .bg(Color::Rgb(25, 28, 40)),
            )
            .percent(pct)
            .label(format!("{pct}%"));

        frame.render_widget(gauge, rect);

        let render_time_us = render_start.elapsed().as_micros();
        info!(
            target: "fsm_core::view::components::file_operations_overlay",
            marker = "UI_RENDER_COMPLETE",
            operation_type = "file_operation_overlay_render",
            render_time_us = render_time_us,
            avg_progress = avg,
            active_operations_count = count,
            area_width = rect.width,
            area_height = rect.height,
            message = "File operations overlay render completed"
        );

        if render_time_us > 3000 {
            warn!(
                target: "fsm_core::view::components::file_operations_overlay",
                marker = "UI_RENDER_SLOW",
                render_time_us = render_time_us,
                area_size = format!("{}x{}", rect.width, rect.height),
                message = "Slow file operations overlay render detected"
            );
        }
    }
}

impl Default for OptimizedFileOperationsOverlay {
    fn default() -> Self {
        Self::new()
    }
}
