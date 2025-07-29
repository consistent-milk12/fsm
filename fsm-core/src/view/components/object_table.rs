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
    widgets::{Block, Borders, Cell, Clear, HighlightSpacing, Row, Table, TableState},
};
use std::path::Path;
use std::sync::atomic::Ordering;
use tracing::{debug, info, instrument, trace, warn};

pub struct OptimizedFileTable;

impl OptimizedFileTable {
    pub fn new() -> Self {
        debug!(
            target: "fsm_core::view::object_table",
            "Creating new OptimizedFileTable component"
        );
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
        let render_start = std::time::Instant::now();
        let entries = &pane_state.entries;
        let selected_index = pane_state.selected.load(Ordering::Relaxed);

        info!(
            target: "fsm_core::view::object_table",
            entries_count = entries.len(),
            selected_index = selected_index,
            cwd = %path.display(),
            area_width = area.width,
            area_height = area.height,
            "=== UI RENDER: File table with {} entries for {} ===",
            entries.len(),
            path.display()
        );

        trace!(
            target: "fsm_core::view::object_table",
            area_width = area.width,
            area_height = area.height,
            "Table area: {}x{}", area.width, area.height
        );

        // Header row with column names
        let header = Row::new(vec!["Name", "Size", "Modified"])
            .style(Style::default().fg(theme::YELLOW).bold())
            .bottom_margin(1);

        // Build rows for each entry with appropriate icons and styles
        let row_build_start = std::time::Instant::now();
        let mut dirs_count = 0;
        let mut files_count = 0;
        let mut symlinks_count = 0;

        let rows: Vec<Row> = entries
            .iter()
            .map(|obj| {
                let (icon, style) = if obj.is_dir {
                    dirs_count += 1;
                    (icons::FOLDER_ICON, Style::default().fg(theme::CYAN))
                } else if obj.is_symlink {
                    symlinks_count += 1;
                    (icons::SYMLINK_ICON, Style::default().fg(theme::PINK))
                } else {
                    files_count += 1;
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

        let row_build_time_us = row_build_start.elapsed().as_micros();
        trace!(
            target: "fsm_core::view::object_table",
            row_build_time_us = row_build_time_us,
            dirs_count = dirs_count,
            files_count = files_count,
            symlinks_count = symlinks_count,
            total_rows = rows.len(),
            "Row construction completed"
        );

        // Column widths
        let widths = [
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(22),
        ];

        // Table selection state based on pane's selected index
        let mut table_state = TableState::default().with_selected(Some(selected_index));

        debug!(
            target: "fsm_core::view::object_table",
            selected_index = selected_index,
            entries_count = entries.len(),
            selection_valid = selected_index < entries.len(),
            "Table selection state configured"
        );

        // Table title shows the current path
        let title = format!(" {} ", path.display());

        trace!(
            target: "fsm_core::view::object_table",
            title = %title,
            path = %path.display(),
            "Table title generated from current path"
        );

        // Clear the area with explicit fill to prevent UI corruption during navigation
        info!(
            target: "fsm_core::view::object_table",
            area_width = area.width,
            area_height = area.height,
            "=== UI CLEAR: Clearing area before rendering table ==="
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::BACKGROUND)),
            area,
        );

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

        info!(
            target: "fsm_core::view::object_table",
            entries_count = entries.len(),
            selected_index = selected_index,
            "=== UI COMPLETE: Table rendering finished ==="
        );

        let render_time_us = render_start.elapsed().as_micros();
        trace!(
            target: "fsm_core::view::object_table",
            render_time_us = render_time_us,
            entries_count = entries.len(),
            selected_index = selected_index,
            dirs_count = dirs_count,
            files_count = files_count,
            symlinks_count = symlinks_count,
            table_area = format!("{}x{}", area.width, area.height),
            cwd = %path.display(),
            "File table render completed"
        );

        // performance monitoring and alerting
        if render_time_us > 10000 {
            warn!(
                target: "fsm_core::view::object_table",
                render_time_us = render_time_us,
                entries_count = entries.len(),
                table_area = format!("{}x{}", area.width, area.height),
                cwd = %path.display(),
                "Slow file table render detected"
            );
        }

        // large directory handling monitoring
        if entries.len() > 1000 {
            debug!(
                target: "fsm_core::view::object_table",
                entries_count = entries.len(),
                render_time_us = render_time_us,
                dirs_count = dirs_count,
                files_count = files_count,
                symlinks_count = symlinks_count,
                cwd = %path.display(),
                "Large directory rendered"
            );
        }

        // selection boundary validation
        if selected_index >= entries.len() && !entries.is_empty() {
            warn!(
                target: "fsm_core::view::object_table",
                selected_index = selected_index,
                entries_count = entries.len(),
                cwd = %path.display(),
                "Selection index out of bounds detected"
            );
        }
    }
}

impl Default for OptimizedFileTable {
    fn default() -> Self {
        Self::new()
    }
}
