//! Enhanced input prompt overlay with autocomplete and command history
use crate::controller::actions::InputPromptType;
use crate::model::ui_state::UIState;
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub struct OptimizedPromptOverlay;

impl OptimizedPromptOverlay {
    pub fn new() -> Self {
        Self
    }

    pub fn render_input(
        &self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        prompt_type: &InputPromptType,
        area: Rect,
    ) {
        frame.render_widget(Clear, area);

        let title = match prompt_type {
            InputPromptType::CreateFile => " Create New File ",
            InputPromptType::CreateDirectory => " Create New Directory ",
            InputPromptType::Rename => " Rename ",
            InputPromptType::Search => " Search ",
            InputPromptType::GoToPath => " Go To Path ",
            InputPromptType::Custom(name) => {
                if name == "command" {
                    " Command Mode "
                } else {
                    " Custom Input "
                }
            }
            _ => " Input ",
        };

        // For command mode, show autocomplete and history
        if let InputPromptType::Custom(name) = prompt_type {
            if name == "command" {
                self.render_command_mode(frame, ui_state, area, title);
                return;
            }
        }

        // Standard input prompt
        self.render_standard_input(frame, ui_state, area, title, prompt_type);
    }
    
    fn render_command_mode(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect, title: &str) {
        // Split area: input field + autocomplete/history
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input field
                Constraint::Min(0),    // Autocomplete/history
            ])
            .split(area);

        // Render input field with cursor
        self.render_input_field(frame, ui_state, chunks[0], title);
        
        // Show autocomplete or history
        if chunks[1].height > 2 {
            if ui_state.input.is_empty() {
                self.render_command_history(frame, ui_state, chunks[1]);
            } else {
                self.render_command_autocomplete(frame, ui_state, chunks[1]);
            }
        }
    }
    
    fn render_standard_input(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect, title: &str, prompt_type: &InputPromptType) {
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));

        // Create input text with cursor indicator
        let mut input_text = ui_state.input.to_string();
        if ui_state.input_cursor <= input_text.len() {
            if ui_state.input_cursor == input_text.len() {
                input_text.push('│');
            } else {
                input_text.insert(ui_state.input_cursor, '│');
            }
        }

        let input_paragraph = Paragraph::new(input_text)
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, area);

        // Show context-specific help
        let help_text = match prompt_type {
            InputPromptType::CreateFile => "Enter filename • Tab for suggestions • Esc to cancel",
            InputPromptType::CreateDirectory => "Enter directory name • Esc to cancel", 
            InputPromptType::Rename => "Enter new name • Esc to cancel",
            InputPromptType::GoToPath => "Enter path • Tab for completion • Esc to cancel",
            _ => "Enter text • Esc to cancel",
        };
        
        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        let help_area = Rect {
            x: area.x,
            y: area.y + area.height,
            width: area.width,
            height: 1,
        };

        if help_area.y < frame.area().height {
            frame.render_widget(help_paragraph, help_area);
        }
    }
    
    fn render_input_field(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect, title: &str) {
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        // Create input text with cursor and prompt
        let prompt = ":";
        let mut display_text = format!("{}{}", prompt, ui_state.input);
        let cursor_pos = prompt.len() + ui_state.input_cursor;
        
        if cursor_pos <= display_text.len() {
            if cursor_pos == display_text.len() {
                display_text.push('│');
            } else {
                display_text.insert(cursor_pos, '│');
            }
        }

        let input_paragraph = Paragraph::new(display_text)
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(input_paragraph, area);
    }
    
    fn render_command_autocomplete(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        let commands = self.get_matching_commands(&ui_state.input);
        
        if commands.is_empty() {
            return;
        }
        
        let title = format!(" {} Suggestions ", commands.len());
        
        let items: Vec<ListItem> = commands
            .into_iter()
            .take(8) // Limit suggestions
            .enumerate()
            .map(|(i, (cmd, desc))| {
                Line::from(vec![
                    Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                    Span::styled(cmd, Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD)),
                    Span::styled(" - ", Style::default().fg(theme::COMMENT)),
                    Span::styled(desc, Style::default().fg(theme::FOREGROUND)),
                ])
            })
            .map(ListItem::new)
            .collect();
        
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::YELLOW))
            .style(Style::default().bg(theme::BACKGROUND));
        
        let list = List::new(items)
            .block(block)
            .style(Style::default().fg(theme::FOREGROUND));
        
        frame.render_widget(list, area);
    }
    
    fn render_command_history(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        if ui_state.input_history.is_empty() {
            // Show available commands
            self.render_available_commands(frame, area);
            return;
        }
        
        let title = " Recent Commands ";
        
        let items: Vec<ListItem> = ui_state.input_history
            .iter()
            .rev() // Most recent first
            .take(8)
            .enumerate()
            .map(|(i, cmd)| {
                Line::from(vec![
                    Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                    Span::styled(cmd.as_str(), Style::default().fg(theme::FOREGROUND)),
                ])
            })
            .map(ListItem::new)
            .collect();
        
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));
        
        let list = List::new(items)
            .block(block)
            .style(Style::default().fg(theme::FOREGROUND));
        
        frame.render_widget(list, area);
        
        // Show help text
        let help_area = Rect {
            x: area.x + 2,
            y: area.y + area.height - 2,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        
        let help_text = "↑↓ Navigate history • Tab Autocomplete • Enter Execute";
        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT));
        
        frame.render_widget(help_paragraph, help_area);
    }
    
    fn render_available_commands(&self, frame: &mut Frame<'_>, area: Rect) {
        let commands = vec![
            ("reload", "Reload current directory"),
            ("cd <path>", "Change directory"),
            ("mkdir <name>", "Create directory"),
            ("touch <name>", "Create file"),
            ("grep <pattern>", "Search file contents"),
            ("find <pattern>", "Find files by name"),
            ("config", "Open configuration"),
            ("help", "Show help"),
        ];
        
        let title = " Available Commands ";
        
        let items: Vec<ListItem> = commands
            .into_iter()
            .map(|(cmd, desc)| {
                Line::from(vec![
                    Span::styled(cmd, Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD)),
                    Span::styled(" - ", Style::default().fg(theme::COMMENT)),
                    Span::styled(desc, Style::default().fg(theme::FOREGROUND)),
                ])
            })
            .map(ListItem::new)
            .collect();
        
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::GREEN))
            .style(Style::default().bg(theme::BACKGROUND));
        
        let list = List::new(items)
            .block(block)
            .style(Style::default().fg(theme::FOREGROUND));
        
        frame.render_widget(list, area);
    }
    
    fn get_matching_commands(&self, input: &str) -> Vec<(&'static str, &'static str)> {
        let commands = vec![
            ("reload", "Reload current directory"),
            ("cd", "Change directory"),
            ("mkdir", "Create directory"),
            ("touch", "Create file"),
            ("grep", "Search file contents"),
            ("find", "Find files by name"),
            ("config", "Open configuration"),
            ("help", "Show help"),
            ("quit", "Exit application"),
            ("q", "Exit application"),
            ("ls", "List directory contents"),
            ("pwd", "Print working directory"),
            ("clear", "Clear screen"),
        ];
        
        if input.is_empty() {
            return commands;
        }
        
        commands
            .into_iter()
            .filter(|(cmd, _)| cmd.starts_with(input))
            .collect()
    }
}

impl Default for OptimizedPromptOverlay {
    fn default() -> Self {
        Self::new()
    }
}
