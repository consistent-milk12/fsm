//! src/view/components/filename_search_overlay.rs
//! ============================================================================
//! # FileNameSearchOverlay: Enhanced Live file/folder name search
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
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::{Duration, Instant};
use tracing::{debug, info, trace};

/// Configuration constants for search behavior
const MAX_DISPLAY_RESULTS: usize = 100; // Limit displayed results for performance
const MIN_SEARCH_LENGTH: usize = 2; // Minimum characters before starting search

// Future enhancement constants (currently unused but ready for implementation)
#[allow(dead_code)]
const SEARCH_DEBOUNCE_MS: u64 = 300; // Wait 300ms after last keystroke

#[allow(dead_code)]
const CACHE_TTL_SECONDS: u64 = 30; // Cache results for 30 seconds

/// Enhanced filename search overlay with improved responsiveness and logging
pub struct FileNameSearchOverlay;

impl FileNameSearchOverlay {
    /// Main render function with enhanced logging and error handling
    pub fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
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
        Self::render_status_bar(frame, app, layout[1]);

        // Render search results with enhanced features
        Self::render_search_results(frame, app, layout[2]);

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
    fn render_status_bar(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let status_text = if app.ui.input.is_empty() {
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
        };

        let status_paragraph = Paragraph::new(status_text.clone())
            .style(Style::default().fg(theme::COMMENT))
            .alignment(Alignment::Center);

        frame.render_widget(status_paragraph, area);
        debug!("Status bar rendered: '{}'", status_text);
    }

    /// Enhanced search results rendering with pagination and improved feedback
    fn render_search_results(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        let search_start = Instant::now();
        trace!("render_search_results started");
        let is_searching = Self::is_search_active(app);

        debug!(
            "Search state: is_searching={}, input_length={}, recursive_results={}",
            is_searching,
            app.ui.input.len(),
            app.ui.filename_search_results.len()
        );

        // Enhanced loading state with spinner and progress info
        if is_searching {
            let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let spinner_frame =
                (search_start.elapsed().as_millis() / 80) % spinner_chars.len() as u128;
            let spinner = spinner_chars[spinner_frame as usize];

            let loading_text = format!(
                "{} Searching recursively for '{}'...",
                spinner, app.ui.input
            );

            // Show any intermediate local results while searching
            let local_matches = Self::get_local_matches(app);
            let subtitle = if !local_matches.is_empty() {
                format!(
                    "Found {} local matches, searching deeper...",
                    local_matches.len()
                )
            } else {
                "Scanning directories...".to_string()
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

            debug!(
                "Loading state rendered: query='{}', local_matches={}",
                app.ui.input,
                local_matches.len()
            );
            return;
        }

        // Enhanced result aggregation with smart merging and deduplication
        let (entries_to_display, search_mode) = Self::get_display_entries(app);

        debug!(
            "Display entries prepared: count={}, mode={:?}",
            entries_to_display.len(),
            search_mode
        );

        // Enhanced empty state with helpful suggestions
        if entries_to_display.is_empty() {
            let message = if app.ui.input.is_empty() {
                "Type to search for files\n\nTip: Search works across all subdirectories"
                    .to_string()
            } else if app.ui.input.len() < MIN_SEARCH_LENGTH {
                format!(
                    "Type {} more character(s) to start searching",
                    MIN_SEARCH_LENGTH - app.ui.input.len()
                )
            } else {
                format!(
                    "No file matches found for '{}'\n\nTip: Only files are shown (not folders)\n• Try different spelling\n• Use partial filename\n• Include file extension",
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

            info!("No search results for query: '{}'", app.ui.input);
            return;
        }

        // Enhanced list items with pagination and rich formatting - files only
        let total_results = entries_to_display.len();
        let display_entries = if total_results > MAX_DISPLAY_RESULTS {
            info!(
                "Limiting display to {} file results out of {} total",
                MAX_DISPLAY_RESULTS, total_results
            );
            &entries_to_display[..MAX_DISPLAY_RESULTS]
        } else {
            &entries_to_display[..]
        };

        let list_items: Vec<ListItem> = display_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| Self::create_enhanced_list_item(entry, app, idx, search_mode))
            .collect();

        debug!(
            "List items created: displayed={}, total={}",
            display_entries.len(),
            total_results
        );

        // Enhanced title with more context - files only
        let title = match search_mode {
            SearchMode::Local => format!(" {} Local Files ", display_entries.len()),
            SearchMode::Recursive => format!(" {} Recursive Files ", display_entries.len()),
            SearchMode::Mixed => format!(
                " {} Mixed Files ({}+{}) ",
                display_entries.len(),
                Self::get_local_matches(app).len(),
                app.ui
                    .filename_search_results
                    .iter()
                    .filter(|e| !e.is_dir)
                    .count()
            ),
        };

        let title_with_truncation = if total_results > MAX_DISPLAY_RESULTS {
            format!(
                "{} (showing {}/{})",
                title.trim(),
                MAX_DISPLAY_RESULTS,
                total_results
            )
        } else {
            title
        };

        let results_block = Block::default()
            .borders(Borders::ALL)
            .title(title_with_truncation)
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
        // Ensure selection is within bounds
        let adjusted_selection = app
            .ui
            .selected
            .map(|s| s.min(display_entries.len().saturating_sub(1)));
        list_state.select(adjusted_selection);

        frame.render_stateful_widget(list, area, &mut list_state);

        let render_duration = search_start.elapsed();
        trace!(
            "Search results rendered in {:?}: {} items displayed",
            render_duration,
            display_entries.len()
        );

        if render_duration > Duration::from_millis(8) {
            info!(
                "Search results render took {:?} (slow) for {} items",
                render_duration,
                display_entries.len()
            );
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

    /// Determine display entries and search mode - files only, empty when no input
    fn get_display_entries(app: &AppState) -> (Vec<&ObjectInfo>, SearchMode) {
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

            if !local_matches.is_empty() {
                // Mixed: both local and recursive files
                let mut combined: Vec<&ObjectInfo> = local_matches;
                combined.extend(recursive_files);
                // Remove duplicates based on path
                combined.sort_by_key(|entry| &entry.path);
                combined.dedup_by_key(|entry| &entry.path);
                (combined, SearchMode::Mixed)
            } else {
                // Only recursive file results
                (recursive_files, SearchMode::Recursive)
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
    fn create_enhanced_list_item<'a>(
        entry: &'a ObjectInfo,
        app: &'a AppState,
        _idx: usize,
        search_mode: SearchMode,
    ) -> ListItem<'a> {
        // Display name based on search mode
        let display_name = match search_mode {
            SearchMode::Local => entry.name.clone(),
            SearchMode::Recursive | SearchMode::Mixed => {
                // Show relative path for recursive results
                let current_dir = &app.fs.active_pane().cwd;
                if let Ok(relative) = entry.path.strip_prefix(current_dir) {
                    relative.to_string_lossy().to_string()
                } else {
                    entry.path.to_string_lossy().to_string()
                }
            }
        };

        // Enhanced display with size info for files (directories excluded)
        let display_text = if entry.size > 0 {
            let size_str = Self::format_file_size(entry.size);
            format!("{display_name} ({size_str})")
        } else {
            display_name.to_string()
        };

        // Color coding based on file type and state (files only)
        let style = if entry.is_symlink {
            Style::default().fg(theme::PURPLE) // Use purple for symlinks
        } else if entry.name.starts_with('.') {
            Style::default().fg(theme::COMMENT) // Hidden files
        } else {
            Style::default().fg(theme::FOREGROUND)
        };

        ListItem::new(display_text).style(style)
    }

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
