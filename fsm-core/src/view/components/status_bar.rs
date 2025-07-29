//! src/view/components/status_bar.rs
//! ============================================================
//! Lock-free status bar that pulls just a few scalar values from
//! the coordinator on demand (two quick Mutex guards).  All other
//! data is supplied by the `UiSnapshot` captured by the renderer.

use ratatui::{prelude::*, widgets::Paragraph};
use tracing::{debug, instrument, trace};

use crate::{
    controller::state_coordinator::StateCoordinator, model::ui_state::UIMode,
    view::snapshots::UiSnapshot, view::theme,
};

/// One-line status bar shown at the bottom of the screen.
pub struct OptimizedStatusBar;

impl OptimizedStatusBar {
    pub fn new() -> Self {
        debug!("Creating new OptimizedStatusBar");
        Self
    }

    /// Render the bar.
    ///
    /// * `ui`    – immutable snapshot of UIState
    /// * `coord` – coordinator for small pieces of global state
    /// * `rect`  – bar rectangle (height == 1 in the renderer)
    #[instrument(level = "trace", skip_all, fields(mode = ?ui.mode))]
    pub fn render_with_metrics(
        &self,
        frame: &mut Frame<'_>,
        ui: &UiSnapshot,
        coord: &StateCoordinator,
        rect: Rect,
    ) {
        trace!("Rendering status bar");
        // -----------------------------------------------------
        // 1) Gather a few live figures (short lock scope)
        // -----------------------------------------------------
        let (cwd, marked) = {
            let fs = coord.fs_state();
            let pane = fs.active_pane();
            let cwd = pane.cwd.clone();
            let marked_count = pane.marked_entries.len();
            trace!("Status bar data: cwd={:?}, marked={}", cwd, marked_count);
            (cwd, marked_count)
        };

        let task_count = {
            let app = coord.app_state();
            let count = app.tasks.len();
            trace!("Active tasks: {}", count);
            count
        };

        // -----------------------------------------------------
        // 2) Compose left / right strings
        // -----------------------------------------------------
        let mode_str = match ui.overlay {
            _ if ui.clipboard_active => "Clipboard",
            _ if ui.overlay.is_search_mode() => "Search",
            _ => match ui.search_mode {
                _ if ui.search_mode != crate::model::fs_state::SearchMode::None => "Search",
                _ => match &ui.prompt_type {
                    Some(_) => "Command",
                    None => match ui.mode {
                        UIMode::Browse => "Browse",
                        UIMode::Search => "Search",
                        UIMode::Command => "Command",
                        UIMode::Visual => "Visual",
                    },
                },
            },
        };

        // truncate path if the bar is narrow ------------------
        let mut path = cwd.to_string_lossy().to_string();
        let max_path = rect.width.saturating_sub(40) as usize; // leave space
        if path.len() > max_path {
            if let Some(tail) = path.get(path.len() - max_path + 3..) {
                path = format!("…{tail}");
            }
        }

        let left = format!("{mode_str} | {path} | Marked: {marked}");
        let right = format!("Tasks: {task_count}");

        // -----------------------------------------------------
        // 3) Split area 50 / 50 and paint
        // -----------------------------------------------------
        let [l, r] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(rect);

        Paragraph::new(left)
            .style(Style::default().fg(theme::FOREGROUND).bg(theme::BACKGROUND))
            .alignment(Alignment::Left)
            .render(l, frame.buffer_mut());

        Paragraph::new(right)
            .style(Style::default().fg(theme::FOREGROUND).bg(theme::BACKGROUND))
            .alignment(Alignment::Right)
            .render(r, frame.buffer_mut());
    }
}

impl Default for OptimizedStatusBar {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Helper: quick predicate on UIOverlay so we don't match manually
// ------------------------------------------------------------------
trait OverlayExt {
    fn is_search_mode(&self) -> bool;
}

impl OverlayExt for crate::model::ui_state::UIOverlay {
    fn is_search_mode(&self) -> bool {
        matches!(
            self,
            crate::model::ui_state::UIOverlay::Search
                | crate::model::ui_state::UIOverlay::FileNameSearch
                | crate::model::ui_state::UIOverlay::ContentSearch
        )
    }
}
