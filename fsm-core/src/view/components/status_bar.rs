//! src/view/components/status_bar.rs - Updated status bar for unified StateCoordinator
//!
//! The status bar displays the current mode, path, number of marked items and
//! basic performance metrics.  In the new architecture the old
//! `StateCoordinator::get_performance_stats()` and `current_directory()` methods
//! have been removed.  This rewrite acquires the necessary information
//! directly from the `AppState` and `FSState` via the [`StateCoordinator`].
//! Performance statistics are derived from the handler registry and task list.

use crate::{
    controller::state_coordinator::StateCoordinator,
    model::ui_state::{UIMode, UIState},
    view::theme,
};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Widget},
};

/// Optimized status bar renderer
pub struct OptimizedStatusBar;

impl OptimizedStatusBar {
    pub fn new() -> Self {
        Self
    }

    /// Render the status bar with updated metrics.
    pub fn render_with_metrics(
        &self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        state_coordinator: &StateCoordinator,
        area: Rect,
    ) {
        // Determine mode string
        let mode_str = match ui_state.mode {
            UIMode::Browse => "Browse",
            UIMode::Visual => "Visual",
            UIMode::Search => "Search",
            UIMode::Prompt => "Prompt",
            UIMode::Command => "Command",
            UIMode::Scripting => "Scripting",
            UIMode::BatchOp => "BatchOp",
        };

        // Acquire current path from the active pane in FSState
        let current_path = {
            let fs_state = state_coordinator.fs_state();

            fs_state.active_pane().cwd.clone()
        };
        let path_display = current_path.to_string_lossy();

        let left_text = format!(
            "{} | {} | Marked: {}",
            mode_str,
            path_display,
            ui_state.marked_indices.len()
        );

        // Compute simple performance metrics
        let (active_tasks, handler_count, enabled_count, avg_time_us) = {
            // Count active tasks in AppState
            let app_state = state_coordinator.app_state();
            let active_tasks = app_state.tasks.len();
            drop(app_state);

            // Handler performance report
            let perf_report = state_coordinator.handler_performance_report();
            let handler_count = perf_report.len();
            let enabled_count = perf_report.iter().filter(|h| h.is_enabled).count();
            let avg_time_us = if handler_count > 0 {
                perf_report
                    .iter()
                    .map(|h| h.average_processing_time_ns as f64 / 1000.0)
                    .sum::<f64>()
                    / handler_count as f64
            } else {
                0.0
            };
            (active_tasks, handler_count, enabled_count, avg_time_us)
        };

        let right_text = format!(
            "Tasks: {active_tasks} | Handlers: {enabled_count}/{handler_count} | Avg: {avg_time_us:.1}μs"
        );

        // Split area into left and right halves
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        Paragraph::new(left_text)
            .style(Style::default().fg(theme::FOREGROUND).bg(theme::BACKGROUND))
            .alignment(Alignment::Left)
            .render(layout[0], frame.buffer_mut());

        Paragraph::new(right_text)
            .style(Style::default().fg(theme::FOREGROUND).bg(theme::BACKGROUND))
            .alignment(Alignment::Right)
            .render(layout[1], frame.buffer_mut());
    }
}

impl Default for OptimizedStatusBar {
    fn default() -> Self {
        Self::new()
    }
}
