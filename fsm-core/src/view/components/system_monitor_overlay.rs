//! System and process monitor overlay with proper FSM integration.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph},
};
use tracing::{debug, instrument};

use crate::{
    controller::actions::{ProcessData, SystemData},
    model::app_state::AppState,
    view::snapshots::UiSnapshot,
};

/// Optimized system monitor overlay component
#[derive(Debug)]
pub struct OptimizedSystemMonitorOverlay {
    /// Process list state for selection
    process_list_state: ListState,
    /// Cache for process data formatting
    cached_process_items: Vec<ListItem<'static>>,
    /// Last update timestamp
    last_update: std::time::Instant,
    /// Update interval (2 seconds)
    update_interval: std::time::Duration,
}

impl OptimizedSystemMonitorOverlay {
    pub fn new() -> Self {
        Self {
            process_list_state: ListState::default(),
            cached_process_items: Vec::new(),
            last_update: std::time::Instant::now() - std::time::Duration::from_secs(3), // Force initial update
            update_interval: std::time::Duration::from_secs(2),
        }
    }

    /// Render system monitor overlay with proper state integration
    #[instrument(level = "trace", skip(self, frame, ui_snapshot, app_state))]
    pub fn render_system_monitor(
        &mut self,
        frame: &mut Frame,
        ui_snapshot: &UiSnapshot,
        app_state: &AppState,
        area: Rect,
    ) {
        // Clear the overlay area
        frame.render_widget(Clear, area);

        // Check if we have system data
        let system_data = if let Some(ref data) = app_state.system_data {
            data.clone()
        } else {
            // Render placeholder if no data available
            self.render_placeholder(frame, area);
            return;
        };

        // Main layout: system info (top) + process list (bottom)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(0)])
            .split(area);

        // Render system information
        self.render_system_info(frame, &system_data, chunks[0]);

        // Render process list
        self.render_process_list(frame, app_state, ui_snapshot, chunks[1]);
    }

    /// Render system information gauges
    fn render_system_info(&self, frame: &mut Frame, data: &SystemData, area: Rect) {
        let block = Block::default()
            .title(" System Monitor ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));

        frame.render_widget(block, area);

        // Inner area for gauges
        let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
        let gauge_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // CPU
                Constraint::Length(1), // Memory
                Constraint::Length(1), // Swap
                Constraint::Length(1), // Spacer
            ])
            .split(inner);

        // CPU Usage Gauge
        let cpu_percentage = data.cpu_usage.min(100.0).max(0.0) as u16;
        let cpu_gauge = Gauge::default()
            .label(format!("CPU: {:.1}%", data.cpu_usage))
            .gauge_style(self.get_cpu_color(data.cpu_usage))
            .percent(cpu_percentage);
        frame.render_widget(cpu_gauge, gauge_chunks[0]);

        // Memory Usage Gauge
        let mem_ratio = if data.total_mem > 0 {
            (data.mem_usage as f64 / data.total_mem as f64).min(1.0).max(0.0)
        } else {
            0.0
        };
        let mem_gauge = Gauge::default()
            .label(format!(
                "Memory: {} / {}",
                format_bytes(data.mem_usage),
                format_bytes(data.total_mem)
            ))
            .gauge_style(self.get_memory_color(mem_ratio))
            .ratio(mem_ratio);
        frame.render_widget(mem_gauge, gauge_chunks[1]);

        // Swap Usage Gauge
        let swap_ratio = if data.total_swap > 0 {
            (data.swap_usage as f64 / data.total_swap as f64).min(1.0).max(0.0)
        } else {
            0.0
        };
        let swap_gauge = Gauge::default()
            .label(format!(
                "Swap: {} / {}",
                format_bytes(data.swap_usage),
                format_bytes(data.total_swap)
            ))
            .gauge_style(self.get_swap_color(swap_ratio))
            .ratio(swap_ratio);
        frame.render_widget(swap_gauge, gauge_chunks[2]);
    }

    /// Render process list with selection support
    fn render_process_list(
        &mut self,
        frame: &mut Frame,
        app_state: &AppState,
        ui_snapshot: &UiSnapshot,
        area: Rect,
    ) {
        let block = Block::default()
            .title(" Processes ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .style(Style::default().bg(Color::Black));

        // Update process list if needed
        if self.should_update_processes() {
            self.update_process_cache(&app_state.process_data);
        }

        // Update selection based on UI state
        if let Some(selected_idx) = ui_snapshot.selected_process_idx {
            if selected_idx < self.cached_process_items.len() {
                self.process_list_state.select(Some(selected_idx));
            }
        }

        // Create process list widget
        let process_list = List::new(self.cached_process_items.clone())
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");

        // Render stateful widget
        frame.render_stateful_widget(process_list, area, &mut self.process_list_state);
    }

    /// Check if process list should be updated
    fn should_update_processes(&self) -> bool {
        self.last_update.elapsed() >= self.update_interval
    }

    /// Update cached process items
    fn update_process_cache(&mut self, processes: &[ProcessData]) {
        let now = std::time::Instant::now();
        
        // Sort processes by CPU usage (descending)
        let mut sorted_processes = processes.to_vec();
        sorted_processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 50 processes to avoid UI clutter
        sorted_processes.truncate(50);

        self.cached_process_items = sorted_processes
            .iter()
            .map(|process| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>8}", process.pid),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:<20}", truncate_string(&process.name, 20)),
                        Style::default().fg(Color::White),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:>6.1}%", process.cpu_usage),
                        self.get_process_cpu_color(process.cpu_usage),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:>10}", format_bytes(process.memory)),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:<10}", truncate_string(&process.status, 10)),
                        self.get_process_status_color(&process.status),
                    ),
                ]))
            })
            .collect();

        self.last_update = now;
        
        debug!(
            processes_count = sorted_processes.len(),
            cache_size = self.cached_process_items.len(),
            "Process cache updated"
        );
    }

    /// Render placeholder when no system data is available
    fn render_placeholder(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" System Monitor ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray))
            .style(Style::default().bg(Color::Black));

        frame.render_widget(block, area);

        let inner = area.inner(Margin { vertical: 2, horizontal: 2 });
        let text = Text::from(vec![
            Line::from("System monitoring not available"),
            Line::from("Press 'S' to enable system monitoring"),
        ]);

        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, inner);
    }

    /// Get CPU gauge color based on usage
    fn get_cpu_color(&self, usage: f32) -> Style {
        match usage {
            x if x >= 80.0 => Style::default().fg(Color::Red),
            x if x >= 60.0 => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Green),
        }
    }

    /// Get memory gauge color based on usage ratio
    fn get_memory_color(&self, ratio: f64) -> Style {
        match ratio {
            x if x >= 0.9 => Style::default().fg(Color::Red),
            x if x >= 0.7 => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Blue),
        }
    }

    /// Get swap gauge color based on usage ratio
    fn get_swap_color(&self, ratio: f64) -> Style {
        match ratio {
            x if x >= 0.5 => Style::default().fg(Color::Red),
            x if x >= 0.2 => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Magenta),
        }
    }

    /// Get process CPU usage color
    fn get_process_cpu_color(&self, usage: f32) -> Style {
        match usage {
            x if x >= 50.0 => Style::default().fg(Color::Red),
            x if x >= 20.0 => Style::default().fg(Color::Yellow),
            x if x >= 5.0 => Style::default().fg(Color::Green),
            _ => Style::default().fg(Color::Gray),
        }
    }

    /// Get process status color
    fn get_process_status_color(&self, status: &str) -> Style {
        match status.to_lowercase().as_str() {
            s if s.contains("running") => Style::default().fg(Color::Green),
            s if s.contains("sleeping") => Style::default().fg(Color::Blue),
            s if s.contains("stopped") => Style::default().fg(Color::Yellow),
            s if s.contains("zombie") => Style::default().fg(Color::Red),
            _ => Style::default().fg(Color::Gray),
        }
    }

    /// Get selected process index
    pub fn selected_process(&self) -> Option<usize> {
        self.process_list_state.selected()
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        let selected = self.process_list_state.selected().unwrap_or(0);
        if selected > 0 {
            self.process_list_state.select(Some(selected - 1));
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let selected = self.process_list_state.selected().unwrap_or(0);
        if selected + 1 < self.cached_process_items.len() {
            self.process_list_state.select(Some(selected + 1));
        }
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.process_list_state.select(None);
    }
}

impl Default for OptimizedSystemMonitorOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Format bytes in human readable format
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: f64 = 1024.0;

    if bytes == 0 {
        return "0 B".to_string();
    }

    let bytes_f = bytes as f64;
    let unit_index = ((bytes_f.ln() / THRESHOLD.ln()) as usize).min(UNITS.len() - 1);
    let value = bytes_f / THRESHOLD.powi(unit_index as i32);

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

/// Truncate string to specified length with ellipsis
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        let mut result = s.chars().take(max_len - 3).collect::<String>();
        result.push_str("...");
        result
    }
}

/// Calculate centered rectangle for overlay positioning
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}