//! src/view/components/loading_overlay.rs
use crate::model::ui_state::LoadingState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};

pub struct OptimizedLoadingOverlay;

impl OptimizedLoadingOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_progress(&self, frame: &mut Frame<'_>, loading_state: &LoadingState, area: Rect) {
        let summary = loading_state.get_summary();

        let mut lines = vec![Line::from(Span::styled(
            summary.format_progress(),
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        ))];

        if let Some(item) = &summary.current_item {
            lines.push(Line::from(format!("Current: {item}")));
        }

        if let Some(eta) = summary.estimated_remaining {
            lines.push(Line::from(format!("ETA: {:.0?}", eta)));
        }

        lines.push(Line::from(""));

        frame.render_widget(Clear, area);

        let block = Block::default()
            .title("Loading")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND));

        if !summary.is_indeterminate {
            let gauge = Gauge::default()
                .block(block)
                .gauge_style(Style::default().fg(theme::PINK).bg(theme::CURRENT_LINE))
                .percent(summary.progress as u16)
                .label(format!("{:.1}%", summary.progress));
            frame.render_widget(gauge, area);

            let text_area = area.inner(Margin {
                vertical: 2,
                horizontal: 2,
            });
            let para = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
            frame.render_widget(para, text_area);
        } else {
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
