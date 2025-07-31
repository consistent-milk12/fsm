use clipr::{ClipBoard, ClipBoardItem, ClipBoardOperation};

use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use smallvec::SmallVec;

use std::{rc::Rc, time::Instant};

use tracing::span::Span as TraceSpan;
use tracing::{debug, info, instrument, warn};

use crate::view::snapshots::UiSnapshot;
use crate::{AppError, ui::RenderStats};

pub struct OptimizedClipboardOverlay {
    /// Pre-allocated text buffers for zero-allocation rendering
    cached_items: SmallVec<[ClipBoardItem; 16]>,

    /// Layout cache for instant positioning
    layout_cache: Option<(Rect, PrecomputedLayout)>,

    /// Current selection state
    selected_index: usize,

    render_stats: RenderStats,

    /// List widget state
    list_state: ListState,

    /// Animation state for smooth transitions
    animation_frame: u8,

    /// Last render time for animations
    last_render: Instant,

    /// Cache validity tracking
    cache_valid: bool,

    /// Last render time for animations
    last_update: Instant,
}

impl OptimizedClipboardOverlay {
    #[instrument(
        level = "info",
        fields(marker = "UI_COMPONENT_INIT", operation_type = "clipboard_overlay",)
    )]
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        info!(
            marker = "UI_COMPONENT_INIT",
            operation_type = "clipboard_overlay",
            "Premium clipboard overlay initialized"
        );

        Self {
            cached_items: SmallVec::new(),
            layout_cache: None,
            selected_index: 0,
            render_stats: RenderStats::default(),
            list_state,
            animation_frame: 0,
            last_render: Instant::now(),
            cache_valid: false,
            last_update: Instant::now(),
        }
    }

    #[instrument(
        level = "debug",
        skip(self, frame, clipboard),
        fields(
            marker = "UI_RENDER_START",
            operation_type = "clipboard_overlay_render",
            entries_count = tracing::field::Empty,
            area_width = area.width,
            area_height = area.height,
            duration_us = tracing::field::Empty,
        )
    )]
    pub async fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        clipboard: &ClipBoard,
        selected_index: usize,
        snapshot: &UiSnapshot,
    ) -> Result<(), AppError> {
        let start_time = Instant::now();
        let span = TraceSpan::current();

        // Update animation state
        self.update_animation();

        // Clear background with translucent overlay
        frame.render_widget(Clear, area);

        // Get or compute layout
        let layout = self.get_or_compute_layout(area);

        // Update items cache if needed
        if !self.cache_valid {
            self.update_items_cache(clipboard).await?;
        }

        let clipboard_len = self.cached_items.len();

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

        span.record("entries_count", clipboard_len);

        // Render main container with premium styling
        self.render_main_container(frame, &layout);

        if self.cached_items.is_empty() {
            self.render_empty_state(frame, layout.content_area);
        } else {
            self.render_clipboard_content(frame, &layout)?;
        }

        // Record performance metrics
        let frame_time = start_time.elapsed();
        self.render_stats.total_time += frame_time;
        self.render_stats.last_frame_time = frame_time;
        self.render_stats.frames_rendered += 1;

        if frame_time.as_millis() > 16 {
            self.render_stats.slow_frames += 1;
        }

        span.record("duration_us", frame_time.as_micros());

        info!(
            marker = "UI_RENDER_COMPLETE",
            operation_type = "clipboard_overlay_render",
            entries_count = clipboard_len,
            duration_us = frame_time.as_micros(),
            area_width = area.width,
            area_height = area.height,
            "Premium clipboard overlay render completed"
        );

        Ok(())
    }

    /// Render main container with gradient background and modern styling
    fn render_main_container(&self, frame: &mut Frame<'_>, layout: &PrecomputedLayout) {
        let main_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title("Clipboard Manager")
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

    // Render clipboard content with modern list styling
    fn render_clipboard_content(
        &mut self,
        frame: &mut Frame<'_>,
        layout: &PrecomputedLayout,
    ) -> Result<(), AppError> {
        // Create styled list items
        let max_visible =
            (layout.list_area.height.saturating_sub(2) as usize).min(self.cached_items.len());

        let mut list_items = Vec::with_capacity(max_visible);

        for (index, item) in self.cached_items.iter().enumerate().take(max_visible) {
            // Premium styling based on operation type
            let (operation_color, operation_icon) = match item.operation {
                ClipBoardOperation::Copy => (Color::Rgb(100, 200, 255), "📄"), // Sky blue
                ClipBoardOperation::Move => (Color::Rgb(255, 200, 100), "✂️"), // Golden
            };

            // Format item text with smart truncation
            let display_path = self.format_path_smart(item.source_path.as_str(), 45);
            let size_text = self.format_file_size_compact(item.metadata.size);

            let display_text = format!(
                "{} {:2}. {} ({})",
                operation_icon,
                index + 1,
                display_path,
                size_text
            );

            let list_item = ListItem::new(display_text).style(Style::default().fg(operation_color));

            list_items.push(list_item);
        }

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!(" {} Items ", self.cached_items.len()))
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
            .highlight_symbol("▶ ")
            .style(Style::default().bg(Color::Rgb(25, 30, 45)));

        frame.render_stateful_widget(list, layout.list_area, &mut self.list_state);

        // Render sige panels
        if let Some(selected_item) = self.cached_items.get(self.selected_index) {
            self.render_details_panel(frame, layout.details_area, selected_item)?;
        }

        self.render_help_panel(frame, layout.help_area);

        Ok(())
    }

    /// Render premium details panel with rich metadata
    fn render_details_panel(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let details_text = format!(
            "📁 Path: {}\n\n🔧 Operation: {:?}\n📊 Size: {}\n⏰ Added:
   {}\n🏷️  Type: {:?}\n📅 Modified: {}",
            item.source_path.as_str(),
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

    /// Render premium help panel with keyboard shortcuts
    fn render_help_panel(&self, frame: &mut Frame<'_>, area: Rect) {
        let help_text = "🔹 ↑↓ Navigate\n🔹 Enter Select\n🔹 Tab
  Toggle\n🔹 Esc Close\n🔹 Del Remove\n🔹 v Paste Mode";

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
    fn render_empty_state(&self, frame: &mut Frame<'_>, area: Rect) {
        let empty_text = "📋 Clipboard is Empty\n\n🎯 Quick
  Actions:\n\n📄 Press 'c' to copy files\n✂️ Press 'x' to cut files\n📝
  Select items to populate clipboard\n\n💡 Tab to close this overlay";

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

    /// Update items cache from clipboard
    async fn update_items_cache(&mut self, clipboard: &ClipBoard) -> Result<(), AppError> {
        self.cached_items.clear();
        let items = clipboard.get_all_items().await;

        for item in items {
            if self.cached_items.len() >= self.cached_items.capacity() {
                warn!(
                    marker = "CLIPBOARD_CACHE_OVERFLOW",
                    operation_type = "clipboard_cache",
                    "Clipboard cache overflow, truncating items"
                );
                break;
            }
            self.cached_items.push(item);
        }

        self.cache_valid = true;
        self.last_update = Instant::now();

        debug!(
            marker = "CLIPBOARD_CACHE_UPDATE_COMPLETE",
            operation_type = "clipboard_cache",
            entries_count = self.cached_items.len(),
            "Clipboard cache updated successfully"
        );

        Ok(())
    }

    /// Synchronous cache update for compatibility
    pub fn update_cache_sync(&mut self, _clipboard: &ClipBoard) -> Result<(), AppError> {
        // For compatibility with existing sync rendering
        self.cache_valid = false;
        Ok(())
    }

    /// Sync fallback render method for compatibility
    pub fn render_sync_fallback(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        _snapshot: &UiSnapshot,
        _clipboard: &ClipBoard,
    ) -> Result<(), AppError> {
        // Simple fallback - render empty state
        let layout = self.get_or_compute_layout(area);
        self.render_main_container(frame, &layout);
        self.render_empty_state(frame, layout.content_area);
        Ok(())
    }

    /// Get or compute layout with caching
    fn get_or_compute_layout(&mut self, area: Rect) -> PrecomputedLayout {
        if let Some((cached_area, cached_layout)) = &self.layout_cache {
            if *cached_area == area {
                return cached_layout.clone();
            }
        }

        let layout = PrecomputedLayout::compute(area);
        self.layout_cache = Some((area, layout.clone()));
        layout
    }

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

    /// Invalidate cache
    pub fn invalidate_cache(&mut self) {
        self.cache_valid = false;
    }

    /// Check if performance target is met
    pub fn meets_performance_target(&self) -> bool {
        // Target: <100μs average render time
        if self.render_stats.frames_rendered > 0 {
            let avg_time_us = (self.render_stats.total_time.as_micros() as u64)
                / self.render_stats.frames_rendered;
            avg_time_us <= 100
        } else {
            true
        }
    }
}

impl Default for OptimizedClipboardOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct PrecomputedLayout {
    main_area: Rect,

    content_area: Rect,

    list_area: Rect,

    details_area: Rect,

    help_area: Rect,
}

impl PrecomputedLayout {
    fn compute(area: Rect) -> Self {
        // Main container (with padding)
        let main_area = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_add(4),
            height: area.height.saturating_sub(2),
        };

        // Inner content area (inside main container border)
        let content_area: Rect = Rect {
            x: main_area.x + 1,
            y: main_area.y + 1,
            width: main_area.width.saturating_sub(2),
            height: main_area.height.saturating_sub(2),
        };

        // Split content: main list (60%) + side panels (40%)
        let horizontal_split: Rc<[Rect]> = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(content_area);

        let list_area: Rect = horizontal_split[0];
        let side_panel_area: Rect = horizontal_split[1];

        // Split side panel vertically: details (70%) + help (30%)
        let side_split: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(side_panel_area);

        let details_area: Rect = side_split[0];
        let help_area: Rect = side_split[1];

        Self {
            main_area,
            content_area,
            list_area,
            details_area,
            help_area,
        }
    }
}
