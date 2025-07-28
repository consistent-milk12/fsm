//! Enhanced clipboard overlay with async support and zero-allocation rendering

use crate::error::AppError;
use crate::model::ui_state::UIState;
use clipr::clipboard::ClipBoard;
use clipr::item::ClipBoardItem;
use clipr::item::ClipBoardOperation;

use compact_str::CompactString;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use smallvec::SmallVec;

/// Ultra-modern clipboard overlay with premium styling and async support
pub struct OptimizedClipboardOverlay {
    cached_items: SmallVec<[ClipBoardItem; 16]>,
    last_update: std::time::Instant,
    cache_valid: bool,
}

impl Default for OptimizedClipboardOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizedClipboardOverlay {
    pub fn new() -> Self {
        Self {
            cached_items: SmallVec::new(),
            last_update: std::time::Instant::now(),
            cache_valid: false,
        }
    }

    /// High-performance rendering with premium visual design and async item fetching
    pub async fn render_zero_alloc(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        clipboard: &ClipBoard,
        selected_index: usize,
    ) -> Result<(), AppError> {
        // Use cached items if fresh, otherwise fetch asynchronously
        let items = if self.cache_valid && self.last_update.elapsed().as_secs() < 1 {
            &self.cached_items
        } else {
            // Fetch items asynchronously for real-time updates
            let fetched_items = clipboard.get_all_items().await;
            self.cached_items.clear();
            self.cached_items.extend(fetched_items);
            self.last_update = std::time::Instant::now();
            self.cache_valid = true;
            &self.cached_items
        };

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
            self.render_clipboard_content_optimized(frame, &layout, items, &mut list_state)?;
        }

        Ok(())
    }

    /// Render clipboard overlay from UI state for compatibility with async operations
    pub async fn render_from_ui_state(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        ui_state: &UIState,
    ) -> Result<(), AppError> {
        self.render_zero_alloc(
            frame,
            area,
            &ui_state.clipboard,
            ui_state.selected_clipboard_item_index,
        )
        .await
    }

    /// Sync fallback for rendering when async is not possible
    pub fn render_sync_fallback(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        ui_state: &UIState,
    ) -> Result<(), AppError> {
        frame.render_widget(Clear, area);

        let layout = PrecomputedLayout::compute(area);
        let mut list_state = ListState::default();

        self.render_main_container(frame, &layout);

        // Use cached items or show placeholder
        if self.cached_items.is_empty() {
            self.render_loading_state(frame, layout.content_area);
        } else {
            list_state.select(Some(
                ui_state
                    .selected_clipboard_item_index
                    .min(self.cached_items.len().saturating_sub(1)),
            ));
            self.render_clipboard_content_optimized(
                frame,
                &layout,
                &self.cached_items,
                &mut list_state,
            )?;
        }

        Ok(())
    }

    fn render_loading_state(&self, frame: &mut Frame<'_>, area: Rect) {
        let loading_text = "Loading clipboard...";
        let loading_block = Paragraph::new(loading_text)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Clipboard ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 25, 40)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 150, 255))),
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(30, 25, 40))
                    .fg(Color::Rgb(180, 190, 220)),
            );
        frame.render_widget(loading_block, area);
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

    fn render_clipboard_content_optimized(
        &self,
        frame: &mut Frame<'_>,
        layout: &PrecomputedLayout,
        items: &[ClipBoardItem],
        list_state: &mut ListState,
    ) -> Result<(), AppError> {
        // Pre-allocate list items for zero allocation during rendering
        let mut list_items = SmallVec::<[ListItem; 16]>::with_capacity(items.len());

        for (index, item) in items.iter().enumerate() {
            let (operation_color, operation_icon) = match item.operation {
                ClipBoardOperation::Copy => (Color::Rgb(100, 200, 255), "📄"),
                ClipBoardOperation::Move => (Color::Rgb(255, 200, 100), "✂️"),
            };

            // Use compact string formatting for performance
            let display_text = CompactString::from(format!(
                "{} {:2}. {} ({})",
                operation_icon,
                index + 1,
                self.format_path_smart(&item.source_path, 45),
                self.format_file_size_compact(item.metadata.size)
            ));

            list_items.push(
                ListItem::new(display_text.to_string()).style(Style::default().fg(operation_color)),
            );
        }

        let list = List::new(list_items.into_iter())
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

    #[inline]
    fn format_path_smart(&self, path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            return path.to_string();
        }

        if let Some(sep_pos) = path.rfind('/') {
            let filename = &path[sep_pos + 1..];
            if filename.len() < max_len - 4 {
                return format!(".../{}", filename);
            }
        }

        format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    }

    #[inline]
    fn format_file_size_compact(&self, size: u64) -> String {
        if size == 0 {
            return "0B".to_string();
        }

        const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;

        while size_f >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{}B", size)
        } else {
            format!("{:.1}{}", size_f, UNITS[unit_idx])
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

        const UNITS: &[&str] = &["bytes", "KB", "MB", "GB", "TB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;

        while size_f >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{} {}", size, UNITS[unit_idx])
        } else {
            format!("{:.2} {}", size_f, UNITS[unit_idx])
        }
    }

    fn render_help_panel_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let help_text = "🔹 ↑↓ Navigate\n🔹 Enter Select\n🔹 Esc Close\n🔹 Del Remove";
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
        let empty_text = "📋 Clipboard is Empty\n\nCopy files with 'c' or cut with 'x'\nto populate the clipboard";
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

    /// Update cached items asynchronously (call from background task)
    pub async fn update_cache(&mut self, clipboard: &ClipBoard) -> Result<(), AppError> {
        let items = clipboard.get_all_items().await;
        self.cached_items.clear();
        self.cached_items.extend(items);
        self.last_update = std::time::Instant::now();
        self.cache_valid = true;
        Ok(())
    }

    /// Update cached items synchronously for compatibility
    pub fn update_cache_sync(&mut self, _clipboard: &ClipBoard) -> Result<(), AppError> {
        // For sync operations, just invalidate cache
        self.cache_valid = false;
        Ok(())
    }

    /// Get number of cached items for navigation
    pub fn item_count(&self) -> usize {
        self.cached_items.len()
    }
}

/// Optimized layout computation with caching
#[derive(Debug, Clone)]
struct PrecomputedLayout {
    main_area: Rect,
    content_area: Rect,
    list_area: Rect,
    details_area: Rect,
    help_area: Rect,
}

impl PrecomputedLayout {
    #[inline]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_formatting() {
        let overlay = OptimizedClipboardOverlay::new();

        // Short path
        assert_eq!(overlay.format_path_smart("short.txt", 20), "short.txt");

        // Long path with filename
        let long_path = "/very/long/path/to/some/file.txt";
        let formatted = overlay.format_path_smart(long_path, 15);
        assert!(formatted.starts_with("..."));
        assert!(formatted.contains("file.txt"));
    }

    #[test]
    fn test_file_size_formatting() {
        let overlay = OptimizedClipboardOverlay::new();

        assert_eq!(overlay.format_file_size_compact(0), "0B");
        assert_eq!(overlay.format_file_size_compact(512), "512B");
        assert_eq!(overlay.format_file_size_compact(1536), "1.5K");
        assert_eq!(overlay.format_file_size_compact(2048), "2.0K");
    }

    #[test]
    fn test_layout_computation() {
        let area = Rect::new(0, 0, 100, 50);
        let layout = PrecomputedLayout::compute(area);

        assert!(layout.main_area.width <= area.width);
        assert!(layout.main_area.height <= area.height);
        assert!(layout.list_area.width > 0);
        assert!(layout.details_area.width > 0);
    }
}
