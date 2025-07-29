//! System and process monitor overlay.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, List, ListItem},
};

use crate::controller::actions::{ProcessData, SystemData};
use crate::model::app_state::AppState;

pub fn system_monitor_overlay(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let system_data = state.system_data.clone().unwrap_or_default();

    let overlay_area = centered_rect(80, 80, area);
    frame.render_widget(Clear, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)].as_ref())
        .split(overlay_area);

    system_info_widget(frame, chunks[0], &system_data);
    process_list_widget(frame, chunks[1], &state.process_data);
}

fn system_info_widget(frame: &mut Frame, area: Rect, data: &SystemData) {
    let block = Block::default()
        .title("System Monitor")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(area);

    let cpu_gauge = Gauge::default()
        .label(format!("CPU: {:.2}%", data.cpu_usage))
        .gauge_style(Style::default().fg(Color::Green))
        .percent(data.cpu_usage as u16);
    frame.render_widget(cpu_gauge, chunks[0]);

    let mem_gauge = Gauge::default()
        .label(format!(
            "Mem: {} / {}",
            bytesize::ByteSize(data.mem_usage).to_string(),
            bytesize::ByteSize(data.total_mem).to_string()
        ))
        .gauge_style(Style::default().fg(Color::Yellow))
        .ratio(data.mem_usage as f64 / data.total_mem as f64);
    frame.render_widget(mem_gauge, chunks[1]);

    let swap_gauge = Gauge::default()
        .label(format!(
            "Swap: {} / {}",
            bytesize::ByteSize(data.swap_usage).to_string(),
            bytesize::ByteSize(data.total_swap).to_string()
        ))
        .gauge_style(Style::default().fg(Color::Red))
        .ratio(data.swap_usage as f64 / data.total_swap as f64);
    frame.render_widget(swap_gauge, chunks[2]);
}

fn process_list_widget(frame: &mut Frame, area: Rect, processes: &[ProcessData]) {
    let block = Block::default()
        .title("Processes")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    let items: Vec<ListItem> = processes
        .iter()
        .map(|p| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<8}", p.pid), Style::default().fg(Color::Cyan)),
                Span::raw(format!("{:<25}", p.name)),
                Span::styled(
                    format!("{:<8.2}%", p.cpu_usage),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("{:<10}", bytesize::ByteSize(p.memory).to_string()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(p.status.to_string()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::Blue));

    frame.render_widget(list, area);
}

/// Helper to create a centered rect.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
