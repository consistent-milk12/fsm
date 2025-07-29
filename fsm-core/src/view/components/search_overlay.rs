//! src/view/components/search_overlay.rs
//! ============================================================
//! Search / filter overlay that is **lock-free**: it consumes a
//! pre-built `SearchSnapshot` (prepared by the renderer) and draws
//! the search box plus a small preview list of results.

use ratatui::{
    prelude::*,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use smallvec::SmallVec;
use tracing::{debug, instrument, trace};

use crate::model::ui_state::UIOverlay;
use crate::view::snapshots::SearchSnapshot;
use crate::view::theme; // colour constants

/// Paints a search bar + live results preview.
pub struct OptimizedSearchOverlay {
    overlay_type: UIOverlay,
}

impl OptimizedSearchOverlay {
    pub fn new(overlay_type: UIOverlay) -> Self {
        debug!("Creating new search overlay: {:?}", overlay_type);
        Self { overlay_type }
    }

    // ---------------------------------------------------------
    // Public API called by the renderer
    // ---------------------------------------------------------
    #[instrument(level = "trace", skip_all, fields(overlay_type = ?self.overlay_type, query_len = snap.query.len(), result_count = snap.results.len()))]
    pub fn render_with_input(&self, frame: &mut Frame<'_>, snap: &SearchSnapshot, rect: Rect) {
        trace!(
            "Rendering search overlay with {} results",
            snap.results.len()
        );
        // clear background -----------------------------------
        trace!("Clearing search overlay background");
        frame.render_widget(Clear, rect);

        // title differs per overlay kind ---------------------
        let title = match self.overlay_type {
            UIOverlay::FileNameSearch => " File Name Search ",
            UIOverlay::ContentSearch => " Content Search ",
            _ => " Search ",
        };

        // split: input line + results list -------------------
        let [input_rect, res_rect] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(rect);

        self.draw_input_field(frame, snap, input_rect, title);

        if res_rect.height >= 3 {
            if !snap.results.is_empty() && !snap.query.is_empty() {
                debug!("Drawing {} search results", snap.results.len());
                self.draw_results(frame, snap, res_rect);
            } else if !snap.query.is_empty() {
                self.draw_help(frame, res_rect);
            }
        }
    }

    // ---------------------------------------------------------
    // Input line with blinking cursor
    // ---------------------------------------------------------
    fn draw_input_field(
        &self,
        frame: &mut Frame<'_>,
        snap: &SearchSnapshot,
        rect: Rect,
        title: &str,
    ) {
        let mut buf = snap.query.to_string();
        let cur = snap.cursor.min(buf.len());
        if cur == buf.len() {
            buf.push('│');
        } else {
            buf.insert(cur, '│');
        }

        frame.render_widget(
            Paragraph::new(buf)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .border_style(Style::default().fg(theme::CYAN))
                        .style(Style::default().bg(theme::BACKGROUND)),
                )
                .style(Style::default().fg(theme::FOREGROUND)),
            rect,
        );
    }

    // ---------------------------------------------------------
    // Results preview (first ≤10 hits)
    // ---------------------------------------------------------
    fn draw_results(&self, frame: &mut Frame<'_>, snap: &SearchSnapshot, rect: Rect) {
        let total = snap.results.len();
        let title = format!(" {total} Result(s) ");

        let items: SmallVec<[ListItem; 10]> = snap
            .results
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, obj)| {
                let fname = obj.name.as_str();
                let dir = obj
                    .path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();

                // build a single line
                let line = if dir.is_empty() {
                    Line::from(vec![
                        Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                        Span::styled(fname, Style::default().fg(theme::FOREGROUND)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                        Span::styled(fname, Style::default().fg(theme::FOREGROUND)),
                        Span::raw(" in "),
                        Span::styled(dir, Style::default().fg(theme::PURPLE)),
                    ])
                };

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND)),
            )
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(list, rect);

        // “…and more” footer if truncated --------------------
        if total > 10 && rect.height >= 2 {
            let footer = Paragraph::new(format!("… and {} more", total - 10))
                .style(Style::default().fg(theme::COMMENT));
            let footer_rect = Rect {
                x: rect.x + 2,
                y: rect.y + rect.height - 2,
                width: rect.width.saturating_sub(4),
                height: 1,
            };
            frame.render_widget(footer, footer_rect);
        }
    }

    // ---------------------------------------------------------
    // Help panel when there is no match yet
    // ---------------------------------------------------------
    fn draw_help(&self, frame: &mut Frame<'_>, rect: Rect) {
        let lines = vec![
            Line::from("Start typing to search…"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Tips:",
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from("• Use * wildcards (*.rs, test*)"),
            Line::from("• Use / for path segments (src/lib.rs)"),
            Line::from("• Enter → open first result"),
            Line::from("• Esc → cancel search"),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Search Help ")
                        .title_alignment(Alignment::Center)
                        .border_style(Style::default().fg(theme::COMMENT))
                        .style(Style::default().bg(theme::BACKGROUND)),
                )
                .style(Style::default().fg(theme::FOREGROUND)),
            rect,
        );
    }
}
