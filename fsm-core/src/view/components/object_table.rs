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
        info!(
            target: "fsm_core::view::components::object_table",
            marker = "UI_COMPONENT_INIT",
            operation_type = "ui_render",
            component = "OptimizedFileTable",
            message = "Creating new OptimizedFileTable component"
        );
        Self
    }

    /// Render the file table using a [`PaneState`].  The `PaneState` holds
    /// directory entries, sorting and selection state.  The selected index
    /// is loaded atomically to configure the table’s highlight row.  The
    /// provided `path` is used for the table title.
    #[instrument(
        level = "trace",
        skip_all,
        fields(
            marker = "UI_RENDER_START",
            operation_type = "file_table_render",
            entries_count = pane_state.entries.len(),
            current_path = %path.display(),
            selected_index = pane_state.selected.load(Ordering::Relaxed),
            area_width = area.width,
            area_height = area.height,
            message = "File table render initiated"
        )
    )]
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
            target: "fsm_core::view::components::object_table",
            marker = "UI_RENDER_START",
            operation_type = "file_table_render",
            entries_count = entries.len(),
            selected_index = selected_index,
            current_path = %path.display(),
            area_width = area.width,
            area_height = area.height,
            message = format!("File table with {} entries for {}", entries.len(), path.display())
        );

        trace!(
            target: "fsm_core::view::components::object_table",
            marker = "UI_TABLE_AREA_INFO",
            operation_type = "ui_render",
            area_width = area.width,
            area_height = area.height,
            message = format!("Table area: {}x{}", area.width, area.height)
        );

        // Header row with column names
        let header = Row::new(vec!["Name", "Size", "Count", "Modified"])
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

                let (size_str, count_str) = if obj.is_dir {
                    let size_display = if obj.metadata_loaded && obj.items_count > 0 {
                        if obj.items_count == 1 {
                            "1 item".to_string()
                        } else {
                            format!("{} items", obj.items_count)
                        }
                    } else if obj.metadata_loaded {
                        "empty".to_string()
                    } else {
                        "-".to_string()
                    };
                    (size_display, "-".to_string()) // Directories show '-' in Count column
                } else {
                    let size_display = if obj.metadata_loaded {
                        crate::util::humanize::human_readable_size(obj.size)
                    } else {
                        "-".to_string()
                    };
                    (size_display, "1".to_string()) // Files always count as 1
                };

                Row::new(vec![
                    Cell::from(format!("{icon} {}", obj.name)),
                    Cell::from(size_str),
                    Cell::from(count_str),
                    Cell::from(obj.modified.format("%d/%m/%Y %I:%M %p").to_string()),
                ])
                .style(style)
            })
            .collect();

        let row_build_time_us = row_build_start.elapsed().as_micros();

        // Count metadata loading status for live update tracking
        let metadata_loaded_count = entries.iter().filter(|e| e.metadata_loaded).count();
        let pending_metadata_count = entries.len() - metadata_loaded_count;

        trace!(
            target: "fsm_core::view::components::object_table",
            marker = "UI_TABLE_ROW_BUILD_COMPLETE",
            operation_type = "ui_render",
            duration_us = row_build_time_us,
            dirs_count = dirs_count,
            files_count = files_count,
            symlinks_count = symlinks_count,
            total_rows = rows.len(),
            metadata_loaded_count = metadata_loaded_count,
            pending_metadata_count = pending_metadata_count,
            message = "Row construction completed with live update tracking"
        );

        // Column widths
        let widths = [
            Constraint::Fill(1),    // Name
            Constraint::Length(10), // Size
            Constraint::Length(8),  // Count
            Constraint::Length(22), // Modified
        ];

        // Table selection state based on pane's selected index
        let mut table_state = TableState::default().with_selected(Some(selected_index));

        debug!(
            target: "fsm_core::view::components::object_table",
            marker = "UI_TABLE_SELECTION_CONFIGURED",
            operation_type = "ui_render",
            selected_index = selected_index,
            entries_count = entries.len(),
            selection_valid = selected_index < entries.len(),
            message = "Table selection state configured"
        );

        // Table title shows the current path
        let title = format!(" {} ", path.display());

        trace!(
            target: "fsm_core::view::components::object_table",
            title = %title,
            path = %path.display(),
            "Table title generated from current path"
        );

        // Clear the area with explicit fill to prevent UI corruption during navigation
        info!(
            target: "fsm_core::view::components::object_table",
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
            target: "fsm_core::view::components::object_table",
            marker = "UI_FILE_TABLE_RENDERED",
            entries_count = entries.len(),
            selected_index = selected_index,
            "=== UI COMPLETE: Table rendering finished ==="
        );

        let render_time_us = render_start.elapsed().as_micros();
        trace!(
            target: "fsm_core::view::components::object_table",
            render_time_us = render_time_us,
            entries_count = entries.len(),
            selected_index = selected_index,
            dirs_count = dirs_count,
            files_count = files_count,
            symlinks_count = symlinks_count,
            metadata_loaded_count = metadata_loaded_count,
            pending_metadata_count = pending_metadata_count,
            live_update_progress = if !entries.is_empty() {
                (metadata_loaded_count as f32 / entries.len() as f32 * 100.0) as u32
            } else {
                100
            },
            table_area = format!("{}x{}", area.width, area.height),
            cwd = %path.display(),
            "File table render completed with live update progress"
        );

        // performance monitoring and alerting
        if render_time_us > 10000 {
            warn!(
                target: "fsm_core::view::components::object_table",
                marker = "UI_RENDER_SLOW",
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
                target: "fsm_core::view::components::object_table",
                marker = "LARGE_DIRECTORY_RENDERED",
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
                target: "fsm_core::view::components::object_table",
                marker = "SELECTION_OUT_OF_BOUNDS",
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
