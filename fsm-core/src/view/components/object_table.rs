//! src/view/components/object_table.rs - Updated for unified StateCoordinator
//!
//! This component renders the file table within the main pane.  The original
//! implementation depended on a `DirState` type returned from the legacy
//! `StateCoordinator`.  With the updated architecture the file table should
//! operate directly on a [`PaneState`], which contains all necessary
//! directory metadata (entries, selected index, etc.).  This rewrite
//! replaces the `DirState` parameter with `PaneState` and uses the
//! pane’s atomic `selected` field to set the table’s selected row.

use crate::{
    model::fs_state::PaneState,
    model::ui_state::UIState,
    view::{icons, theme},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, HighlightSpacing, Row, Table, TableState},
};
use std::path::Path;
use std::sync::atomic::Ordering;
use tracing::{debug, trace, instrument};

pub struct OptimizedFileTable;

impl OptimizedFileTable {
    pub fn new() -> Self {
        debug!("Creating new OptimizedFileTable");
        Self
    }

    /// Render the file table using a [`PaneState`].  The `PaneState` holds
    /// directory entries, sorting and selection state.  The selected index
    /// is loaded atomically to configure the table’s highlight row.  The
    /// provided `path` is used for the table title.
    #[instrument(level = "trace", skip_all, fields(entry_count = pane_state.entries.len(), path = ?path))]
    pub fn render_optimized(
        &self,
        frame: &mut Frame<'_>,
        _ui_state: &UIState,
        pane_state: &PaneState,
        path: &Path,
        area: Rect,
    ) {
        trace!("Rendering file table with {} entries", pane_state.entries.len());
        let entries = &pane_state.entries;
        debug!("Table area: {}x{}", area.width, area.height);

        // Header row with column names
        let header = Row::new(vec!["Name", "Size", "Modified"])
            .style(Style::default().fg(theme::YELLOW).bold())
            .bottom_margin(1);

        // Build rows for each entry with appropriate icons and styles
        let rows: Vec<Row> = entries
            .iter()
            .map(|obj| {
                let (icon, style) = if obj.is_dir {
                    (icons::FOLDER_ICON, Style::default().fg(theme::CYAN))
                } else if obj.is_symlink {
                    (icons::SYMLINK_ICON, Style::default().fg(theme::PINK))
                } else {
                    (icons::FILE_ICON, Style::default().fg(theme::FOREGROUND))
                };

                let size_str = if obj.is_dir {
                    String::new()
                } else {
                    crate::util::humanize::human_readable_size(obj.size)
                };

                Row::new(vec![
                    Cell::from(format!("{icon} {}", obj.name)),
                    Cell::from(size_str),
                    Cell::from(obj.modified.format("%d/%m/%Y %I:%M %p").to_string()),
                ])
                .style(style)
            })
            .collect();

        // Column widths
        let widths = [
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(22),
        ];

        // Table selection state based on pane’s selected index
        let selected_index = pane_state.selected.load(Ordering::Relaxed);
        let mut table_state = TableState::default().with_selected(Some(selected_index));

        // Table title shows the current path
        let title = format!(" {} ", path.display());

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(Style::default().fg(theme::PURPLE).bold())
                    .border_style(Style::default().fg(theme::COMMENT))
                    .style(Style::default().bg(theme::BACKGROUND)),
            )
            .row_highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(table, area, &mut table_state);
    }
}

impl Default for OptimizedFileTable {
    fn default() -> Self {
        Self::new()
    }
}
