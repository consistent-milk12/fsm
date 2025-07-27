//! ui.rs - Updated High-Performance UI Renderer for Phase 4.0
//!
//! This file contains a rewrite of the original `UIRenderer` to work with the
//! new `StateCoordinator` API.  The original implementation assumed that
//! `StateCoordinator` provided helper methods like `current_directory()` and
//! `get_dir_state()`.  In the rewritten design, all application state is
//! accessed through a single `StateCoordinator` which internally holds
//! `AppState`, `UIState` and `FSState` behind locks.  To render the main
//! file table we now lock the filesystem state, fetch the active pane and
//! its current working directory, and then either render the directory
//! contents or a loading widget depending on the pane’s loading flag.  The
//! rest of the renderer remains largely unchanged, retaining the layout
//! caching and zero‑allocation overlay logic.

use crate::controller::state_coordinator::StateCoordinator;
use crate::model::fs_state::PaneState;
use crate::model::ui_state::{NotificationLevel, RedrawFlag, UIOverlay, UIState};
use crate::view::components::*;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// High-performance UI renderer with atomic optimization
pub struct UIRenderer {
    last_render: Instant,
    frame_count: u64,
    layout_cache: LayoutCache,
    component_dirty_flags: u32,
}

#[derive(Debug, Clone, Default)]
struct LayoutCache {
    main_layout: Option<(Rect, [Rect; 2])>,
    overlay_layout: Option<(Rect, Rect)>,
    last_screen_size: Rect,
}

mod component_flags {
    pub const MAIN_TABLE: u32 = 1;
    pub const STATUS_BAR: u32 = 1 << 1;
}

impl UIRenderer {
    pub fn new() -> Self {
        Self {
            last_render: Instant::now(),
            frame_count: 0,
            layout_cache: LayoutCache::default(),
            component_dirty_flags: u32::MAX,
        }
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, state_coordinator: &StateCoordinator) {
        tracing::debug!("UIRenderer::render called.");
        let render_start = Instant::now();
        let ui_state = state_coordinator.ui_state();

        // Early return optimization - skip rendering if nothing needs redraw
        if !ui_state.needs_redraw() && self.component_dirty_flags == 0 && self.frame_count > 0 {
            tracing::debug!("Skipping render - no redraw needed");
            return;
        }

        self.update_layout_cache(frame.area());

        let main_layout = self
            .layout_cache
            .main_layout
            .as_ref()
            .map(|(_, layout)| *layout)
            .unwrap_or_else(|| self.calculate_main_layout(frame.area()));

        self.render_main_components(frame, &ui_state, state_coordinator, &main_layout);

        if ui_state.overlay != UIOverlay::None || ui_state.clipboard_overlay_active {
            self.render_overlays(frame, &ui_state);
        }

        if ui_state.notification.is_some() {
            self.render_notifications(frame, &ui_state);
        }

        self.update_performance_metrics(render_start);
        self.component_dirty_flags = 0;
        self.frame_count += 1;

        // Clear redraw flags AFTER rendering completes
        ui_state.clear_redraw();
    }

    fn render_main_components(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        state_coordinator: &StateCoordinator,
        layout: &[Rect; 2],
    ) {
        tracing::debug!("render_main_components called, layout: {:?}", layout);

        let redraw_flags = ui_state.redraw_flags.load(Ordering::Relaxed);
        let needs_main_redraw = (redraw_flags & RedrawFlag::Main.bits() as u32) != 0
            || (self.component_dirty_flags & component_flags::MAIN_TABLE) != 0;
        let needs_status_redraw = (redraw_flags & RedrawFlag::StatusBar.bits() as u32) != 0
            || (self.component_dirty_flags & component_flags::STATUS_BAR) != 0;

        tracing::debug!(
            "Redraw flags: main={}, status={}",
            needs_main_redraw,
            needs_status_redraw
        );

        if needs_main_redraw {
            tracing::debug!("Rendering main content (file table).");
            // Acquire the filesystem state and fetch the active pane and its path.
            {
                let fs_state = state_coordinator.fs_state();
                let pane: &PaneState = fs_state.active_pane();
                let current_path: PathBuf = pane.cwd.clone();
                // Determine if the pane is still loading entries.
                let is_loading = pane.is_loading.load(Ordering::Relaxed);
                if !is_loading {
                    let _fs_st = fs_state.clone();

                    let file_table = OptimizedFileTable::new();
                    // Render the file table using the pane state and current path.
                    file_table.render_optimized(frame, ui_state, pane, &current_path, layout[0]);
                } else {
                    // Render a loading state if the directory isn't in the cache yet
                    let loading_block = Block::default()
                        .title(" Loading Directory... ")
                        .borders(Borders::ALL);
                    frame.render_widget(loading_block, layout[0]);
                }
                // fs_state lock is dropped here at the end of the scope
            }
        }

        if needs_status_redraw {
            tracing::debug!("Rendering status bar.");
            let status_bar = OptimizedStatusBar::new();
            status_bar.render_with_metrics(frame, ui_state, state_coordinator, layout[1]);
        }

        if !ui_state.active_file_operations.is_empty() {
            self.render_file_operations_progress(frame, ui_state, layout[0]);
        }
    }

    fn render_overlays(&mut self, frame: &mut Frame<'_>, ui_state: &UIState) {
        let screen_area = frame.area();
        if ui_state.overlay != UIOverlay::None {
            let overlay_area = self.get_cached_overlay_area(screen_area, ui_state.overlay);
            self.render_modal_overlay(frame, ui_state, overlay_area);
        }
        if ui_state.clipboard_overlay_active {
            let clipboard_area = self.calculate_premium_clipboard_area(screen_area);
            self.render_clipboard_overlay(frame, ui_state, clipboard_area);
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

    fn render_clipboard_overlay(&mut self, frame: &mut Frame<'_>, ui_state: &UIState, area: Rect) {
        let clipboard_overlay = OptimizedClipboardOverlay::new();
        if clipboard_overlay
            .render_zero_alloc(
                frame,
                area,
                &ui_state.clipboard,
                ui_state.selected_clipboard_item_index,
            )
            .is_err()
        {
            let error_area = Rect {
                y: area.y + area.height.saturating_sub(3),
                height: 3,
                ..area
            };
            ErrorOverlay::new("Clipboard render error".into()).render(frame, error_area);
        }
    }

    fn render_notifications(&mut self, frame: &mut Frame<'_>, ui_state: &UIState) {
        if let Some(notification) = &ui_state.notification {
            let area = self.calculate_notification_area(frame.area(), notification);
            let overlay = OptimizedNotificationOverlay::new();
            overlay.render_notification(frame, notification, area);
        }
    }

    fn render_file_operations_progress(
        &mut self,
        frame: &mut Frame<'_>,
        ui_state: &UIState,
        content_area: Rect,
    ) {
        let area = self
            .calculate_progress_overlay_area(content_area, ui_state.active_file_operations.len());
        let overlay = OptimizedFileOperationsOverlay::new();
        overlay.render_operations(frame, &ui_state.active_file_operations, area);
    }

    fn update_layout_cache(&mut self, screen_size: Rect) {
        if self.layout_cache.last_screen_size != screen_size {
            self.layout_cache.main_layout =
                Some((screen_size, self.calculate_main_layout(screen_size)));
            self.layout_cache.overlay_layout = None;
            self.layout_cache.last_screen_size = screen_size;
            self.component_dirty_flags = u32::MAX;
        }
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
            return cached_area;
        }
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
        self.frame_count += 1;
        let render_duration = render_start.elapsed();
        if render_duration.as_millis() > 16 {
            tracing::warn!("Slow render detected: {:?}", render_duration);
        }
        self.last_render = Instant::now();
    }
}

impl Default for UIRenderer {
    fn default() -> Self {
        Self::new()
    }
}
