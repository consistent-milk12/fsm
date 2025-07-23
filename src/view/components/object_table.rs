//! src/view/components/object_table.rs
//! ============================================================================
//! # ObjectTable: Advanced Filesystem Table Component
//!
//! Renders a live directory table using PaneState entries.
//! - Fully async-updatable, selection-aware
//! - Handles directories, symlinks, files, and custom types
//! - Shows keymap in the footer, all using ratatui v0.25+
//! - Visual cues for type, selection, and focus

use crate::{
    model::app_state::AppState,
    view::{icons, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style, Stylize},
    widgets::{Block, Borders, Cell, HighlightSpacing, Row, Table, TableState},
};

pub struct ObjectTable;

impl ObjectTable {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let pane = &app.fs.panes[app.fs.active_pane];

        let header = Row::new(vec!["Name", "Type", "Items", "Size", "Modified"])
            .style(Style::default().fg(theme::YELLOW).bold())
            .bottom_margin(1);

        let rows = pane.entries.iter().map(|obj| {
            let (icon, style, type_str) = if obj.is_dir {
                (icons::FOLDER_ICON, Style::default().fg(theme::CYAN), "Dir")
            } else if obj.is_symlink {
                (
                    icons::SYMLINK_ICON,
                    Style::default().fg(theme::PINK),
                    "Symlink",
                )
            } else {
                (
                    icons::FILE_ICON,
                    Style::default().fg(theme::FOREGROUND),
                    obj.extension.as_deref().unwrap_or("File"),
                )
            };

            let items_str = if obj.is_dir {
                if obj.items_count > 0 {
                    obj.items_count.to_string()
                } else {
                    "-".to_string()
                }
            } else {
                String::new()
            };

            let size_str = if obj.is_dir {
                String::new()
            } else {
                obj.size_human()
            };

            let row = Row::new(vec![
                Cell::from(format!("{icon} {}", obj.name)),
                Cell::from(type_str.to_string()),
                Cell::from(items_str),
                Cell::from(size_str),
                Cell::from(obj.modified.format("%Y-%m-%d %H:%M").to_string()),
            ])
            .style(style);

            row
        });

        let widths = [
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(16),
        ];

        let mut table_state = TableState::default();
        table_state.select(app.ui.selected);

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", pane.cwd.display()))
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
            .highlight_spacing(HighlightSpacing::Always)
            .column_spacing(2);

        frame.render_stateful_widget(table, area, &mut table_state);
    }
}