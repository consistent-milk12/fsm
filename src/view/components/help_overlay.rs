use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::AppState;

pub struct HelpOverlay;

/// Renders the help overlay centered in the given area.
impl HelpOverlay {
    pub fn render(frame: &mut Frame<'_>, _app: &AppState, area: Rect) {
        // Construct help text (this should ideally be dynamic)
        let help_text = vec![
            Line::from(Span::styled(
                "File Manager — Help",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Navigation:"),
            Line::from("  Up/Down       Move selection"),
            Line::from("  Left/Right    Switch pane"),
            Line::from("  Enter         Open/Enter directory"),
            Line::from("  Backspace     Go to parent directory"),
            Line::from(""),
            Line::from("File Operations:"),
            Line::from("  n             New file"),
            Line::from("  f             New folder"),
            Line::from("  d             Delete selected"),
            Line::from("  r             Rename"),
            Line::from("  y             Copy"),
            Line::from("  p             Paste"),
            Line::from("  m             Move"),
            Line::from(""),
            Line::from("Bulk/Advanced:"),
            Line::from("  Space         Select for batch"),
            Line::from("  a             Select all"),
            Line::from("  : or Ctrl+P   Command palette"),
            Line::from("  /             Filter/search"),
            Line::from(""),
            Line::from("UI/General:"),
            Line::from("  t             Toggle theme"),
            Line::from("  h or ?        Show/hide help"),
            Line::from("  q             Quit"),
            Line::from(""),
            Line::from("Press Esc or ? to close this help."),
        ];

        // Center overlay (usually 70% width, 80% height)
        let overlay_area = Self::centered_rect(70, 80, area);

        // Clear area before drawing modal
        frame.render_widget(Clear, overlay_area);

        // Main help text widget
        let help_paragraph = Paragraph::new(Text::from(help_text))
            .block(
                Block::default()
                    .title("Help")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightBlue)),
            )
            .alignment(Alignment::Left)
            .wrap(ratatui::widgets::Wrap { trim: true });

        // Render to the frame
        frame.render_widget(help_paragraph, overlay_area);
    }

    /// Returns a centered rectangle of given width/height percentages inside area.
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
}
