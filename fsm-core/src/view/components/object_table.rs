//! src/view/components/object_table.rs

use crate::{
    controller::state_coordinator::DirState,
    model::ui_state::UIState,
    view::{icons, theme},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, HighlightSpacing, Row, Table, TableState},
};
use std::path::Path;

pub struct OptimizedFileTable;

impl OptimizedFileTable {
    pub fn new() -> Self {
        Self
    }

    pub fn render_optimized(
        &self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        dir_state: &DirState,
        path: &Path,
        area: Rect,
    ) {
        let entries = &dir_state.entries;

        let header = Row::new(vec!["Name", "Size", "Modified"])
            .style(Style::default().fg(theme::YELLOW).bold())
            .bottom_margin(1);

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

        let widths = [
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(22),
        ];

        let mut table_state = TableState::default().with_selected(ui_state.selected);

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
            .highlight_style(
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
