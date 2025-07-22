//! src/ui/help_overlay.rs
//! -----------------------------------------------------------------------------
//! HelpOverlay: Modal overlay for all keybindings and usage tips.
//! Uses ratatui v0.25+ idioms and visual polish.
//!

use ratatui::{
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct HelpOverlay;

impl HelpOverlay {
    /// Renders the help overlay as a modal, centered in the terminal.
    pub fn render(f: &mut Frame<'_>, area: Rect) {
        // Example keybindings; ideally pull from config or static struct
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

        let help_paragraph = Paragraph::new(Text::from(help_text))
            .block(
                Block::default()
                    .title("Help")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::LightBlue)),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        // Center the overlay
        let overlay_area = Self::centered_rect(70, 80, area); // 70% width, 80% height
        f.render_widget(Clear, overlay_area); // Clear beneath overlay
        f.render_widget(help_paragraph, overlay_area);
    }

    /// Centers a rect of percent width/height in area
    pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
            .split(area);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage((100 - percent_x) / 2),
                    Constraint::Percentage(percent_x),
                    Constraint::Percentage((100 - percent_x) / 2),
                ]
                .as_ref(),
            )
            .split(popup_layout[1]);

        horizontal[1]
    }
}
