//! Enhanced search overlay with live filtering and results preview
use crate::model::ui_state::{UIOverlay, UIState};
use crate::view::theme;
use ratatui::{
    prelude::*,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub struct OptimizedSearchOverlay {
    overlay_type: UIOverlay,
}

impl OptimizedSearchOverlay {
    pub fn new(overlay_type: UIOverlay) -> Self {
        Self { overlay_type }
    }

    pub fn render_with_input(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        // Clear the area
        frame.render_widget(Clear, area);
        
        let title = match self.overlay_type {
            UIOverlay::Search => " Search ",
            UIOverlay::FileNameSearch => " File Name Search ",
            UIOverlay::ContentSearch => " Content Search ",
            _ => " Search ",
        };

        // Create layout: input area + results preview
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input field
                Constraint::Min(0),    // Results preview
            ])
            .split(area);

        // Render input field
        self.render_input_field(frame, ui_state, chunks[0], title);
        
        // Render results preview if there are results
        if !ui_state.filename_search_results.is_empty() && !ui_state.input.is_empty() {
            self.render_results_preview(frame, ui_state, chunks[1]);
        } else if !ui_state.input.is_empty() {
            self.render_search_help(frame, chunks[1]);
        }
    }
    
    fn render_input_field(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect, title: &str) {
        // Create input text with cursor indicator
        let mut input_text = ui_state.input.to_string();
        
        // Add cursor indicator if needed
        if ui_state.input_cursor <= input_text.len() {
            if ui_state.input_cursor == input_text.len() {
                input_text.push('│'); // Cursor at end
            } else {
                input_text.insert(ui_state.input_cursor, '│'); // Cursor in middle
            }
        }
        
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(input_text)
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND));

        frame.render_widget(input_paragraph, area);
    }
    
    fn render_results_preview(&self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        if area.height < 3 {
            return; // Not enough space
        }
        
        let results_count = ui_state.filename_search_results.len();
        let title = format!(" {} Results ", results_count);
        
        // Create list items from search results
        let items: Vec<ListItem> = ui_state.filename_search_results
            .iter()
            .take(10) // Limit to first 10 results
            .enumerate()
            .map(|(i, obj)| {
                let file_name = obj.name.as_str();
                let path = obj.path.parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_else(|| "".into());
                
                let line = if path.is_empty() {
                    Line::from(vec![
                        Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                        Span::styled(file_name, Style::default().fg(theme::FOREGROUND)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(format!("{:2} ", i + 1), Style::default().fg(theme::COMMENT)),
                        Span::styled(file_name, Style::default().fg(theme::FOREGROUND)),
                        Span::styled(" in ", Style::default().fg(theme::COMMENT)),
                        Span::styled(path, Style::default().fg(theme::PURPLE)),
                    ])
                };
                
                ListItem::new(line)
            })
            .collect();
        
        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::PURPLE))
            .style(Style::default().bg(theme::BACKGROUND));
        
        let results_list = List::new(items)
            .block(results_block)
            .style(Style::default().fg(theme::FOREGROUND));
        
        frame.render_widget(results_list, area);
        
        // Show "more results" indicator if truncated
        if results_count > 10 {
            let more_area = Rect {
                x: area.x + 2,
                y: area.y + area.height - 2,
                width: area.width.saturating_sub(4),
                height: 1,
            };
            
            let more_text = format!("... and {} more", results_count - 10);
            let more_paragraph = Paragraph::new(more_text)
                .style(Style::default().fg(theme::COMMENT));
            
            frame.render_widget(more_paragraph, more_area);
        }
    }
    
    fn render_search_help(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.height < 3 {
            return;
        }
        
        let help_lines = vec![
            Line::from("Start typing to search files..."),
            Line::from(""),
            Line::from(vec![
                Span::styled("Tips:", Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD)),
            ]),
            Line::from("• Use * for wildcards (*.rs, test*)"),
            Line::from("• Use / for path separators (src/lib.rs)"),
            Line::from("• Press Enter to navigate to first result"),
            Line::from("• Press Esc to cancel search"),
        ];
        
        let help_block = Block::default()
            .borders(Borders::ALL)
            .title(" Search Help ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme::COMMENT))
            .style(Style::default().bg(theme::BACKGROUND));
        
        let help_paragraph = Paragraph::new(help_lines)
            .block(help_block)
            .style(Style::default().fg(theme::FOREGROUND));
        
        frame.render_widget(help_paragraph, area);
    }
}
