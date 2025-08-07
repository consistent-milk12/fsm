use crate::model::shared_state::SharedState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};

pub struct LoadingOverlay;

impl LoadingOverlay {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub fn render(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let loading = {
            let ui_guard = shared_state.lock_ui();
            ui_guard.loading.clone()
        };

        let Some(loading) = loading else {
            return;
        };

        let spinner_frames = ["⠁", "⠃", "⠇", "⠧", "⠷", "⠿", "⠻", "⠹", "⠸"];
        let spinner = spinner_frames[loading.spinner_frame % spinner_frames.len()];

        let mut lines = vec![Line::from(Span::styled(
            format!("{} {}", spinner, loading.message),
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        ))];

        if let Some(ref item) = loading.current_item {
            lines.push(Line::from(format!("Current: {item}")));
        }

        if let (Some(done), Some(total)) = (loading.completed, loading.total) {
            lines.push(Line::from(format!("Completed: {done}/{total}")));
        }

        lines.push(Line::from(""));

        let overlay_area = Self::centered_rect(50, 20, area);
        frame.render_widget(Clear, overlay_area);

        let block = Block::default()
            .title("Loading")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND));

        if let Some(progress) = loading.progress {
            let gauge = Gauge::default()
                .block(block)
                .gauge_style(Style::default().fg(theme::PINK).bg(theme::CURRENT_LINE))
                .percent((progress * 100.0) as u16)
                .label(format!("{:.0}%", progress * 100.0));
            frame.render_widget(gauge, overlay_area);

            let text_area = Self::inset_rect(overlay_area, 2, 2);
            let para = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
            frame.render_widget(para, text_area);
        } else {
            let para = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(para, overlay_area);
        }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vertical[1]);

        horizontal[1]
    }

    const fn inset_rect(area: Rect, margin_x: u16, margin_y: u16) -> Rect {
        Rect {
            x: area.x + margin_x,
            y: area.y + margin_y,
            width: area.width.saturating_sub(margin_x * 2),
            height: area.height.saturating_sub(margin_y * 2),
        }
    }
}
