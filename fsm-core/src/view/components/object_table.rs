//! `src/view/components/object_table.rs`
//! ===================================================================
//! Fully-compiling, allocation-light `ObjectTable` implementation.
//!
//! Fixes relative to the previous draft:
//! • use `calculate_required_height` (not `required_height`)
//! • fall back to `Layout::split` (Rc<[Rect]>) because
//!   `split_into` is only on ratatui 0.30+
//! • build header from `Cell`s, not `&str`
//! • pass `&*entry` from `DashMap` to `AdaptiveRow::from`
//! • avoid `extend_from_slice` (requires `Copy`); push manually
//! • drop hypothetical `cached_*` fields – use existing helpers
//! • build footer `Line` from `Vec<Span>` (`SmallVec` → `Vec`)
//! -------------------------------------------------------------------

use std::{ffi::OsStr, rc::Rc};

use crate::{
    fs::object_info::ObjectInfo,
    icons,
    model::{PaneState, object_registry::SortableEntry, shared_state::SharedState},
    theme,
    view::components::command_completion::{CommandCompletion, CompletionConfig},
};
use compact_str::CompactString;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, HighlightSpacing, Paragraph, Row, Table},
};
use smallvec::SmallVec;

// ── static column metadata ───────────────────────────────────────────
const NAME: &str = "Name";
const TYPE: &str = "Type";
const ITEMS: &str = "Items";
const SIZE: &str = "Size";
const MODIFIED: &str = "Modified";

static HEADERS_ULTRA: &[&str] = &[NAME];
static CONSTR_ULTRA: &[Constraint] = &[Constraint::Fill(1)];

static HEADERS_COMPACT: &[&str] = &[NAME, TYPE];
static CONSTR_COMPACT: &[Constraint] = &[Constraint::Fill(1), Constraint::Length(6)];

static HEADERS_NORMAL: &[&str] = &[NAME, TYPE, SIZE];
static CONSTR_NORMAL: &[Constraint] = &[
    Constraint::Fill(1),
    Constraint::Length(6),
    Constraint::Length(8),
];

static HEADERS_FULL: &[&str] = &[NAME, TYPE, ITEMS, SIZE, MODIFIED];
static CONSTR_FULL: &[Constraint] = &[
    Constraint::Fill(1),
    Constraint::Length(8),
    Constraint::Length(6),
    Constraint::Length(10),
    Constraint::Length(16),
];

#[inline]
fn layout_for(w: u16) -> (&'static [&'static str], &'static [Constraint]) {
    match w {
        0..=19 => (HEADERS_ULTRA, CONSTR_ULTRA),

        20..=39 => (HEADERS_COMPACT, CONSTR_COMPACT),

        40..=79 => (HEADERS_NORMAL, CONSTR_NORMAL),

        _ => (HEADERS_FULL, CONSTR_FULL),
    }
}

// ── static footer keys ───────────────────────────────────────────────
static HOTKEYS_COMPACT: &[(&str, &str)] = &[(":", "Cmd"), ("/", "Find"), ("h", "Help")];

static HOTKEYS_NORMAL: &[(&str, &str)] = &[
    (":", "Command"),
    ("/", "Search"),
    (":nf", "New File"),
    ("h", "Help"),
];

static HOTKEYS_FULL: &[(&str, &str)] = &[
    (":nf", "New File"),
    (":nd", "New Folder"),
    ("/", "File Search"),
    (":grep", "Content Search"),
    (":", "Command"),
    ("h", "Help"),
];

// ── ObjectTable entry type ───────────────────────────────────────────
pub struct ObjectTable;

impl ObjectTable {
    pub fn render(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        // 1) vertical split (Rc<[Rect]> unavoidable on ratatui 0.29)
        let (cmd_mode, cmd_height) = {
            let ui_guard = shared_state.lock_ui();
            let cmd_mode = ui_guard.is_in_command_mode() && area.width >= 40;

            let cmd_height = if cmd_mode {
                let cfg: CompletionConfig = CompletionConfig::default();

                CommandCompletion::calculate_required_height(&ui_guard.command_palette, &cfg)
            } else {
                0
            };

            (cmd_mode, cmd_height)
        };

        let regions: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(cmd_height),
                Constraint::Length(1),
            ])
            .split(area);

        let table_area: Rect = regions[0];
        let command_area: Option<Rect> = if cmd_mode { Some(regions[1]) } else { None };
        let footer_area: Rect = regions[2];

        // 2) viewport preparation - get pane info with fine-grained locking
        let active_pane_idx = {
            let mut fs_guard = shared_state.lock_fs();
            let active_pane_idx = fs_guard.active_pane;
            let pane = &mut fs_guard.panes[active_pane_idx];
            pane.set_viewport_height(table_area.height.saturating_sub(3) as usize);
            active_pane_idx
        };

        // 3) column metadata and header
        let (headers, widths): (&'static [&'static str], &'static [Constraint]) =
            layout_for(table_area.width);

        let header_cells: Vec<Cell<'_>> = headers
            .iter()
            .map(|h: &&str| -> Cell<'_> { Cell::from(*h).style(Style::default().bold()) })
            .collect::<Vec<_>>();

        let header_row: Row<'_> = Row::new(header_cells)
            .style(Style::default().fg(theme::YELLOW))
            .bottom_margin(1);

        // 4) rows - build from entries with metadata lookup
        let rows = {
            let fs_guard = shared_state.lock_fs();
            let pane = &fs_guard.panes[active_pane_idx];

            pane.visible_entries()
                .iter()
                .filter_map(|e: &SortableEntry| -> Option<std::sync::Arc<ObjectInfo>> {
                    shared_state.metadata.get_by_id(e.id)
                })
                .map(|entry: std::sync::Arc<ObjectInfo>| -> Row<'static> {
                    AdaptiveRow::build(&entry, table_area.width)
                })
                .collect::<Vec<Row<'static>>>()
        };

        // 5) table
        let title: String = {
            let fs_guard = shared_state.lock_fs();
            let pane = &fs_guard.panes[active_pane_idx];
            AdaptiveTitle::make(pane, table_area.width)
        };
        let table: Table<'_> = Table::new(rows, widths)
            .header(header_row)
            .block(
                Block::default()
                    .borders(if table_area.height > 2 {
                        Borders::ALL
                    } else {
                        Borders::NONE
                    })
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
            .highlight_symbol(if table_area.width < 20 { ">" } else { "▶ " })
            .highlight_spacing(HighlightSpacing::Always)
            .column_spacing(if table_area.width < 40 { 1 } else { 2 });

        // Render with mutable access to table state
        {
            let mut fs_guard = shared_state.lock_fs();
            let pane = &mut fs_guard.panes[active_pane_idx];
            frame.render_stateful_widget(table, table_area, &mut pane.table_state);
        }

        // 6) command palette
        if let Some(cmd) = command_area {
            let cfg: CompletionConfig = CompletionConfig::default();
            CommandCompletion::render_command_interface(frame, shared_state, cmd, &cfg);
        }

        // 7) footer
        AdaptiveFooter::render(frame, footer_area);
    }
}

// ── adaptive row -----------------------------------------------------
struct AdaptiveRow;

impl AdaptiveRow {
    #[inline]
    fn build(obj: &ObjectInfo, width: u16) -> Row<'static> {
        let (icon, style, typ) = if obj.is_dir {
            (
                icons::FOLDER_ICON,
                Style::default().fg(theme::CYAN),
                "Dir".to_string(),
            )
        } else if obj.is_symlink {
            (
                icons::SYMLINK_ICON,
                Style::default().fg(theme::PINK),
                "Link".to_string(),
            )
        } else {
            let extension: String = obj.extension.clone().map_or_else(
                || -> String { "File".to_string() },
                |ext: CompactString| -> String { ext.to_string() },
            );

            (
                icons::FILE_ICON,
                Style::default().fg(theme::FOREGROUND),
                extension,
            )
        };

        let mut cells: SmallVec<[Cell<'_>; 5]> = SmallVec::new();

        match width {
            0..=19 => {
                // icon + truncated name
                let max: usize = (width as usize).saturating_sub(2);
                let truncated: String = obj.name.chars().take(max).collect::<String>();
                cells.push(Cell::from(format!("{icon} {truncated}")));
            }

            20..=39 => {
                cells.push(Cell::from(format!(
                    "{icon} {}",
                    &obj.name[..obj.name.len().min(15)]
                )));
                cells.push(Cell::from(typ[..typ.len().min(4)].to_string()));
            }

            40..=79 => {
                cells.push(Cell::from(format!("{icon} {}", obj.name)));
                cells.push(Cell::from(typ));
                cells.push(Cell::from(if obj.is_dir {
                    String::new()
                } else {
                    obj.size_human()
                }));
            }

            _ => {
                let items = if obj.is_dir {
                    if obj.items_count > 0 {
                        obj.items_count.to_string()
                    } else {
                        "-".into()
                    }
                } else {
                    String::new()
                };
                let size = if obj.is_dir {
                    String::new()
                } else {
                    obj.size_human()
                };
                cells.push(Cell::from(format!("{icon} {}", obj.name)));
                cells.push(Cell::from(typ));
                cells.push(Cell::from(items));
                cells.push(Cell::from(size));
                cells.push(Cell::from(obj.format_date("%d/%m/%y %H:%M")));
            }
        }

        Row::new(cells).style(style)
    }
}

// ── adaptive title ---------------------------------------------------
struct AdaptiveTitle;

impl AdaptiveTitle {
    fn make(pane: &PaneState, w: u16) -> String {
        let total: usize = pane.entries.len();
        let visible: usize = pane.visible_entries().len();
        let idx: usize = pane.scroll_offset + 1;

        match w {
            0..=19 => pane
                .cwd
                .file_name()
                .and_then(|n: &OsStr| -> Option<&str> { n.to_str() })
                .unwrap_or("?")
                .to_string(),

            20..=39 => format!(
                " {} ",
                pane.cwd.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            ),

            40..=79 => {
                if total > visible {
                    format!(
                        " {} ({idx}/{total}) ",
                        pane.cwd.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                    )
                } else {
                    format!(" {} ", pane.cwd.display())
                }
            }

            _ => {
                if total > visible {
                    format!(" {} ({idx}/{total}) ", pane.cwd.display())
                } else {
                    format!(" {} ", pane.cwd.display())
                }
            }
        }
    }
}

// ── adaptive footer --------------------------------------------------
struct AdaptiveFooter;

impl AdaptiveFooter {
    fn render(frame: &mut Frame<'_>, area: Rect) {
        if area.width < 20 || area.height == 0 {
            return;
        }

        let keys = match area.width {
            20..=39 => HOTKEYS_COMPACT,
            40..=79 => HOTKEYS_NORMAL,
            _ => HOTKEYS_FULL,
        };

        let mut spans: Vec<Span<'static>> = Vec::with_capacity(keys.len() * 3);

        for (i, (key, desc)) in keys.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    " | ",
                    Style::default()
                        .fg(theme::PURPLE)
                        .add_modifier(Modifier::DIM),
                ));
            }

            spans.push(Span::styled(
                *key,
                Style::default()
                    .fg(theme::PURPLE)
                    .add_modifier(Modifier::BOLD),
            ));

            spans.push(Span::styled(
                format!(" {desc}"),
                Style::default()
                    .fg(theme::PURPLE)
                    .add_modifier(Modifier::DIM),
            ));
        }

        let line: Line<'_> = Line::from(spans);
        let para: Paragraph<'_> =
            Paragraph::new(line).style(Style::default().bg(theme::BACKGROUND));

        frame.render_widget(para, area);
    }
}
