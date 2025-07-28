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

use crate::controller::actions::InputPromptType;
use crate::view::snapshots::PromptSnapshot;
use crate::view::theme; // colour constants

pub struct OptimizedPromptOverlay;

impl OptimizedPromptOverlay {
    pub fn new() -> Self {
        Self
    }

    // ---------------------------------------------------------
    // Public entry
    // ---------------------------------------------------------
    pub fn render_input(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect) {
        frame.render_widget(Clear, rect); // wipe bg

        // choose title by prompt-type -------------------------
        let title = Self::title_for(&snap.prompt_type);

        // command mode gets its own two-row layout ------------
        if matches!(&snap.prompt_type, InputPromptType::Custom(s)
                    if s == "command")
        {
            self.draw_command_mode(frame, snap, rect, title);
        } else {
            self.draw_standard(frame, snap, rect, title);
        }
    }

    // ---------------------------------------------------------
    // Standard (one-box) prompt
    // ---------------------------------------------------------
    fn draw_standard(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect, title: &str) {
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
        // split: input row + suggestions/history -------------
        let [inp_row, bottom] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(rect);

        self.draw_input_field(frame, snap, inp_row, title);

        if bottom.height <= 2 {
            return;
        }

        if snap.buffer.is_empty() {
            self.draw_history(frame, snap, bottom);
        } else {
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
        let matches = Self::match_cmds(&snap.buffer);
        if matches.is_empty() {
            return;
        }

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
    }

    // ---------------------------------------------------------
    // Command history (buffer empty)
    // ---------------------------------------------------------
    fn draw_history(&self, frame: &mut Frame<'_>, snap: &PromptSnapshot, rect: Rect) {
        if snap.history.is_empty() {
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
    }

    // ---------------------------------------------------------
    // Default command list (no history & empty buffer)
    // ---------------------------------------------------------
    fn draw_available_cmds(&self, frame: &mut Frame<'_>, rect: Rect) {
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
