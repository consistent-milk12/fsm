//! src/view/components/search_results_overlay.rs
//! ============================================================
//! Pure, lock-free overlay that shows the final list of matches
//! after a content / filename search.  The caller passes an
//! immutable results slice plus the row index that should be
//! highlighted.

use crate::fs::object_info::ObjectInfo;
use crate::view::theme;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

/// Results pane (no internal state)
pub struct OptimizedSearchResultsOverlay;

impl OptimizedSearchResultsOverlay {
    pub fn new() -> Self {
        Self
    }

    /// Paint the results list.
    ///
    /// * `results`  – slice of hits (pre-sorted by the caller)  
    /// * `selected` – optional highlight index  
    /// * `rect`     – target rectangle
    pub fn render_results(
        &self,
        frame: &mut Frame<'_>,
        results: &[ObjectInfo],
        selected: Option<usize>,
        rect: Rect,
    ) {
        // -----------------------------------------------------
        // Chrome
        // -----------------------------------------------------
        let chrome = Block::default()
            .title(" Search Results ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        // clear background first
        frame.render_widget(Clear, rect);

        // -----------------------------------------------------
        // Empty results
        // -----------------------------------------------------
        if results.is_empty() {
            frame.render_widget(
                Paragraph::new("No results found.")
                    .block(chrome)
                    .alignment(Alignment::Center),
                rect,
            );
            return;
        }

        // -----------------------------------------------------
        // Build list items
        // -----------------------------------------------------
        let items: Vec<ListItem<'_>> = results
            .iter()
            .map(|obj| ListItem::new(obj.path.to_string_lossy()))
            .collect();

        // highlight state
        let mut state = ListState::default();
        state.select(selected);

        // list widget
        let list = List::new(items)
            .block(chrome)
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .fg(theme::PINK)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, rect, &mut state);
    }
}

impl Default for OptimizedSearchResultsOverlay {
    fn default() -> Self {
        Self::new()
    }
}
