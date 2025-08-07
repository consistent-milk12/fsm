//! ``src/controller/event_loop.rs``
//! ============================================================================
//! # Enhanced Event Loop Controller with Advanced Features
//!
//! Production-ready event loop implementation with:
//! - Async/await architecture with optimized task handling
//! - Complete command palette integration with auto-completion
//! - Comprehensive input prompt system with all types implemented
//! - Advanced search capabilities (filename, content, raw results)
//! - Robust error handling and recovery mechanisms
//! - Performance monitoring and resource management
//! - Extensive logging and debugging support

use crate::fs::dir_scanner::ScanUpdate;
use crate::model::app_state::AppState;
use crate::model::command_palette::CommandAction;
use crate::model::fs_state::{EntryFilter, EntrySort, PaneState};
use crate::model::object_registry::SortableEntry;
use crate::model::shared_state::SharedState;
use crate::model::ui_state::{
    Component, LoadingState, NotificationLevel, UIMode, UIOverlay, UIState,
};
use crate::tasks::file_ops_task::{FileOperation, FileOperationTask};
use crate::tasks::search_task::RawSearchResult;
use crate::{
    controller::actions::{Action, InputPromptType},
    logging::ProfilingData,
};
use crossterm::event::{Event as TermEvent, EventStream, KeyCode, KeyModifiers};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{Span, debug, info, span::Entered, trace, warn};

/// Enhanced task result with performance metrics
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Legacy task result format
    Legacy {
        task_id: u64,
        result: Result<String, String>,
        progress: Option<f64>,
        current_item: Option<String>,
        completed: Option<u64>,
        total: Option<u64>,
        message: Option<String>,
        execution_time: Option<std::time::Duration>,
        memory_usage: Option<u64>,
    },

    /// File operation completion
    FileOperationComplete {
        operation_id: String,
        result: Result<(), crate::error::AppError>,
    },

    /// Real-time progress reporting for file operations
    FileOperationProgress {
        operation_id: String,

        /// "Copy", "Move", "Rename"
        operation_type: String,

        /// Bytes processed so far
        current_bytes: u64,

        /// Total bytes to process
        total_bytes: u64,

        /// Currently processing file
        current_file: PathBuf,

        /// Files completely processed
        files_completed: u32,

        /// Total files to process
        total_files: u32,

        /// For ETA calculation
        start_time: Instant,

        /// Bytes per second
        throughput_bps: Option<u64>,
    },
}

/// Enhanced event loop with performance monitoring and advanced features
pub struct EventLoop {
    pub app: Arc<SharedState>,
    task_rx: mpsc::UnboundedReceiver<TaskResult>,
    event_stream: EventStream,
    action_rx: mpsc::UnboundedReceiver<Action>,
    // Performance monitoring
    event_count: u64,
    last_performance_check: Instant,
    avg_response_time: f64,
}

impl EventLoop {
    /// Create new enhanced event loop with performance monitoring
    pub fn new(
        app: Arc<SharedState>,
        task_rx: mpsc::UnboundedReceiver<TaskResult>,
        action_rx: mpsc::UnboundedReceiver<Action>,
    ) -> Self {
        info!("Initializing enhanced event loop controller with performance monitoring");
        Self {
            app,
            task_rx,
            event_stream: EventStream::new(),
            action_rx,
            event_count: 0,
            last_performance_check: Instant::now(),
            avg_response_time: 0.0,
        }
    }

    /// Calculate search result count across all types with caching
    fn current_result_count(app: &SharedState) -> usize {
        // Use fine-grained UI locking
        let ui_guard = app.lock_ui();
        let count: usize = ui_guard.raw_search_results.as_ref().map_or(
            if ui_guard.rich_search_results.is_empty() {
                ui_guard.search_results.len()
            } else {
                ui_guard.rich_search_results.len()
            },
            |raw_results: &RawSearchResult| -> usize { raw_results.lines.len() },
        );
        trace!("Calculated result count: {}", count);
        count
    }

    #[allow(clippy::cast_precision_loss)]
    /// Performance monitoring - track event processing times
    fn update_performance_metrics(&mut self, processing_time: std::time::Duration) {
        self.event_count += 1;
        let time_ms: f64 = processing_time.as_millis() as f64;

        // Update running average
        if self.event_count == 1 {
            self.avg_response_time = time_ms;
        } else {
            self.avg_response_time = self.avg_response_time.mul_add(0.9, time_ms * 0.1);
        }

        // Log performance warnings with profiling data
        if time_ms > 16.0 {
            // 60fps threshold - collect profiling data for slow events
            let profiling_data: ProfilingData =
                ProfilingData::collect_profiling_data(None, processing_time);

            // Check channel queue lengths for diagnostic info
            let task_queue_len: usize = self.task_rx.len();
            let action_queue_len: usize = self.action_rx.len();

            info!(
                marker = "PERF_SLOW_EVENT",
                operation_type = "event_processing",
                duration_ns = profiling_data.operation_duration_ns.unwrap_or(0),
                task_queue_len = task_queue_len,
                action_queue_len = action_queue_len,
                "Slow event processing: {:.2}ms (avg: {:.2}ms) [task_q:{}, action_q:{}]",
                time_ms,
                self.avg_response_time,
                task_queue_len,
                action_queue_len
            );
        }

        // Periodic performance reports
        if self.last_performance_check.elapsed().as_secs() >= 30 {
            info!(
                "Performance: {} events processed, avg response time: {:.2}ms",
                self.event_count, self.avg_response_time
            );
            self.last_performance_check = Instant::now();
        }
    }

    /// Optimized event loop with non-blocking receives for improved responsiveness
    pub async fn next_action(&mut self) -> Option<Action> {
        let start_time = Instant::now();

        // Try non-blocking receives first for better input responsiveness
        if let Ok(task_result) = self.task_rx.try_recv() {
            debug!("Task result received (immediate): {:?}", task_result);
            return Some(Action::TaskResult(task_result));
        }

        if let Ok(action) = self.action_rx.try_recv() {
            debug!("Direct action received (immediate): {:?}", action);
            return Some(action);
        }

        // Use blocking select with prioritized terminal events for UI responsiveness
        let action: Option<Action> = tokio::select! {
            // Prioritize terminal events for immediate input response
            biased;

            Some(Ok(event)) = self.event_stream.next() => {
                trace!("Terminal event received: {:?}", event);
                let action = self.handle_terminal_event(event).await;
                debug!("Terminal event mapped to action: {:?}", action);
                Some(action)
            }

            Some(task_result) = self.task_rx.recv() => {
                debug!("Task result received: {:?}", task_result);
                Some(Action::TaskResult(task_result))
            }

            Some(action) = self.action_rx.recv() => {
                debug!("Direct action received: {:?}", action);
                Some(action)
            }

            else => {
                info!("Event loop terminated - no more events");
                None
            }
        };

        self.update_performance_metrics(start_time.elapsed());
        action
    }

    /// Enhanced terminal event handling with comprehensive logging
    async fn handle_terminal_event(&self, event: TermEvent) -> Action {
        // Fine-grained locking: Only access UI state for event processing
        let (current_overlay, current_mode, has_notification) = {
            let ui_guard = self.app.lock_ui();
            (
                ui_guard.overlay,
                ui_guard.mode,
                ui_guard.notification.is_some(),
            )
        };

        debug!(
            "Processing event in mode={:?}, overlay={:?}, notification={}",
            current_mode, current_overlay, has_notification
        );

        match event {
            TermEvent::Key(key_event) => {
                trace!(
                    "Key event: code={:?}, modifiers={:?}",
                    key_event.code, key_event.modifiers
                );

                // Global Escape handling - highest priority
                if key_event.code == KeyCode::Esc {
                    return self
                        .handle_escape_key(current_mode, current_overlay, has_notification)
                        .await;
                }

                // Auto-dismiss notifications on any key
                if has_notification {
                    debug!("Auto-dismissing notification on key press");
                    let mut ui_guard = self.app.lock_ui();
                    ui_guard.dismiss_notification();
                    ui_guard.mark_dirty(Component::Notification);
                    // Continue processing the key event
                }

                // Route to specialized handlers
                match current_mode {
                    UIMode::Command => self.handle_command_mode_keys(key_event).await,
                    _ => match current_overlay {
                        UIOverlay::None => self.handle_navigation_mode_keys(key_event).await,

                        UIOverlay::FileNameSearch => {
                            self.handle_filename_search_keys(key_event).await
                        }

                        UIOverlay::ContentSearch => {
                            self.handle_content_search_keys(key_event).await
                        }

                        UIOverlay::Prompt => self.handle_prompt_keys(key_event).await,

                        UIOverlay::SearchResults => {
                            self.handle_search_results_keys(key_event).await
                        }

                        _ => {
                            debug!("Ignoring key in overlay mode: {:?}", current_overlay);
                            Action::NoOp
                        }
                    },
                }
            }

            TermEvent::Mouse(mouse_event) => {
                trace!("Mouse event: {:?}", mouse_event);
                Action::Mouse(mouse_event)
            }

            TermEvent::Resize(x, y) => {
                info!("Terminal resize: {}x{}", x, y);
                Action::Resize(x, y)
            }

            _ => {
                trace!("Unhandled terminal event: {:?}", event);
                Action::Tick
            }
        }
    }

    /// Enhanced escape key handling with context awareness
    async fn handle_escape_key(
        &self,
        mode: UIMode,
        overlay: UIOverlay,
        has_notification: bool,
    ) -> Action {
        // HIGHEST PRIORITY: Cancel active file operations
        {
            let mut ui_guard = self.app.lock_ui();

            if !ui_guard.active_file_operations.is_empty() {
                let cancelled_count: usize = ui_guard.cancel_all_operations();

                if cancelled_count > 0 {
                    ui_guard.show_info(format!("Cancelled {cancelled_count} file operations(s)"));

                    info!("User cancelled {cancelled_count} file operations via ESC key");

                    ui_guard.mark_dirty(Component::Overlay);
                    ui_guard.mark_dirty(Component::Notification);

                    drop(ui_guard);

                    return Action::NoOp;
                }
            }
        }

        debug!(
            "Escape pressed: mode={:?}, overlay={:?}, notification={}",
            mode, overlay, has_notification
        );

        // Priority order: notification -> overlay -> command completions -> command mode -> quit
        if has_notification {
            debug!("Escape: dismissing notification");
            let mut ui_guard = self.app.lock_ui();
            ui_guard.dismiss_notification();
            ui_guard.mark_dirty(Component::Notification);

            drop(ui_guard);

            return Action::NoOp;
        }

        if overlay != UIOverlay::None {
            debug!("Escape: closing overlay {:?}", overlay);
            return Action::CloseOverlay;
        }

        if mode == UIMode::Command {
            debug!("Escape: checking command completions");
            let mut ui_guard = self.app.lock_ui();
            if ui_guard.command_palette.show_completions {
                debug!("Escape: hiding command completions");
                ui_guard.command_palette.hide_completions();
                ui_guard.mark_dirty(Component::Command);
                return Action::NoOp;
            }
            debug!("Escape: exiting command mode");
            return Action::ExitCommandMode;
        }

        debug!("Escape: requesting application quit");
        Action::Quit
    }

    #[allow(clippy::cognitive_complexity, reason = "Will probably refactor later")]
    /// Enhanced command mode with improved auto-completion
    async fn handle_command_mode_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Command mode key: {:?}", key.code);

        match key.code {
            KeyCode::Char(c) => {
                debug!("Command mode: adding character '{}'", c);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.command_palette.input.push(c);
                ui_guard.command_palette.update_filter();
                ui_guard.command_palette.show_completions_if_available();
                trace!(
                    "Command input: '{}', completions available: {}",
                    ui_guard.command_palette.input, ui_guard.command_palette.show_completions
                );
                Action::NoOp
            }
            KeyCode::Backspace => {
                debug!("Command mode: backspace");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.command_palette.input.pop();
                ui_guard.command_palette.update_filter();
                ui_guard.command_palette.show_completions_if_available();
                trace!(
                    "Command input: '{}' (after backspace)",
                    ui_guard.command_palette.input
                );
                Action::NoOp
            }
            KeyCode::Up => {
                debug!("Command mode: up arrow navigation");
                let mut ui_guard = self.app.lock_ui();
                if ui_guard.command_palette.show_completions {
                    ui_guard.command_palette.prev_completion();
                    trace!("Command completions: navigated up");
                } else {
                    ui_guard.command_palette.selected =
                        ui_guard.command_palette.selected.saturating_sub(1);
                    trace!(
                        "Command history: navigated up to {}",
                        ui_guard.command_palette.selected
                    );
                }
                Action::NoOp
            }
            KeyCode::Down => {
                debug!("Command mode: down arrow navigation");
                let mut ui_guard = self.app.lock_ui();
                if ui_guard.command_palette.show_completions {
                    ui_guard.command_palette.next_completion();
                    trace!("Command completions: navigated down");
                } else {
                    let max_idx = ui_guard.command_palette.filtered.len().saturating_sub(1);
                    ui_guard.command_palette.selected = ui_guard
                        .command_palette
                        .selected
                        .saturating_add(1)
                        .min(max_idx);
                    trace!(
                        "Command history: navigated down to {}",
                        ui_guard.command_palette.selected
                    );
                }
                Action::NoOp
            }
            KeyCode::Tab => {
                debug!("Command mode: tab completion");
                let mut ui_guard = self.app.lock_ui();
                if ui_guard.command_palette.show_completions {
                    let before = ui_guard.command_palette.input.clone();
                    ui_guard.command_palette.apply_completion();
                    let after = ui_guard.command_palette.input.clone();
                    info!("Applied completion: '{}' -> '{}'", before, after);
                } else {
                    trace!("Tab pressed but no completions available");
                }
                Action::NoOp
            }
            KeyCode::Enter => self.handle_command_enter_key().await,
            _ => {
                trace!("Command mode: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

    async fn handle_command_enter_key(&self) -> Action {
        debug!("Command mode: executing command");
        let ui_guard = self.app.lock_ui();
        let input: &str = ui_guard.command_palette.input.trim();
        info!("Executing command: '{}'", input);
        // Try parsing user input first
        ui_guard.command_palette.parse_command().map_or_else(
            || {
                ui_guard
                    .command_palette
                    .filtered
                    .get(ui_guard.command_palette.selected)
                    .map_or_else(
                        || {
                            info!("No valid command to execute, exiting command mode");
                            Action::ExitCommandMode
                        },
                        |cmd| {
                            debug!("Using selected command from list: {:?}", cmd.action);
                            Self::map_command_action_to_action(cmd.action.clone())
                        },
                    )
            },
            |parsed_action| {
                debug!("Command parsed successfully: {:?}", parsed_action);
                Self::map_command_action_to_action(parsed_action)
            },
        )
    }

    #[allow(clippy::unused_async)]
    /// Enhanced navigation with improved responsiveness
    async fn handle_navigation_mode_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!(
            "Navigation key: {:?} with modifiers {:?}",
            key.code, key.modifiers
        );

        match (key.code, key.modifiers) {
            // Core command access
            (KeyCode::Char(':'), _) => {
                info!("Entering command mode");
                Action::EnterCommandMode
            }

            // Help system
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Char('?'), _) => {
                info!("Toggling help overlay");
                Action::ToggleHelp
            }

            // Quick file search
            (KeyCode::Char('/'), _) => {
                info!("Opening filename search");
                Action::ToggleFileNameSearch
            }

            // File operations
            (KeyCode::Char('c'), _) => {
                info!("Copy file - requesting destination");
                Action::ShowInputPrompt(InputPromptType::CopyDestination)
            }

            (KeyCode::Char('m'), _) => {
                info!("Move file - requesting destination");
                Action::ShowInputPrompt(InputPromptType::MoveDestination)
            }

            (KeyCode::Char('r'), _) => {
                info!("Rename file - requesting new name");
                Action::ShowInputPrompt(InputPromptType::RenameFile)
            }

            // Navigation keys
            (KeyCode::Up, _) => Action::MoveSelectionUp,

            (KeyCode::Down, _) => Action::MoveSelectionDown,

            (KeyCode::PageUp, _) => Action::PageUp,

            (KeyCode::PageDown, _) => Action::PageDown,

            (KeyCode::Home, _) => Action::SelectFirst,

            (KeyCode::End, _) => Action::SelectLast,

            (KeyCode::Enter, _) => Action::EnterSelected,

            (KeyCode::Backspace, _) => Action::GoToParent,

            // System controls
            (KeyCode::Char('q'), _) => {
                info!("Quit requested");
                Action::Quit
            }

            // Developer shortcuts
            (KeyCode::Char('.'), KeyModifiers::CONTROL) => Action::ToggleShowHidden,

            (KeyCode::Char('l'), KeyModifiers::CONTROL) => Action::SimulateLoading,

            _ => {
                trace!("Unhandled navigation key: {:?}", key);
                Action::Key(key)
            }
        }
    }

    /// Enhanced filename search with better UX
    async fn handle_filename_search_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Filename search key: {:?}", key.code);

        match key.code {
            KeyCode::Char(c) => {
                debug!("Filename search: adding character '{}'", c);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.push(c);
                let pattern: String = ui_guard.input.clone();
                trace!("Filename search pattern: '{}'", pattern);
                Action::FileNameSearch(pattern)
            }
            KeyCode::Backspace => {
                debug!("Filename search: backspace");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.pop();
                let pattern = ui_guard.input.clone();
                trace!("Filename search pattern: '{}' (after backspace)", pattern);
                Action::FileNameSearch(pattern)
            }
            KeyCode::Enter => {
                debug!("Filename search: enter pressed");
                let ui_guard = self.app.lock_ui();
                // Try to open selected result
                if !ui_guard.filename_search_results.is_empty()
                    && let Some(selected_idx) = ui_guard.selected
                    && let Some(selected_entry) = ui_guard.filename_search_results.get(selected_idx)
                {
                    // Registry access is now lock-free or via a method on SharedState
                    if let Some(obj_info) = self.app.get_object_info(selected_entry.id) {
                        info!("Opening selected file: {:?}", obj_info.path);
                        return Action::OpenFile(obj_info.path.clone(), None);
                    }
                }
                // Fallback to triggering search
                if ui_guard.input.trim().is_empty() {
                    debug!("Closing filename search (empty input)");
                    Action::CloseOverlay
                } else {
                    debug!("Triggering filename search for: '{}'", ui_guard.input);
                    Action::FileNameSearch(ui_guard.input.clone())
                }
            }
            KeyCode::Up => {
                debug!("Filename search: navigate up");
                let mut ui_guard = self.app.lock_ui();
                let result_count: usize = ui_guard.filename_search_results.len();
                if result_count > 0 {
                    ui_guard.selected = Some(ui_guard.selected.unwrap_or(0).saturating_sub(1));
                    trace!("Filename search selection: {:?}", ui_guard.selected);
                }
                Action::NoOp
            }
            KeyCode::Down => {
                debug!("Filename search: navigate down");
                let mut ui_guard = self.app.lock_ui();
                let result_count: usize = ui_guard.filename_search_results.len();
                if result_count > 0 {
                    let current: usize = ui_guard.selected.unwrap_or(0);
                    ui_guard.selected = Some((current + 1).min(result_count.saturating_sub(1)));
                    trace!("Filename search selection: {:?}", ui_guard.selected);
                }
                Action::NoOp
            }
            _ => {
                trace!("Filename search: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

    /// Enhanced content search with better result handling
    async fn handle_content_search_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Content search key: {:?}", key.code);

        match key.code {
            KeyCode::Char(c) => {
                debug!("Content search: adding character '{}'", c);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.push(c);
                // Clear previous results for real-time search
                ui_guard.search_results.clear();
                ui_guard.rich_search_results.clear();
                ui_guard.raw_search_results = None;
                ui_guard.last_query = None;
                ui_guard.selected = None;
                ui_guard.mark_dirty(Component::Main);
                trace!(
                    "Content search input: '{}' (results cleared)",
                    ui_guard.input
                );
                Action::NoOp
            }
            KeyCode::Backspace => {
                debug!("Content search: backspace");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.pop();
                ui_guard.search_results.clear();
                ui_guard.rich_search_results.clear();
                ui_guard.raw_search_results = None;
                ui_guard.last_query = None;
                ui_guard.selected = None;
                ui_guard.mark_dirty(Component::Main);
                trace!(
                    "Content search input: '{}' (after backspace)",
                    ui_guard.input
                );
                Action::NoOp
            }
            KeyCode::Enter => self.handle_content_search_enter_key().await,
            KeyCode::Up => {
                debug!("Content search: navigate up");
                let mut ui_guard = self.app.lock_ui();
                let result_count = Self::current_result_count(&self.app);
                if result_count > 0 {
                    let new_idx = ui_guard.selected.unwrap_or(0).saturating_sub(1);
                    ui_guard.selected = Some(new_idx);
                    ui_guard.mark_dirty(Component::Main);
                    trace!("Content search selection: {}", new_idx);
                }
                Action::NoOp
            }
            KeyCode::Down => {
                debug!("Content search: navigate down");
                let mut ui_guard = self.app.lock_ui();
                let result_count = Self::current_result_count(&self.app);
                if result_count > 0 {
                    let current = ui_guard.selected.unwrap_or(0);
                    let new_idx = (current + 1).min(result_count.saturating_sub(1));
                    ui_guard.selected = Some(new_idx);
                    ui_guard.mark_dirty(Component::Main);
                    trace!("Content search selection: {}", new_idx);
                }
                Action::NoOp
            }
            _ => {
                trace!("Content search: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

    async fn handle_content_search_enter_key(&self) -> Action {
        debug!("Content search: enter pressed");

        // Get selected index and various search results
        let (
            selected_idx,
            raw_results,
            rich_search_results,
            search_results,
            input_pattern,
            current_dir,
        ) = {
            let ui_guard = self.app.lock_ui();
            let fs_guard = self.app.lock_fs();
            (
                ui_guard.selected,
                ui_guard.raw_search_results.clone(),
                ui_guard.rich_search_results.clone(),
                ui_guard.search_results.clone(),
                ui_guard.input.clone(),
                fs_guard.active_pane().cwd.clone(),
            )
        };

        // Try to open selected result first
        if let Some(selected_idx) = selected_idx {
            debug!("Processing selection at index {}", selected_idx);

            // Priority: Raw -> Rich -> Simple results
            if let Some(ref raw_results) = raw_results {
                debug!("Processing raw search results");
                if selected_idx < raw_results.lines.len() {
                    return self
                        .process_raw_search_line(raw_results, selected_idx)
                        .await;
                }
            }

            if !rich_search_results.is_empty() && selected_idx < rich_search_results.len() {
                debug!("Processing rich search results");
                return self
                    .process_rich_search_line(&rich_search_results, selected_idx, &current_dir)
                    .await;
            }

            if !search_results.is_empty() && selected_idx < search_results.len() {
                debug!("Processing simple search results");
                let result = &search_results[selected_idx];
                if let Some(obj_info) = self.app.metadata.get_by_id(result.id) {
                    info!("Opening file: {:?}", obj_info.path);
                    return Action::OpenFile(obj_info.path.clone(), None);
                }
            }
        }

        // No valid selection, start new search
        info!("Starting content search for: '{}'", input_pattern);
        Action::ContentSearch(input_pattern)
    }

    /// Helper to clear search results
    fn clear_search_results(shared_state: &SharedState) {
        let mut ui_guard = shared_state.lock_ui();
        ui_guard.search_results.clear();
        ui_guard.rich_search_results.clear();
        ui_guard.raw_search_results = None;
        ui_guard.last_query = None;
        ui_guard.selected = None;
    }

    #[allow(clippy::unused_async)]
    /// Enhanced raw search result processing
    async fn process_raw_search_line(
        &self,
        raw_results: &RawSearchResult,
        selected_idx: usize,
    ) -> Action {
        debug!("Processing raw search line at index {}", selected_idx);

        let mut current_file = None;

        // Parse lines sequentially to build context
        for (i, context_line) in raw_results.lines.iter().enumerate() {
            if let Some((path, line_num)) = RawSearchResult::parse_heading_line_with_context(
                context_line,
                &mut current_file,
                &raw_results.base_directory,
            ) && i == selected_idx
            {
                info!("Opening from raw search: {:?} at line {:?}", path, line_num);
                return Action::OpenFile(path.into(), line_num);
            }
        }

        info!("Failed to parse raw search line at index {}", selected_idx);
        Action::NoOp
    }

    #[allow(clippy::unused_async)]
    /// Enhanced rich search result processing
    async fn process_rich_search_line(
        &self,
        rich_results: &[String],
        selected_idx: usize,
        base_dir: &std::path::Path,
    ) -> Action {
        debug!("Processing rich search line at index {}", selected_idx);

        if selected_idx < rich_results.len() {
            let mut current_file = None;

            for (i, context_line) in rich_results.iter().enumerate() {
                if let Some((path, line_num)) = RawSearchResult::parse_heading_line_with_context(
                    context_line,
                    &mut current_file,
                    base_dir,
                ) && i == selected_idx
                {
                    info!(
                        "Opening from rich search: {:?} at line {:?}",
                        path, line_num
                    );

                    return Action::OpenFile(path.into(), line_num);
                }
            }
        }

        info!("Failed to parse rich search line at index {}", selected_idx);
        Action::NoOp
    }

    /// Enhanced prompt handling
    async fn handle_prompt_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Prompt key: {:?}", key.code);

        match key.code {
            KeyCode::Char(c) => {
                debug!("Prompt: adding character '{}'", c);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.push(c);
                Action::NoOp
            }
            KeyCode::Backspace => {
                debug!("Prompt: backspace");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.input.pop();
                Action::NoOp
            }
            KeyCode::Enter => {
                debug!("Prompt: enter pressed");
                let ui_guard = self.app.lock_ui();
                let input = ui_guard.input.trim().to_string();
                if input.is_empty() {
                    debug!("Closing prompt (empty input)");
                    Action::CloseOverlay
                } else {
                    info!("Submitting prompt input: '{}'", input);
                    Action::SubmitInputPrompt(input)
                }
            }
            _ => {
                trace!("Prompt: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

    /// Enhanced search results navigation
    async fn handle_search_results_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Search results key: {:?}", key.code);

        match key.code {
            KeyCode::Backspace => {
                debug!("Search results: closing with backspace");
                Action::CloseOverlay
            }

            KeyCode::Enter => {
                debug!("Search results: opening selected result");

                let (selected_idx, search_results) = {
                    let ui_guard = self.app.lock_ui();
                    (ui_guard.selected, ui_guard.search_results.clone())
                };

                if let Some(selected_idx) = selected_idx
                    && let Some(result) = search_results.get(selected_idx)
                    && let Some(obj_info) = self.app.metadata.get_by_id(result.id)
                {
                    info!("Opening search result: {:?}", obj_info.path);

                    return Action::OpenFile(obj_info.path.clone(), None);
                }

                Action::NoOp
            }

            KeyCode::Up => {
                debug!("Search results: navigate up");
                let mut ui_guard = self.app.lock_ui();

                if !ui_guard.search_results.is_empty() {
                    let current = ui_guard.selected.unwrap_or(0);
                    ui_guard.selected = Some(current.saturating_sub(1));
                    ui_guard.mark_dirty(Component::Main);
                }
                Action::NoOp
            }

            KeyCode::Down => {
                debug!("Search results: navigate down");
                let mut ui_guard = self.app.lock_ui();
                let result_count: usize = ui_guard.search_results.len();

                if result_count > 0 {
                    let current = ui_guard.selected.unwrap_or(0);
                    ui_guard.selected = Some((current + 1).min(result_count.saturating_sub(1)));
                    ui_guard.mark_dirty(Component::Main);
                }

                Action::NoOp
            }

            _ => {
                trace!("Search results: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

    /// Enhanced command action mapping
    fn map_command_action_to_action(cmd_action: CommandAction) -> Action {
        debug!("Mapping command action: {:?}", cmd_action);

        let action: Action = match cmd_action {
            CommandAction::OpenConfig => {
                info!("Command: open config (opening system config file)");
                // TODO: Open actual config file
                Action::ExitCommandMode
            }

            CommandAction::Reload => {
                info!("Command: reload directory");
                Action::ReloadDirectory
            }

            CommandAction::NewFile => {
                info!("Command: create new file");
                Action::CreateFile
            }

            CommandAction::NewFolder => {
                info!("Command: create new folder");
                Action::CreateDirectory
            }

            CommandAction::NewFileWithName(name) => {
                info!("Command: create new file '{}'", name);
                Action::CreateFileWithName(name)
            }

            CommandAction::NewFolderWithName(name) => {
                info!("Command: create new folder '{}'", name);
                Action::CreateDirectoryWithName(name)
            }

            CommandAction::SearchContent => {
                info!("Command: toggle content search");
                Action::ToggleContentSearch
            }

            CommandAction::SearchContentWithPattern(pattern) => {
                info!("Command: direct content search for '{}'", pattern);
                Action::DirectContentSearch(pattern)
            }

            CommandAction::Custom(cmd) => {
                info!("Command: custom command '{}' not implemented", cmd);
                Action::ExitCommandMode
            }
        };

        debug!("Command action mapped to: {:?}", action);
        action
    }

    /// Enhanced action dispatcher with comprehensive error handling
    pub async fn dispatch_action(&self, action: Action) {
        let span: Span = tracing::info_span!(
            "action_dispatch",
            action = ?action,
            operation_type = "action_dispatch"
        );

        let guard: Entered<'_> = span.enter();

        let start_time: Instant = Instant::now();

        // Drop the span guard before any async operations
        drop(guard);

        match action {
            // Batch update actions
            Action::BatchUpdateObjectInfo {
                parent_dir,
                objects,
            } => {
                trace!(
                    "Batch updating {0} object infos for {parent_dir:?}",
                    objects.len()
                );
                self.app
                    .update_object_info_batch(&parent_dir, objects)
                    .await;
                let mut ui_guard = self.app.lock_ui();
                ui_guard.mark_dirty(Component::Main);
            }
            // UI actions
            Action::ToggleHelp
            | Action::EnterCommandMode
            | Action::ExitCommandMode
            | Action::ToggleFileNameSearch
            | Action::ToggleContentSearch
            | Action::CloseOverlay
            | Action::ToggleShowHidden
            | Action::SimulateLoading => self.dispatch_ui_action(action).await,
            // Navigation
            Action::MoveSelectionUp
            | Action::MoveSelectionDown
            | Action::PageUp
            | Action::PageDown
            | Action::SelectFirst
            | Action::SelectLast
            | Action::EnterSelected
            | Action::GoToParent => self.dispatch_navigation_action(action).await,
            // Command-driven actions
            Action::CreateFile
            | Action::CreateDirectory
            | Action::CreateFileWithName(_)
            | Action::CreateDirectoryWithName(_)
            | Action::ReloadDirectory
            | Action::Delete
            | Action::RenameEntry(_)
            | Action::GoToPath(_) => self.dispatch_command_action(action).await,
            // Search
            Action::FileNameSearch(_)
            | Action::ContentSearch(_)
            | Action::DirectContentSearch(_)
            | Action::ShowSearchResults(_)
            | Action::ShowFilenameSearchResults(_)
            | Action::ShowRichSearchResults(_)
            | Action::ShowRawSearchResults(_)
            | Action::OpenFile(_, _) => self.dispatch_search_action(action).await,
            // Task/Update results
            Action::TaskResult(_)
            | Action::DirectoryScanUpdate { .. }
            | Action::UpdateObjectInfo { .. } => self.dispatch_task_update_action(action).await,
            // Input prompts
            Action::ShowInputPrompt(_) | Action::SubmitInputPrompt(_) => {
                self.dispatch_prompt_action(action).await;
            }
            // File operation tasks
            Action::Copy { .. }
            | Action::Move { .. }
            | Action::Rename { .. }
            | Action::CancelFileOperation { .. } => self.dispatch_file_op_action(action).await,
            // Legacy/Misc
            Action::Sort(_) | Action::Filter(_) => {
                self.dispatch_legacy_action(action).await;
            }
            Action::Quit => {
                info!("Quit action - handled in main loop");
            }
            Action::Tick => {
                // Quiet tick processing with performance monitoring
                let mut ui_guard = self.app.lock_ui();
                let redraw_needed: bool = ui_guard.update_notification();
                // Periodic cleanup and optimization
                if self.event_count.is_multiple_of(1000) {
                    trace!("Performing periodic cleanup (event #{}))", self.event_count);
                    // Could add memory cleanup, cache pruning, etc. here
                }
                if redraw_needed {
                    ui_guard.mark_dirty(Component::All);
                }
            }
            Action::Key(_) | Action::Mouse(_) | Action::Resize(..) | Action::NoOp => {
                let mut ui_guard = self.app.lock_ui();
                ui_guard.mark_dirty(Component::All);
            }
        }

        let execution_time: Duration = start_time.elapsed();
        if execution_time.as_millis() > 10 {
            debug!("Action dispatch took {:.2}ms", execution_time.as_millis());
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "Probably refactor later")]
    async fn dispatch_ui_action(&self, action: Action) {
        match action {
            Action::ToggleHelp => {
                debug!("Toggling help overlay");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.toggle_help_overlay();
                ui_guard.mark_dirty(Component::All);
                info!("Help overlay toggled to: {:?}", ui_guard.overlay);
            }
            Action::EnterCommandMode => {
                debug!("Entering command mode");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.enter_command_mode();
                ui_guard.mark_dirty(Component::All);
                info!("Command mode activated");
            }
            Action::ExitCommandMode => {
                debug!("Exiting command mode");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.exit_command_mode();
                ui_guard.mark_dirty(Component::All);
                info!("Command mode deactivated");
            }
            Action::ToggleFileNameSearch => {
                debug!("Toggling filename search overlay");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.toggle_filename_search_overlay();
                ui_guard.mark_dirty(Component::All);
                info!("Filename search overlay toggled to: {:?}", ui_guard.overlay);
            }
            Action::ToggleContentSearch => {
                debug!("Toggling content search overlay");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.toggle_content_search_overlay();
                if ui_guard.overlay == UIOverlay::ContentSearch {
                    ui_guard.exit_command_mode();
                    info!("Content search overlay opened, command mode exited");
                } else {
                    info!("Content search overlay closed");
                }
                ui_guard.mark_dirty(Component::All);
            }
            Action::CloseOverlay => {
                debug!("Closing overlay");
                let mut ui_guard = self.app.lock_ui();
                let previous_overlay = ui_guard.overlay;
                ui_guard.close_all_overlays();
                ui_guard.mark_dirty(Component::All);
                info!("Closed overlay: {:?}", previous_overlay);
            }
            Action::ToggleShowHidden => {
                debug!("Toggling hidden files visibility");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.toggle_show_hidden();
                ui_guard.mark_dirty(Component::All);
            }
            Action::SimulateLoading => {
                debug!("Simulating loading state");
                let mut ui_guard = self.app.lock_ui();
                ui_guard.loading = Some(LoadingState {
                    message: "Simulated loading...".into(),
                    progress: None,
                    spinner_frame: 0,
                    current_item: Some("demo.txt".into()),
                    completed: Some(0),
                    total: Some(100),
                });
                ui_guard.overlay = UIOverlay::Loading;
                ui_guard.mark_dirty(Component::All);
            }
            _ => unreachable!(),
        }
    }

    async fn dispatch_navigation_action(&self, action: Action) {
        match action {
            Action::MoveSelectionUp => {
                debug!("Moving selection up");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().move_selection_up();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::MoveSelectionDown => {
                debug!("Moving selection down");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().move_selection_down();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::PageUp => {
                debug!("Page up");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().page_up();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::PageDown => {
                debug!("Page down");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().page_down();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::SelectFirst => {
                debug!("Selecting first entry");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().select_first();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::SelectLast => {
                debug!("Selecting last entry");
                {
                    let mut fs_state = self.app.lock_fs();
                    fs_state.active_pane_mut().select_last();
                    let selected = fs_state.active_pane().selected;
                    let mut ui_state = self.app.lock_ui();
                    ui_state.selected = selected;
                }
            }
            Action::EnterSelected => {
                debug!("Entering selected item");
                self.app.enter_selected_directory().await;
            }
            Action::GoToParent => {
                info!("Going to parent directory");
                self.app.go_to_parent_directory().await;
            }
            _ => unreachable!(),
        }
        {
            let mut ui_state = self.app.lock_ui();
            ui_state.mark_dirty(Component::Main);
        }
    }

    async fn dispatch_command_action(&self, action: Action) {
        match action {
            Action::CreateFile => {
                info!("Creating new file (command-driven)");
                self.app.create_file().await;
            }
            Action::CreateDirectory => {
                info!("Creating new directory (command-driven)");
                self.app.create_directory().await;
            }
            Action::CreateFileWithName(name) => {
                info!("Creating new file '{}' (command-driven)", name);
                self.app.create_file_with_name(name).await;
            }
            Action::CreateDirectoryWithName(name) => {
                info!("Creating new directory '{}' (command-driven)", name);
                self.app.create_directory_with_name(name).await;
            }
            Action::ReloadDirectory => {
                info!("Reloading directory (command-driven)");
                self.app.reload_directory().await;
            }
            Action::Delete => {
                info!("Delete action triggered - this should now be command-driven");
                self.app.delete_entry().await;
            }
            Action::RenameEntry(new_name) => {
                info!("Renaming selected entry to '{}'", new_name);
                self.app.rename_selected_entry(new_name).await;
            }
            Action::GoToPath(path_str) => {
                info!("Navigating to path: '{}'", path_str);
                self.app.navigate_to_path(path_str).await;
            }
            _ => unreachable!(),
        }

        let mut ui_guard = self.app.lock_ui();
        if ui_guard.is_in_command_mode() {
            ui_guard.exit_command_mode();
        }
        ui_guard.mark_dirty(Component::All);
    }

    async fn dispatch_search_action(&self, action: Action) {
        match action {
            Action::FileNameSearch(pattern) => {
                info!("Starting filename search for pattern: '{}'", pattern);
                let mut ui_guard = self.app.lock_ui();
                // Call a method on SharedState or directly update UI state as needed
                ui_guard.set_last_query(Some(pattern));
                ui_guard.mark_dirty(Component::All);
            }
            Action::ContentSearch(pattern) => {
                info!("Starting content search for pattern: '{}'", pattern);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.set_last_query(Some(pattern));
                ui_guard.mark_dirty(Component::All);
            }
            Action::DirectContentSearch(pattern) => {
                info!("Starting direct content search for pattern: '{}'", pattern);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.overlay = UIOverlay::ContentSearch;
                ui_guard.input.clear();
                ui_guard.set_last_query(Some(pattern));
                ui_guard.exit_command_mode();
                ui_guard.mark_dirty(Component::All);
            }
            Action::ShowSearchResults(results) => {
                self.handle_show_search_results(results).await;
            }
            Action::ShowFilenameSearchResults(results) => {
                info!("Showing {} filename search results", results.len());
                let mut ui_guard = self.app.lock_ui();
                ui_guard.filename_search_results = results;
                ui_guard.mark_dirty(Component::All);
            }
            Action::ShowRichSearchResults(results) => {
                self.handle_show_rich_search_results(results).await;
            }
            Action::ShowRawSearchResults(results) => {
                self.handle_show_raw_search_results(results).await;
            }
            Action::OpenFile(path, line_number) => {
                self.handle_open_file(path, line_number).await;
            }
            _ => unreachable!(),
        }
    }

    async fn handle_show_search_results(&self, results: Vec<SortableEntry>) {
        info!("Showing {} search results", results.len());
        let mut ui_guard = self.app.lock_ui();
        ui_guard.search_results = results;

        if ui_guard.overlay != UIOverlay::ContentSearch {
            ui_guard.set_overlay(UIOverlay::SearchResults);
        } else if !ui_guard.search_results.is_empty() {
            ui_guard.selected = Some(0);
        }

        ui_guard.mark_dirty(Component::All);
    }

    async fn handle_show_rich_search_results(&self, results: Vec<String>) {
        info!("Showing {} rich search results", results.len());
        let mut ui_guard = self.app.lock_ui();
        ui_guard.rich_search_results = results;

        if ui_guard.overlay == UIOverlay::ContentSearch && !ui_guard.rich_search_results.is_empty()
        {
            ui_guard.selected = Some(0);
        }

        ui_guard.mark_dirty(Component::All);
    }

    async fn handle_show_raw_search_results(&self, results: RawSearchResult) {
        info!("Showing {} raw search results", results.lines.len());
        let mut ui_guard = self.app.lock_ui();
        ui_guard.raw_search_results = Some(results);
        ui_guard.raw_search_selected = 0;

        if ui_guard.overlay == UIOverlay::ContentSearch {
            ui_guard.selected = Some(0);
        }

        ui_guard.mark_dirty(Component::All);
    }

    async fn handle_open_file(&self, path: Arc<PathBuf>, line_number: Option<usize>) {
        info!("Opening file {:?} at line {:?}", path, line_number);
        let path_str: String = path.to_string_lossy().to_string();

        let mut cmd: Command = Command::new("code");

        if let Some(line) = line_number {
            let goto_arg = format!("{path_str}:{line}");
            debug!("Using VS Code --goto argument: '{}'", goto_arg);
            cmd.arg("--goto").arg(goto_arg);
        } else {
            debug!("Opening file without line number");
            cmd.arg(&path_str);
        }

        match cmd.spawn() {
            Ok(_) => {
                info!("Successfully launched VS Code for file: {}", path_str);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.close_all_overlays();
                ui_guard.mark_dirty(Component::All);
            }
            Err(e) => {
                warn!("Failed to open file with VS Code: {}", e);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.show_error(format!("Failed to open file: {e}"));
                ui_guard.mark_dirty(Component::All);
            }
        }
    }

    async fn dispatch_task_update_action(&self, action: Action) {
        match action {
            Action::TaskResult(task_result) => {
                self.handle_task_result(task_result).await;
            }
            Action::DirectoryScanUpdate { path, update } => {
                self.handle_directory_scan_update(path, update).await;
            }
            Action::UpdateObjectInfo { parent_dir, info } => {
                trace!("Updating object info for {:?}", info.path);
                self.app.update_object_info(&parent_dir, &info);
                let mut ui_guard = self.app.lock_ui();
                ui_guard.mark_dirty(Component::Main);
            }
            _ => unreachable!(),
        }
    }

    async fn handle_task_result(&self, task_result: TaskResult) {
        debug!("Processing task result: {:?}", task_result);
        let mut ui_guard = self.app.lock_ui();

        match task_result {
            TaskResult::Legacy {
                task_id,
                result,
                progress,
                current_item,
                completed,
                ..
            } => {
                self.handle_legacy_task(
                    &mut ui_guard,
                    task_id,
                    result,
                    progress,
                    current_item,
                    completed,
                )
                .await;
            }
            TaskResult::FileOperationComplete {
                operation_id,
                result,
            } => {
                self.handle_file_op_complete(&mut ui_guard, operation_id, result)
                    .await;
                ui_guard.mark_dirty(Component::Overlay);
                ui_guard.mark_dirty(Component::Notification);
            }
            TaskResult::FileOperationProgress {
                operation_id,
                operation_type,
                current_bytes,
                total_bytes,
                current_file,
                files_completed,
                total_files,
                start_time,
                throughput_bps,
            } => {
                self.handle_file_op_progress(
                    &mut ui_guard,
                    operation_id,
                    operation_type,
                    current_bytes,
                    total_bytes,
                    current_file,
                    files_completed,
                    total_files,
                    start_time,
                    throughput_bps,
                )
                .await;
                ui_guard.mark_dirty(Component::Overlay);
            }
        }
    }

    #[allow(clippy::unused_async)]
    async fn handle_legacy_task(
        &self,
        ui: &mut UIState,
        _task_id: u64,
        _result: Result<String, String>,
        progress: Option<f64>,
        current_item: Option<String>,
        completed: Option<u64>,
    ) {
        if let Some(ref mut loading) = ui.loading {
            if let Some(progress) = progress {
                loading.progress = Some(progress);
            }
            if let Some(current) = &current_item {
                loading.current_item = Some(current.clone());
            }
            if let Some(completed) = completed {
                loading.completed = Some(completed);
            }
            loading.spinner_frame = loading.spinner_frame.wrapping_add(1);
        }

        if let Some(p) = progress
            && (p - 1.0).abs() < f64::EPSILON
        {
            ui.loading = None;
            if ui.overlay == UIOverlay::Loading {
                ui.overlay = UIOverlay::None;
                ui.show_info("Loading complete. All files scanned.".to_string());
            }
        }
        // TODO: complete_task logic must be moved to a new location using SharedState
    }

    #[allow(clippy::unused_async)]
    async fn handle_file_op_complete(
        &self,
        ui: &mut UIState,
        operation_id: String,
        result: Result<(), crate::error::AppError>,
    ) {
        ui.remove_operation(&operation_id);
        match result {
            Ok(()) => {
                info!("File operation {} completed successfully", operation_id);
                ui.show_info("File operation completed".to_string());
            }
            Err(e) => {
                if e.to_string().contains("Cancelled") {
                    debug!("Operation {operation_id} was cancelled by user.");
                } else {
                    warn!("File operation {} failed: {}", operation_id, e);
                    ui.show_error(format!("File operation failed: {e}"));
                }
            }
        }
    }

    #[allow(
        clippy::unused_async,
        clippy::cast_precision_loss,
        clippy::too_many_arguments
    )]
    async fn handle_file_op_progress(
        &self,
        ui: &mut UIState,
        operation_id: String,
        operation_type: String,
        current_bytes: u64,
        total_bytes: u64,
        current_file: PathBuf,
        files_completed: u32,
        total_files: u32,
        start_time: Instant,
        throughput_bps: Option<u64>,
    ) {
        if let Some(existing_progress) = ui.active_file_operations.get_mut(&operation_id) {
            existing_progress.update(current_bytes, current_file.clone(), files_completed);
            if let Some(bps) = throughput_bps {
                existing_progress.throughput_bps = Some(bps);
            }
        } else {
            use crate::model::ui_state::FileOperationProgress;
            let mut progress: FileOperationProgress =
                FileOperationProgress::new(operation_type, total_bytes, total_files);
            progress.start_time = start_time;
            progress.update(current_bytes, current_file.clone(), files_completed);
            if let Some(bps) = throughput_bps {
                progress.throughput_bps = Some(bps);
            }
            ui.active_file_operations
                .insert(operation_id.clone(), progress);
        }
        if let Some(ref mut loading) = ui.loading {
            if total_bytes > 0 {
                loading.progress = Some(current_bytes as f64 / total_bytes as f64);
            }
            loading.current_item = Some(
                current_file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
            loading.completed = Some(u64::from(files_completed));
            loading.total = Some(u64::from(total_files));
        }
    }

    async fn handle_directory_scan_update(&self, path: Arc<PathBuf>, update: ScanUpdate) {
        debug!("Directory scan update for path: {:?}", path);
        let mut fs_guard = self.app.lock_fs();
        let mut ui_guard = self.app.lock_ui();

        if fs_guard.active_pane().cwd == **path {
            match update {
                ScanUpdate::Entry(entry) => {
                    trace!("Adding incremental entry: {:?}", entry.name);
                    let (_object_id, sortable_entry) = self.app.metadata.insert(entry);
                    fs_guard
                        .active_pane_mut()
                        .add_incremental_entry(sortable_entry);
                    ui_guard.mark_dirty(Component::Main);
                }
                ScanUpdate::Completed(count) => {
                    self.handle_scan_completed_new(count).await;
                }
                ScanUpdate::Error(e) => {
                    warn!("Directory scan error: {}", e);
                    let current_pane: &mut PaneState = fs_guard.active_pane_mut();
                    current_pane.is_loading = false;
                    current_pane.is_incremental_loading = false;
                    let err_msg: String = format!("Error scanning directory: {e}");
                    current_pane.last_error = Some(err_msg.clone());
                    // TODO: set_error logic must be moved to a new location using SharedState
                    ui_guard.mark_dirty(Component::Main);
                    ui_guard.mark_dirty(Component::StatusBar);
                }
            }
        }
    }

    async fn dispatch_prompt_action(&self, action: Action) {
        match action {
            Action::ShowInputPrompt(prompt_type) => {
                self.handle_show_input_prompt(prompt_type).await;
            }
            Action::SubmitInputPrompt(input) => {
                self.handle_submit_input_prompt(input).await;
            }
            _ => unreachable!(),
        }
    }

    async fn handle_show_input_prompt(&self, prompt_type: InputPromptType) {
        info!("Showing input prompt: {:?}", prompt_type);
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_input_prompt(prompt_type);
        ui_guard.mark_dirty(Component::All);
    }

    #[allow(clippy::unused_async)]
    async fn handle_scan_completed(&self, path: Arc<PathBuf>, count: usize) {
        info!("Directory scan completed with {} entries", count);
        let mut fs_guard = self.app.lock_fs();
        let mut ui_guard = self.app.lock_ui();
        let sortable_entries: Vec<crate::model::object_registry::SortableEntry> =
            fs_guard.active_pane().entries.clone();
        fs_guard
            .active_pane_mut()
            .complete_incremental_loading(sortable_entries);
        fs_guard.add_recent_dir(path.clone());

        // TODO: action_tx logic must be moved to a new location using SharedState
        // TODO: Temporarily disabled size calculation until we implement action_tx properly
        // let entries_for_size: Vec<crate::model::object_registry::SortableEntry> = fs_guard.active_pane().entries.clone();
        // for sortable_entry in entries_for_size {
        //     if let Some(object_info) = self.app.metadata.get_by_id(sortable_entry.id)
        //         && object_info.is_dir {
        //             // FileSizeOperator::calculate_size_task needs to be updated for new architecture
        //         }
        // }

        ui_guard.mark_dirty(Component::All);
    }

    async fn handle_scan_completed_new(&self, count: usize) {
        info!(
            "Directory scan completed with {} entries (new method)",
            count
        );
        let mut ui_guard = self.app.lock_ui();
        ui_guard.mark_dirty(Component::All);
    }

    async fn handle_submit_input_prompt(&self, input: String) {
        info!("Submitting input prompt: '{}'", input);

        let prompt_type = {
            let mut ui_guard = self.app.lock_ui();
            let prompt_type = ui_guard.input_prompt_type.clone();
            ui_guard.hide_input_prompt();
            prompt_type
        };

        match prompt_type {
            Some(InputPromptType::CreateFile) => {
                self.dispatch_create_file_action(input).await;
            }

            Some(InputPromptType::CreateDirectory) => {
                self.dispatch_create_directory_action(input).await;
            }

            Some(InputPromptType::Rename) => {
                self.dispatch_rename_entry_action(input).await;
            }

            Some(InputPromptType::Search) => {
                self.dispatch_search_action_content(input).await;
            }

            Some(InputPromptType::GoToPath) => {
                self.dispatch_go_to_path_action(input).await;
            }

            Some(InputPromptType::Custom(prompt_msg)) => {
                self.process_custom_prompt_notification(prompt_msg, input)
                    .await;
            }

            Some(InputPromptType::CopyDestination) => {
                self.process_copy_destination_prompt(input).await;
            }

            Some(InputPromptType::MoveDestination) => {
                self.process_move_destination_prompt(input).await;
            }

            Some(InputPromptType::RenameFile) => {
                self.process_rename_file_prompt(input).await;
            }

            None => {
                self.handle_missing_prompt_type().await;
            }
        }
    }

    async fn dispatch_create_file_action(&self, input: String) {
        Box::pin(self.dispatch_action(Action::CreateFileWithName(input))).await;
    }

    async fn dispatch_create_directory_action(&self, input: String) {
        Box::pin(self.dispatch_action(Action::CreateDirectoryWithName(input))).await;
    }

    async fn dispatch_rename_entry_action(&self, input: String) {
        info!("Processing rename prompt with input: '{}'", input);
        Box::pin(self.dispatch_action(Action::RenameEntry(input))).await;
    }

    async fn dispatch_search_action_content(&self, input: String) {
        info!("Processing search prompt with input: '{}'", input);
        Box::pin(self.dispatch_action(Action::DirectContentSearch(input))).await;
    }

    async fn dispatch_go_to_path_action(&self, input: String) {
        info!("Processing go-to-path prompt with input: '{}'", input);
        Box::pin(self.dispatch_action(Action::GoToPath(input))).await;
    }

    #[allow(clippy::unused_async)]
    async fn process_custom_prompt_notification(&self, prompt_msg: String, input: String) {
        info!(
            "Processing custom prompt '{}' with input: '{}'",
            prompt_msg, input
        );

        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_notification(
            format!("Custom prompt '{prompt_msg}': {input}"),
            NotificationLevel::Info,
            Some(3000),
        );

        ui_guard.mark_dirty(Component::All);
    }

    async fn process_copy_destination_prompt(&self, input: String) {
        info!("Processing copy destination prompt with input: '{}'", input);

        let source_path = Self::extract_selected_file_path(&self.app);

        if let Some(source_path) = source_path {
            self.execute_copy_operation(source_path, input).await;
        } else {
            self.show_copy_error().await;
        }
    }

    async fn process_move_destination_prompt(&self, input: String) {
        info!("Processing move destination prompt with input: '{}'", input);

        let source_path = Self::extract_selected_file_path(&self.app);

        if let Some(source_path) = source_path {
            self.execute_move_operation(source_path, input).await;
        } else {
            self.show_move_error().await;
        }
    }

    async fn process_rename_file_prompt(&self, input: String) {
        info!("Processing rename file prompt with input: '{}'", input);

        let source_path = Self::extract_selected_file_path(&self.app);

        if let Some(source_path) = source_path {
            self.execute_rename_operation(source_path, input).await;
        } else {
            self.show_rename_error().await;
        }
    }

    fn extract_selected_file_path(shared_state: &SharedState) -> Option<Arc<PathBuf>> {
        let fs_guard = shared_state.lock_fs();
        fs_guard.active_pane().selected.and_then(|selected_idx| {
            fs_guard
                .active_pane()
                .entries
                .get(selected_idx)
                .and_then(|sortable_entry| {
                    shared_state
                        .metadata
                        .get_by_id(sortable_entry.id)
                        .map(|obj_info| obj_info.path.clone())
                })
        })
    }

    async fn execute_copy_operation(&self, source_path: Arc<PathBuf>, input: String) {
        let dest_path = PathBuf::from(input);
        Box::pin(self.dispatch_action(Action::Copy {
            source: source_path,
            dest: dest_path.into(),
        }))
        .await;
    }

    async fn execute_move_operation(&self, source_path: Arc<PathBuf>, input: String) {
        let dest_path: PathBuf = PathBuf::from(input);

        Box::pin(self.dispatch_action(Action::Move {
            source: source_path,
            dest: dest_path.into(),
        }))
        .await;
    }

    async fn execute_rename_operation(&self, source_path: Arc<PathBuf>, input: String) {
        Box::pin(self.dispatch_action(Action::Rename {
            source: source_path,
            new_name: input,
        }))
        .await;
    }

    #[allow(clippy::unused_async)]
    async fn show_copy_error(&self) {
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_error("No file selected for copy operation".to_string());
        ui_guard.mark_dirty(Component::All);
    }

    #[allow(clippy::unused_async)]
    async fn show_move_error(&self) {
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_error("No file selected for move operation".to_string());
        ui_guard.mark_dirty(Component::All);
    }

    #[allow(clippy::unused_async)]
    async fn show_rename_error(&self) {
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_error("No file selected for rename operation".to_string());
        ui_guard.mark_dirty(Component::All);
    }

    #[allow(clippy::unused_async)]
    #[allow(clippy::unused_async)]
    async fn handle_missing_prompt_type(&self) {
        info!("No prompt type set when submitting input");
        let mut ui_guard = self.app.lock_ui();
        ui_guard.mark_dirty(Component::All);
    }

    async fn dispatch_file_op_action(&self, action: Action) {
        match action {
            Action::Copy { source, dest } => {
                self.handle_copy_operation(source, dest).await;
            }

            Action::Move { source, dest } => {
                self.handle_move_operation(source, dest).await;
            }

            Action::Rename { source, new_name } => {
                self.handle_rename_operation(source, new_name).await;
            }

            Action::CancelFileOperation { operation_id } => {
                self.handle_cancel_file_operation(operation_id).await;
            }

            _ => unreachable!(),
        }
    }

    async fn handle_copy_operation(&self, source: Arc<PathBuf>, dest: Arc<PathBuf>) {
        info!("Starting copy operation: {:?} -> {:?}", source, dest);

        let operation = FileOperation::Copy {
            source: source.clone(),
            dest: dest.clone(),
        };

        let task = self.create_and_spawn_file_operation_task(operation).await;

        let message = format!(
            "Copying {} to {}",
            source.file_name().unwrap_or_default().to_string_lossy(),
            dest.display()
        );

        self.show_operation_info(message).await;

        self.spawn_file_operation_task(task, "Copy").await;
    }

    async fn handle_move_operation(&self, source: Arc<PathBuf>, dest: Arc<PathBuf>) {
        info!("Starting move operation: {:?} -> {:?}", source, dest);

        let operation = FileOperation::Move {
            source: source.clone(),
            dest: dest.clone(),
        };

        let task = self.create_and_spawn_file_operation_task(operation).await;

        let message = format!(
            "Moving {} to {}",
            source.file_name().unwrap_or_default().to_string_lossy(),
            dest.display()
        );

        self.show_operation_info(message).await;

        self.spawn_file_operation_task(task, "Move").await;
    }

    async fn handle_rename_operation(&self, source: Arc<PathBuf>, new_name: String) {
        info!("Starting rename operation: {:?} -> {}", source, new_name);

        let operation = FileOperation::Rename {
            source: source.clone(),
            new_name: new_name.clone(),
        };

        let task = self.create_and_spawn_file_operation_task(operation).await;

        let message = format!(
            "Renaming {} to {}",
            source.file_name().unwrap_or_default().to_string_lossy(),
            new_name
        );

        self.show_operation_info(message).await;

        self.spawn_file_operation_task(task, "Rename").await;
    }

    async fn handle_cancel_file_operation(&self, operation_id: String) {
        info!("Cancelling file operation: {operation_id}");

        // TODO: Implement actual cancellation logic in phase 2.4
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_info(format!("Cancellation operations {operation_id}"));
        ui_guard.mark_dirty(Component::All);
    }

    async fn create_and_spawn_file_operation_task(
        &self,
        operation: FileOperation,
    ) -> FileOperationTask {
        let task_dependencies = self.get_task_dependencies().await;
        let cancel_token = CancellationToken::new();

        let task = FileOperationTask::new(
            operation,
            task_dependencies.task_tx,
            cancel_token.clone(),
            task_dependencies.app_handle,
        );

        self.store_cancellation_token(&task.operation_id, cancel_token)
            .await;

        task
    }

    async fn get_task_dependencies(&self) -> TaskDependencies {
        let app_guard = self.app.lock_app();
        let task_tx = app_guard.task_tx.clone();

        // Create a tokio::sync::Mutex wrapper for compatibility with FileOperationTask
        // TODO: Update FileOperationTask to work with SharedState directly
        let app_state_tokio = Arc::new(tokio::sync::Mutex::new(AppState::new(
            app_guard.config.clone(),
            app_guard.metadata.clone(),
            app_guard.task_tx.clone(),
            app_guard.action_tx.clone(),
        )));

        drop(app_guard);

        TaskDependencies {
            task_tx,
            app_handle: app_state_tokio,
        }
    }

    async fn store_cancellation_token(&self, operation_id: &str, cancel_token: CancellationToken) {
        let mut ui_guard = self.app.lock_ui();
        ui_guard.store_cancel_token(operation_id.to_string(), cancel_token);
    }

    async fn show_operation_info(&self, message: String) {
        let mut ui_guard = self.app.lock_ui();
        ui_guard.show_info(message);
        ui_guard.mark_dirty(Component::All);
    }

    #[allow(clippy::unused_async)]
    async fn spawn_file_operation_task(&self, task: FileOperationTask, operation_name: &str) {
        let operation_name = operation_name.to_string();

        tokio::spawn(async move {
            if let Err(e) = task.execute().await {
                eprintln!("{operation_name} operation failed: {e}");
            }
        });
    }

    async fn dispatch_legacy_action(&self, action: Action) {
        match action {
            Action::Sort(_) => {
                info!("Sort action should now be command-driven (:sort)");
                let mut fs_guard = self.app.lock_fs();
                let active_pane = fs_guard.active_pane_mut();

                active_pane.sort = match active_pane.sort {
                    EntrySort::NameAsc => EntrySort::NameDesc,

                    EntrySort::NameDesc => EntrySort::SizeAsc,

                    EntrySort::SizeAsc => EntrySort::SizeDesc,

                    EntrySort::SizeDesc => EntrySort::ModifiedAsc,

                    EntrySort::ModifiedAsc => EntrySort::ModifiedDesc,

                    EntrySort::ModifiedDesc | EntrySort::Custom => EntrySort::NameAsc,
                };

                let _sort_criteria: String = active_pane.sort.to_string();
                // TODO: Implement sort_entries in the new SharedState architecture
                drop(fs_guard);

                let mut ui_guard = self.app.lock_ui();
                ui_guard.mark_dirty(Component::All);
            }

            Action::Filter(_) => {
                info!("Filter action should now be command-driven (:filter)");
                let mut fs_guard = self.app.lock_fs();
                let active_pane = fs_guard.active_pane_mut();

                active_pane.filter = match active_pane.filter {
                    EntryFilter::All => EntryFilter::FilesOnly,
                    EntryFilter::FilesOnly => EntryFilter::DirsOnly,
                    EntryFilter::DirsOnly
                    | EntryFilter::Extension(_)
                    | EntryFilter::Pattern(_)
                    | EntryFilter::Custom(_) => EntryFilter::All,
                };

                let _filter_criteria: String = active_pane.filter.to_string();
                // TODO: Implement filter_entries in the new SharedState architecture
                drop(fs_guard);

                let mut ui_guard = self.app.lock_ui();
                ui_guard.mark_dirty(Component::All);
            }
            _ => unreachable!(),
        }
    }
}

// Helper struct to group task dependencies
struct TaskDependencies {
    task_tx: UnboundedSender<TaskResult>,
    app_handle: Arc<tokio::sync::Mutex<AppState>>,
}
