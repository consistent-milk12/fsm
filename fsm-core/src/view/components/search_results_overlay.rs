//! src/view/components/search_results_overlay.rs
use crate::model::ui_state::UIState;
use crate::view::theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

pub struct OptimizedSearchResultsOverlay;

impl OptimizedSearchResultsOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_results(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        let results = &ui_state.search_results;
        let block = Block::default()
            .title("Search Results")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        frame.render_widget(Clear, area);

        if results.is_empty() {
            let message = Paragraph::new("No results found.")
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(message, area);
            return;
        }

        let items: Vec<ListItem> = results
            .iter()
            .map(|obj| ListItem::new(obj.path.to_string_lossy().into_owned()))
            .collect();

        let mut list_state = ListState::default();
        list_state.select(ui_state.selected);

        let list = List::new(items)
            .block(block)
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .fg(theme::PINK)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, area, &mut list_state);
    }
}

impl Default for OptimizedSearchResultsOverlay {
    fn default() -> Self {
        Self::new()
    }
}
