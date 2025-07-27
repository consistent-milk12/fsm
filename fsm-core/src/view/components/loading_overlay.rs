use crate::model::ui_state::LoadingState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};
use std::time::{Duration, Instant};

pub struct OptimizedLoadingOverlay;

impl OptimizedLoadingOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_progress(&self, frame: &mut Frame<'_>, loading_state: &LoadingState, area: Rect) {
        // Atomically snapshot the counters
        let current = loading_state
            .current
            .load(std::sync::atomic::Ordering::Relaxed);
        let total = loading_state
            .total
            .load(std::sync::atomic::Ordering::Relaxed);
        let progress = if total > 0 {
            (current as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        // Optionally fetch current item
        let binding = loading_state.current_item.read();
        let current_item = binding.as_ref().map(|cs| cs.as_str());

        // Compute ETA if start_time is known and total > current
        let elapsed = loading_state.start_time.elapsed();
        let estimated_remaining = if total > current && current > 0 {
            let rate = current as f64 / elapsed.as_secs_f64();
            let rem = ((total - current) as f64 / rate).max(0.0);
            Some(Duration::from_secs_f64(rem))
        } else {
            None
        };

        // Summary lines
        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("{:.1}% ({}/{} items)", progress, current, total),
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        )));
        if let Some(item) = current_item {
            lines.push(Line::from(format!("Current: {}", item)));
        }
        if let Some(eta) = estimated_remaining {
            lines.push(Line::from(format!("ETA: {:.0?}", eta)));
        }
        lines.push(Line::from(""));

        // Clear the overlay area and render background block
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("Loading")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND));

        if total > 0 {
            // determinate gauge
            let gauge = Gauge::default()
                .block(block)
                .gauge_style(Style::default().fg(theme::PINK).bg(theme::CURRENT_LINE))
                .percent(progress as u16)
                .label(format!("{:.1}%", progress));
            frame.render_widget(gauge, area);

            let inner = area.inner(Margin {
                vertical: 2,
                horizontal: 2,
            });
            let para = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
            frame.render_widget(para, inner);
        } else {
            // indeterminate fallback
            let para = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(para, area);
        }
    }
}

impl Default for OptimizedLoadingOverlay {
    fn default() -> Self {
        Self::new()
    }
}
