//! src/view/components/input_prompt_overlay.rs
//! ============================================================
//! Prompt / command-line overlay fully decoupled from `UIState`.
//!
//! It consumes an immutable `PromptSnapshot` (prepared by the
//! renderer after locks are released) so the widget never touches
//! shared state during drawing.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use smallvec::SmallVec;
use tracing::{debug, info, instrument, trace, warn};

use crate::controller::actions::InputPromptType;
use crate::view::snapshots::PromptSnapshot;
use crate::view::theme; // colour constants

pub struct OptimizedPromptOverlay;

impl OptimizedPromptOverlay {
    pub fn new() -> Self {
        debug!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_COMPONENT_INIT",
            component = "OptimizedPromptOverlay",
            message = "Creating new OptimizedPromptOverlay component"
        );
        Self
    }

    // ---------------------------------------------------------
    // Public entry
    // ---------------------------------------------------------
    #[instrument(
        level = "trace",
        skip_all,
        fields(
            marker = "INPUT_PROMPT_RENDER_START",
            operation_type = "input_prompt_render",
            prompt_type = ?snap.prompt_type,
            buffer_len = snap.buffer.len(),
            cursor_pos = snap.cursor,
            area_width = rect.width,
            area_height = rect.height,
            message = "Input prompt overlay render initiated"
        )
    )]
    pub fn render_input(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect) {
        let render_start = std::time::Instant::now();
        info!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "INPUT_PROMPT_RENDER_START",
            operation_type = "input_prompt_render",
            prompt_type = ?snap.prompt_type,
            buffer_len = snap.buffer.len(),
            cursor_pos = snap.cursor,
            area_width = rect.width,
            area_height = rect.height,
            message = "Input prompt overlay render initiated"
        );

        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            area_width = rect.width,
            area_height = rect.height,
            "Clearing background for input prompt overlay"
        );
        frame.render_widget(Clear, rect); // wipe bg

        // choose title by prompt-type -------------------------
        let title = Self::title_for(&snap.prompt_type);
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            prompt_type = ?snap.prompt_type,
            title = title,
            "Determined title for input prompt"
        );

        // command mode gets its own two-row layout ------------
        if matches!(&snap.prompt_type, InputPromptType::Custom(s)
                    if s == "command")
        {
            debug!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_MODE_ENTER",
                message = "Drawing command mode prompt"
            );
            self.draw_command_mode(frame, snap, rect, title);
        } else {
            debug!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "INPUT_PROMPT_SHOW",
                message = "Drawing standard input prompt"
            );
            self.draw_standard(frame, snap, rect, title);
        }

        let render_time_us = render_start.elapsed().as_micros();
        info!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_RENDER_COMPLETE",
            operation_type = "input_prompt_render",
            render_time_us = render_time_us,
            prompt_type = ?snap.prompt_type,
            buffer_len = snap.buffer.len(),
            area_width = rect.width,
            area_height = rect.height,
            message = "Input prompt overlay render completed"
        );

        if render_time_us > 5000 {
            warn!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "UI_RENDER_SLOW",
                render_time_us = render_time_us,
                area_size = format!("{}x{}", rect.width, rect.height),
                prompt_type = ?snap.prompt_type,
                message = "Slow input prompt overlay render detected"
            );
        }
    }

    // ---------------------------------------------------------
    // Standard (one-box) prompt
    // ---------------------------------------------------------
    fn draw_standard(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect, title: &str) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_DRAW_STANDARD_PROMPT",
            area_width = rect.width,
            area_height = rect.height,
            title = title,
            message = "Drawing standard input prompt UI"
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        // insert cursor glyph
        let mut buf = snap.buffer.to_string();
        if snap.cursor <= buf.len() {
            if snap.cursor == buf.len() {
                buf.push('│');
            } else {
                buf.insert(snap.cursor, '│');
            }
        }
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "PROMPT_BUFFER_UPDATE",
            buffer = %buf,
            cursor = snap.cursor,
            message = "Prompt buffer updated with cursor"
        );

        let para = Paragraph::new(buf)
            .block(block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(para, rect);

        // help line below box --------------------------------
        let help = Self::standard_help(&snap.prompt_type);
        let help_rect = Rect {
            x: rect.x,
            y: rect.y + rect.height,
            width: rect.width,
            height: 1,
        };
        if help_rect.y < frame.area().height {
            frame.render_widget(
                Paragraph::new(help)
                    .style(Style::default().fg(theme::COMMENT))
                    .alignment(Alignment::Center),
                help_rect,
            );
        }
    }

    // ---------------------------------------------------------
    // Command-mode prompt (input + suggestions/history)
    // ---------------------------------------------------------
    fn draw_command_mode(
        &self,
        frame: &mut Frame<'_>,
        snap: &PromptSnapshot,
        rect: Rect,
        title: &str,
    ) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_DRAW_COMMAND_MODE",
            area_width = rect.width,
            area_height = rect.height,
            title = title,
            message = "Drawing command mode UI"
        );
        // split: input row + suggestions/history -------------
        let [inp_row, bottom] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(rect);

        self.draw_input_field(frame, snap, inp_row, title);

        if bottom.height <= 2 {
            trace!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_MODE_NO_SPACE",
                bottom_height = bottom.height,
                message = "Not enough space for command mode suggestions/history"
            );
            return;
        }

        if snap.buffer.is_empty() {
            debug!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_MODE_HISTORY",
                message = "Drawing command history"
            );
            self.draw_history(frame, snap, bottom);
        } else {
            debug!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_MODE_AUTOCOMPLETE",
                message = "Drawing command autocomplete suggestions"
            );
            self.draw_autocomplete(frame, snap, bottom);
        }
    }

    // ---------------------------------------------------------
    // Input helper (shared by command-mode & standard)
    // ---------------------------------------------------------
    fn draw_input_field(
        &self,
        frame: &mut Frame<'_>,
        snap: &PromptSnapshot,
        rect: Rect,
        title: &str,
    ) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_DRAW_INPUT_FIELD",
            area_width = rect.width,
            area_height = rect.height,
            title = title,
            message = "Drawing input field"
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        // ":" + buffer + cursor
        let mut txt = format!(":{}", snap.buffer);
        let cur = 1 + snap.cursor; // ':' offset
        if cur <= txt.len() {
            if cur == txt.len() {
                txt.push('│');
            } else {
                txt.insert(cur, '│');
            }
        }
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "PROMPT_BUFFER_UPDATE",
            buffer = %txt,
            cursor = snap.cursor,
            message = "Input field buffer updated with cursor"
        );

        frame.render_widget(
            Paragraph::new(txt)
                .block(block)
                .style(Style::default().fg(theme::FOREGROUND)),
            rect,
        );
    }

    // ---------------------------------------------------------
    // Autocomplete suggestions (command begins typing)
    // ---------------------------------------------------------
    fn draw_autocomplete(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "COMMAND_PALETTE_APPLY_COMPLETION_START",
            area_width = rect.width,
            area_height = rect.height,
            query = %snap.buffer,
            message = "Drawing autocomplete suggestions"
        );
        let matches = Self::match_cmds(&snap.buffer);
        if matches.is_empty() {
            trace!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_PALETTE_NO_COMPLETION",
                query = %snap.buffer,
                message = "No autocomplete matches found"
            );
            return;
        }

        let match_len = matches.len();

        let items: SmallVec<[ListItem; 8]> = matches
            .into_iter()
            .take(8)
            .enumerate()
            .map(|(i, (cmd, desc))| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                    Span::styled(
                        cmd,
                        Style::default()
                            .fg(theme::CYAN)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" - "),
                    Span::styled(desc, Style::default().fg(theme::FOREGROUND)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Suggestions ")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::YELLOW))
                    .style(Style::default().bg(theme::BACKGROUND)),
            )
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(list, rect);

        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "COMMAND_PALETTE_APPLY_COMPLETION_AFTER",
            matches_count = match_len,
            message = "Autocomplete suggestions drawing completed"
        );
    }

    // ---------------------------------------------------------
    // Command history (buffer empty)
    // ---------------------------------------------------------
    fn draw_history(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_DRAW_HISTORY",
            area_width = rect.width,
            area_height = rect.height,
            history_len = snap.history.len(),
            message = "Drawing command history"
        );
        if snap.history.is_empty() {
            debug!(
                target: "fsm_core::view::components::input_prompt_overlay",
                marker = "COMMAND_HISTORY_EMPTY",
                message = "Command history is empty, drawing available commands"
            );
            self.draw_available_cmds(frame, rect);
            return;
        }

        let items: SmallVec<[ListItem; 8]> = snap
            .history
            .iter()
            .rev()
            .take(8)
            .enumerate()
            .map(|(i, cmd)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                    Span::raw(cmd),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Recent Commands ")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND)),
            )
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(list, rect);

        // help footer ----------------------------------------
        if rect.height >= 4 {
            let y = rect.y + rect.height - 2;
            let help_rect = Rect {
                x: rect.x + 2,
                y,
                width: rect.width.saturating_sub(4),
                height: 1,
            };
            let help = "↑↓ history  •  Tab complete  •  ↵ execute";
            frame.render_widget(
                Paragraph::new(help).style(Style::default().fg(theme::COMMENT)),
                help_rect,
            );
        }
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "COMMAND_HISTORY_DRAWN",
            history_len = snap.history.len(),
            message = "Command history drawing completed"
        );
    }

    // ---------------------------------------------------------
    // Default command list (no history & empty buffer)
    // ---------------------------------------------------------
    fn draw_available_cmds(&self, frame: &mut Frame<'_>, rect: Rect) {
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "UI_DRAW_AVAILABLE_COMMANDS",
            area_width = rect.width,
            area_height = rect.height,
            message = "Drawing available commands list"
        );
        let cmds = Self::all_cmds();
        let items: SmallVec<[ListItem; 8]> = cmds
            .into_iter()
            .map(|(c, d)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        c,
                        Style::default()
                            .fg(theme::CYAN)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" - "),
                    Span::styled(d, Style::default().fg(theme::FOREGROUND)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Available Commands ")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::GREEN))
                    .style(Style::default().bg(theme::BACKGROUND)),
            )
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(list, rect);
        trace!(
            target: "fsm_core::view::components::input_prompt_overlay",
            marker = "AVAILABLE_COMMANDS_DRAWN",
            message = "Available commands list drawing completed"
        );
    }

    // ---------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------
    #[inline]
    fn title_for(t: &InputPromptType) -> &'static str {
        match t {
            InputPromptType::CreateFile => " New File ",
            InputPromptType::CreateDirectory => " New Directory ",
            InputPromptType::Rename => " Rename ",
            InputPromptType::Search => " Search ",
            InputPromptType::GoToPath => " Go To Path ",
            InputPromptType::Custom(c) if c == "command" => " Command Mode ",
            _ => " Input ",
        }
    }

    #[inline]
    fn standard_help(t: &InputPromptType) -> &'static str {
        match t {
            InputPromptType::CreateFile => "filename • Esc cancel",
            InputPromptType::CreateDirectory => "directory name • Esc cancel",
            InputPromptType::Rename => "new name • Esc cancel",
            InputPromptType::GoToPath => "path • Tab complete • Esc cancel",
            _ => "text • Esc cancel",
        }
    }

    fn match_cmds(buf: &str) -> SmallVec<[(&'static str, &'static str); 12]> {
        Self::all_cmds()
            .into_iter()
            .filter(|(cmd, _)| cmd.starts_with(buf))
            .collect()
    }

    fn all_cmds() -> SmallVec<[(&'static str, &'static str); 12]> {
        SmallVec::from_slice(&[
            ("reload", "Reload current directory"),
            ("cd", "Change directory"),
            ("mkdir", "Create directory"),
            ("touch", "Create file"),
            ("grep", "Search file contents"),
            ("find", "Find files by name"),
            ("config", "Open config"),
            ("help", "Show help"),
            ("quit", "Exit application"),
            ("ls", "List directory"),
            ("pwd", "Print working dir"),
            ("clear", "Clear screen"),
        ])
    }
}

impl Default for OptimizedPromptOverlay {
    fn default() -> Self {
        Self::new()
    }
}
