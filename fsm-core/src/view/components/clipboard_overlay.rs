//! src/view/components/clipboard_overlay.rs
//! ============================================================
//! Lock-free clipboard overlay tailored for the new renderer.
//!
//! ⚠  **No widget function touches a Mutex/RwLock**.  The overlay
//!     shows whatever was prefetched into its internal cache
//!     through `update_cache_sync/async`.  The renderer therefore
//!     must call one of those *before* rendering when the cache is
//!     stale.
//!
//! Rendering is synchronous because the main thread is already in
//! the draw phase.  An optional async helper is provided for a
//! background refresher task.

use std::time::{Duration, Instant};

use clipr::clipboard::ClipBoard;
use clipr::item::{ClipBoardItem, ClipBoardOperation};
use compact_str::CompactString;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use smallvec::SmallVec;

use crate::error::AppError;
use crate::view::snapshots::UiSnapshot;

// ------------------------------------------------------------
// Main struct
// ------------------------------------------------------------
pub struct OptimizedClipboardOverlay {
    /// Cached items (max 16 copied / moved entries shown)
    cached_items: SmallVec<[ClipBoardItem; 16]>,

    /// When cache was last refreshed
    last_update: Instant,

    /// Cache validity flag
    cache_valid: bool,
}

impl Default for OptimizedClipboardOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizedClipboardOverlay {
    // --------------------------------------------------------
    // ctor
    // --------------------------------------------------------
    pub fn new() -> Self {
        Self {
            cached_items: SmallVec::new(),
            last_update: Instant::now(),
            cache_valid: false,
        }
    }

    // --------------------------------------------------------
    // PUBLIC – sync render entry used by UIRenderer
    // --------------------------------------------------------
    pub fn render_sync_fallback(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        ui: &UiSnapshot,
    ) -> Result<(), AppError> {
        frame.render_widget(Clear, area); // wipe bg

        let layout = PrecomputedLayout::new(area);
        let mut state = ListState::default();

        // outer frame ---------------------------------------------------
        self.draw_container(frame, &layout);

        // no items → empty message -------------------------------------
        if self.cached_items.is_empty() {
            self.draw_empty(frame, layout.content);
            return Ok(());
        }

        // clamp cursor to list length ----------------------------------
        let sel = ui
            .selected_clipboard_item_idx
            .min(self.cached_items.len().saturating_sub(1));
        state.select(Some(sel));

        // list + help panel --------------------------------------------
        self.draw_items(frame, &layout, &mut state)?;
        self.draw_help(frame, layout.help_area);

        // details panel -------------------------------------------------
        if let Some(item) = state.selected().and_then(|i| self.cached_items.get(i)) {
            self.draw_details(frame, layout.details, item);
        }
        Ok(())
    }

    // --------------------------------------------------------
    // PUBLIC – async updater (call in background task)
    // --------------------------------------------------------
    pub async fn update_cache(&mut self, clipboard: &ClipBoard) -> Result<(), AppError> {
        let items = clipboard.get_all_items().await;
        self.cached_items.clear();
        self.cached_items.extend(items.into_iter().take(16));
        self.last_update = Instant::now();
        self.cache_valid = true;
        Ok(())
    }

    // --------------------------------------------------------
    // PUBLIC – quick invalidation; renderer calls when needed
    // --------------------------------------------------------
    #[inline]
    pub fn update_cache_sync(&mut self, clipboard: &ClipBoard) -> Result<(), AppError> {
        if self.last_update.elapsed() > Duration::from_secs(1) {
            // refresh synchronously – still no locks held
            let items = futures_lite::future::block_on(clipboard.get_all_items());
            self.cached_items.clear();
            self.cached_items.extend(items.into_iter().take(16));
            self.last_update = Instant::now();
            self.cache_valid = true;
        }
        Ok(())
    }

    // --------------------------------------------------------
    // tiny helpers (internal)
    // --------------------------------------------------------
    fn draw_container(&self, frame: &mut Frame<'_>, lay: &PrecomputedLayout) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 📋 Clipboard ")
            .title_alignment(Alignment::Center)
            .style(Style::default().bg(Color::Rgb(25, 27, 38)).fg(Color::White))
            .border_style(Style::default().fg(Color::Rgb(100, 149, 237)));
        frame.render_widget(block, lay.main);
    }

    fn draw_empty(&self, frame: &mut Frame<'_>, area: Rect) {
        let txt = "Clipboard empty – copy (c) or cut (x) files";
        let p = Paragraph::new(txt)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().bg(Color::Rgb(30, 25, 40)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 150, 255))),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(p, area);
    }

    fn draw_items(
        &self,
        frame: &mut Frame<'_>,
        lay: &PrecomputedLayout,
        list_state: &mut ListState,
    ) -> Result<(), AppError> {
        let mut rows = SmallVec::<[ListItem; 16]>::new();
        for (idx, it) in self.cached_items.iter().enumerate() {
            let (icon, col) = match it.operation {
                ClipBoardOperation::Copy => ("📄", Color::Rgb(100, 200, 255)),
                ClipBoardOperation::Move => ("✂", Color::Rgb(255, 200, 120)),
            };
            let txt = CompactString::from(format!(
                "{icon} {:2}. {} ({})",
                idx + 1,
                Self::short_path(&it.source_path, 42),
                Self::human_size(it.metadata.size),
            ));
            rows.push(ListItem::new(txt.to_string()).style(Style::default().fg(col)));
        }

        let list = List::new(rows)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(format!(" {} item(s) ", self.cached_items.len()))
                    .style(Style::default().bg(Color::Rgb(28, 30, 46)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(75, 125, 200))),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(60, 100, 180))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, lay.list, list_state);
        Ok(())
    }

    fn draw_details(&self, frame: &mut Frame<'_>, area: Rect, item: &ClipBoardItem) {
        let txt = format!(
            "Path:\n  {}\n\nOperation: {:?}\nSize: {}",
            item.source_path,
            item.operation,
            Self::human_size(item.metadata.size),
        );
        let block = Paragraph::new(txt)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Details ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(30, 35, 50)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(150, 100, 200))),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(block, area);
    }

    fn draw_help(&self, frame: &mut Frame<'_>, area: Rect) {
        let txt = "↑/↓ navigate  •  ⏎ select  •  Del remove  •  Esc close";
        let block = Paragraph::new(txt)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Controls ")
                    .title_alignment(Alignment::Center)
                    .style(Style::default().bg(Color::Rgb(40, 30, 20)).fg(Color::White))
                    .border_style(Style::default().fg(Color::Rgb(255, 200, 100))),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(block, area);
    }

    // --------------------------------------------------------
    // util
    // --------------------------------------------------------
    fn short_path(path: &str, max: usize) -> String {
        if path.len() <= max {
            return path.to_owned();
        }
        path.rsplit_once('/')
            .and_then(|(_, f)| {
                if f.len() < max - 4 {
                    Some(format!(".../{}", f))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("...{}", &path[path.len() - max + 3..]))
    }

    fn human_size(sz: u64) -> String {
        const U: [&str; 5] = ["B", "K", "M", "G", "T"];
        if sz == 0 {
            return "0B".into();
        }
        let mut n = sz as f64;
        let mut u = 0;
        while n >= 1024.0 && u < U.len() - 1 {
            n /= 1024.0;
            u += 1;
        }
        if u == 0 {
            format!("{sz}B")
        } else {
            format!("{n:.1}{}", U[u])
        }
    }
}

// ------------------------------------------------------------
// Layout helper – caches nothing, pure function
// ------------------------------------------------------------
#[derive(Clone, Debug)]
struct PrecomputedLayout {
    /// Outer chrome (rounded border)
    main: Rect,
    /// Padding inside `main`
    content: Rect,
    /// Left-hand list area
    list: Rect,
    /// Upper panel on the right
    details: Rect,
    /// Lower panel on the right
    help_area: Rect,
}

impl PrecomputedLayout {
    fn new(area: Rect) -> Self {
        // ① outer frame (2 cols, 1 row padding)
        let main = area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        let content = main.inner(Margin::new(1, 1));

        // ② split 60 : 40 horizontally
        let [list, side] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(content);

        // ③ split right side 70 : 30 vertically
        let [details, help] =
            Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)]).areas(side);

        Self {
            main,
            content,
            list,
            details,
            help_area: help,
        }
    }
}
