//! src/view/renderer.rs
//! ============================================================
//! Frame renderer that draws the entire TUI from an *immutable*
//! `UiSnapshot`.  No locks are taken while painting.

use std::{collections::HashMap, time::Instant};

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use tracing::{debug, instrument, warn};

use crate::{
    controller::state_coordinator::StateCoordinator,
    fs::object_info::ObjectInfo,
    model::{
        fs_state::PaneState,
        snapshots::{SearchSnapshot, UiSnapshot},
        ui_state::{NotificationLevel, RedrawFlag, UIOverlay},
    },
    view::components::{
        clipboard_overlay::OptimizedClipboardOverlay, error_overlay::ErrorOverlay,
        file_operations_overlay::OptimizedFileOperationsOverlay,
        help_overlay::OptimizedHelpOverlay, input_prompt_overlay::OptimizedPromptOverlay,
        loading_overlay::OptimizedLoadingOverlay,
        notification_overlay::OptimizedNotificationOverlay, object_table::OptimizedFileTable,
        search_overlay::OptimizedSearchOverlay,
        search_results_overlay::OptimizedSearchResultsOverlay, status_bar::OptimizedStatusBar,
    },
};

/// ---------------------------------------------------------------------------
/// Renderer struct (contains only caches + stats)
/// ---------------------------------------------------------------------------
pub struct UIRenderer {
    /// layout / area caches
    cache: LayoutCache,
    /// clipboard overlay (holds its own cache)
    clip: OptimizedClipboardOverlay,
    /// perf
    stats: RenderStats,
    frame: u64,
    dirty: u32,
}

#[derive(Default)]
struct LayoutCache {
    main: Option<(Rect, [Rect; 2])>,
    overlay: Option<(Rect, Rect)>,
    screen: Rect,
    hit: u64,
    miss: u64,
}

#[derive(Default)]
pub struct RenderStats {
    pub frames: u64,
    pub skips: u64,
    pub slow: u64,
    pub total: std::time::Duration,
}

/// ---------------------------------------------------------------------------
/// ctor
/// ---------------------------------------------------------------------------
impl UIRenderer {
    pub fn new() -> Self {
        Self {
            cache: LayoutCache::default(),
            clip: OptimizedClipboardOverlay::new(),
            stats: RenderStats::default(),
            frame: 0,
            dirty: u32::MAX,
        }
    }
}

/// ---------------------------------------------------------------------------
/// public API
/// ---------------------------------------------------------------------------
impl UIRenderer {
    #[instrument(level = "trace", skip(self, f, snap, coord))]
    pub fn render(&mut self, f: &mut Frame<'_>, snap: &UiSnapshot, coord: &StateCoordinator) {
        // -----------------------------------------------------
        // quick bail-out if nothing changed
        // -----------------------------------------------------
        if self.frame > 0 && snap.redraw_flags == 0 && self.dirty == 0 {
            self.stats.skips += 1;
            return;
        }

        let start = Instant::now();
        self.update_layout_cache(f.size());

        // -----------------------------------------------------
        // split layout (content + status bar)
        // -----------------------------------------------------
        let main = self.cache.main.as_ref().expect("cache filled").1;

        self.draw_main(f, snap, coord, main[0]);
        self.draw_status_bar(f, snap, coord, main[1]);
        self.draw_overlays(f, snap, coord);

        // perf
        let dur = start.elapsed();
        self.stats.total += dur;
        if dur.as_millis() > 16 {
            self.stats.slow += 1;
        }
        self.stats.frames += 1;
        self.frame += 1;
        self.dirty = 0; // reset component dirty flags
    }
}

/// ---------------------------------------------------------------------------
/// main components
/// ---------------------------------------------------------------------------
impl UIRenderer {
    fn draw_main(
        &mut self,
        f: &mut Frame<'_>,
        snap: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        if snap.redraw_flags & RedrawFlag::Main.bits() as u32 == 0 && self.dirty & 1 == 0 {
            return;
        }

        let fs = coord.fs_state();
        let pane: &PaneState = fs.active_pane();

        if pane.is_loading.load(std::sync::atomic::Ordering::Relaxed) {
            self.draw_dir_loading(f, area);
        } else {
            let file_table = OptimizedFileTable::new();
            file_table.render_optimized(f, pane, area);
        }
        self.dirty |= 1;
    }

    fn draw_dir_loading(&self, f: &mut Frame<'_>, r: Rect) {
        let b = Block::default()
            .title(" Loading… ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        f.render_widget(b, r);
    }

    fn draw_status_bar(
        &mut self,
        f: &mut Frame<'_>,
        snap: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        if snap.redraw_flags & RedrawFlag::StatusBar.bits() as u32 == 0 && self.dirty & 2 == 0 {
            return;
        }

        OptimizedStatusBar::new().render_with_metrics(f, snap, coord, area);
        self.dirty |= 2;
    }
}

/// ---------------------------------------------------------------------------
/// overlays
/// ---------------------------------------------------------------------------
impl UIRenderer {
    fn draw_overlays(&mut self, f: &mut Frame<'_>, snap: &UiSnapshot, coord: &StateCoordinator) {
        let scr = f.size();

        // modal overlay ------------------------------------------------------
        if snap.overlay != UIOverlay::None {
            let o_rect = self.overlay_area(scr, snap.overlay);
            self.draw_modal(f, snap, coord, o_rect);
        }

        // clipboard overlay --------------------------------------------------
        if snap.clipboard_active {
            let r = self.centered(scr, 85, 80);
            if let Err(_) = self.clip.render_sync_fallback(f, r, snap) {
                self.draw_error(f, "Clipboard error", r);
            }
        }

        // toast / banner -----------------------------------------------------
        if let Some(n) = &snap.notification {
            let r = self.notification_rect(scr, n.level);
            OptimizedNotificationOverlay::new().render_notification(f, n, r);
        }
    }

    fn draw_modal(
        &mut self,
        f: &mut Frame<'_>,
        snap: &UiSnapshot,
        coord: &StateCoordinator,
        area: Rect,
    ) {
        match snap.overlay {
            UIOverlay::Help => OptimizedHelpOverlay::new().render_fast(f, area),

            UIOverlay::FileNameSearch | UIOverlay::ContentSearch => {
                let pane = coord.fs_state().active_pane();
                let search_snap = SearchSnapshot::from(pane, snap);
                OptimizedSearchOverlay::new(snap.overlay).render_with_input(f, &search_snap, area);
            }

            UIOverlay::SearchResults => {
                let pane = coord.fs_state().active_pane();
                let sel = pane.selected.load(std::sync::atomic::Ordering::Relaxed);
                OptimizedSearchResultsOverlay::new().render_results(
                    f,
                    &pane.search_results,
                    Some(sel),
                    area,
                );
            }

            UIOverlay::Loading => {
                if let Some(l) = &snap.loading {
                    OptimizedLoadingOverlay::new().render_progress(f, l, area);
                }
            }

            UIOverlay::Prompt => {
                if let Some(tp) = &snap.prompt_type {
                    OptimizedPromptOverlay::new().render_input(f, snap, tp, area);
                }
            }

            _ => {}
        }
    }
}

/// ---------------------------------------------------------------------------
/// util: layout / rectangles
/// ---------------------------------------------------------------------------
impl UIRenderer {
    fn update_layout_cache(&mut self, scr: Rect) {
        if self.cache.screen == scr {
            self.cache.hit += 1;
            return;
        }

        self.cache.screen = scr;
        self.cache.miss += 1;
        self.cache.main = Some((scr, {
            let [c, s] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(scr);
            [c, s]
        }));
        self.cache.overlay = None;
        self.dirty = u32::MAX;
    }

    fn overlay_area(&mut self, scr: Rect, kind: UIOverlay) -> Rect {
        if let Some((prev, a)) = self.cache.overlay {
            if prev == scr {
                return a;
            }
        }
        let r = match kind {
            UIOverlay::Help => self.centered(scr, 80, 80),
            UIOverlay::FileNameSearch | UIOverlay::ContentSearch => self.search_rect(scr),
            UIOverlay::SearchResults => self.centered(scr, 90, 70),
            UIOverlay::Loading => self.centered(scr, 50, 30),
            UIOverlay::Prompt => self.centered(scr, 60, 25),
            _ => self.centered(scr, 70, 60),
        };
        self.cache.overlay = Some((scr, r));
        r
    }

    fn centered(&self, r: Rect, w_pct: u16, h_pct: u16) -> Rect {
        let w = (r.width * w_pct / 100).min(r.width);
        let h = (r.height * h_pct / 100).min(r.height);
        Rect {
            x: (r.width - w) / 2,
            y: (r.height - h) / 2,
            width: w,
            height: h,
        }
    }
    fn search_rect(&self, r: Rect) -> Rect {
        Rect {
            x: (r.width * 15) / 100,
            y: r.height / 4,
            width: (r.width * 70) / 100,
            height: 5,
        }
    }

    fn notification_rect(&self, scr: Rect, lvl: NotificationLevel) -> Rect {
        let h = if lvl == NotificationLevel::Error {
            5
        } else {
            3
        };
        let w = (scr.width * 60) / 100;
        Rect {
            x: (scr.width - w) / 2,
            y: 2,
            width: w,
            height: h,
        }
    }
}

/// ---------------------------------------------------------------------------
/// misc
/// ---------------------------------------------------------------------------
impl UIRenderer {
    fn draw_error(&self, f: &mut Frame<'_>, msg: &str, r: Rect) {
        ErrorOverlay::new(msg.into()).render(f, r);
    }
}

/// ---------------------------------------------------------------------------
/// stats helpers
/// ---------------------------------------------------------------------------
impl RenderStats {
    pub fn fps(&self) -> f64 {
        if self.frames > 0 {
            self.frames as f64 / self.total.as_secs_f64()
        } else {
            0.0
        }
    }
}

/// ---------------------------------------------------------------------------
/// tests (basic smoke only – heavy tests live elsewhere)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState};
    use std::sync::{Mutex, RwLock};

    fn dummy_coord() -> Arc<StateCoordinator> {
        let app = Arc::new(Mutex::new(AppState::default()));
        let ui = RwLock::new(UIState::default());
        let fs = Arc::new(Mutex::new(FSState::default()));
        Arc::new(StateCoordinator::new(app, ui, fs))
    }

    #[test]
    fn cache_hit() {
        let mut r = UIRenderer::new();
        r.update_layout_cache(Rect::new(0, 0, 100, 40));
        r.update_layout_cache(Rect::new(0, 0, 100, 40));
        assert_eq!(r.cache.hit, 1);
    }
}
