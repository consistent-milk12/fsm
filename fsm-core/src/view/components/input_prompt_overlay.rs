//! src/view/components/input_prompt_overlay.rs
use crate::controller::actions::InputPromptType;
use crate::model::ui_state::UIState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct OptimizedPromptOverlay;

impl OptimizedPromptOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_input(
        &self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        prompt_type: &InputPromptType,
        area: Rect,
    ) {
        frame.render_widget(Clear, area);

        let title = match prompt_type {
            InputPromptType::CreateFile => " Create New File ",
            InputPromptType::CreateDirectory => " Create New Directory ",
            InputPromptType::Rename => " Rename ",
            InputPromptType::Search => " Search ",
            // InputPromptType::ContentSearch => " Content Search ",
            _ => " Content Search ",
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(ui_state.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, area);

        frame.set_cursor(area.x + ui_state.input.len() as u16 + 1, area.y + 1);

        let help_text = "Type name • Enter to confirm • Esc to cancel";
        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        let help_area = Rect {
            x: area.x,
            y: area.y + area.height,
            width: area.width,
            height: 1,
        };

        if help_area.y < frame.size().height {
            frame.render_widget(help_paragraph, help_area);
        }
    }
}

impl Default for OptimizedPromptOverlay {
    fn default() -> Self {
        Self::new()
    }
}
