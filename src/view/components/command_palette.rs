// src/view/components/command_palette.rs

use crate::model::{app_state::AppState, command_palette::CommandPaletteState};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub struct CommandPalette;

impl CommandPalette {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let state: &CommandPaletteState = &app.ui.command_palette;

        if !state.visible {
            return;
        }
        // Centered modal
        let popup_area: Rect = Self::centered_rect(60, 40, area);

        frame.render_widget(Clear, popup_area);

        // Palette input at top
        let input_para: Paragraph<'_> = Paragraph::new(format!(":{}", state.input))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Command Palette"),
            )
            .alignment(Alignment::Left)
            .style(Style::default().add_modifier(Modifier::BOLD));
        let input_area = Rect {
            x: popup_area.x,
            y: popup_area.y,
            width: popup_area.width,
            height: 3,
        };
        frame.render_widget(input_para, input_area);

        // List of filtered commands
        let items: Vec<ListItem> = state
            .filtered
            .iter()
            .map(|cmd| ListItem::new(cmd.title.clone()))
            .collect();
        let mut list_state: ListState = ratatui::widgets::ListState::default();
        list_state.select(Some(state.selected));
        let list_area = Rect {
            x: popup_area.x,
            y: popup_area.y + 3,
            width: popup_area.width,
            height: popup_area.height - 3,
        };

        let list = List::new(items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM))
            .highlight_symbol(">> ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        
        frame.render_stateful_widget(list, list_area, &mut list_state);
    }

    // Helper: center a rectangle of % width/height within another rect
    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_width: u16 = r.width * percent_x / 100;
        let popup_height: u16 = r.height * percent_y / 100;
        Rect {
            x: r.x + (r.width - popup_width) / 2,
            y: r.y + (r.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        }
    }
}
