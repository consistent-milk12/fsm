//! ``src/view/components/input_prompt_overlay.rs``
//! ============================================================================
//! # `InputPromptOverlay`: Stylized Input Prompt for File/Folder Creation

use crate::model::app_state::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct InputPromptOverlay;

impl InputPromptOverlay {
    #[allow(clippy::cast_possible_truncation)]
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let overlay_area = Self::centered_rect(50, 10, area);
        frame.render_widget(Clear, overlay_area);

        let title = match app.ui.input_prompt_type {
            Some(crate::controller::actions::InputPromptType::CreateFile) => " Create New File ",
            Some(crate::controller::actions::InputPromptType::CreateDirectory) => {
                " Create New Directory "
            }
            _ => " Input ",
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(app.ui.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, overlay_area);

        // Show cursor
        frame.set_cursor_position((
            overlay_area.x + app.ui.input.len() as u16 + 1,
            overlay_area.y + 1,
        ));

        // Render help text at bottom
        let help_text = "Type name • Enter to confirm • Esc to cancel";
        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        let help_area = Rect {
            x: overlay_area.x,
            y: overlay_area.y + overlay_area.height,
            width: overlay_area.width,
            height: 1,
        };

        if help_area.y < area.height {
            frame.render_widget(help_paragraph, help_area);
        }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}
