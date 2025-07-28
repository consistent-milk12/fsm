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

/// Simple progress overlay
pub struct OptimizedLoadingOverlay;

impl OptimizedLoadingOverlay {
    /// ctor
    pub fn new() -> Self {
        Self
    }

    /// Draw a determinate / indeterminate loading box.
    ///
    /// * `loading` – immutable `LoadingState` snapshot from the
    /// * `loading` – immutable `LoadingState` snapshot from the
    ///   renderer (no atomics inside).  
    /// * `rect`    – screen rectangle where the overlay is drawn.
    pub fn render_progress(&self, frame: &mut Frame<'_>, loading: &LoadingState, rect: Rect) {
        // -----------------------------------------------------
        // Derive percentage from the compact fixed-point field
        // -----------------------------------------------------
        let pct = loading.progress(); // 0.00 - 100.00
        let determinate = pct > 0.0;

        // elapsed time for info line
        let elapsed = loading.start_time.elapsed();

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
    }
}

impl Default for OptimizedLoadingOverlay {
    fn default() -> Self {
        Self::new()
    }
}
