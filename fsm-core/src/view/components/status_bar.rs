//! src/view/components/status_bar.rs
use crate::{
    controller::state_coordinator::StateCoordinator,
    model::ui_state::{UIMode, UIState},
    view::theme,
};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Widget},
};

pub struct OptimizedStatusBar;

impl OptimizedStatusBar {
    pub fn new() -> Self {
        Self
    }

    pub fn render_with_metrics(
        &self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        state_coordinator: &StateCoordinator,
        area: Rect,
    ) {
        let perf_snapshot = state_coordinator.get_performance_stats();

        let mode_str = match ui_state.mode {
            UIMode::Browse => "Browse",
            UIMode::Visual => "Visual",
            UIMode::Search => "Search",
            UIMode::Prompt => "Prompt",
            UIMode::Command => "Command",
            UIMode::Scripting => "Scripting",
            UIMode::BatchOp => "BatchOp",
        };

        let current_path = state_coordinator.current_directory();
        let path_display = current_path.to_string_lossy();
        let left_text = format!("{} | {} | Marked: {}", mode_str, path_display, ui_state.marked_indices.len());

        let right_text = format!(
            "Tasks: {} | Cache: {:.0}% | Resp: {:.1}μs",
            perf_snapshot.active_tasks,
            perf_snapshot.cache_hit_ratio * 100.0,
            perf_snapshot.avg_response_time_us
        );

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
