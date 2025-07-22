//! src/view/components/object_table.rs
//! ============================================================================
//! # ObjectTable: Advanced Filesystem Table Component
//!
//! Renders a live directory table using PaneState entries.
//! - Fully async-updatable, selection-aware
//! - Handles directories, symlinks, files, and custom types
//! - Shows keymap in the footer, all using ratatui v0.25+
//! - Visual cues for type, selection, and focus

use crate::model::app_state::AppState;
use crate::model::fs_state::{ObjectType, PaneState};

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Cell, Row, Table, TableState},
};

pub struct ObjectTable;

impl ObjectTable {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let pane: &PaneState = &app.fs.panes[app.fs.active_pane];

        // Table columns: Name, Type, Items, Size, Modified
        let header: Row<'_> = Row::new(vec!["Name", "Type", "Items", "Size", "Last Modified"])
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

        // Keymap footer, always visible for power users
        let keymap: String = [
            "[F1] Help",
            "[q] Quit",
            "[←/→] Pane",
            "[↑/↓] Nav",
            "[Enter] Open",
            "[d] Delete",
            "[r] Rename",
            "[/] Search",
        ]
        .join("   ");

        let footer: Row<'_> = Row::new(vec![keymap]).style(
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        );

        // Render each entry (ObjectInfo) as a table row
        let rows = pane.entries.iter().enumerate().map(|(_idx, obj)| {
            // Visual cues for directories, symlinks, etc.
            let style: Style = if obj.is_dir {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if obj.is_symlink {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default()
            };

            let obj_type: String = ObjectType::object_type(obj).to_string();
            let items: String = if obj.is_dir {
                obj.items_count.to_string()
            } else {
                String::new()
            };

            Row::new(vec![
                Cell::from(obj.name.clone()).style(style),
                Cell::from(obj_type),
                Cell::from(items),
                Cell::from(obj.size_human()),
                Cell::from(obj.modified.to_string()),
            ])
        });

        // Table column widths
        let widths: [Constraint; 5] = [
            Constraint::Percentage(35), // Name
            Constraint::Length(8),      // Type
            Constraint::Length(6),      // Items
            Constraint::Percentage(15), // Size
            Constraint::Percentage(25), // Modified
        ];

        let mut table_state: TableState = pane.table_state.clone();
        // Keep selection in sync with UIState (for navigation)
        if let Some(selected) = app.ui.selected {
            table_state.select(Some(selected));
        } else {
            table_state.select(None);
        }

        let table: Table<'_> = Table::new(rows, widths)
            .header(header)
            .footer(footer)
            .block(Block::default().title(format!(
                " {} — {} entries ",
                pane.cwd.display(),
                pane.entries.len()
            )))
            .row_highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut table_state);
    }
}
