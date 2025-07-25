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
    view::{
        components::command_completion::{CommandCompletion, CompletionConfig},
        icons, theme,
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, HighlightSpacing, Paragraph, Row, Table},
};

pub struct ObjectTable;

impl ObjectTable {
    pub fn render(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
        // Split the area into table, command line (if active), and footer
        let constraints = if app.ui.is_in_command_mode() {
            // Use new completion system to calculate required height
            let config = CompletionConfig::default();
            let command_area_height =
                CommandCompletion::calculate_required_height(&app.ui.command_palette, &config);

            vec![
                Constraint::Fill(1),                     // Table area
                Constraint::Length(command_area_height), // Command line area (dynamic)
                Constraint::Length(1),                   // Footer area
            ]
        } else {
            vec![
                Constraint::Fill(1),   // Table area
                Constraint::Length(1), // Footer area
            ]
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let table_area = layout[0];
        let (command_area, footer_area) = if app.ui.is_in_command_mode() {
            (Some(layout[1]), layout[2])
        } else {
            (None, layout[1])
        };

        let pane = &mut app.fs.panes[app.fs.active_pane];

        // Update viewport height based on available area (account for borders, header, and footer)
        let content_height = table_area.height.saturating_sub(3); // Account for borders and header
        pane.set_viewport_height(content_height as usize);

        let header = Row::new(vec!["Name", "Type", "Items", "Size", "Modified"])
            .style(Style::default().fg(theme::YELLOW).bold())
            .bottom_margin(1);

        // Use virtual scrolling - only render visible entries
        let visible_entries = pane.visible_entries();
        let total_entries = pane.entries.len();

        let rows = visible_entries.iter().map(|obj| {
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

            Row::new(vec![
                Cell::from(format!("{icon} {}", obj.name)),
                Cell::from(type_str.to_string()),
                Cell::from(items_str),
                Cell::from(size_str),
                Cell::from(obj.modified.format("%d/%m/%Y %I:%M:%S %p").to_string()),
            ])
            .style(style)
        });

        let widths = [
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(24),
        ];

        // Get the table state from the pane (already set up for virtual scrolling)
        let mut table_state = pane.table_state.clone();

        // Update title to show scroll position for large directories
        let title = if total_entries > visible_entries.len() {
            format!(
                " {} ({}/{}) ",
                pane.cwd.display(),
                pane.scroll_offset + 1,
                total_entries
            )
        } else {
            format!(" {} ", pane.cwd.display())
        };

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
            .highlight_spacing(HighlightSpacing::Always)
            .column_spacing(2);

        frame.render_stateful_widget(table, table_area, &mut table_state);

        // Render command line if in command mode using new completion system
        if let Some(cmd_area) = command_area {
            let config = CompletionConfig::default();
            CommandCompletion::render_command_interface(frame, app, cmd_area, &config);
        }

        // Render footer with hotkeys
        Self::render_footer(frame, footer_area);

        // Update the pane's table state
        let pane = &mut app.fs.panes[app.fs.active_pane];
        pane.table_state = table_state;
    }

    // Command line rendering is now handled by the dedicated CommandCompletion module

    /// Renders the footer bar with hotkey information using dark purple theme
    fn render_footer(frame: &mut Frame<'_>, area: Rect) {
        // Create hotkey spans with command-line focused styling
        let hotkeys = [
            (":nf", "New File"),
            (":nd", "New Folder"),
            (":reload", "Reload Dir"),
            ("/", "File Search"),
            (":grep", "Content Search"),
            (":", "Command Mode"),
            ("h", "Help"),
        ];

        let mut spans = Vec::new();
        for (i, (key, desc)) in hotkeys.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    " │ ",
                    Style::default()
                        .fg(theme::PURPLE)
                        .add_modifier(Modifier::DIM),
                ));
            }

            // Key in bold purple
            spans.push(Span::styled(
                key.to_string(),
                Style::default()
                    .fg(theme::PURPLE)
                    .add_modifier(Modifier::BOLD),
            ));

            // Description in dimmed purple
            spans.push(Span::styled(
                format!(" {desc}"),
                Style::default()
                    .fg(theme::PURPLE)
                    .add_modifier(Modifier::DIM),
            ));
        }

        let footer_line = Line::from(spans);
        let footer = Paragraph::new(footer_line).style(Style::default().bg(theme::BACKGROUND));

        frame.render_widget(footer, area);
    }
}
