//! Enhanced help overlay with tabbed sections and better organization
use crate::view::theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
};

pub struct OptimizedHelpOverlay {
    selected_tab: usize,
}

impl OptimizedHelpOverlay {
    pub fn new() -> Self {
        Self { selected_tab: 0 }
    }

    pub fn render_fast(&self, frame: &mut Frame<'_>, area: Rect) {
        let overlay_area = self.centered_rect(85, 90, area);
        frame.render_widget(Clear, overlay_area);

        // Create layout with tabs
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Min(0),    // Content
            ])
            .split(overlay_area);

        // Render tab bar
        self.render_tabs(frame, chunks[0]);
        
        // Render content based on selected tab
        self.render_tab_content(frame, chunks[1]);
    }
    
    fn render_tabs(&self, frame: &mut Frame<'_>, area: Rect) {
        let titles = vec!["Navigation", "Commands", "Search", "Advanced"];
        
        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" FSM Help ")
                    .title_alignment(Alignment::Center)
                    .border_style(Style::default().fg(theme::CYAN))
            )
            .style(Style::default().fg(theme::FOREGROUND))
            .highlight_style(
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD)
            )
            .select(self.selected_tab);
        
        frame.render_widget(tabs, area);
    }
    
    fn render_tab_content(&self, frame: &mut Frame<'_>, area: Rect) {
        let content = match self.selected_tab {
            0 => self.get_navigation_help(),
            1 => self.get_commands_help(),
            2 => self.get_search_help(),
            3 => self.get_advanced_help(),
            _ => self.get_navigation_help(),
        };
        
        let help_paragraph = Paragraph::new(Text::from(content))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::PURPLE))
                    .style(Style::default().bg(theme::BACKGROUND))
            )
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: true })
            .scroll((0, 0));
        
        frame.render_widget(help_paragraph, area);
        
        // Add footer with navigation help
        self.render_footer(frame, area);
    }
    
    fn get_navigation_help(&self) -> Vec<Line> {
        vec![
            Line::from(Span::styled(
                "🗂  Navigation & Movement",
                Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            
            Line::from(Span::styled("Basic Movement:", Style::default().fg(theme::CYAN))),
            Line::from("  ↑↓             Move selection up/down"),
            Line::from("  PgUp/PgDn      Page up/down"),
            Line::from("  Home/End       Jump to first/last item"),
            Line::from("  Enter          Open file or enter directory"),
            Line::from("  Backspace      Go to parent directory"),
            Line::from(""),
            
            Line::from(Span::styled("Selection:", Style::default().fg(theme::CYAN))),
            Line::from("  Space          Mark/unmark item for batch operations"),
            Line::from("  Ctrl+A         Select all items"),
            Line::from("  Ctrl+D         Deselect all items"),
            Line::from("  Tab            Switch between panes (future)"),
            Line::from(""),
            
            Line::from(Span::styled("Quick Navigation:", Style::default().fg(theme::CYAN))),
            Line::from("  g g            Go to top"),
            Line::from("  G              Go to bottom"),
            Line::from("  Ctrl+O         Open directory chooser (future)"),
            Line::from("  Ctrl+L         Refresh current directory"),
            Line::from(""),
            
            Line::from(Span::styled("Application:", Style::default().fg(theme::CYAN))),
            Line::from("  q/Q            Quit application"),
            Line::from("  Ctrl+C         Force quit"),
            Line::from("  h/?            Toggle this help"),
        ]
    }
    
    fn get_commands_help(&self) -> Vec<Line> {
        vec![
            Line::from(Span::styled(
                "⌨  Command Mode & Operations",
                Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            
            Line::from(Span::styled("Command Mode (: to enter):", Style::default().fg(theme::CYAN))),
            Line::from("  :cd <path>     Change directory"),
            Line::from("  :mkdir <name>  Create new directory"),
            Line::from("  :touch <name>  Create new file"),
            Line::from("  :reload        Refresh current directory"),
            Line::from("  :config        Open configuration"),
            Line::from("  :help          Show this help"),
            Line::from("  :quit/:q       Exit application"),
            Line::from(""),
            
            Line::from(Span::styled("File Operations:", Style::default().fg(theme::CYAN))),
            Line::from("  d              Delete selected items"),
            Line::from("  r              Rename selected item"),
            Line::from("  y              Copy (yank) selected items"),
            Line::from("  p              Paste copied items"),
            Line::from("  x              Cut selected items"),
            Line::from("  n              Create new file"),
            Line::from("  f              Create new folder"),
            Line::from(""),
            
            Line::from(Span::styled("Command Features:", Style::default().fg(theme::CYAN))),
            Line::from("  Tab            Auto-complete commands"),
            Line::from("  ↑↓             Navigate command history"),
            Line::from("  Esc            Exit command mode"),
            Line::from("  Enter          Execute command"),
        ]
    }
    
    fn get_search_help(&self) -> Vec<Line> {
        vec![
            Line::from(Span::styled(
                "🔍 Search & Filtering",
                Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            
            Line::from(Span::styled("File Name Search (/):", Style::default().fg(theme::CYAN))),
            Line::from("  /              Open filename search"),
            Line::from("  *.rs           Search for Rust files"),
            Line::from("  test*          Files starting with 'test'"),
            Line::from("  *config*       Files containing 'config'"),
            Line::from("  src/*.rs       Rust files in src directory"),
            Line::from(""),
            
            Line::from(Span::styled("Content Search:", Style::default().fg(theme::CYAN))),
            Line::from("  :grep <term>   Search file contents"),
            Line::from("  :find <name>   Find files by name"),
            Line::from("  Ctrl+F         Global content search (future)"),
            Line::from(""),
            
            Line::from(Span::styled("Search Tips:", Style::default().fg(theme::CYAN))),
            Line::from("  • Use wildcards (*) for flexible matching"),
            Line::from("  • Search is case-insensitive by default"),
            Line::from("  • Use / for path separators in patterns"),
            Line::from("  • Enter navigates to first result"),
            Line::from("  • Esc cancels search and returns to browser"),
            Line::from(""),
            
            Line::from(Span::styled("Filter Operations:", Style::default().fg(theme::CYAN))),
            Line::from("  Ctrl+H         Toggle hidden files"),
            Line::from("  Ctrl+S         Sort options menu (future)"),
        ]
    }
    
    fn get_advanced_help(&self) -> Vec<Line> {
        vec![
            Line::from(Span::styled(
                "⚙  Advanced Features",
                Style::default().fg(theme::YELLOW).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            
            Line::from(Span::styled("Themes & UI:", Style::default().fg(theme::CYAN))),
            Line::from("  t              Toggle theme"),
            Line::from("  Ctrl+T         Theme selector (future)"),
            Line::from("  F11            Toggle fullscreen"),
            Line::from(""),
            
            Line::from(Span::styled("Clipboard & Selection:", Style::default().fg(theme::CYAN))),
            Line::from("  Ctrl+V         Show clipboard overlay"),
            Line::from("  v              Enter visual selection mode"),
            Line::from("  V              Visual line mode"),
            Line::from("  Ctrl+A         Select all"),
            Line::from(""),
            
            Line::from(Span::styled("Performance:", Style::default().fg(theme::CYAN))),
            Line::from("  • 60fps rendering with sub-ms response"),
            Line::from("  • Lock-free navigation for large directories"),
            Line::from("  • Async directory loading"),
            Line::from("  • Optimized file metadata caching"),
            Line::from(""),
            
            Line::from(Span::styled("Configuration:", Style::default().fg(theme::CYAN))),
            Line::from("  :config        Open config file"),
            Line::from("  • Custom key bindings"),
            Line::from("  • Theme customization"),
            Line::from("  • Performance tuning"),
            Line::from("  • Plugin system (future)"),
            Line::from(""),
            
            Line::from(Span::styled("Version Info:", Style::default().fg(theme::COMMENT))),
            Line::from("  FSM v4.1 - Enhanced Overlay System"),
            Line::from("  Built with Rust + Ratatui"),
        ]
    }
    
    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let footer_area = Rect {
            x: area.x + 2,
            y: area.y + area.height - 2,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        
        let footer_text = "←→ Switch tabs • Esc/h/? Close help • Enter Open section";
        let footer = Paragraph::new(footer_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);
        
        frame.render_widget(footer, footer_area);
    }

    fn centered_rect(&self, percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(vertical[1]);

        horizontal[1]
    }
    
    // Methods for tab navigation (future enhancement)
    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % 4;
    }
    
    pub fn prev_tab(&mut self) {
        if self.selected_tab > 0 {
            self.selected_tab -= 1;
        } else {
            self.selected_tab = 3;
        }
    }
}

impl Default for OptimizedHelpOverlay {
    fn default() -> Self {
        Self::new()
    }
}
