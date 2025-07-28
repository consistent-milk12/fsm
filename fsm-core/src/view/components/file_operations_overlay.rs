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

/// Single-gauge file-operation overlay
pub struct OptimizedFileOperationsOverlay;

impl OptimizedFileOperationsOverlay {
    /// ctor
    pub fn new() -> Self {
        Self
    }

    /// Draws a single progress bar summarising *all* operations.
    ///
    /// * `avg`   – average progress in the 0.0‒1.0 range  
    /// * `count` – number of active operations
    pub fn render_summary(&self, frame: &mut Frame<'_>, avg: f32, count: usize, rect: Rect) {
        if count == 0 {
            return;
        }

        // percentage with two decimals => u16 [0‒100]
        let pct = (avg.clamp(0.0, 1.0) * 100.0) as u16;

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
    }
}

impl Default for OptimizedFileOperationsOverlay {
    fn default() -> Self {
        Self::new()
    }
}
