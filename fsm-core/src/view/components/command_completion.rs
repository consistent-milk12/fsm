//! ``src/view/components/command_completion.rs``
//! ============================================================================
//! # Command Completion: Advanced Auto-completion System
//!
//! Production-quality command auto-completion with flexible rendering,
//! extensible command definitions, and robust UI handling for single/multiple matches.
//! This is a core feature for command-line driven interaction.

use crate::{
    model::{app_state::AppState, command_palette::get_command_description},
    view::theme,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

/// Configuration for completion rendering behavior
#[derive(Debug, Clone)]
pub struct CompletionConfig {
    /// Maximum completions to show at once
    pub max_visible: usize,
    /// Minimum height for completion area (ensures single completions show)
    pub min_height: u16,
    /// Show descriptions for commands
    pub show_descriptions: bool,
    /// Show navigation hints
    pub show_nav_hints: bool,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            max_visible: 6,
            min_height: 3, // Minimum 3 lines: border + 1 completion + border
            show_descriptions: true,
            show_nav_hints: true,
        }
    }
}

/// Main command completion renderer
pub struct CommandCompletion;

impl CommandCompletion {
    #[allow(clippy::cast_possible_truncation)]
    /// Render the complete command line with auto-completion
    /// Returns the area used for the command line
    pub fn render_command_interface(
        frame: &mut Frame<'_>,
        app: &AppState,
        area: Rect,
        config: &CompletionConfig,
    ) -> Rect {
        let palette = &app.ui.command_palette;
        let input = &palette.input;
        let command_text = format!(":{input}");

        // Calculate required height for completions
        let completion_height = Self::calculate_completion_height(palette, config);

        // Split area based on whether completions should be shown
        if palette.show_completions && !palette.completions.is_empty() {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),                 // Command line
                    Constraint::Length(completion_height), // Completions (dynamic)
                ])
                .split(area);

            // Render command line
            Self::render_command_line(frame, &command_text, layout[0]);

            // Set cursor position
            let cursor_x = (layout[0].x + format!(":{input}").len() as u16)
                .min(layout[0].x + layout[0].width - 1);
            frame.set_cursor_position((cursor_x, layout[0].y));

            // Render completions
            Self::render_completions(frame, app, layout[1], config);

            area // Return full area used
        } else {
            // Just command line, no completions
            Self::render_command_line(frame, &command_text, area);

            // Set cursor position
            let cursor_x = (area.x + command_text.len() as u16).min(area.x + area.width - 1);
            frame.set_cursor_position((cursor_x, area.y));

            // Return just the command line area
            Rect { height: 1, ..area }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    /// Calculate the optimal height for completion display
    fn calculate_completion_height(
        palette: &crate::model::command_palette::CommandPaletteState,
        config: &CompletionConfig,
    ) -> u16 {
        if palette.completions.is_empty() {
            return 0;
        }

        let completion_count = palette.completions.len();
        let visible_completions = completion_count.min(config.max_visible);

        // Height = borders (2) + visible completions + optional nav hints
        let base_height = 2 + visible_completions as u16;

        // Ensure minimum height for single completions
        base_height.max(config.min_height)
    }

    /// Render just the command line input
    fn render_command_line(frame: &mut Frame<'_>, command_text: &str, area: Rect) {
        let command_line = Paragraph::new(command_text).style(
            Style::default()
                .bg(theme::BACKGROUND)
                .fg(theme::PURPLE)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(command_line, area);
    }

    /// Render completion suggestions with advanced UI
    fn render_completions(
        frame: &mut Frame<'_>,
        app: &AppState,
        area: Rect,
        config: &CompletionConfig,
    ) {
        let palette = &app.ui.command_palette;

        // Early return check with debug
        if palette.completions.is_empty() {
            tracing::debug!("render_completions: empty completions, area: {:?}", area);
            return;
        }

        tracing::debug!(
            "render_completions: {} completions, area: {:?}, min_height: {}",
            palette.completions.len(),
            area,
            config.min_height
        );

        // Calculate scrolling parameters
        let completion_count = palette.completions.len();
        let max_visible = config.max_visible;
        let selected_idx = palette.completion_index;

        // Ensure we have enough space
        if area.height < config.min_height {
            tracing::info!(
                "Insufficient space for completions: {} < {}",
                area.height,
                config.min_height
            );
            // Still try to render with available space
        }

        // Calculate scroll window
        let (scroll_offset, visible_count) = Self::calculate_scroll_window(
            completion_count,
            selected_idx,
            max_visible,
            area.height.saturating_sub(2) as usize, // Account for borders
        );

        let displayed_completions =
            &palette.completions[scroll_offset..scroll_offset + visible_count];

        // Create completion list items
        let completion_items: Vec<ListItem> = displayed_completions
            .iter()
            .enumerate()
            .map(|(display_idx, completion)| {
                let actual_idx = scroll_offset + display_idx;
                let is_selected = actual_idx == selected_idx;

                Self::create_completion_item(completion, is_selected, config)
            })
            .collect();

        // Create title with completion info
        let title = Self::create_completion_title(
            completion_count,
            selected_idx,
            scroll_offset,
            visible_count,
            config,
        );

        let item_count = completion_items.len();

        // Render the completion list
        let completions_list = List::new(completion_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(Alignment::Left)
                .border_style(Style::default().fg(theme::CYAN))
                .style(Style::default().bg(theme::BACKGROUND)),
        );

        frame.render_widget(completions_list, area);

        tracing::debug!("Successfully rendered {} completion items", item_count);
    }

    /// Calculate optimal scroll window for completions
    fn calculate_scroll_window(
        total_count: usize,
        selected_idx: usize,
        max_visible: usize,
        available_lines: usize,
    ) -> (usize, usize) {
        let effective_max = max_visible.min(available_lines).max(1);

        if total_count <= effective_max {
            // All completions fit
            (0, total_count)
        } else {
            // Need scrolling
            let scroll_offset = if selected_idx < effective_max / 2 {
                0
            } else if selected_idx >= total_count - effective_max / 2 {
                total_count - effective_max
            } else {
                selected_idx - effective_max / 2
            };

            let visible_count = (total_count - scroll_offset).min(effective_max);
            (scroll_offset, visible_count)
        }
    }

    /// Create a single completion list item
    fn create_completion_item(
        completion: &str,
        is_selected: bool,
        config: &CompletionConfig,
    ) -> ListItem<'static> {
        let (prefix, style) = if is_selected {
            (
                "▶ ",
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .fg(theme::FOREGROUND)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                "  ",
                Style::default()
                    .fg(theme::COMMENT)
                    .add_modifier(Modifier::DIM),
            )
        };

        let display_text: String = if config.show_descriptions {
            get_command_description(completion)
            .map_or_else(
                || -> String 
                {format!("{prefix}{completion}")},
                 |desc: &'static str| -> String 
                 {format!("{prefix}{completion:<12} - {desc}")})
        } else {
            format!("{prefix}{completion}")
        };

        ListItem::new(display_text).style(style)
    }

    /// Create completion title with navigation info
    fn create_completion_title(
        total_count: usize,
        selected_idx: usize,
        scroll_offset: usize,
        visible_count: usize,
        config: &CompletionConfig,
    ) -> String {
        let base_title = if total_count == 1 {
            " 1 completion ".to_string()
        } else {
            format!(" {total_count} completions ")
        };

        if !config.show_nav_hints {
            return base_title;
        }

        if total_count <= config.max_visible {
            // All visible, just show navigation hint
            format!("{base_title}(↑↓ navigate, Tab apply) ")
        } else {
            // Show scroll indicators and position
            let can_scroll_up = scroll_offset > 0;
            let can_scroll_down = scroll_offset + visible_count < total_count;

            let scroll_indicators = match (can_scroll_up, can_scroll_down) {
                (true, true) => " ↕ ",
                (true, false) => " ↑ ",
                (false, true) => " ↓ ",
                (false, false) => " ",
            };

            format!(
                "{scroll_indicators}{}/{}{scroll_indicators}(↑↓ navigate, Tab apply) ",
                selected_idx + 1,
                total_count
            )
        }
    }

    #[must_use]
    /// Calculate the total area needed for command interface
    pub fn calculate_required_height(
        palette: &crate::model::command_palette::CommandPaletteState,
        config: &CompletionConfig,
    ) -> u16 {
        let command_line_height = 1;
        let completion_height = if palette.show_completions && !palette.completions.is_empty() {
            Self::calculate_completion_height(palette, config)
        } else {
            0
        };

        command_line_height + completion_height
    }
}

/// Convenience function for default completion rendering
pub fn render_command_with_completions(frame: &mut Frame<'_>, app: &AppState, area: Rect) -> Rect {
    let config = CompletionConfig::default();
    CommandCompletion::render_command_interface(frame, app, area, &config)
}
