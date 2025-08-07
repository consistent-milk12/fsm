//! High-performance filename search overlay — ratatui 0.29 + moka 0.12

use crate::{fs::object_info::ObjectInfo, model::shared_state::SharedState, view::theme};
use moka::sync::Cache;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use rustc_hash::{FxBuildHasher, FxHashSet};
use std::{
    borrow::Cow,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, info, instrument, trace, warn};

// ---------- tuning knobs ----------
const MAX_RESULTS: usize = 64;
const MIN_LEN: usize = 2;
const CACHE_CAP: u64 = 256;
const POPUP_W: u16 = 75;
const POPUP_H: u16 = 70;
const SPIN_MS: u64 = 80;

// Performance monitoring thresholds
const SLOW_RENDER_MS: u128 = 8; // > 8ms is slow for UI
const SLOW_BUILD_MS: u128 = 5; // > 5ms cache build is slow

// ---------- cache payload ----------
#[derive(Clone)]
#[expect(dead_code)]
struct CacheEntry {
    // Pre-computed display strings (no ListItem storage due to lifetime issues)
    items: Arc<Vec<String>>,
    status: Arc<str>,
    mode: Mode,
    made: Instant,
    build_time_us: u64, // Track cache build performance
}

// ---------- tiny helper enums ----------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Empty,
    Local,
    Recursive,
    Mixed,
}

// ---------- moka wrapper --------------
struct SearchCache {
    inner: Cache<String, Arc<CacheEntry>>,
}

impl SearchCache {
    #[instrument(fields(marker = "CACHE_INIT", operation_type = "cache_init"))]
    fn new() -> Self {
        trace!("Initializing search cache with capacity: {}", CACHE_CAP);
        let inner: Cache<String, Arc<CacheEntry>> = Cache::builder()
            .max_capacity(CACHE_CAP)
            .time_to_live(Duration::from_secs(120))
            .build();

        debug!("Search cache initialized successfully");

        Self { inner }
    }

    #[instrument(
        skip(self, f),
        fields(
            marker = "CACHE_GET_OR_BUILD",
            operation_type = "cache_operation",
            cache_key = %k,
        )
    )]
    fn get_or<F>(&self, k: &str, f: F) -> Arc<CacheEntry>
    where
        F: FnOnce() -> CacheEntry,
    {
        let start: Instant = Instant::now();

        // Check if we have a cache hit first
        if let Some(cached) = self.inner.get(k) {
            trace!(
                cache_key = %k,
                build_time_us = %cached.build_time_us,
                items_count = %cached.items.len(),
                mode = ?cached.mode,
                "Cache HIT - returning existing entry"
            );

            return cached;
        }

        // Cache miss - need to build
        trace!(cache_key = %k, "Cache MISS - building new entry");
        let entry: Arc<CacheEntry> = self.inner.get_with(k.to_owned(), || Arc::new(f()));
        let lookup_time: Duration = start.elapsed();

        info!(
            cache_key = %k,
            lookup_time_us = %lookup_time.as_micros(),
            build_time_us = %entry.build_time_us,
            items_count = %entry.items.len(),
            mode = ?entry.mode,
            "Cache entry created and stored"
        );

        if lookup_time.as_millis() > SLOW_BUILD_MS {
            warn!(
                cache_key = %k,
                lookup_time_ms = %lookup_time.as_millis(),
                "Slow cache lookup detected (threshold: {}ms)",
                SLOW_BUILD_MS
            );
        }

        entry
    }

    #[instrument(
        skip(self),
        fields(marker = "CACHE_INVALIDATE_ALL", operation_type = "cache_operation")
    )]
    fn invalidate_all(&self) {
        let estimated_size: u64 = self.inner.entry_count();

        trace!(entries_invalidated = %estimated_size, "Invalidating entire cache");

        self.inner.invalidate_all();
        debug!("Cache invalidated completely");
    }
}

// ---------- main component -----------
pub struct FileNameSearchOverlay {
    cache: SearchCache,
    last: String,
    epoch: Instant,
}

impl Default for FileNameSearchOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl FileNameSearchOverlay {
    #[must_use]
    #[instrument(fields(marker = "OVERLAY_NEW", operation_type = "component_init"))]
    pub fn new() -> Self {
        trace!("Creating new FileNameSearchOverlay");
        Self {
            cache: SearchCache::new(),
            last: String::new(),
            epoch: Instant::now(),
        }
    }
}

// ---------- public API ---------------
impl FileNameSearchOverlay {
    #[instrument(
        skip(self, f, shared_state, all),
        fields(
            marker = "UI_RENDER_FILENAME_SEARCH",
            operation_type = "ui_render",
            area_width = %all.width,
            area_height = %all.height
        )
    )]
    pub fn render(&mut self, f: &mut Frame<'_>, shared_state: &SharedState, all: Rect) {
        let render_start = Instant::now();
        let ui_guard = shared_state.lock_ui();

        trace!(
            input = %ui_guard.input,
            search_results_count = %ui_guard.filename_search_results.len(),
            is_searching = %is_searching(shared_state),
            "Starting filename search overlay render"
        );

        // Invalidate cache if input changed
        if self.last != ui_guard.input {
            debug!(
                old_input = %self.last,
                new_input = %ui_guard.input,
                "Input changed - invalidating cache"
            );
            self.cache.invalidate_all();
            self.last.clone_from(&ui_guard.input);
        }

        // Release guard early before layout work
        drop(ui_guard);

        let popup: Rect = centered(all);
        f.render_widget(Clear, popup);

        // Create layout areas
        let chunks: Rc<[Rect]> = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // input
                Constraint::Length(1), // status
                Constraint::Min(0),    // list
            ])
            .split(popup);

        trace!(
            input_area = ?chunks[0],
            status_area = ?chunks[1],
            results_area = ?chunks[2],
            "Layout areas calculated"
        );

        Self::draw_input(f, shared_state, chunks[0]);
        self.draw_results(f, shared_state, chunks[1], chunks[2]);

        let render_time: Duration = render_start.elapsed();
        if render_time.as_millis() > SLOW_RENDER_MS {
            let ui_guard = shared_state.lock_ui();
            warn!(
                render_time_ms = %render_time.as_millis(),
                input = %ui_guard.input,
                "Slow filename search render detected (threshold: {}ms)",
                SLOW_RENDER_MS
            );
        } else {
            trace!(
                render_time_us = %render_time.as_micros(),
                "Filename search overlay render completed"
            );
        }
    }
}

// ---------- drawing helpers ----------
impl FileNameSearchOverlay {
    #[expect(clippy::cast_possible_truncation, reason = "Expected accuracy loss")]
    #[instrument(
        skip(f, shared_state, r),
        fields(marker = "UI_DRAW_INPUT", operation_type = "ui_draw")
    )]
    fn draw_input(f: &mut Frame<'_>, shared_state: &SharedState, r: Rect) {
        let ui_guard = shared_state.lock_ui();

        let (ttl, col) = match ui_guard.input.len() {
            0 => {
                trace!("Drawing empty input state");
                (" File Search ", theme::CYAN)
            }
            n if n < MIN_LEN => {
                trace!(chars_needed = %(MIN_LEN - n), "Drawing 'type more' state");
                (" File Search (type more) ", theme::COMMENT)
            }
            n => {
                trace!(input_length = %n, "Drawing active search state");
                (" File Search ", theme::YELLOW)
            }
        };

        let block: Block<'_> = Block::default()
            .borders(Borders::ALL)
            .title(ttl)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(col))
            .style(Style::default().bg(theme::BACKGROUND));

        let p = Paragraph::new(ui_guard.input.as_str())
            .block(block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        f.render_widget(p, r);

        // Set cursor position
        let cursor_x: u16 =
            (r.x + ui_guard.input.len() as u16 + 1).min(r.x + r.width.saturating_sub(2));
        f.set_cursor_position((cursor_x, r.y + 1));

        trace!(cursor_x = %cursor_x, cursor_y = %(r.y + 1), "Input cursor positioned");
    }

    #[instrument(
        skip(self, f, shared_state, stat, lst),
        fields(
            marker = "UI_DRAW_RESULTS",
            operation_type = "ui_draw",
            input = %shared_state.lock_ui().input
        )
    )]
    fn draw_results(&self, f: &mut Frame<'_>, shared_state: &SharedState, stat: Rect, lst: Rect) {
        let cache_start: Instant = Instant::now();
        let ce: Arc<CacheEntry> = self
            .cache
            .get_or(&shared_state.lock_ui().input, || -> CacheEntry {
                Self::build(shared_state)
            });
        let cache_time: Duration = cache_start.elapsed();

        trace!(
            cache_lookup_us = %cache_time.as_micros(),
            items_count = %ce.items.len(),
            mode = ?ce.mode,
            build_time_us = %ce.build_time_us,
            "Cache lookup completed"
        );

        // status bar
        let bar: Paragraph<'_> = Paragraph::new(ce.status.as_ref())
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        f.render_widget(bar, stat);

        // spinner or empty message
        if is_searching(shared_state) {
            trace!("Rendering spinner (search in progress)");
            self.spinner(f, shared_state, lst);
            return;
        }

        if ce.items.is_empty() {
            trace!("Rendering empty state (no results)");
            Self::empty(f, shared_state, lst);
            return;
        }

        let list_start: Instant = Instant::now();

        // Create list items from cached strings
        let list_items: Vec<ListItem> = ce
            .items
            .iter()
            .enumerate()
            .map(|(idx, text)| -> ListItem<'_> {
                ListItem::new(text.as_str()).style(if idx % 2 == 0 {
                    Style::default().fg(theme::FOREGROUND)
                } else {
                    Style::default().fg(theme::COMMENT)
                })
            })
            .collect();

        let blk: Block<'_> = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} Files ", ce.items.len()))
            .border_style(Style::default().fg(theme::CYAN));

        let mut st: ListState = ListState::default();
        let selection = shared_state
            .lock_ui()
            .raw_search_selected
            .min(ce.items.len().saturating_sub(1));
        st.select(Some(selection));

        f.render_stateful_widget(
            List::new(list_items)
                .block(blk)
                .highlight_symbol("▶ ")
                .highlight_style(
                    Style::default()
                        .bg(theme::CURRENT_LINE)
                        .fg(theme::FOREGROUND)
                        .add_modifier(Modifier::BOLD),
                ),
            lst,
            &mut st,
        );

        let list_time: Duration = list_start.elapsed();

        trace!(
            list_render_us = %list_time.as_micros(),
            items_rendered = %ce.items.len(),
            selected_index = %selection,
            "Results list rendered"
        );
    }

    // ----------------- spinner / empty -----------------
    #[instrument(
        skip(self, f, shared_state, area),
        fields(marker = "UI_DRAW_SPINNER", operation_type = "ui_draw")
    )]
    fn spinner(&self, f: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let idx: usize = ((self.epoch.elapsed().as_millis() / u128::from(SPIN_MS))
            % FRAMES.len() as u128) as usize;
        let txt: String = format!(
            "{} Searching \"{}\" …",
            FRAMES[idx],
            shared_state.lock_ui().input
        );

        trace!(
            spinner_frame = %idx,
            spinner_char = %FRAMES[idx],
            "Rendering search spinner"
        );

        let w: Paragraph<'_> = Paragraph::new(txt)
            .style(Style::default().fg(theme::CYAN))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" Searching "));
        f.render_widget(w, area);
    }

    #[instrument(
        skip(f, shared_state, area),
        fields(
            marker = "UI_DRAW_EMPTY",
            operation_type = "ui_draw",
            input_empty = %shared_state.lock_ui().input.is_empty()
        )
    )]
    fn empty(f: &mut Frame<'_>, shared_state: &SharedState, area: Rect) {
        let msg: &'static str = if shared_state.lock_ui().input.is_empty() {
            trace!("Rendering empty input message");
            "Type to search"
        } else {
            trace!(input = %shared_state.lock_ui().input, "Rendering no results message");
            "No files found"
        };

        let w: Paragraph<'_> = Paragraph::new(msg)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" No Results "));
        f.render_widget(w, area);
    }

    // ----------------- cache builder ------------------
    #[expect(clippy::cast_possible_truncation, reason = "Expected accuracy loss")]
    #[expect(clippy::cast_sign_loss, reason = "Expected accuracy loss")]
    #[instrument(
        skip(shared_state),
        fields(marker = "CACHE_BUILD_ENTRY", operation_type = "cache_build")
    )]
    fn build(shared_state: &SharedState) -> CacheEntry {
        let build_start = Instant::now();

        if shared_state.lock_ui().input.len() < MIN_LEN {
            trace!("Building empty cache entry (input too short)");
            return CacheEntry {
                items: Arc::new(Vec::new()),
                status: Arc::from("Type more characters"),
                mode: Mode::Empty,
                made: Instant::now(),
                build_time_us: build_start.elapsed().as_micros() as u64,
            };
        }

        let term: String = shared_state.lock_ui().input.to_lowercase();
        let cwd: PathBuf = shared_state.lock_fs().active_pane().cwd.clone();

        debug!(
            search_term = %term,
            current_dir = %cwd.display(),
            "Building cache entry for search"
        );

        let mut seen: HashSet<PathBuf, FxBuildHasher> = FxHashSet::default();
        let mut items: Vec<String> = Vec::new();
        let mut local_count: i32 = 0;
        let mut recursive_count: i32 = 0;

        // Helper closure to process entries
        let mut process_entry = |o: &ObjectInfo, is_local: bool| {
            if !o.is_dir
                && o.name.to_lowercase().contains(&term)
                && seen.insert(o.path.clone().to_path_buf())
            {
                let display_text = build_display(o, &cwd);

                items.push(display_text);

                if is_local {
                    local_count += 1;
                } else {
                    recursive_count += 1;
                }
            }
        };

        // Process local entries first - need registry lookup for SortableEntry
        for sortable_entry in &shared_state.lock_fs().active_pane().entries {
            if let Some(object_info) = shared_state.metadata.get_by_id(sortable_entry.id) {
                process_entry(&object_info, true);
            }
        }

        // Process recursive search results
        for entry in &shared_state.lock_ui().filename_search_results {
            if let Some(obj_info) = shared_state.metadata.get_by_id(entry.id) {
                process_entry(&obj_info, false);
            }
        }

        // Determine mode
        let mode = match (local_count, recursive_count) {
            (0, 0) => Mode::Empty,
            (_, 0) => Mode::Local,
            (0, _) => Mode::Recursive,
            (_, _) => Mode::Mixed,
        };

        let status = match mode {
            Mode::Empty => "No matches".to_string(),
            Mode::Local => format!("{local_count} local files"),
            Mode::Recursive => format!("{recursive_count} recursive files"),
            Mode::Mixed => format!("{local_count} local + {recursive_count} recursive files"),
        };

        // Limit results for performance
        let original_count = items.len();
        items.truncate(MAX_RESULTS);

        let build_time = build_start.elapsed();
        let build_time_us = build_time.as_micros() as u64;

        if items.len() < original_count {
            debug!(
                original_count = %original_count,
                truncated_count = %items.len(),
                max_results = %MAX_RESULTS,
                "Results truncated for performance"
            );
        }

        info!(
            search_term = %term,
            local_matches = %local_count,
            recursive_matches = %recursive_count,
            total_items = %items.len(),
            mode = ?mode,
            build_time_us = %build_time_us,
            duplicates_removed = %(original_count + local_count as usize + recursive_count as usize - items.len()),
            "Cache entry built successfully"
        );

        if build_time.as_millis() > SLOW_BUILD_MS {
            warn!(
                build_time_ms = %build_time.as_millis(),
                search_term = %term,
                items_processed = %(shared_state.lock_fs().active_pane().entries.len() + shared_state.lock_ui().filename_search_results.len()),
                "Slow cache build detected (threshold: {}ms)",
                SLOW_BUILD_MS
            );
        }

        CacheEntry {
            items: Arc::new(items),
            status: Arc::from(status),
            mode,
            made: Instant::now(),
            build_time_us,
        }
    }
}

// ---------- free helpers ------------
fn centered(area: Rect) -> Rect {
    let popup_layout: Rc<[Rect]> = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - POPUP_H) / 2),
            Constraint::Percentage(POPUP_H),
            Constraint::Percentage((100 - POPUP_H) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - POPUP_W) / 2),
            Constraint::Percentage(POPUP_W),
            Constraint::Percentage((100 - POPUP_W) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[instrument(
    skip(shared_state),
    fields(marker = "SEARCH_STATUS_CHECK", operation_type = "search_check")
)]
fn is_searching(shared_state: &SharedState) -> bool {
    let app_guard = shared_state.lock_app();
    let is_active = app_guard
        .tasks
        .values()
        .any(|t| t.description.contains("Filename search") && !t.is_completed);

    trace!(is_searching = %is_active, active_tasks = %app_guard.tasks.len(), "Search status checked");

    is_active
}

#[instrument(
    skip(obj, cwd),
    fields(
        marker = "BUILD_DISPLAY_STRING",
        operation_type = "string_format",
        file_name = %obj.name,
        file_size = %obj.size
    )
)]
fn build_display(obj: &ObjectInfo, cwd: &Path) -> String {
    let display_path = obj.path.strip_prefix(cwd).map_or_else(
        |_| {
            trace!("Using absolute path (strip_prefix failed)");
            Cow::from(obj.name.as_str())
        },
        |p: &Path| -> Cow<'_, str> {
            trace!("Using relative path");
            p.to_string_lossy()
        },
    );

    if obj.size == 0 {
        trace!("No size info available");
        return display_path.into_owned();
    }

    let formatted = format!("{} ({})", display_path, fmt_size(obj.size));
    trace!(display_string = %formatted, "Display string built");
    formatted
}

fn fmt_size(mut size: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit_index: usize = 0;

    while size >= 1024 && unit_index + 1 < UNITS.len() {
        size /= 1024;
        unit_index += 1;
    }

    format!("{} {}", size, UNITS[unit_index])
}

// Required trait implementations for compatibility
impl Clone for FileNameSearchOverlay {
    fn clone(&self) -> Self {
        Self::new() // Create fresh instance with new cache
    }
}

impl PartialEq for FileNameSearchOverlay {
    fn eq(&self, other: &Self) -> bool {
        self.last == other.last
    }
}

impl Eq for FileNameSearchOverlay {}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "Intentional for performance"
)]
impl std::fmt::Debug for FileNameSearchOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileNameSearchOverlay")
            .field("last_input", &self.last)
            .finish()
    }
}
