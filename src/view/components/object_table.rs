//! src/view/components/object_table.rs
//! ============================================================================
//! # ObjectTable: Advanced Filesystem Table Component
//!
//! Uses ratatui's latest Table API. Displays name, type, item count, size, and last modified.
//! Includes a flexible ObjectType enum for easy extension and formatting.

use crate::model::fs_state::{ObjectType, PaneState};
use crate::{fs::object_info::ObjectInfo, model::app_state::AppState};

use ratatui::widgets::TableState;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    widgets::{Block, Row, Table},
};

/// ObjectTable: draws a table with full metadata using AppState/FSState.
pub struct ObjectTable;

impl ObjectTable {
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let pane: &PaneState = &app.fs.panes[app.fs.active_pane];

        // Header row
        let header: Row<'_> = Row::new(vec!["Name", "Ext", "Items", "Size", "Last Mod."])
            .style(Style::new().bold())
            .bottom_margin(1);

        // Keymap footer row
        let keymap: String = [
            "[F1] Help",
            "[q] Quit",
            "[←/→] Switch Pane",
            "[↑/↓] Navigate",
            "[Enter] Open",
            "[d] Delete",
            "[r] Rename",
            "[/] Search",
        ]
        .join("   ");
        let footer: Row<'_> = Row::new(vec![keymap])
            .style(Style::new().italic())
            .bottom_margin(0);

        // Table rows: handle directories, files, symlinks, etc.
        let rows: Vec<Row> = pane
            .entries
            .iter()
            .map(|obj: &ObjectInfo| {
                let obj_type: String = ObjectType::object_type(obj).to_string();

                let items: String = if obj.is_dir {
                    obj.items_count.to_string()
                } else {
                    String::new()
                };

                Row::new(vec![
                    obj.name.clone(),
                    obj_type,
                    items,
                    obj.size_human(),
                    obj.modified.to_string(),
                ])
            })
            .collect();

        // Column widths
        let widths: [Constraint; 5] = [
            Constraint::Percentage(35), // Name
            Constraint::Length(7),      // Ext/type
            Constraint::Length(7),      // Items
            Constraint::Percentage(15), // Size
            Constraint::Percentage(25), // Last Mod.
        ];

        // Build table
        let table: Table<'_> = Table::new(rows, widths)
            .column_spacing(1)
            .style(Style::new())
            .header(header)
            .footer(footer)
            .block(Block::new().title("Objects"))
            .row_highlight_style(Style::new().reversed())
            .column_highlight_style(Style::new().red())
            .cell_highlight_style(Style::new().blue())
            .highlight_symbol(">> ");

        // Table selection
        let mut state: TableState = pane.table_state.clone();
        if let Some(selected) = app.ui.selected {
            state.select(Some(selected));
        }

        frame.render_stateful_widget(table, area, &mut state);
    }
}
