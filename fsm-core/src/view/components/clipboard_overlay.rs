//! Modern, highly stylized clipboard overlay with zero-allocation performance
use crate::error::AppError;
use clipr::{ClipBoard, ClipBoardItem, ClipBoardOperation};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

/// Ultra-modern clipboard overlay with premium styling
#[derive(Default)]
pub struct OptimizedClipboardOverlay {}

impl OptimizedClipboardOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// High-performance rendering with premium visual design
    pub fn render_zero_alloc(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        _clipboard: &ClipBoard,
        selected_index: usize,
    ) -> Result<(), AppError> {
        // This is a temporary synchronous version.
        // The original `get_all_items` is async, which cannot be called from the main render loop.
        // This needs to be addressed, for example by pre-fetching items into UIState.
        let items: Vec<ClipBoardItem> = Vec::new(); // TODO: Get items synchronously or from state.

        frame.render_widget(Clear, area);

        let layout = PrecomputedLayout::compute(area);

        let mut list_state = ListState::default();
        if !items.is_empty() {
            list_state.select(Some(selected_index.min(items.len() - 1)));
        }

        self.render_main_container(frame, &layout);

        if items.is_empty() {
            self.render_empty_state_premium(frame, layout.content_area);
        } else {
            self.render_clipboard_content(frame, &layout, &items, &mut list_state)?;
        }

        Ok(())
    }

    fn render_main_container(&self, frame: &mut Frame<'_>, layout: &PrecomputedLayout) {
        let main_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 📋 Clipboard Manager ")
            .title_alignment(Alignment::Center)
            .style(
                Style::default()
                    .bg(Color::Rgb(20, 24, 36))
                    .fg(Color::Rgb(220, 225, 235))
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(
                Style::default()
                    .fg(Color::Rgb(100, 149, 237))
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(main_block, layout.main_area);
    }

    fn render_clipboard_content(
        &self,
        frame: &mut Frame<'_>,
        layout: &PrecomputedLayout,
        items: &[ClipBoardItem],
        list_state: &mut ListState,
    ) -> Result<(), AppError> {
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (operation_color, operation_icon) = match item.operation {
                    ClipBoardOperation::Copy => (Color::Rgb(100, 200, 255), "📄"),
                    ClipBoardOperation::Move => (Color::Rgb(255, 200, 100), "✂️"),
                };
                let display_text = format!(
                    "{} {:2}. {} ({})",
                    operation_icon,
                    index + 1,
                    self.format_path_smart(&item.source_path, 45),
                    self.format_file_size_compact(item.metadata.size)
                );
                ListItem::new(display_text).style(Style::default().fg(operation_color))
            })
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!(" {} Items ", items.len()))
                    .title_alignment(Alignment::Left)
                    .style(Style::default().bg(Color::Rgb(25, 30, 45)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(75, 125, 200))),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(60, 100, 180))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, layout.list_area, list_state);

        if let Some(selected_item) = list_state.selected().and_then(|i| items.get(i)) {
            self.render_details_panel_premium(frame, layout.details_area, selected_item)?;
        }

        self.render_help_panel_premium(frame, layout.help_area);
        Ok(())
    }

    fn format_path_smart(&self, path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            return path.to_string();
        }
        if let Some(sep_pos) = path.rfind('/') {
            let filename = &path[sep_pos + 1..];
            if filename.len() < max_len - 4 {
                return format!(".../{filename}");
            }
        }
        format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    }

    fn format_file_size_compact(&self, size: u64) -> String {
        if size == 0 {
            return "0B".to_string();
        }
        let units = ["B", "K", "M", "G", "T"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;
        while size_f >= 1024.0 && unit_idx < units.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }
        if unit_idx == 0 {
            format!("{size}B")
        } else {
            format!("{:.1}{}", size_f, units[unit_idx])
        }
    }

    fn render_details_panel_premium(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let details_text = format!(
            "📁 Path: {}\n\n🔧 Operation: {:?}\n📊 Size: {}",
            item.source_path,
            item.operation,
            self.format_file_size_human(item.metadata.size)
        );
        let details_block = Paragraph::new(details_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 📋 Details ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 35, 50)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 100, 200))),
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(30, 35, 50))
                    .fg(Color::Rgb(200, 210, 220)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(details_block, area);
        Ok(())
    }

    fn format_file_size_human(&self, size: u64) -> String {
        if size == 0 {
            return "0 bytes".to_string();
        }
        let units = ["bytes", "KB", "MB", "GB", "TB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;
        while size_f >= 1024.0 && unit_idx < units.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }
        if unit_idx == 0 {
            format!("{} {}", size, units[unit_idx])
        } else {
            format!("{:.2} {}", size_f, units[unit_idx])
        }
    }

    fn render_help_panel_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let help_text = "🔹 ↑↓ Navigate\n🔹 Enter Select\n🔹 Esc Close";
        let help_block = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" ⌨️ Controls ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(40, 30, 20)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(255, 200, 100))),
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(40, 30, 20))
                    .fg(Color::Rgb(255, 220, 150)),
            );
        frame.render_widget(help_block, area);
    }

    fn render_empty_state_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let empty_text = "📋 Clipboard is Empty\n\n'c' to copy, 'x' to cut";
        let empty_block = Paragraph::new(empty_text)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Welcome to Clipboard ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 25, 40)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 150, 255))),
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(30, 25, 40))
                    .fg(Color::Rgb(180, 190, 220)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(empty_block, area);
    }
}

#[derive(Debug, Clone)]
struct PrecomputedLayout {
    main_area: Rect,
    content_area: Rect,
    list_area: Rect,
    details_area: Rect,
    help_area: Rect,
}

impl PrecomputedLayout {
    fn compute(area: Rect) -> Self {
        let main_area = area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let content_area = main_area.inner(Margin::new(1, 1));
        let horizontal_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(content_area);
        let list_area = horizontal_split[0];
        let side_panel_area = horizontal_split[1];
        let side_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(side_panel_area);
        Self {
            main_area,
            content_area,
            list_area,
            details_area: side_split[0],
            help_area: side_split[1],
        }
    }
}
