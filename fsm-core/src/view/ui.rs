//! src/view/ui.rs
//! Enhanced UI renderer with proper state integration and lock-free rendering

use ratatui::{
    prelude::*,
    widgets::{Block, Borders},
};
use std::{collections::HashMap, sync::atomic::Ordering, time::Instant};
use tracing::{debug, instrument, warn};

use crate::{
    controller::state_coordinator::StateCoordinator,
    model::ui_state::{NotificationLevel, RedrawFlag, UIOverlay},
    view::{
        components::{
            clipboard_overlay::OptimizedClipboardOverlay, error_overlay::ErrorOverlay,
            file_operations_overlay::OptimizedFileOperationsOverlay,
            help_overlay::OptimizedHelpOverlay, input_prompt_overlay::OptimizedPromptOverlay,
            loading_overlay::OptimizedLoadingOverlay,
            notification_overlay::OptimizedNotificationOverlay, object_table::OptimizedFileTable,
            search_overlay::OptimizedSearchOverlay,
            search_results_overlay::OptimizedSearchResultsOverlay, status_bar::OptimizedStatusBar,
            system_monitor_overlay::OptimizedSystemMonitorOverlay,
        },
        snapshots::{PromptSnapshot, SearchSnapshot, UiSnapshot},
    },
};

/// Enhanced UI renderer with component caches and performance tracking
pub struct UIRenderer {
    /// Layout area caches
    cache: LayoutCache,

    /// Component instances with internal state
    clipboard_overlay: OptimizedClipboardOverlay,
    file_table: OptimizedFileTable,
    status_bar: OptimizedStatusBar,
    help_overlay: OptimizedHelpOverlay,
    prompt_overlay: OptimizedPromptOverlay,
    loading_overlay: OptimizedLoadingOverlay,
    notification_overlay: OptimizedNotificationOverlay,
    search_overlay: OptimizedSearchOverlay,
    search_results_overlay: OptimizedSearchResultsOverlay,
    file_ops_overlay: OptimizedFileOperationsOverlay,
    system_monitor_overlay: OptimizedSystemMonitorOverlay,

    /// Performance tracking
    stats: RenderStats,
    frame_count: u64,
    dirty_flags: u32,
}

#[derive(Default)]
pub struct LayoutCache {
    main_layout: Option<(Rect, [Rect; 2])>,
    overlay_areas: HashMap<UIOverlay, Rect>,
    screen_size: Rect,
    hit_count: u64,
    miss_count: u64,
}

#[derive(Default)]
pub struct RenderStats {
    pub frames_rendered: u64,
    pub frames_skipped: u64,
    pub slow_frames: u64,
    pub total_time: std::time::Duration,
    pub last_frame_time: std::time::Duration,
}

impl UIRenderer {
    pub fn new() -> Self {
        debug!("Creating new UIRenderer instance");
        Self {
            cache: LayoutCache::default(),
            clipboard_overlay: OptimizedClipboardOverlay::new(),
            file_table: OptimizedFileTable::new(),
            status_bar: OptimizedStatusBar::new(),
            help_overlay: OptimizedHelpOverlay::new(),
            prompt_overlay: OptimizedPromptOverlay::new(),
            loading_overlay: OptimizedLoadingOverlay::new(),
            notification_overlay: OptimizedNotificationOverlay::new(),
            search_overlay: OptimizedSearchOverlay::new(UIOverlay::FileNameSearch),
            search_results_overlay: OptimizedSearchResultsOverlay::new(),
            file_ops_overlay: OptimizedFileOperationsOverlay::new(),
            system_monitor_overlay: OptimizedSystemMonitorOverlay::new(),
            stats: RenderStats::default(),
            frame_count: 0,
            dirty_flags: u32::MAX,
        }
    }

    /// Main render entry point with enhanced state integration
    #[instrument(level = "trace", skip(self, frame, coord))]
    pub fn render(&mut self, frame: &mut Frame<'_>, coord: &StateCoordinator) {
        let render_start = Instant::now();

        // Create immutable UI snapshot
        let ui_snapshot = self.create_ui_snapshot(coord);

        // Skip if no redraw needed (but always render on RedrawFlag::All)
        if self.frame_count > 0 && ui_snapshot.redraw_flags == 0 && self.dirty_flags == 0 {
            self.stats.frames_skipped += 1;
            debug!(
                frame = self.frame_count,
                redraw_flags = ui_snapshot.redraw_flags,
                dirty_flags = self.dirty_flags,
                "Frame skipped - no redraw needed"
            );
            return;
        }

        // Update layout cache
        self.update_layout_cache(frame.area());

        // Get main layout areas
        let main_areas = self
            .cache
            .main_layout
            .as_ref()
            .expect("Layout cache populated")
            .1;

        // Render components
        self.render_main_content(frame, &ui_snapshot, coord, main_areas[0]);
        self.render_status_bar(frame, &ui_snapshot, coord, main_areas[1]);
        self.render_overlays(frame, &ui_snapshot, coord);

        // Update performance metrics
        let frame_time = render_start.elapsed();
        self.stats.total_time += frame_time;
        self.stats.last_frame_time = frame_time;
        self.stats.frames_rendered += 1;
        self.frame_count += 1;

        if frame_time.as_millis() > 16 {
            self.stats.slow_frames += 1;
        }

        self.dirty_flags = 0;
    }

    /// Create UI snapshot from enhanced state
    #[instrument(level = "trace", skip(self, coord))]
    fn create_ui_snapshot(&self, coord: &StateCoordinator) -> UiSnapshot {
        let ui_state = coord.ui_state();
        let ui_guard = ui_state.read().expect("UI state lock poisoned");
        UiSnapshot::from(&*ui_guard)
    }

    /// Update layout cache on screen size change
    #[instrument(level = "trace", skip(self), fields(width = screen_size.width, height = screen_size.height))]
    fn update_layout_cache(&mut self, screen_size: Rect) {
        if self.cache.screen_size == screen_size {
            self.cache.hit_count += 1;
            return;
        }

        self.cache.screen_size = screen_size;
        self.cache.miss_count += 1;

        // Main layout: content + status bar
        let main_areas =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(screen_size);

        self.cache.main_layout = Some((screen_size, main_areas));
        self.cache.overlay_areas.clear();
        self.dirty_flags = u32::MAX;
    }

    /// Render main file browser with enhanced pane state
    fn render_main_content(
        &mut self,
        frame: &mut Frame<'_>,
        ui_snapshot: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        // Always render main content during navigation to prevent UI corruption
        // Skip only if explicitly no Main redraw flag AND no dirty flags
        if ui_snapshot.redraw_flags & RedrawFlag::Main.bits() as u32 == 0
            && self.dirty_flags & 1 == 0
            && ui_snapshot.redraw_flags == 0
        // No redraw flags at all
        {
            return;
        }

        let fs_state = coord.fs_state();
        let active_pane = fs_state.active_pane();

        // Show loading or file table
        if active_pane
            .is_loading
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.render_loading_placeholder(frame, area);
        } else {
            // Get UI state for file table
            let ui_state = coord.ui_state();
            let ui_guard = ui_state.read().expect("UI state lock poisoned");

            self.file_table
                .render_optimized(frame, &ui_guard, active_pane, &active_pane.cwd, area);
        }

        self.dirty_flags |= 1;
    }

    /// Render status bar with enhanced metrics
    fn render_status_bar(
        &mut self,
        frame: &mut Frame<'_>,
        ui_snapshot: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        if ui_snapshot.redraw_flags & RedrawFlag::StatusBar.bits() as u32 == 0
            && self.dirty_flags & 2 == 0
        {
            return;
        }

        self.status_bar
            .render_with_metrics(frame, ui_snapshot, coord, area);
        self.dirty_flags |= 2;
    }

    /// Render all overlays with enhanced integration
    fn render_overlays(
        &mut self,
        frame: &mut Frame<'_>,
        ui_snapshot: &UiSnapshot,
        coord: &StateCoordinator,
    ) {
        let screen_size = frame.area();

        // Modal overlays
        if ui_snapshot.overlay != UIOverlay::None {
            let overlay_area = self.get_overlay_area(screen_size, ui_snapshot.overlay);
            self.render_modal_overlay(frame, ui_snapshot, coord, overlay_area);
        }

        // Clipboard overlay
        if ui_snapshot.clipboard_active {
            let clipboard_area = self.centered_rect(screen_size, 85, 80);
            if self
                .clipboard_overlay
                .render_sync_fallback(frame, clipboard_area, ui_snapshot)
                .is_err()
            {
                self.render_error_overlay(frame, "Clipboard unavailable", clipboard_area);
            }
        }

        // File operations progress (using enhanced FSState)
        self.render_file_operations_progress(frame, coord, screen_size);

        // Notifications
        if let Some(notification) = &ui_snapshot.notification {
            let notification_area = self.notification_rect(screen_size, notification.level);
            self.notification_overlay
                .render_notification(frame, notification, notification_area);
        }
    }

    /// Render modal overlays with proper snapshot creation
    fn render_modal_overlay(
        &mut self,
        frame: &mut Frame<'_>,
        ui_snapshot: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        match ui_snapshot.overlay {
            UIOverlay::Help => {
                self.help_overlay.render_fast(frame, area);
            }

            UIOverlay::FileNameSearch | UIOverlay::ContentSearch => {
                let search_snapshot = self.create_search_snapshot(coord, ui_snapshot);
                self.search_overlay = OptimizedSearchOverlay::new(ui_snapshot.overlay);
                self.search_overlay
                    .render_with_input(frame, &search_snapshot, area);
            }

            UIOverlay::SearchResults => {
                let fs_state = coord.fs_state();
                let active_pane = fs_state.active_pane();
                let selected = active_pane
                    .selected
                    .load(std::sync::atomic::Ordering::Relaxed);

                self.search_results_overlay.render_results(
                    frame,
                    &active_pane.search_results,
                    Some(selected),
                    area,
                );
            }

            UIOverlay::Prompt => {
                if let Some(prompt_snapshot) = self.create_prompt_snapshot(coord) {
                    self.prompt_overlay
                        .render_input(frame, &prompt_snapshot, area);
                }
            }

            UIOverlay::Loading => {
                if let Some(loading_state) = &ui_snapshot.loading {
                    self.loading_overlay
                        .render_progress(frame, loading_state, area);
                }
            }

            UIOverlay::SystemMonitor => {
                let app_guard = coord.app_state();
                self.system_monitor_overlay.render_system_monitor(
                    frame,
                    ui_snapshot,
                    &app_guard,
                    area,
                );
            }

            _ => {
                self.render_error_overlay(frame, "Unknown overlay", area);
            }
        }
    }

    /// Create search snapshot with enhanced state integration
    fn create_search_snapshot(
        &self,
        coord: &StateCoordinator,
        ui_snapshot: &UiSnapshot,
    ) -> SearchSnapshot {
        let fs_state = coord.fs_state();
        let active_pane = fs_state.active_pane();
        let ui_state = coord.ui_state();
        let ui_guard = ui_state.read().expect("UI state lock poisoned");

        SearchSnapshot::from_states(&ui_guard, active_pane).unwrap_or_else(|| SearchSnapshot {
            query: ui_snapshot.search_query.clone().unwrap_or_default(),
            cursor: 0,
            results: std::sync::Arc::from([]),
            mode: ui_snapshot.search_mode,
            is_searching: active_pane.is_loading.load(Ordering::Relaxed),
            total_matches: 0,
            selected_idx: 0,
        })
    }

    /// Create prompt snapshot from enhanced UI state
    fn create_prompt_snapshot(&self, coord: &StateCoordinator) -> Option<PromptSnapshot> {
        let ui_state = coord.ui_state();
        let ui_guard = ui_state.read().expect("UI state lock poisoned");
        PromptSnapshot::from_ui(&ui_guard)
    }

    /// Enhanced file operations progress with proper FSState integration
    fn render_file_operations_progress(
        &mut self,
        _frame: &mut Frame<'_>,
        _coord: &StateCoordinator,
        _screen_size: Rect,
    ) {
        // Operation tracking is now handled by FileSystemOperator
        // TODO: Implement operation progress display via FileSystemOperator if needed
        // The old operation progress display has been removed as part of cleanup
    }

    /// Get or compute overlay area with caching
    fn get_overlay_area(&mut self, screen_size: Rect, overlay_type: UIOverlay) -> Rect {
        if let Some(&area) = self.cache.overlay_areas.get(&overlay_type) {
            return area;
        }

        let area = match overlay_type {
            UIOverlay::Help => self.centered_rect(screen_size, 85, 90),
            UIOverlay::FileNameSearch | UIOverlay::ContentSearch => {
                self.search_input_rect(screen_size)
            }
            UIOverlay::SearchResults => self.centered_rect(screen_size, 90, 70),
            UIOverlay::Prompt => self.centered_rect(screen_size, 60, 25),
            UIOverlay::Loading => self.centered_rect(screen_size, 50, 30),
            UIOverlay::SystemMonitor => self.centered_rect(screen_size, 85, 80),
            _ => self.centered_rect(screen_size, 70, 60),
        };

        self.cache.overlay_areas.insert(overlay_type, area);
        area
    }

    /// Calculate centered rectangle
    fn centered_rect(&self, container: Rect, width_pct: u16, height_pct: u16) -> Rect {
        let width = (container.width * width_pct / 100).min(container.width);
        let height = (container.height * height_pct / 100).min(container.height);

        Rect {
            x: container.x + (container.width.saturating_sub(width)) / 2,
            y: container.y + (container.height.saturating_sub(height)) / 2,
            width,
            height,
        }
    }

    /// Search input area positioning
    fn search_input_rect(&self, screen_size: Rect) -> Rect {
        let width = (screen_size.width * 70) / 100;
        let height = 8;

        Rect {
            x: (screen_size.width.saturating_sub(width)) / 2,
            y: screen_size.height / 4,
            width,
            height,
        }
    }

    /// Notification positioning based on level
    fn notification_rect(&self, screen_size: Rect, level: NotificationLevel) -> Rect {
        let width = (screen_size.width * 60) / 100;
        let height = match level {
            NotificationLevel::Error => 6,
            _ => 4,
        };

        Rect {
            x: (screen_size.width.saturating_sub(width)) / 2,
            y: 2,
            width,
            height,
        }
    }

    /// Loading placeholder for directory scan
    fn render_loading_placeholder(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title(" Loading Directory... ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black));

        frame.render_widget(block, area);
    }

    /// Generic error overlay
    fn render_error_overlay(&self, frame: &mut Frame<'_>, message: &str, area: Rect) {
        ErrorOverlay::new(message.to_string()).render(frame, area);
    }

    /// Performance statistics access
    pub fn get_stats(&self) -> &RenderStats {
        &self.stats
    }

    /// Reset performance counters
    pub fn reset_stats(&mut self) {
        self.stats = RenderStats::default();
        self.frame_count = 0;
    }
}

impl Default for UIRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderStats {
    /// Calculate frames per second
    pub fn fps(&self) -> f64 {
        if self.total_time.as_secs_f64() > 0.0 {
            self.frames_rendered as f64 / self.total_time.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Frame time in milliseconds
    pub fn frame_time_ms(&self) -> f64 {
        self.last_frame_time.as_secs_f64() * 1000.0
    }

    /// Cache efficiency
    pub fn cache_hit_rate(&self, cache: &LayoutCache) -> f64 {
        let total = cache.hit_count + cache.miss_count;
        if total > 0 {
            cache.hit_count as f64 / total as f64
        } else {
            0.0
        }
    }
}
