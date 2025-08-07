
//! ``src/view/components/search_results_overlay.rs``
//! # `SearchResultsOverlay`: Displays Search Results

use crate::AppState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub struct SearchResultsOverlay;

impl SearchResultsOverlay {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let results = &app.ui.search_results;

        let mut lines = vec![Line::from(Span::styled(
            "Search Results",
            Style::default()
                .fg(theme::YELLOW)
                .add_modifier(Modifier::BOLD),
        ))];

        if results.is_empty() {
            lines.push(Line::from(Span::raw("No results found.")));
        } else {
            lines.push(Line::from(Span::raw(format!(
                "Found {} results:",
                results.len()
            ))));
            lines.push(Line::from(""));

            let items: Vec<ListItem> = results
                .iter()
                .filter_map(|entry| app.registry.get(entry.id))
                .map(|obj_info| ListItem::new(obj_info.path.to_string_lossy().into_owned()))
                .collect();

            let mut list_state = ListState::default();
            list_state.select(app.ui.selected);

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
                )
                .highlight_symbol("▶ ")
                .highlight_style(
                    Style::default()
                        .bg(theme::CURRENT_LINE)
                        .fg(theme::PINK)
                        .add_modifier(Modifier::BOLD),
                );

            let list_area = Self::inset_rect(area, 2, 4); // Adjust inset as needed
            frame.render_stateful_widget(list, list_area, &mut list_state);
        }

        let overlay_area = Self::centered_rect(80, 80, area);
        frame.render_widget(Clear, overlay_area);

        let para = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title("Search Results")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND).fg(theme::FOREGROUND)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(para, overlay_area);
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
