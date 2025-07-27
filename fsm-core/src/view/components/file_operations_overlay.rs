//! src/view/components/file_operations_overlay.rs
use crate::model::ui_state::FileOperationProgress;
use compact_str::CompactString;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge},
};
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::Ordering};

pub struct OptimizedFileOperationsOverlay;

impl OptimizedFileOperationsOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_operations(
        &self,
        frame: &mut Frame<'_>,
        operations: &HashMap<CompactString, Arc<FileOperationProgress>>,
        area: Rect,
    ) {
        if operations.is_empty() {
            return;
        }

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                operations
                    .iter()
                    .map(|_| Constraint::Length(3))
                    .collect::<Vec<_>>(),
            )
            .split(area);

        for (i, (_, progress)) in operations.iter().enumerate() {
            if let Some(&op_area) = layout.get(i) {
                self.render_single_operation(frame, op_area, progress);
            }
        }
    }

    fn render_single_operation(&self, f: &mut Frame, area: Rect, progress: &FileOperationProgress) {
        let percentage = (progress.progress_ratio() * 100.0) as u16;
        let color = match progress.operation_type.as_str() {
            "Copy" => Color::Blue,
            "Move" => Color::Yellow,
            "Delete" => Color::Red,
            _ => Color::Cyan,
        };

        let label = format!(
            "{} of {} files",
            progress.files_completed.load(Ordering::Relaxed),
            progress.total_files.load(Ordering::Relaxed)
        );

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(progress.operation_type.as_str()),
            )
            .gauge_style(Style::default().fg(color))
            .percent(percentage)
            .label(label);

        f.render_widget(gauge, area);
    }
}

impl Default for OptimizedFileOperationsOverlay {
    fn default() -> Self {
        Self::new()
    }
}
