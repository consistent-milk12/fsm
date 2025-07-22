use std::rc::Rc;

use crate::AppState;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
};

pub struct LoadingOverlay;

impl LoadingOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let Some(loading) = &app.ui.loading else {
            return;
        };

        let spinner_frames: [&'static str; 9] = ["⠁", "⠃", "⠇", "⠧", "⠷", "⠿", "⠻", "⠹", "⠸"];
        let spinner: &'static str = spinner_frames[loading.spinner_frame % spinner_frames.len()];

        // Main message
        let mut lines: Vec<Line<'_>> = vec![Line::from(Span::styled(
            format!("{} {}", spinner, loading.message),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))];

        // Show current item if present
        if let Some(ref item) = loading.current_item {
            lines.push(Line::from(format!("Current: {item}")));
        }

        // Show completed/total if present
        if let (Some(done), Some(total)) = (loading.completed, loading.total) {
            lines.push(Line::from(format!("Completed: {}/{}", done, total)));
        }

        // Progress bar if determinate, otherwise blank line
        lines.push(Line::from(""));

        // Centered modal area
        let overlay_area: Rect = Self::centered_rect(50, 20, area);
        frame.render_widget(Clear, overlay_area);

        let block: Block<'_> = Block::default()
            .title("Loading")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightBlue));

        if let Some(progress) = loading.progress {
            // Determinate progress bar
            let gauge: Gauge<'_> = Gauge::default()
                .block(block)
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .percent((progress * 100.0) as u16)
                .label(format!("{:.0}%", progress * 100.0));
            frame.render_widget(gauge, overlay_area);
            // Overlay main text above gauge
            let text_area: Rect = Self::inset_rect(overlay_area, 2, 2);
            let para: Paragraph<'_> =
                Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
            frame.render_widget(para, text_area);
        } else {
            // Spinner only + info
            let para: Paragraph<'_> = Paragraph::new(Text::from(lines))
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(para, overlay_area);
        }
    }

    /// Standard centered modal rectangle
    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vertical: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        let horizontal: Rc<[Rect]> = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vertical[1]);

        horizontal[1]
    }

    /// Helper: inset a rect by margins (for overlaying text above gauge)
    fn inset_rect(area: Rect, margin_x: u16, margin_y: u16) -> Rect {
        Rect {
            x: area.x + margin_x,
            y: area.y + margin_y,
            width: area.width.saturating_sub(margin_x * 2),
            height: area.height.saturating_sub(margin_y * 2),
        }
    }
}
