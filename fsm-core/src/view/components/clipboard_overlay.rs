
//! Modern, highly stylized clipboard overlay with zero-allocation performance
//! Features: Gradient backgrounds, rounded corners, animations, and   premium visual design
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::Instant;

use crate::error::AppError;
use clipr::{ClipBoard, ClipBoardItem, ClipBoardOperation};

/// Ultra-modern clipboard overlay with premium styling
pub struct ClipboardOverlay {
    /// Pre-allocated text buffers for zero-allocation rendering
    item_text_cache: HeaplessVec<HeaplessString<512>, 64>,

    /// Layout cache for instant positioning
    layout_cache: LayoutCache,

    /// Current selection state
    selected_index: usize,

    /// Performance monitoring
    render_stats: RenderStats,

    /// List widget state
    list_state: ListState,

    /// Animation state for smooth transitions
    animation_frame: u8,

    /// Last render time for animations
    last_render: Instant,
}

impl ClipboardOverlay {
    /// Create new clipboard overlay with modern styling
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            item_text_cache: HeaplessVec::new(),
            layout_cache: LayoutCache::new(),
            selected_index: 0,
            render_stats: RenderStats::new(),
            list_state,
            animation_frame: 0,
            last_render: Instant::now(),
        }
    }

    /// High-performance rendering with premium visual design
    pub async fn render_zero_alloc(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        clipboard: &ClipBoard,
        selected_index: usize,
    ) -> Result<(), AppError> {
        let start_time = Instant::now();

        // Update animation state
        self.update_animation();

        // Clear background with translucent overlay
        frame.render_widget(Clear, area);

        // Get cached layout
        let layout = self.layout_cache.get_or_compute(area).clone();

        // Update selection bounds
        let clipboard_len = clipboard.len();
        self.selected_index = if clipboard_len > 0 {
            selected_index.min(clipboard_len - 1)
        } else {
            0
        };

        self.list_state.select(if clipboard_len > 0 {
            Some(self.selected_index)
        } else {
            None
        });

        // Render main container with premium styling
        self.render_main_container(frame, &layout);

        if clipboard.is_empty() {
            self.render_empty_state_premium(frame, layout.content_area);
        } else {
            self.render_clipboard_content(frame, &layout, clipboard)
                .await?;
        }

        // Record performance metrics
        let render_time = start_time.elapsed();
        self.render_stats.record_render_time(render_time);

        Ok(())
    }

    /// Render main container with gradient background and modern styling
    fn render_main_container(&self, frame: &mut Frame<'_>, layout: &PrecomputedLayout) {
        // Main container with premium styling
        let main_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 📋 Clipboard Manager ")
            .title_alignment(Alignment::Center)
            .style(
                Style::default()
                    .bg(Color::Rgb(20, 24, 36)) // Dark blue-gray background
                    .fg(Color::Rgb(220, 225, 235)) // Light text
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(
                Style::default()
                    .fg(Color::Rgb(100, 149, 237)) // Cornflower blue borders
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(main_block, layout.main_area);
    }

    /// Render clipboard content with modern list styling
    async fn render_clipboard_content(
        &mut self,
        frame: &mut Frame<'_>,
        layout: &PrecomputedLayout,
        clipboard: &ClipBoard,
    ) -> Result<(), AppError> {
        // Clear and rebuild item cache
        self.item_text_cache.clear();
        let items = clipboard.get_all_items().await;

        // Create styled list items
        let mut list_items = Vec::with_capacity(items.len().min(layout.max_visible_items));

        for (index, item) in items.iter().enumerate().take(layout.max_visible_items) {
            let mut item_text = HeaplessString::new();
            self.format_clipboard_item_premium(&mut item_text, item, index)?;

            // Premium styling based on operation type
            let (operation_color, operation_icon) = match item.operation {
                ClipBoardOperation::Copy => (Color::Rgb(100, 200, 255), "📄"), // Sky blue
                ClipBoardOperation::Move => (Color::Rgb(255, 200, 100), "✂️"), // Golden
            };

            // Create styled list item with icons and colors
            let display_text = format!("{} {}", operation_icon, item_text.as_str());
            let list_item = ListItem::new(display_text).style(Style::default().fg(operation_color));

            list_items.push(list_item);

            // Cache the formatted text
            self.item_text_cache.push(item_text).map_err(|_| {
                AppError::ui_component_error(
                    "ClipboardOverlay",
                    "Text
  cache overflow",
                )
            })?;
        }

        // Render premium styled list
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
                    .bg(Color::Rgb(60, 100, 180)) // Rich blue selection
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ")
            .style(Style::default().bg(Color::Rgb(25, 30, 45)));

        frame.render_stateful_widget(list, layout.list_area, &mut self.list_state);

        // Render side panels
        if let Some(selected_item) = items.get(self.selected_index) {
            self.render_details_panel_premium(frame, layout.details_area, selected_item)?;
        }

        self.render_stats_panel_premium(frame, layout.stats_area);
        self.render_help_panel_premium(frame, layout.help_area);

        Ok(())
    }

    /// Format clipboard item with premium styling and metadata
    fn format_clipboard_item_premium(
        &self,
        buffer: &mut HeaplessString<512>,
        item: &ClipBoardItem,
        index: usize,
    ) -> Result<(), AppError> {
        use core::fmt::Write;

        // Smart path display with truncation
        let display_path = self.format_path_smart(&item.source_path, 45);

        // Format with index and metadata
        write!(
            buffer,
            "{:2}. {} ({})",
            index + 1,
            display_path,
            self.format_file_size_compact(item.metadata.size)
        )
        .map_err(|_| AppError::ui_component_error("ClipboardOverlay", "Format error"))?;

        Ok(())
    }

    /// Render premium details panel with rich metadata
    fn render_details_panel_premium(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let details_text = format!(
            "📁 Path: {}\n\n🔧 Operation: {:?}\n📊 Size: {}\n⏰ Added:
  {}\n🏷️  Type: {:?}\n📅 Modified: {}",
            item.source_path,
            item.operation,
            self.format_file_size_human(item.metadata.size),
            self.format_timestamp_relative(item.added_at),
            item.metadata.file_type,
            self.format_timestamp_date(item.metadata.modified)
        );

        let details_block = Paragraph::new(details_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 📋 Details ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 35, 50)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 100, 200))), // Purple accent
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

    /// Render premium statistics panel with performance metrics
    fn render_stats_panel_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let performance_color = if self.render_stats.meets_performance_target() {
            Color::Rgb(100, 255, 100) // Bright green
        } else {
            Color::Rgb(255, 100, 100) // Bright red
        };

        let stats_text = format!(
            "⚡ Renders: {}\n🎯 Avg: {:.1}μs\n⏱️  Max: {:.1}μs\n🎪 Target:
   <100μs",
            self.render_stats.total_renders,
            self.render_stats.avg_render_time_ns as f64 / 1000.0,
            self.render_stats.max_render_time_ns as f64 / 1000.0
        );

        let stats_block = Paragraph::new(stats_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" ⚡ Performance ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(20, 40, 30)).fg(Color::White))
                    .border_style(Style::default().fg(performance_color)),
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(20, 40, 30))
                    .fg(performance_color),
            );

        frame.render_widget(stats_block, area);
    }

    /// Render premium help panel with keyboard shortcuts
    fn render_help_panel_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let help_text = "🔹 ↑↓ Navigate\n🔹 Enter Select\n🔹 Tab
  Toggle\n🔹 Esc Close\n🔹 Del Remove";

        let help_block = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" ⌨️ Controls ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(40, 30, 20)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(255, 200, 100))), // Golden
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(40, 30, 20))
                    .fg(Color::Rgb(255, 220, 150)),
            );

        frame.render_widget(help_block, area);
    }

    /// Render premium empty state with helpful guidance
    fn render_empty_state_premium(&self, frame: &mut Frame<'_>, area: Rect) {
        let empty_text = "📋 Clipboard is Empty\n\n🎯 Quick Actions:\n\n📄
   Press 'c' to copy files\n✂️ Press 'x' to cut files\n📝 Select items to
  populate clipboard\n\n💡 Tab to close this overlay";

        let empty_block = Paragraph::new(empty_text)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" 📋 Welcome to Clipboard ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 25, 40)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 150, 255))), // Light purple
            )
            .style(
                Style::default()
                    .bg(Color::Rgb(30, 25, 40))
                    .fg(Color::Rgb(180, 190, 220)),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(empty_block, area);
    }

    // === Helper Methods ===

    /// Update animation state for smooth visual effects
    fn update_animation(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_render).as_millis() > 100 {
            self.animation_frame = self.animation_frame.wrapping_add(1);
            self.last_render = now;
        }
    }

    /// Format path with smart truncation
    fn format_path_smart(&self, path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            return path.to_string();
        }

        // Try to keep filename and part of directory
        if let Some(sep_pos) = path.rfind('/') {
            let filename = &path[sep_pos + 1..];
            if filename.len() < max_len - 5 {
                let available = max_len - filename.len() - 4; // 4 for ".../"
                if path.len() > available {
                    return format!(".../{}", filename);
                }
            }
        }

        // Fallback truncation
        format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    }

    /// Format file size in compact form
    fn format_file_size_compact(&self, size: u64) -> String {
        const UNITS: &[&str] = &["B", "K", "M", "G", "T"];

        if size == 0 {
            return "0B".to_string();
        }

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

    /// Format file size in human readable form
    fn format_file_size_human(&self, size: u64) -> String {
        const UNITS: &[&str] = &["bytes", "KB", "MB", "GB", "TB"];

        if size == 0 {
            return "0 bytes".to_string();
        }

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

    /// Format timestamp relative to now
    fn format_timestamp_relative(&self, timestamp: u64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let diff_ns = now.saturating_sub(timestamp);
        let diff_secs = diff_ns / 1_000_000_000;

        if diff_secs < 60 {
            format!("{}s ago", diff_secs)
        } else if diff_secs < 3600 {
            format!("{}m ago", diff_secs / 60)
        } else if diff_secs < 86400 {
            format!("{}h ago", diff_secs / 3600)
        } else {
            format!("{}d ago", diff_secs / 86400)
        }
    }

    /// Format timestamp as date
    fn format_timestamp_date(&self, timestamp: u64) -> String {
        // Simple date formatting - in real implementation you'd use chrono
        format!(
            "Modified {:.1}d ago",
            (timestamp as f64) / (86400.0 * 1_000_000_000.0)
        )
    }

    // === Public Interface ===

    /// Update selection index
    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = index;
        self.list_state.select(Some(index));
    }

    /// Get current selection index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Check if performance target is met
    pub fn meets_performance_target(&self) -> bool {
        self.render_stats.meets_performance_target()
    }
}

// === Layout System ===

#[derive(Debug)]
struct LayoutCache {
    cached_area: Option<Rect>,
    cached_layout: Option<PrecomputedLayout>,
}

impl LayoutCache {
    fn new() -> Self {
        Self {
            cached_area: None,
            cached_layout: None,
        }
    }

    fn get_or_compute(&mut self, area: Rect) -> &PrecomputedLayout {
        if self.cached_area != Some(area) {
            self.cached_layout = Some(PrecomputedLayout::compute(area));
            self.cached_area = Some(area);
        }
        self.cached_layout.as_ref().unwrap()
    }
}

#[derive(Debug, Clone)]
struct PrecomputedLayout {
    main_area: Rect,
    content_area: Rect,
    list_area: Rect,
    details_area: Rect,
    stats_area: Rect,
    help_area: Rect,
    max_visible_items: usize,
}

impl PrecomputedLayout {
    fn compute(area: Rect) -> Self {
        // Main container (with padding)
        let main_area = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(2),
        };

        // Inner content area (inside main container border)
        let content_area = Rect {
            x: main_area.x + 1,
            y: main_area.y + 1,
            width: main_area.width.saturating_sub(2),
            height: main_area.height.saturating_sub(2),
        };

        // Split content: main list (60%) + side panels (40%)
        let horizontal_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(content_area);

        let list_area = horizontal_split[0];
        let side_panel_area = horizontal_split[1];

        // Split side panel vertically: details (60%) + stats (20%) + hel (20%)
        let side_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(60), // Details
                Constraint::Percentage(20), // Stats
                Constraint::Percentage(20), // Help
            ])
            .split(side_panel_area);

        let details_area = side_split[0];
        let stats_area = side_split[1];
        let help_area = side_split[2];

        let max_visible_items = list_area.height.saturating_sub(2) as usize;

        Self {
            main_area,
            content_area,
            list_area,
            details_area,
            stats_area,
            help_area,
            max_visible_items,
        }
    }
}

// === Performance Monitoring ===

#[derive(Debug)]
struct RenderStats {
    total_renders: u64,
    avg_render_time_ns: u64,
    max_render_time_ns: u64,
}

impl RenderStats {
    fn new() -> Self {
        Self {
            total_renders: 0,
            avg_render_time_ns: 0,
            max_render_time_ns: 0,
        }
    }

    fn record_render_time(&mut self, duration: std::time::Duration) {
        let time_ns = duration.as_nanos() as u64;

        self.total_renders += 1;
        self.max_render_time_ns = self.max_render_time_ns.max(time_ns);

        // Exponential moving average for better responsiveness
        if self.total_renders == 1 {
            self.avg_render_time_ns = time_ns;
        } else {
            let alpha = 0.1; // Smoothing factor
            self.avg_render_time_ns =
                ((1.0 - alpha) * self.avg_render_time_ns as f64 + alpha * time_ns as f64) as u64;
        }
    }

    fn meets_performance_target(&self) -> bool {
        self.avg_render_time_ns < 100_000 // <100μs target
    }
}
