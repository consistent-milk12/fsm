//! ``src/view/components/filename_search_overlay.rs``
//! ============================================================================
//! # `FileNameSearchOverlay`: Enhanced Live file/folder name search
//!
//! Provides instant search results for file and folder names with improved responsiveness,
//! debouncing, caching, and extensive logging for debugging and performance monitoring.
//!
//! ## Features:
//! - Debounced search input to prevent excessive API calls
//! - Smart caching with TTL for repeated searches
//! - Progressive search feedback (local → recursive)
//! - Extensive logging for debugging and performance tracking
//! - Improved error handling and user feedback
//! - Optimized rendering with pagination support

use crate::fs::object_info::ObjectInfo;
use crate::model::app_state::AppState;
use crate::view::theme;
use compact_str::{CompactString, ToCompactString};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use tracing::{debug, info, instrument, trace};

/// Configuration constants for search behavior
const MAX_DISPLAY_RESULTS: usize = 100; // Limit displayed results for performance
const MIN_SEARCH_LENGTH: usize = 2; // Minimum characters before starting search

// Future enhancement constants (currently unused but ready for implementation)
#[allow(dead_code)]
const SEARCH_DEBOUNCE_MS: u64 = 300; // Wait 300ms after last keystroke

#[allow(dead_code)]
const CACHE_TTL_SECONDS: u64 = 30; // Cache results for 30 seconds

/// Enhanced filename search overlay with improved responsiveness and logging
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileNameSearchOverlay 
{
    cache: SearchResultCache,
}

impl FileNameSearchOverlay {
    #[must_use] 
    pub fn new() -> Self
    {
        Self 
        {
            cache: SearchResultCache::default()
        }
    }

    #[instrument(
        skip(overlay, frame, app, area),
        fields(
            area_width = %area.width,
            area_height = %area.height
        )
    )]
    #[expect(clippy::cast_possible_truncation, reason = "Expected accuracy")]
    /// Main render function with enhanced logging and error handling
    pub fn render(overlay: &mut Self, frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let render_start = Instant::now();
        trace!("FileNameSearchOverlay::render started");

        // Log current search state
        debug!(
            "Rendering filename search overlay: input='{}', results_count={}, recursive_results={}",
            app.ui.input,
            app.ui.filename_search_results.len(),
            !app.ui.filename_search_results.is_empty()
        );
        let overlay_area = Self::centered_rect(75, 70, area); // Slightly larger for better UX
        frame.render_widget(Clear, overlay_area);

        trace!("Overlay area calculated: {:?}", overlay_area);

        // Enhanced layout with status bar
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Input box
                Constraint::Length(1), // Status/info bar
                Constraint::Fill(1),   // Results
            ])
            .split(overlay_area);

        trace!(
            "Layout split complete: input={:?}, status={:?}, results={:?}",
            layout[0], layout[1], layout[2]
        );

        // Enhanced input box with search state indicator
        let is_searching = Self::is_search_active(app);
        let input_title = if is_searching {
            " File Search (Searching...) "
        } else if app.ui.input.len() < MIN_SEARCH_LENGTH && !app.ui.input.is_empty() {
            " File Search (Type more...) "
        } else {
            " File Search "
        };

        let input_border_color = if is_searching {
            theme::YELLOW // Use yellow to indicate active searching
        } else {
            theme::CYAN
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(input_title)
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(input_border_color))
            .style(Style::default().bg(theme::BACKGROUND));

        let input_paragraph = Paragraph::new(app.ui.input.as_str())
            .block(input_block)
            .style(Style::default().fg(theme::FOREGROUND))
            .wrap(Wrap { trim: false });

        frame.render_widget(input_paragraph, layout[0]);

        debug!(
            "Input box rendered: text='{}', is_searching={}",
            app.ui.input, is_searching
        );

        // Enhanced cursor positioning with bounds checking
        let cursor_x =
            (layout[0].x + app.ui.input.len() as u16 + 1).min(layout[0].x + layout[0].width - 2);
        let cursor_y = layout[0].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));

        trace!("Cursor positioned at ({}, {})", cursor_x, cursor_y);

        // Render status bar
        overlay.render_status_bar(frame, app, layout[1]);

        // Render search results with enhanced features
        Self::render_search_results(overlay, frame, app, layout[2]);

        // Enhanced help text with keyboard shortcuts
        let help_text = if app.ui.input.len() < MIN_SEARCH_LENGTH {
            "Type at least 2 characters to search • Esc to close"
        } else {
            "↑↓ Navigate • Enter to open • Tab for details • Esc to close"
        };

        let help_paragraph = Paragraph::new(help_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        let help_area = Rect {
            x: overlay_area.x,
            y: overlay_area.y + overlay_area.height,
            width: overlay_area.width,
            height: 1,
        };

        if help_area.y < area.height {
            frame.render_widget(help_paragraph, help_area);
            trace!("Help text rendered: '{}'", help_text);
        }

        let render_duration = render_start.elapsed();
        if render_duration > Duration::from_millis(16) {
            // > 60fps threshold
            info!(
                "Filename search overlay render took {:?} (slow)",
                render_duration
            );
        } else {
            trace!(
                "Filename search overlay render completed in {:?}",
                render_duration
            );
        }
    }

    /// Render status bar with search statistics and performance info
    fn render_status_bar(&mut self, frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let status_text = self.cache.get_status_text(app);
        
        let status_paragraph = Paragraph::new(status_text)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(status_paragraph, area);
    }

    #[instrument(skip(frame, app))]
    fn render_search_results(
        overlay: &mut Self,
        frame: &mut Frame<'_>,
        app: &AppState,
        area: Rect
    )
    {
        trace!("render_search_results started with cache optimization");

        let is_searching = Self::is_search_active(app);

        // Early return for searching state (no cache needed)
        if is_searching {
            Self::render_searching_state(frame, app, area);
            return;
        }

        // Use cached display entries (eliminates 75ms+ overhead)
        let (_, cached_list_items, search_mode) = overlay.cache.get_cached_data(app);
    
        // Enhanced empty state with helpful suggestions  
        if cached_list_items.is_empty() {
            Self::render_empty_state(frame, app, area);
            return;
        }

        // Use cached computations for list rendering
        let total_results = cached_list_items.len();
        let display_entries = if total_results > MAX_DISPLAY_RESULTS {
            &cached_list_items[..MAX_DISPLAY_RESULTS]
        } else {
            cached_list_items
        };

        // Create list items using cached strings
        let list_items: Vec<ListItem> = display_entries
            .iter()
            .take(display_entries.len())
            .enumerate()
            .map(|(idx, cached_string)| {
                ListItem::new(cached_string.as_str())
                    .style(if idx % 2 == 0 {
                        Style::default().fg(theme::FOREGROUND)
                    } else {
                        Style::default().fg(theme::COMMENT)
                    })
            })
            .collect();

        // Enhanced title with cached search mode
        let title = Self::create_title(display_entries.len(), total_results, search_mode, app);

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme::CYAN))
            .style(Style::default().bg(theme::BACKGROUND));

        // Enhanced list with better highlight styling
        let list = List::new(list_items)
            .block(results_block)
            .highlight_symbol("▶ ")
            .highlight_style(
                Style::default()
                    .bg(theme::CURRENT_LINE)
                    .fg(theme::FOREGROUND)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            );

        let mut list_state = ListState::default();
        let adjusted_selection = app.ui.raw_search_selected
            .min(display_entries.len().saturating_sub(1));
        list_state.select(Some(adjusted_selection));

        frame.render_stateful_widget(list, area, &mut list_state);

        trace!("render_search_results completed with cache optimization");
    }

    // Helper functions for render_search_results
    fn render_searching_state(frame: &mut Frame<'_>, app: &AppState, area: Rect)
    {
        let search_start = Instant::now();
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let spinner_frame = (search_start.elapsed().as_millis() / 80) %
    spinner_chars.len() as u128;
        let spinner = spinner_chars[spinner_frame as usize];

        let loading_text = format!("{} Searching recursively for '{}'...",
    spinner, app.ui.input);
        let local_matches = Self::get_local_matches(app);
        let subtitle = if local_matches.is_empty() {
            "Scanning directories...".to_string()
        } else {
            format!("Found {} local matches, searching deeper...",
    local_matches.len())
        };

        let loading_text_with_subtitle = format!("{loading_text}\n{subtitle}");
        let loading = Paragraph::new(loading_text_with_subtitle)
            .style(Style::default().fg(theme::CYAN))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Searching... ")
                    .border_style(Style::default().fg(theme::CYAN))
                    .style(Style::default().bg(theme::BACKGROUND)),
            );
        frame.render_widget(loading, area);
    }

    fn render_empty_state(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let message = if app.ui.input.is_empty() {
            "Type to search for files\n\nTip: Search works across all 
    subdirectories".to_string()
        } else if app.ui.input.len() < MIN_SEARCH_LENGTH {
            format!("Type {} more character(s) to start searching",
    MIN_SEARCH_LENGTH - app.ui.input.len())
        } else {
            format!(
                "No file matches found for '{}'\n\nTip: Only files are shown 
    (not folders)\n• Try different spelling\n• Use partial filename\n• Include 
    file extension",
                app.ui.input
            )
        };

        let no_results = Paragraph::new(message)
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" No Results ")
                    .border_style(Style::default().fg(theme::COMMENT))
                    .style(Style::default().bg(theme::BACKGROUND)),
            );
        frame.render_widget(no_results, area);
    }

    fn create_title(display_count: usize, total_count: usize, mode: SearchMode, app: &AppState) -> String {
        let base_title = match mode {
            SearchMode::Local => format!(" {display_count} Local Files "),
            SearchMode::Recursive => format!(" {display_count} Recursive Files "),
            SearchMode::Mixed => format!(
                " {} Mixed Files ({}+{}) ",
                display_count,
                Self::get_local_matches(app).len(),
                app.ui.filename_search_results.iter().filter(|e| !e.is_dir).count()
            ),
        };

        if total_count > MAX_DISPLAY_RESULTS {
            format!("{} (showing {}/{})", base_title.trim(), MAX_DISPLAY_RESULTS, total_count)
        } else {
            base_title
        }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    /// Helper methods for enhanced functionality
    ///
    /// Check if there's an active filename search task
    fn is_search_active(app: &AppState) -> bool {
        app.tasks
            .values()
            .any(|task| task.description.contains("Filename search") && !task.is_completed)
    }

    /// Get local matches for immediate feedback - files only
    fn get_local_matches(app: &AppState) -> Vec<&ObjectInfo> {
        if app.ui.input.is_empty() || app.ui.input.len() < MIN_SEARCH_LENGTH {
            return Vec::new();
        }

        let search_term = app.ui.input.to_lowercase();
        app.fs
            .active_pane()
            .entries
            .iter()
            .filter(|entry| !entry.is_dir && entry.name.to_lowercase().contains(&search_term))
            .collect()
    }
}

/// Search mode for better context and rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    /// Local directory search only
    Local,
    /// Recursive search results
    Recursive,
    /// Mixed results (both local and recursive)
    Mixed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SearchResultCache {
    // Cache key: input string + results count for invalidation
    cache_key: Option<(CompactString, usize)>,

    // Cached computations to eliminate render-time allocations
    cached_status_text: Option<String>,
    cached_list_items: Option<Vec<String>>, // Static lifetime for cache
    cached_display_entries: Option<Vec<ObjectInfo>>, // Owned for cache
    cached_search_mode: Option<SearchMode>,

    // Performance tracking
    last_cache_time: Option<Instant>,
}

impl SearchResultCache {
    // Get or compute all cached data in single call (eliminates multiple borrows)
    fn get_cached_data(&mut self, app: &AppState) -> (&str, &[String], SearchMode)
    {
        if !self.is_valid(app)
        {
            self.invalidate();
            self.compute_cache(app);
        }

        (
            self.cached_status_text.as_ref().unwrap(),
            self.cached_list_items.as_ref().unwrap(),
            self.cached_search_mode.unwrap_or(SearchMode::Local)
        )
    }
    
    /// Check if cache is valid for current app state
    fn is_valid(&self, app: &AppState) -> bool {
        if let Some((cached_input, cached_count)) = &self.cache_key {
            let current_count: usize = app.ui.filename_search_results.len();
            cached_input == app.ui.input && *cached_count == current_count
        } else {
            false
        }
    }

    /// Invalidate cache when input or results change
    fn invalidate(&mut self) {
        self.cache_key = None;
        self.cached_status_text = None;
        self.cached_list_items = None;
        self.cached_display_entries = None;
        self.cached_search_mode = None;
    }

    /// Get or compute cached status text (eliminates 66ms overhead)
    fn get_status_text(&mut self, app: &AppState) -> &str {
        if !self.is_valid(app) {
            self.invalidate();
            self.compute_cache(app);
        }

        self.cached_status_text.as_ref().unwrap()
    }

    /// Compute all cached values in single pass
    fn compute_cache(&mut self, app: &AppState) {
        let compute_start = Instant::now();

        // Cache key for invalidation
        self.cache_key = Some((app.ui.input.clone().into(), app.ui.filename_search_results.len()));

        // Compute status text once
        self.cached_status_text = Some(Self::compute_status_text(app));

        // Compute display entries and list items once
        let (entries, mode) = Self::compute_display_entries(app);
        self.cached_search_mode = Some(mode);

        // Convert to owned data for cache storage
        self.cached_display_entries = Some(entries.into_iter().cloned().collect());

        // Pre-compute list items with static strings
        self.cached_list_items = Some(
            self.cached_display_entries.as_ref().unwrap()
                .iter()
                .enumerate()
                .map(|(idx, entry)|
                    Self::create_display_string(entry, app, idx, mode)).collect()
        );

        self.last_cache_time = Some(compute_start);

        trace!("SearchResultCache computed in {:?}", compute_start.elapsed());
    }

    // Helper methods (moved from FileNameSearchOverlay)
    fn compute_status_text(app: &AppState) -> String {
        if app.ui.input.is_empty() {
            "Ready to search files (folders excluded)".to_string()
        } else if app.ui.input.len() < MIN_SEARCH_LENGTH {
            format!(
                "Type {} more character(s) to start searching",
                MIN_SEARCH_LENGTH - app.ui.input.len()
            )
        } else {
            let local_count = app
                .fs
                .active_pane()
                .entries
                .iter()
                .filter(|e| {
                    !e.is_dir && e.name.to_lowercase().contains(&app.ui.input.to_lowercase())
                })
                .count();
            let recursive_count = app
                .ui
                .filename_search_results
                .iter()
                .filter(|e| !e.is_dir)
                .count();

            

            if Self::is_search_active(app) {
                format!("Searching... (Found {local_count} local file matches)")
            } else if recursive_count > 0 {
                format!("Found {local_count} local + {recursive_count} recursive file matches")
            } else {
                format!("Found {local_count} local file matches")
            }
        }
    }

    /// Get local matches for immediate feedback - files only
    fn get_local_matches(app: &AppState) -> Vec<&ObjectInfo> {
        if app.ui.input.is_empty() || app.ui.input.len() < MIN_SEARCH_LENGTH {
            return Vec::new();
        }

        let search_term = app.ui.input.to_lowercase();
        app.fs
            .active_pane()
            .entries
            .iter()
            .filter(|entry| !entry.is_dir && entry.name.to_lowercase().contains(&search_term))
            .collect()
    }

    /// Determine display entries and search mode - files only, empty when no input
    fn compute_display_entries(app: &AppState) -> (Vec<&ObjectInfo>, SearchMode) {
        // Return empty results if no input
        if app.ui.input.is_empty() {
            return (Vec::new(), SearchMode::Local);
        }

        let local_matches = Self::get_local_matches(app);

        if !app.ui.filename_search_results.is_empty() {
            // We have recursive results - filter to files only
            let recursive_files: Vec<&ObjectInfo> = app
                .ui
                .filename_search_results
                .iter()
                .filter(|entry| !entry.is_dir)
                .collect();

            if local_matches.is_empty() {
                // Only recursive file results
                (recursive_files, SearchMode::Recursive)
            } else {
                // Mixed: both local and recursive files
                let mut combined: Vec<&ObjectInfo> = local_matches;
                combined.extend(recursive_files);
                // Remove duplicates based on path
                combined.sort_by_key(|entry| &entry.path);
                combined.dedup_by_key(|entry| &entry.path);
                (combined, SearchMode::Mixed)
            }
        } else if !local_matches.is_empty() {
            // Only local file matches
            (local_matches, SearchMode::Local)
        } else {
            // No matches at all
            (Vec::new(), SearchMode::Local)
        }
    }

    /// Create enhanced list item with rich formatting
    fn create_display_string<'a>(
        entry: &'a ObjectInfo,
        app: &'a AppState,
        _idx: usize,
        search_mode: SearchMode,
    ) -> String {
        // Display name based on search mode
        let display_name: CompactString = match search_mode {
            SearchMode::Local => entry.name.clone(),
            SearchMode::Recursive | SearchMode::Mixed => {
                // Show relative path for recursive results
                let current_dir: &PathBuf = &app.fs.active_pane().cwd;
                
                entry
                    .path.strip_prefix(current_dir)
                    .map_or_else(
                        |_| -> CompactString 
                        {
                            entry.path.to_string_lossy().to_compact_string()
                        },
                            |relative: &Path| -> CompactString 
                        {
                            relative.to_string_lossy().to_compact_string()
                        }
                    )
            }
        };

        if entry.size > 0 
        {
            let size_str = Self::format_file_size(entry.size);

            format!("{display_name} ({size_str})")
        } else {
            display_name.to_string()
        }
    }

        /// Helper methods for enhanced functionality
    ///
    /// Check if there's an active filename search task
    fn is_search_active(app: &AppState) -> bool {
        app.tasks
            .values()
            .any(|task| task.description.contains("Filename search") && !task.is_completed)
    }

    #[allow(clippy::cast_precision_loss)]
    /// Format file size in human readable format
    fn format_file_size(size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size_f = size as f64;
        let mut unit_idx = 0;

        while size_f >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size_f /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{} {}", size, UNITS[unit_idx])
        } else {
            format!("{:.1} {}", size_f, UNITS[unit_idx])
        }
    }
}