//! Enhanced high-performance UI renderer with atomic operations and compatibility

use crate::controller::state_coordinator::StateCoordinator;
use crate::model::fs_state::PaneState;
use crate::model::ui_state::{NotificationLevel, RedrawFlag, UIOverlay, UIState};
use crate::view::components::clipboard_overlay::OptimizedClipboardOverlay;
use crate::view::components::*;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{debug, instrument, warn};

/// High-performance UI renderer with atomic optimization and smart caching
pub struct UIRenderer {
    last_render: Instant,
    frame_count: u64,
    layout_cache: LayoutCache,
    component_dirty_flags: u32,
    clipboard_overlay: OptimizedClipboardOverlay,
    render_stats: RenderStats,
}

#[derive(Debug, Clone, Default)]
struct LayoutCache {
    main_layout: Option<(Rect, [Rect; 2])>,
    overlay_layout: Option<(Rect, Rect)>,
    last_screen_size: Rect,
    cache_hits: u64,
    cache_misses: u64,
}

#[derive(Debug, Clone, Default)]
struct RenderStats {
    frames_rendered: u64,
    frames_skipped: u64,
    slow_renders: u64,
    total_render_time: std::time::Duration,
}

mod component_flags {
    pub const MAIN_TABLE: u32 = 1;
    pub const STATUS_BAR: u32 = 1 << 1;
    pub const OVERLAY: u32 = 1 << 2;
    pub const NOTIFICATION: u32 = 1 << 3;
}

impl UIRenderer {
    pub fn new() -> Self {
        Self {
            last_render: Instant::now(),
            frame_count: 0,
            layout_cache: LayoutCache::default(),
            component_dirty_flags: u32::MAX,
            clipboard_overlay: OptimizedClipboardOverlay::new(),
            render_stats: RenderStats::default(),
        }
    }

    #[instrument(level = "trace", skip(self, frame, state_coordinator))]
    pub fn render(&mut self, frame: &mut Frame<'_>, state_coordinator: &StateCoordinator) {
        let render_start = Instant::now();
        let ui_state = state_coordinator.ui_state();

        // Early return optimization with atomic check
        if self.should_skip_render(&ui_state) {
            self.render_stats.frames_skipped += 1;
            debug!("Skipping render - no redraw needed");
            return;
        }

        self.update_layout_cache(frame.area());
        let main_layout = self.get_main_layout(frame.area());

        // Render components based on what needs updating
        self.render_main_components(frame, &ui_state, state_coordinator, &main_layout);
        self.render_overlays_optimized(frame, &ui_state);
        self.render_notifications_optimized(frame, &ui_state);

        self.update_performance_metrics(render_start);
        self.component_dirty_flags = 0;
        self.frame_count += 1;
        self.render_stats.frames_rendered += 1;

        // Clear redraw flags atomically
        state_coordinator.clear_redraw();
    }

    #[inline]
    fn should_skip_render(&self, ui_state: &Arc<RwLock<UIState>>) -> bool {
        if self.frame_count == 0 {
            return false; // Always render first frame
        }

        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        !ui_guard.needs_redraw() && self.component_dirty_flags == 0
    }

    fn render_main_components(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
        state_coordinator: &StateCoordinator,
        layout: &[Rect; 2],
    ) {
        let (needs_main_redraw, needs_status_redraw) = {
            let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
            let redraw_flags = ui_guard.redraw_flags.load(Ordering::Relaxed);

            let needs_main = (redraw_flags & RedrawFlag::Main.bits() as u32) != 0
                || (self.component_dirty_flags & component_flags::MAIN_TABLE) != 0;
            let needs_status = (redraw_flags & RedrawFlag::StatusBar.bits() as u32) != 0
                || (self.component_dirty_flags & component_flags::STATUS_BAR) != 0;

            (needs_main, needs_status)
        };

        debug!(
            "Redraw flags: main={}, status={}",
            needs_main_redraw, needs_status_redraw
        );

        // Render main content area
        if needs_main_redraw {
            self.render_main_content_optimized(frame, ui_state, state_coordinator, layout[0]);
        }

        // Render status bar
        if needs_status_redraw {
            self.render_status_bar_optimized(frame, ui_state, state_coordinator, layout[1]);
        }

        // Render file operation progress overlays
        self.render_file_operations_if_needed(frame, ui_state, layout[0]);
    }

    fn render_main_content_optimized(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
        state_coordinator: &StateCoordinator,
        area: Rect,
    ) {
        debug!("Rendering main content (file table)");

        let fs_state = state_coordinator.fs_state();
        let pane = fs_state.active_pane();
        let current_path = pane.cwd.clone();
        let is_loading = pane.is_loading.load(Ordering::Relaxed);

        if !is_loading {
            // Render file table with optimized path
            let file_table = OptimizedFileTable::new();
            let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
            file_table.render_optimized(frame, &ui_guard, pane, &current_path, area);
        } else {
            // Render loading state
            self.render_loading_block(frame, area);
        }
    }

    #[inline]
    fn render_loading_block(&self, frame: &mut Frame<'_>, area: Rect) {
        let loading_block = Block::default()
            .title(" Loading Directory... ")
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(loading_block, area);
    }

    fn render_status_bar_optimized(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
        state_coordinator: &StateCoordinator,
        area: Rect,
    ) {
        debug!("Rendering status bar");
        let status_bar = OptimizedStatusBar::new();
        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        status_bar.render_with_metrics(frame, &ui_guard, state_coordinator, area);
    }

    fn render_file_operations_if_needed(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
        content_area: Rect,
    ) {
        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        if !ui_guard.active_file_operations.is_empty() {
            let area = self.calculate_progress_overlay_area(
                content_area,
                ui_guard.active_file_operations.len(),
            );
            let overlay = OptimizedFileOperationsOverlay::new();
            overlay.render_operations(frame, &ui_guard.active_file_operations, area);
        }
    }

    fn render_overlays_optimized(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
    ) {
        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        let screen_area = frame.area();

        // Render modal overlays
        if ui_guard.overlay != UIOverlay::None {
            let overlay_area = self.get_cached_overlay_area(screen_area, ui_guard.overlay);
            self.render_modal_overlay(frame, &ui_guard, overlay_area);
        }

        // Render clipboard overlay
        if ui_guard.clipboard_overlay_active {
            let clipboard_area = self.calculate_premium_clipboard_area(screen_area);
            self.render_clipboard_overlay_optimized(frame, &ui_guard, clipboard_area);
        }
    }

    fn render_modal_overlay(&mut self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        match ui_state.overlay {
            UIOverlay::Help => {
                let help_overlay = OptimizedHelpOverlay::new();
                help_overlay.render_fast(frame, area);
            }
            UIOverlay::Search | UIOverlay::FileNameSearch | UIOverlay::ContentSearch => {
                let search_overlay = OptimizedSearchOverlay::new(ui_state.overlay);
                search_overlay.render_with_input(frame, ui_state, area);
            }
            UIOverlay::SearchResults => {
                let results_overlay = OptimizedSearchResultsOverlay::new();
                results_overlay.render_results(frame, ui_state, area);
            }
            UIOverlay::Loading => {
                if let Some(loading_state) = &ui_state.loading {
                    let loading_overlay = OptimizedLoadingOverlay::new();
                    loading_overlay.render_progress(frame, loading_state, area);
                }
            }
            UIOverlay::Prompt => {
                if let Some(prompt_type) = &ui_state.input_prompt_type {
                    let prompt_overlay = OptimizedPromptOverlay::new();
                    prompt_overlay.render_input(frame, ui_state, prompt_type, area);
                }
            }
            _ => {}
        }
    }

    fn render_clipboard_overlay_optimized(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        area: Rect,
    ) {
        if let Err(e) = self
            .clipboard_overlay
            .render_from_ui_state(frame, area, ui_state)
        {
            warn!("Clipboard render error: {}", e);
            let error_area = Rect {
                y: area.y + area.height.saturating_sub(3),
                height: 3,
                ..area
            };
            ErrorOverlay::new("Clipboard render error".into()).render(frame, error_area);
        }
    }

    fn render_notifications_optimized(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &Arc<RwLock<UIState>>,
    ) {
        let ui_guard = ui_state.read().expect("UIState RwLock poisoned");
        if let Some(notification) = &ui_guard.notification {
            let area = self.calculate_notification_area(frame.area(), notification);
            let overlay = OptimizedNotificationOverlay::new();
            overlay.render_notification(frame, notification, area);
        }
    }

    fn update_layout_cache(&mut self, screen_size: Rect) {
        if self.layout_cache.last_screen_size != screen_size {
            self.layout_cache.main_layout =
                Some((screen_size, self.calculate_main_layout(screen_size)));
            self.layout_cache.overlay_layout = None;
            self.layout_cache.last_screen_size = screen_size;
            self.component_dirty_flags = u32::MAX;
            self.layout_cache.cache_misses += 1;
        } else {
            self.layout_cache.cache_hits += 1;
        }
    }

    #[inline]
    fn get_main_layout(&self, area: Rect) -> [Rect; 2] {
        self.layout_cache
            .main_layout
            .as_ref()
            .map(|(_, layout)| *layout)
            .unwrap_or_else(|| self.calculate_main_layout(area))
    }

    #[inline]
    fn calculate_main_layout(&self, area: Rect) -> [Rect; 2] {
        let layout = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]);
        let [content, status] = layout.areas(area);
        [content, status]
    }

    fn get_cached_overlay_area(&mut self, screen_size: Rect, overlay_type: UIOverlay) -> Rect {
        if let Some((cached_size, cached_area)) = self.layout_cache.overlay_layout
            && cached_size == screen_size
        {
            self.layout_cache.cache_hits += 1;
            return cached_area;
        }

        self.layout_cache.cache_misses += 1;
        let area = match overlay_type {
            UIOverlay::Help => self.calculate_centered_overlay(screen_size, 80, 80),
            UIOverlay::Search | UIOverlay::FileNameSearch | UIOverlay::ContentSearch => {
                self.calculate_search_overlay_area(screen_size)
            }
            UIOverlay::SearchResults => self.calculate_centered_overlay(screen_size, 90, 70),
            UIOverlay::Loading => self.calculate_centered_overlay(screen_size, 50, 30),
            UIOverlay::Prompt => self.calculate_prompt_overlay_area(screen_size),
            _ => self.calculate_centered_overlay(screen_size, 70, 60),
        };

        self.layout_cache.overlay_layout = Some((screen_size, area));
        area
    }

    #[inline]
    fn calculate_premium_clipboard_area(&self, area: Rect) -> Rect {
        self.calculate_centered_overlay(area, 85, 80)
    }

    #[inline]
    fn calculate_progress_overlay_area(&self, screen_size: Rect, op_count: usize) -> Rect {
        let height = (op_count * 3 + 2) as u16;
        Rect {
            x: 1,
            y: screen_size.height.saturating_sub(height + 2),
            width: screen_size.width.saturating_sub(2),
            height: height.min(screen_size.height / 3),
        }
    }

    fn calculate_notification_area(
        &self,
        screen_size: Rect,
        notification: &crate::model::ui_state::Notification,
    ) -> Rect {
        let height = match notification.level {
            NotificationLevel::Error => 5,
            _ => 3,
        };
        let width = (screen_size.width * 60 / 100).max(40);
        Rect {
            x: (screen_size.width.saturating_sub(width)) / 2,
            y: 2,
            width,
            height,
        }
    }

    #[inline]
    fn calculate_centered_overlay(
        &self,
        area: Rect,
        width_percent: u16,
        height_percent: u16,
    ) -> Rect {
        let width = (area.width * width_percent / 100).min(area.width);
        let height = (area.height * height_percent / 100).min(area.height);
        Rect {
            x: (area.width.saturating_sub(width)) / 2,
            y: (area.height.saturating_sub(height)) / 2,
            width,
            height,
        }
    }

    #[inline]
    fn calculate_search_overlay_area(&self, area: Rect) -> Rect {
        let height = 5;
        let width = (area.width * 70 / 100).max(40);
        Rect {
            x: (area.width.saturating_sub(width)) / 2,
            y: area.height / 4,
            width,
            height,
        }
    }

    #[inline]
    fn calculate_prompt_overlay_area(&self, area: Rect) -> Rect {
        let height = 7;
        let width = (area.width * 60 / 100).max(50);
        Rect {
            x: (area.width.saturating_sub(width)) / 2,
            y: (area.height.saturating_sub(height)) / 2,
            width,
            height,
        }
    }

    fn update_performance_metrics(&mut self, render_start: Instant) {
        let render_duration = render_start.elapsed();
        self.render_stats.total_render_time += render_duration;

        if render_duration.as_millis() > 16 {
            self.render_stats.slow_renders += 1;
            warn!("Slow render detected: {:?}", render_duration);
        }

        self.last_render = Instant::now();
    }

    /// Get performance statistics for monitoring
    pub fn get_render_stats(&self) -> &RenderStats {
        &self.render_stats
    }

    /// Get layout cache statistics
    pub fn get_cache_stats(&self) -> (u64, u64, f64) {
        let hits = self.layout_cache.cache_hits;
        let misses = self.layout_cache.cache_misses;
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };
        (hits, misses, hit_rate)
    }

    /// Reset performance counters
    pub fn reset_stats(&mut self) {
        self.render_stats = RenderStats::default();
        self.layout_cache.cache_hits = 0;
        self.layout_cache.cache_misses = 0;
    }

    /// Force invalidate all caches
    pub fn invalidate_caches(&mut self) {
        self.layout_cache.main_layout = None;
        self.layout_cache.overlay_layout = None;
        self.component_dirty_flags = u32::MAX;
    }

    /// Update clipboard overlay cache asynchronously
    pub async fn update_clipboard_cache(
        &mut self,
        ui_state: &UIState,
    ) -> Result<(), crate::error::AppError> {
        self.clipboard_overlay
            .update_cache(&ui_state.clipboard)
            .await
    }
}

impl Default for UIRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderStats {
    /// Calculate average render time
    pub fn average_render_time(&self) -> std::time::Duration {
        if self.frames_rendered > 0 {
            self.total_render_time / self.frames_rendered as u32
        } else {
            std::time::Duration::ZERO
        }
    }

    /// Calculate frames per second based on total render time
    pub fn estimated_fps(&self) -> f64 {
        let avg_frame_time = self.average_render_time();
        if avg_frame_time.as_secs_f64() > 0.0 {
            1.0 / avg_frame_time.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Get percentage of slow renders
    pub fn slow_render_percentage(&self) -> f64 {
        if self.frames_rendered > 0 {
            (self.slow_renders as f64 / self.frames_rendered as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get frame skip ratio
    pub fn skip_ratio(&self) -> f64 {
        let total_attempts = self.frames_rendered + self.frames_skipped;
        if total_attempts > 0 {
            self.frames_skipped as f64 / total_attempts as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState};
    use std::sync::{Mutex, RwLock};

    fn create_test_state_coordinator() -> Arc<StateCoordinator> {
        let app_state = Arc::new(Mutex::new(AppState::default()));
        let ui_state = RwLock::new(UIState::default());
        let fs_state = Arc::new(Mutex::new(FSState::default()));

        Arc::new(StateCoordinator::new(app_state, ui_state, fs_state))
    }

    #[test]
    fn test_layout_calculation() {
        let renderer = UIRenderer::new();
        let area = Rect::new(0, 0, 100, 50);
        let layout = renderer.calculate_main_layout(area);

        assert_eq!(layout.len(), 2);
        assert!(layout[0].height > layout[1].height); // Content area should be larger than status
        assert_eq!(layout[1].height, 1); // Status bar should be 1 line
    }

    #[test]
    fn test_overlay_area_calculation() {
        let renderer = UIRenderer::new();
        let screen = Rect::new(0, 0, 100, 50);

        let help_area = renderer.calculate_centered_overlay(screen, 80, 80);
        assert!(help_area.width <= screen.width);
        assert!(help_area.height <= screen.height);

        let search_area = renderer.calculate_search_overlay_area(screen);
        assert_eq!(search_area.height, 5);
    }

    #[test]
    fn test_cache_hit_tracking() {
        let mut renderer = UIRenderer::new();
        let screen = Rect::new(0, 0, 100, 50);

        // First access should be a cache miss
        renderer.update_layout_cache(screen);

        // Second access should be a cache hit
        renderer.update_layout_cache(screen);

        let (hits, misses, _) = renderer.get_cache_stats();
        assert!(hits > 0);
        assert!(misses > 0);
    }

    #[test]
    fn test_performance_tracking() {
        let mut renderer = UIRenderer::new();
        let start = Instant::now();

        // Simulate a slow render
        std::thread::sleep(std::time::Duration::from_millis(20));
        renderer.update_performance_metrics(start);

        let stats = renderer.get_render_stats();
        assert!(stats.slow_renders > 0);
        assert!(stats.total_render_time.as_millis() >= 20);
    }
}
