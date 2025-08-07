//! ``src/view/components/status_bar.rs``
//!
//!  # `StatusBar`: Persistent Status/Info/Error Display (ratatui v0.24+)
//!
//! - Renders last error, status message, or hint at bottom of UI
//! - Uses `AppState`'s `last_error` and `last_status` fields
//! - Themed, immediate-mode, power-user friendly

use std::rc::Rc;

use crate::model::shared_state::SharedState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub struct StatusBar;

impl StatusBar {
    pub fn render_with_degradation(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        match area.width {
            0..=19 => Self::render_ultra_minimal(frame, shared_state, area),

            20..=39 => Self::render_compact(frame, shared_state, area),

            40..=79 => Self::render_normal(frame, shared_state, area),

            _ => Self::render_full(frame, shared_state, area),
        }
    }

    fn render_ultra_minimal(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        // 0-20 chars: Show only critical errors, no borders
        let app_guard = shared_state.lock_app();
        if let Some(error) = &app_guard.last_error {
            let para: Paragraph<'_> = Paragraph::new(format!(
                "ERR: {}",
                error
                    .chars()
                    .take(area.width as usize - 5)
                    .collect::<String>()
            ))
            .style(Style::default().fg(theme::RED));

            frame.render_widget(para, area);
        } else {
            let para: Paragraph<'_> = Paragraph::new("OK").style(Style::default().fg(theme::GREEN));

            frame.render_widget(para, area);
        }
    }

    fn render_compact(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let block: Block<'_> = if area.height > 1 {
            Block::default().borders(Borders::TOP)
        } else {
            Block::default()
        };

        frame.render_widget(block, area);

        let (msg, style) = Self::get_status_message(shared_state);
        let truncated: String = msg
            .chars()
            .take((area.width as usize).saturating_sub(2))
            .collect::<String>();

        let para: Paragraph<'_> = Paragraph::new(format!(" {truncated}"))
            .style(style)
            .alignment(Alignment::Left);

        frame.render_widget(para, area);
    }

    fn render_normal(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        // 20-40 chars: Status + item count, simple layout
        let block: Block<'_> = Block::default().borders(Borders::TOP);
        frame.render_widget(block, area);

        let (msg, style) = Self::get_status_message(shared_state);
        let fs_guard = shared_state.lock_fs();
        let item_count: String = format!(
            "{} items",
            fs_guard.panes[fs_guard.active_pane].entries.len()
        );

        let combined: String = format!(
            " {} | {}",
            msg.chars().take(30).collect::<String>(),
            item_count
        );

        let para: Paragraph<'_> = Paragraph::new(combined).style(style);
        frame.render_widget(para, area);
    }

    pub fn render_full(frame: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let status_block: Block<'_> = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme::COMMENT));

        frame.render_widget(status_block, area);

        let (msg, style) = {
            let app_guard = shared_state.lock_app();
            let ui_guard = shared_state.lock_ui();
            app_guard.last_error.as_ref().map_or_else(
                || -> (String, Style) {
                    ui_guard.last_status.as_ref().map_or_else(
                        || -> (String, Style) {
                            ("Ready".to_string(), Style::default().fg(theme::COMMENT))
                        },
                        |status: &String| -> (String, Style) {
                            (status.clone(), Style::default().fg(theme::GREEN))
                        },
                    )
                },
                |err: &String| -> (String, Style) {
                    (
                        format!("🔥 Error: {err}"),
                        Style::default().fg(theme::RED).bold(),
                    )
                },
            )
        };

        let chunks: Rc<[Rect]> = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .margin(0)
            .split(area);

        let left_para: Paragraph<'_> =
            Paragraph::new(Line::from(Span::styled(format!(" {msg} "), style)))
                .alignment(Alignment::Left);

        let fs_guard = shared_state.lock_fs();
        let right_text: String = format!(
            "{} items ",
            fs_guard.panes[fs_guard.active_pane].entries.len()
        );
        let right_para: Paragraph<'_> = Paragraph::new(Line::from(Span::styled(
            right_text,
            Style::default().fg(theme::PURPLE),
        )))
        .alignment(Alignment::Right);

        frame.render_widget(left_para, chunks[0]);
        frame.render_widget(right_para, chunks[1]);
    }

    fn get_status_message(shared_state: &SharedState) -> (String, Style) {
        // Extract existing message logic
        let app_guard = shared_state.lock_app();
        let ui_guard = shared_state.lock_ui();
        app_guard.last_error.as_ref().map_or_else(
            || -> (String, Style) {
                ui_guard.last_status.as_ref().map_or_else(
                    || -> (String, Style) {
                        ("Ready".to_string(), Style::default().fg(theme::COMMENT))
                    },
                    |status: &String| -> (String, Style) {
                        (status.clone(), Style::default().fg(theme::GREEN))
                    },
                )
            },
            |err: &String| -> (String, Style) {
                (
                    format!("Error: {err}"),
                    Style::default().fg(theme::RED).bold(),
                )
            },
        )
    }
}
