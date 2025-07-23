// src/view/components/command_palette.rs

use crate::model::app_state::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub struct CommandPalette;

impl CommandPalette {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let state = &app.ui.command_palette;

        if !state.visible {
            return;
        }

        let popup_area = Self::centered_rect(60, 40, area);
        frame.render_widget(Clear, popup_area);

        let input_para = Paragraph::new(format!(":{}", state.input))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Command Palette")
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
            )
            .alignment(Alignment::Left)
            .style(Style::default().add_modifier(Modifier::BOLD));

        let input_area = Rect {
            height: 3,
            ..popup_area
        };
        frame.render_widget(input_para, input_area);

        let items: Vec<ListItem> = state
            .filtered
            .iter()
            .map(|cmd| ListItem::new(cmd.title.clone()))
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(state.selected));

        let list_area = Rect {
            y: popup_area.y + 3,
            height: popup_area.height - 3,
            ..popup_area
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
            )
            .highlight_symbol(">> ")
            .highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .fg(theme::PINK)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, list_area, &mut list_state);
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_width = r.width * percent_x / 100;
        let popup_height = r.height * percent_y / 100;
        Rect {
            x: r.x + (r.width - popup_width) / 2,
            y: r.y + (r.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        }
    }
}
