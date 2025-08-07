//!
//! Filepath: `src/view/components/file_extension_overlay.rs`
//! Caller File: [src/view/ui.rs]
//!
use crate::model::ui_state::FileOperationProgress;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
};
use std::time::Instant;
use std::{borrow::Cow, collections::HashMap, path::Path};

const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

pub struct FileOperationsOverlay;

impl FileOperationsOverlay {
    /// Main render function - entry point for overlay display
    pub fn render(f: &mut Frame, area: Rect, operations: &HashMap<String, FileOperationProgress>) {
        if operations.is_empty() {
            return;
        }

        let areas: Vec<Rect> = Self::calculate_layout(area, operations.len());

        for (i, (_, progress)) in operations.iter().enumerate() {
            if let Some(op_area) = areas.get(i) {
                Self::render_single_operation(f, *op_area, progress);
            }
        }

        Self::render_cancel_instruction(f, &areas);
    }

    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    fn render_single_operation(f: &mut Frame, area: Rect, progress: &FileOperationProgress) {
        let FileOperationProgress {
            operation_type,
            current_file,
            total_files,
            files_completed,
            throughput_bps,
            estimated_completion,
            ..
        } = progress;

        let percentage: u16 = (progress.progress_ratio() * 100.0) as u16;
        let throughput: String = Self::format_throughput(*throughput_bps);
        let eta: String = Self::format_eta(*estimated_completion);
        let file_display: String = Self::truncate_path(current_file, 35);
        let file_count: String = format!("({files_completed}/{total_files})");

        let color: Color = match operation_type.as_str() {
            "Copy" => Color::Blue,

            "Move" => Color::Yellow,

            "Rename" => Color::Green,

            _ => Color::Cyan,
        };

        let gauge: Gauge<'_> = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(format!(
                "{} {} {}",
                progress.operation_type, file_display, file_count
            )))
            .gauge_style(Style::default().fg(color))
            .percent(percentage)
            .label(format!("{percentage}% ({throughput}, {eta})"));

        f.render_widget(gauge, area);
    }

    #[allow(clippy::cast_possible_truncation)]
    /// Calculate dynamic layout based on operation count
    fn calculate_layout(area: Rect, operation_count: usize) -> Vec<Rect> {
        let available_height: u16 = area.height.saturating_sub(1);
        let op_height: u16 = std::cmp::max(3, available_height / operation_count as u16);

        (0..operation_count)
            .map(|i: usize| Rect {
                x: area.x,
                y: area.y + (i as u16 * op_height),
                width: area.width,
                height: op_height,
            })
            .collect()
    }

    fn render_cancel_instruction(f: &mut Frame, areas: &[Rect]) {
        if let Some(last_area) = areas.last() {
            let instruction_area: Rect = Rect {
                y: last_area.y + last_area.height,
                height: 1,
                ..*last_area
            };

            let text: Paragraph<'_> = Paragraph::new("Press ESC to cancel operations.")
                .style(Style::default().fg(Color::Gray));

            f.render_widget(text, instruction_area);
        }
    }

    fn format_throughput(bps: Option<u64>) -> String {
        bps.map_or_else(
            || -> String { "Calculating...".to_string() },
            |bytes: u64| -> String {
                let (size, unit) = Self::scale_bytes(bytes);

                format!("{size:.1}{unit}/s")
            },
        )
    }

    fn format_eta(eta: Option<Instant>) -> String {
        eta.map_or_else(
            || -> String { "Calculating...".to_string() },
            |time: Instant| -> String {
                let now: Instant = Instant::now();

                if time > now {
                    let remaining: u64 = time.duration_since(now).as_secs();

                    format!("{remaining}s remaining")
                } else {
                    "Finishing...".to_string()
                }
            },
        )
    }

    #[allow(clippy::cast_precision_loss)]
    fn scale_bytes(bytes: u64) -> (f64, &'static str) {
        let mut size: f64 = bytes as f64;
        let mut unit_idx: usize = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        (size, UNITS[unit_idx])
    }

    fn truncate_path(path: &Path, max_len: usize) -> String {
        let path: Cow<str> = path.to_string_lossy();

        if path.len() <= max_len {
            path.to_string()
        } else {
            format!("...{}", &path[(path.len() - max_len + 3)..])
        }
    }
}
