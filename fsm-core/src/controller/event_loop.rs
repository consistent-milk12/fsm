use crate::controller::actions::{Action, InputPromptType};
use crate::controller::eactions::{ActionType, EAction};
use crate::controller::ekey_processor::EKeyProcessor;

use crate::fs::dir_scanner::ScanUpdate;
use crate::fs::object_info::ObjectInfo;
use crate::model::app_state::AppState;
use crate::model::command_palette::CommandAction;
use crate::model::fs_state::{EntryFilter, EntrySort, PaneState};
use crate::model::ui_state::{LoadingState, NotificationLevel, RedrawFlag, UIMode, UIOverlay};
use crate::tasks::file_ops_task::{FileOperation, FileOperationTask};
use crate::tasks::search_task::RawSearchResult;
use clipr::ClipError;
use crossterm::event::{Event as TermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Mutex, MutexGuard, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

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
    pub app: Arc<Mutex<AppState>>,
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
        app: Arc<Mutex<AppState>>,
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
    fn current_result_count(app: &AppState) -> usize {
        let count = if let Some(ref raw_results) = app.ui.raw_search_results {
            raw_results.lines.len()
        } else if !app.ui.rich_search_results.is_empty() {
            app.ui.rich_search_results.len()
        } else {
            app.ui.search_results.len()
        };

        trace!("Calculated result count: {}", count);
        count
    }

    /// Performance monitoring - track event processing times
    fn update_performance_metrics(&mut self, processing_time: std::time::Duration) {
        self.event_count += 1;
        let time_ms = processing_time.as_millis() as f64;

        // Update running average
        if self.event_count == 1 {
            self.avg_response_time = time_ms;
        } else {
            self.avg_response_time = (self.avg_response_time * 0.9) + (time_ms * 0.1);
        }

        // Log performance warnings
        if time_ms > 16.0 {
            // 60fps threshold
            info!(
                "Slow event processing: {:.2}ms (avg: {:.2}ms)",
                time_ms, self.avg_response_time
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

    /// Enhanced event loop with performance monitoring
    pub async fn next_action(&mut self) -> Option<Action> {
        let start_time = Instant::now();

        let action = tokio::select! {
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
    async fn handle_terminal_event(&mut self, event: TermEvent) -> Action {
        if let TermEvent::Key(key_event) = event {
            // Always try performance processor first (unified key handling)
            let kp_exists = self.app.lock().await.key_processor.is_some();
            if kp_exists {
                match self.handle_key_event_performance(key_event).await {
                    Ok(action) => return action,
                    Err(_) => { /* Key not handled by performance processor, fall back to standard handling */
                    }
                }
            } else {
                // One-time initialization
                let mut app = self.app.lock().await;
                if app.key_processor.is_none() {
                    let clipboard = app.ui.clipboard.clone();
                    app.key_processor = Some(EKeyProcessor::new(clipboard));
                }
            }
        }

        let app = self.app.lock().await;
        let current_overlay = app.ui.overlay;
        let current_mode = app.ui.mode;
        let has_notification = app.ui.notification.is_some();
        drop(app);

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
                    let mut app = self.app.lock().await;
                    app.ui.dismiss_notification();
                    app.ui.request_redraw(RedrawFlag::All);
                    // Continue processing the key event
                }

                // Check for clipboard overlay first (has separate state)
                let clipboard_active = {
                    let app = self.app.lock().await;
                    app.ui.clipboard_overlay_active
                };

                if clipboard_active {
                    return self.handle_clipboard_overlay_keys(key_event).await;
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
            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

            if !app.ui.active_file_operations.is_empty() {
                let cancelled_count: usize = app.ui.cancel_all_operations();

                if cancelled_count > 0 {
                    app.ui
                        .show_info(format!("Cancelled {cancelled_count} file operations(s)"));

                    info!("User cancelled {cancelled_count} file operations via ESC key");

                    app.ui.request_redraw(RedrawFlag::All);

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
            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
            app.ui.dismiss_notification();
            app.ui.request_redraw(RedrawFlag::All);
            return Action::NoOp;
        }

        if overlay != UIOverlay::None {
            debug!("Escape: closing overlay {:?}", overlay);
            return Action::CloseOverlay;
        }

        if mode == UIMode::Command {
            debug!("Escape: checking command completions");
            let mut app = self.app.lock().await;
            if app.ui.command_palette.show_completions {
                debug!("Escape: hiding command completions");
                app.ui.command_palette.hide_completions();
                app.ui.request_redraw(RedrawFlag::All);
                return Action::NoOp;
            }
            debug!("Escape: exiting command mode");
            return Action::ExitCommandMode;
        }

        debug!("Escape: requesting application quit");
        Action::Quit
    }

    /// Enhanced command mode with improved auto-completion
    async fn handle_command_mode_keys(&self, key: crossterm::event::KeyEvent) -> Action {
        trace!("Command mode key: {:?}", key.code);

        match key.code {
            KeyCode::Char(c) => {
                debug!("Command mode: adding character '{}'", c);
                let mut app = self.app.lock().await;
                app.ui.command_palette.input.push(c);
                app.ui.command_palette.update_filter();
                app.ui.command_palette.show_completions_if_available();
                trace!(
                    "Command input: '{}', completions available: {}",
                    app.ui.command_palette.input, app.ui.command_palette.show_completions
                );
                Action::NoOp
            }

            KeyCode::Backspace => {
                debug!("Command mode: backspace");
                let mut app = self.app.lock().await;
                app.ui.command_palette.input.pop();
                app.ui.command_palette.update_filter();
                app.ui.command_palette.show_completions_if_available();
                trace!(
                    "Command input: '{}' (after backspace)",
                    app.ui.command_palette.input
                );
                Action::NoOp
            }

            KeyCode::Up => {
                debug!("Command mode: up arrow navigation");
                let mut app = self.app.lock().await;
                if app.ui.command_palette.show_completions {
                    app.ui.command_palette.prev_completion();
                    trace!("Command completions: navigated up");
                } else {
                    app.ui.command_palette.selected =
                        app.ui.command_palette.selected.saturating_sub(1);
                    trace!(
                        "Command history: navigated up to {}",
                        app.ui.command_palette.selected
                    );
                }
                Action::NoOp
            }

            KeyCode::Down => {
                debug!("Command mode: down arrow navigation");
                let mut app = self.app.lock().await;
                if app.ui.command_palette.show_completions {
                    app.ui.command_palette.next_completion();
                    trace!("Command completions: navigated down");
                } else {
                    let max_idx = app.ui.command_palette.filtered.len().saturating_sub(1);
                    app.ui.command_palette.selected = app
                        .ui
                        .command_palette
                        .selected
                        .saturating_add(1)
                        .min(max_idx);
                    trace!(
                        "Command history: navigated down to {}",
                        app.ui.command_palette.selected
                    );
                }
                Action::NoOp
            }

            KeyCode::Tab => {
                debug!("Command mode: tab completion");
                let mut app = self.app.lock().await;
                if app.ui.command_palette.show_completions {
                    let before = app.ui.command_palette.input.clone();
                    app.ui.command_palette.apply_completion();
                    let after = app.ui.command_palette.input.clone();
                    info!("Applied completion: '{}' -> '{}'", before, after);
                } else {
                    trace!("Tab pressed but no completions available");
                }
                Action::NoOp
            }

            KeyCode::Enter => {
                debug!("Command mode: executing command");
                let app = self.app.lock().await;
                let input = app.ui.command_palette.input.trim();
                info!("Executing command: '{}'", input);

                // Try parsing user input first
                if let Some(parsed_action) = app.ui.command_palette.parse_command() {
                    debug!("Command parsed successfully: {:?}", parsed_action);
                    self.map_command_action_to_action(parsed_action)
                } else if let Some(cmd) = app
                    .ui
                    .command_palette
                    .filtered
                    .get(app.ui.command_palette.selected)
                {
                    debug!("Using selected command from list: {:?}", cmd.action);
                    self.map_command_action_to_action(cmd.action.clone())
                } else {
                    info!("No valid command to execute, exiting command mode");
                    Action::ExitCommandMode
                }
            }

            _ => {
                trace!("Command mode: ignoring key {:?}", key.code);
                Action::NoOp
            }
        }
    }

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

            // Clipboard operations
            (KeyCode::Tab, _) => {
                info!("Toggling clipboard overlay");
                Action::ToggleClipboardOverlay
            }

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
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.input.push(c);

                let pattern: String = app.ui.input.clone();
                trace!("Filename search pattern: '{}'", pattern);
                Action::FileNameSearch(pattern)
            }

            KeyCode::Backspace => {
                debug!("Filename search: backspace");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.input.pop();

                let pattern = app.ui.input.clone();
                trace!("Filename search pattern: '{}' (after backspace)", pattern);
                Action::FileNameSearch(pattern)
            }

            KeyCode::Enter => {
                debug!("Filename search: enter pressed");
                let app: MutexGuard<'_, AppState> = self.app.lock().await;

                // Try to open selected result
                if !app.ui.filename_search_results.is_empty()
                    && let Some(selected_idx) = app.ui.selected
                    && let Some(selected_entry) = app.ui.filename_search_results.get(selected_idx)
                {
                    info!("Opening selected file: {:?}", selected_entry.path);
                    return Action::OpenFile(selected_entry.path.clone(), None);
                }

                // Fallback to triggering search
                if !app.ui.input.trim().is_empty() {
                    debug!("Triggering filename search for: '{}'", app.ui.input);
                    Action::FileNameSearch(app.ui.input.clone())
                } else {
                    debug!("Closing filename search (empty input)");
                    Action::CloseOverlay
                }
            }

            KeyCode::Up => {
                debug!("Filename search: navigate up");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                let result_count: usize = app.ui.filename_search_results.len();

                if result_count > 0 {
                    app.ui.selected = Some(app.ui.selected.unwrap_or(0).saturating_sub(1));
                    trace!("Filename search selection: {:?}", app.ui.selected);
                }

                Action::NoOp
            }

            KeyCode::Down => {
                debug!("Filename search: navigate down");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let result_count: usize = app.ui.filename_search_results.len();

                if result_count > 0 {
                    let current: usize = app.ui.selected.unwrap_or(0);
                    app.ui.selected = Some((current + 1).min(result_count.saturating_sub(1)));
                    trace!("Filename search selection: {:?}", app.ui.selected);
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
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.input.push(c);

                // Clear previous results for real-time search
                self.clear_search_results(&mut app);
                app.ui.request_redraw(RedrawFlag::All);
                trace!("Content search input: '{}' (results cleared)", app.ui.input);
                Action::NoOp
            }

            KeyCode::Backspace => {
                debug!("Content search: backspace");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.input.pop();
                self.clear_search_results(&mut app);
                app.ui.request_redraw(RedrawFlag::All);
                trace!("Content search input: '{}' (after backspace)", app.ui.input);
                Action::NoOp
            }

            KeyCode::Enter => {
                debug!("Content search: enter pressed");
                let app: MutexGuard<'_, AppState> = self.app.lock().await;

                // Try to open selected result first
                if let Some(selected_idx) = app.ui.selected {
                    debug!("Processing selection at index {}", selected_idx);

                    // Priority: Raw -> Rich -> Simple results
                    if let Some(ref raw_results) = app.ui.raw_search_results {
                        debug!("Processing raw search results");
                        if selected_idx < raw_results.lines.len() {
                            return self
                                .process_raw_search_line(raw_results, selected_idx)
                                .await;
                        }
                    }

                    if !app.ui.rich_search_results.is_empty()
                        && selected_idx < app.ui.rich_search_results.len()
                    {
                        debug!("Processing rich search results");
                        return self
                            .process_rich_search_line(
                                &app.ui.rich_search_results,
                                selected_idx,
                                &app.fs.active_pane().cwd,
                            )
                            .await;
                    }

                    if !app.ui.search_results.is_empty()
                        && selected_idx < app.ui.search_results.len()
                    {
                        debug!("Processing simple search results");
                        let result = &app.ui.search_results[selected_idx];
                        info!("Opening file: {:?}", result.path);
                        return Action::OpenFile(result.path.clone(), None);
                    }
                }

                // No valid selection, start new search
                let pattern = app.ui.input.clone();
                info!("Starting content search for: '{}'", pattern);
                Action::ContentSearch(pattern)
            }

            KeyCode::Up => {
                debug!("Content search: navigate up");
                let mut app = self.app.lock().await;
                let result_count = Self::current_result_count(&app);
                if result_count > 0 {
                    let new_idx = app.ui.selected.unwrap_or(0).saturating_sub(1);
                    app.ui.selected = Some(new_idx);
                    app.ui.request_redraw(RedrawFlag::All);
                    trace!("Content search selection: {}", new_idx);
                }
                Action::NoOp
            }

            KeyCode::Down => {
                debug!("Content search: navigate down");
                let mut app = self.app.lock().await;
                let result_count = Self::current_result_count(&app);
                if result_count > 0 {
                    let current = app.ui.selected.unwrap_or(0);
                    let new_idx = (current + 1).min(result_count.saturating_sub(1));
                    app.ui.selected = Some(new_idx);
                    app.ui.request_redraw(RedrawFlag::All);
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

    /// Helper to clear search results
    fn clear_search_results(&self, app: &mut AppState) {
        app.ui.search_results.clear();
        app.ui.rich_search_results.clear();
        app.ui.raw_search_results = None;
        app.ui.last_query = None;
        app.ui.selected = None;
    }

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
                return Action::OpenFile(path, line_num);
            }
        }

        info!("Failed to parse raw search line at index {}", selected_idx);
        Action::NoOp
    }

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
                    return Action::OpenFile(path, line_num);
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
                let mut app = self.app.lock().await;
                app.ui.input.push(c);
                Action::NoOp
            }

            KeyCode::Backspace => {
                debug!("Prompt: backspace");
                let mut app = self.app.lock().await;
                app.ui.input.pop();
                Action::NoOp
            }

            KeyCode::Enter => {
                debug!("Prompt: enter pressed");
                let app = self.app.lock().await;
                let input = app.ui.input.trim().to_string();

                if !input.is_empty() {
                    info!("Submitting prompt input: '{}'", input);
                    Action::SubmitInputPrompt(input)
                } else {
                    debug!("Closing prompt (empty input)");
                    Action::CloseOverlay
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
                let app: MutexGuard<'_, AppState> = self.app.lock().await;

                if let Some(selected_idx) = app.ui.selected
                    && let Some(result) = app.ui.search_results.get(selected_idx)
                {
                    info!("Opening search result: {:?}", result.path);

                    return Action::OpenFile(result.path.clone(), None);
                }

                Action::NoOp
            }

            KeyCode::Up => {
                debug!("Search results: navigate up");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                if !app.ui.search_results.is_empty() {
                    let current = app.ui.selected.unwrap_or(0);
                    app.ui.selected = Some(current.saturating_sub(1));
                    app.ui.request_redraw(RedrawFlag::All);
                }
                Action::NoOp
            }

            KeyCode::Down => {
                debug!("Search results: navigate down");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let result_count: usize = app.ui.search_results.len();

                if result_count > 0 {
                    let current = app.ui.selected.unwrap_or(0);
                    app.ui.selected = Some((current + 1).min(result_count.saturating_sub(1)));
                    app.ui.request_redraw(RedrawFlag::All);
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
    fn map_command_action_to_action(&self, cmd_action: CommandAction) -> Action {
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
        let start_time = Instant::now();
        info!("Dispatching action: {:?}", action);

        match action {
            Action::Quit => {
                info!("Quit action - handled in main loop");
            }

            Action::ToggleHelp => {
                debug!("Toggling help overlay");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_help_overlay();
                app.ui.request_redraw(RedrawFlag::All);

                info!("Help overlay toggled to: {:?}", app.ui.overlay);
            }

            Action::EnterCommandMode => {
                debug!("Entering command mode");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.enter_command_mode();
                app.ui.request_redraw(RedrawFlag::All);

                info!("Command mode activated");
            }

            Action::ExitCommandMode => {
                debug!("Exiting command mode");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.exit_command_mode();
                app.ui.request_redraw(RedrawFlag::All);
                info!("Command mode deactivated");
            }

            Action::ToggleFileNameSearch => {
                debug!("Toggling filename search overlay");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_filename_search_overlay();
                app.ui.request_redraw(RedrawFlag::All);

                info!("Filename search overlay toggled to: {:?}", app.ui.overlay);
            }

            Action::ToggleContentSearch => {
                debug!("Toggling content search overlay");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();

                    info!("Exited command mode.")
                }

                // Then toggle content search overlay
                app.ui.toggle_content_search_overlay();

                if app.ui.overlay == UIOverlay::ContentSearch {
                    info!("Content search overlay opened.");
                } else {
                    info!("Content search overlay closed.");
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ToggleClipboardOverlay => {
                debug!("Toggling clipboard overlay");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_clipboard_overlay();
                app.ui.request_redraw(RedrawFlag::All);

                info!(
                    "Clipboard overlay toggled to: {}",
                    app.ui.clipboard_overlay_active
                );
            }

            Action::FileNameSearch(pattern) => {
                info!("Starting filename search for pattern: '{}'", pattern);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.filename_search(pattern);
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ContentSearch(pattern) => {
                info!("Starting content search for pattern: '{}'", pattern);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.start_content_search(pattern);
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::DirectContentSearch(pattern) => {
                info!("Starting direct content search for pattern: '{}'", pattern);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.overlay = UIOverlay::ContentSearch;
                app.ui.input.clear();
                app.start_content_search(pattern);
                app.ui.exit_command_mode();
                app.ui.request_redraw(RedrawFlag::All);
            }

            // Navigation actions - optimized for performance
            Action::MoveSelectionUp => {
                debug!("Moving selection up");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().move_selection_up();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::MoveSelectionDown => {
                debug!("Moving selection down");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().move_selection_down();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::PageUp => {
                debug!("Page up");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().page_up();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::PageDown => {
                debug!("Page down");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().page_down();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::SelectFirst => {
                debug!("Selecting first entry");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().select_first();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::SelectLast => {
                debug!("Selecting last entry");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().select_last();
                app.ui.selected = app.fs.active_pane().selected;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::EnterSelected => {
                debug!("Entering selected item");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.enter_selected_directory().await;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::GoToParent => {
                info!("Going to parent directory");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.go_to_parent_directory().await;
                app.ui.request_redraw(RedrawFlag::All);
            }

            // File operations with enhanced error handling
            Action::CreateFile => {
                info!("Creating new file (command-driven)");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_file().await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::CreateDirectory => {
                info!("Creating new directory (command-driven)");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_directory().await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::CreateFileWithName(name) => {
                info!("Creating new file '{}' (command-driven)", name);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_file_with_name(name).await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::CreateDirectoryWithName(name) => {
                info!("Creating new directory '{}' (command-driven)", name);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_directory_with_name(name).await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ReloadDirectory => {
                info!("Reloading directory (command-driven)");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.reload_directory().await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::Delete => {
                info!("Delete action triggered - this should now be command-driven");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.delete_entry().await;
                app.ui.request_redraw(RedrawFlag::All);
            }

            // NEW IMPLEMENTATIONS - Complete TODO sections
            Action::RenameEntry(new_name) => {
                info!("Renaming selected entry to '{}'", new_name);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.rename_selected_entry(new_name).await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::GoToPath(path_str) => {
                info!("Navigating to path: '{}'", path_str);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.navigate_to_path(path_str).await;

                if app.ui.is_in_command_mode() {
                    app.ui.exit_command_mode();
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            // Search result handling with enhanced performance
            Action::ShowSearchResults(results) => {
                info!("Showing {} search results", results.len());
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.search_results = results;

                if app.ui.overlay != UIOverlay::ContentSearch {
                    app.ui.set_overlay(UIOverlay::SearchResults);
                } else if !app.ui.search_results.is_empty() {
                    app.ui.selected = Some(0);
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ShowFilenameSearchResults(results) => {
                info!("Showing {} filename search results", results.len());
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.filename_search_results = results;
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ShowRichSearchResults(results) => {
                info!("Showing {} rich search results", results.len());
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.rich_search_results = results;

                if app.ui.overlay == UIOverlay::ContentSearch
                    && !app.ui.rich_search_results.is_empty()
                {
                    app.ui.selected = Some(0);
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ShowRawSearchResults(results) => {
                info!("Showing {} raw search results", results.lines.len());
                let mut app = self.app.lock().await;
                app.ui.raw_search_results = Some(results);
                app.ui.raw_search_selected = 0;

                if app.ui.overlay == UIOverlay::ContentSearch {
                    app.ui.selected = Some(0);
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::OpenFile(path, line_number) => {
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
                        let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                        app.ui.close_all_overlays();
                        app.ui.request_redraw(RedrawFlag::All);
                    }
                    Err(e) => {
                        warn!("Failed to open file with VS Code: {}", e);
                        let mut app = self.app.lock().await;
                        app.ui.show_error(format!("Failed to open file: {e}"));
                        app.ui.request_redraw(RedrawFlag::All);
                    }
                }
            }

            Action::CloseOverlay => {
                debug!("Closing overlay");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let previous_overlay = app.ui.overlay;
                app.ui.close_all_overlays();
                app.ui.request_redraw(RedrawFlag::All);
                info!("Closed overlay: {:?}", previous_overlay);
            }

            // Enhanced task result processing
            Action::TaskResult(task_result) => {
                debug!("Processing task result: {:?}", task_result);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                match task_result {
                    TaskResult::Legacy {
                        task_id,
                        result,
                        progress,
                        current_item,
                        completed,
                        ..
                    } => {
                        // Update loading state with enhanced progress tracking
                        if let Some(ref mut loading) = app.ui.loading {
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

                        // Complete task on 100% progress
                        if let Some(p) = progress
                            && (p - 1.0).abs() < f64::EPSILON
                        {
                            app.ui.loading = None;
                            if app.ui.overlay == UIOverlay::Loading {
                                app.ui.overlay = UIOverlay::None;
                                app.ui
                                    .show_info("Loading complete. All files scanned.".to_string());
                            }
                        }

                        app.complete_task(
                            task_id,
                            Some(match &result {
                                Ok(s) => s.clone(),
                                Err(e) => format!("Error: {e}"),
                            }),
                        );
                    }

                    TaskResult::FileOperationComplete {
                        operation_id,
                        result,
                    } => {
                        {
                            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                            // Remove from tracking regardless of sucess/failure
                            app.ui.remove_operation(&operation_id);
                        }

                        match result {
                            Ok(()) => {
                                info!("File operation {} completed successfully", operation_id);
                                app.ui.show_info("File operation completed".to_string());
                            }

                            Err(e) => {
                                if e.to_string().contains("Cancelled") {
                                    // Don't show error for user-initiated cancellation.
                                    debug!("Operation {operation_id} was cancelled by user.")
                                } else {
                                    warn!("File operation {} failed: {}", operation_id, e);
                                    app.ui.show_error(format!("File operation failed: {e}"));
                                }
                            }
                        }
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
                        debug!(
                            "File operation progress: {operation_id} - {current_bytes}/{total_bytes} bytes ({files_completed}/{total_files} files)"
                        );

                        // Update active file operations progress tracking
                        if let Some(existing_progress) =
                            app.ui.active_file_operations.get_mut(&operation_id)
                        {
                            // Update active file operations progress tracking
                            existing_progress.update(
                                current_bytes,
                                current_file.clone(),
                                files_completed,
                            );

                            // Update throughput if provided
                            if let Some(bps) = throughput_bps {
                                existing_progress.throughput_bps = Some(bps);
                            }
                        } else {
                            // Create new progress entry
                            use crate::model::ui_state::FileOperationProgress;

                            let mut progress: FileOperationProgress = FileOperationProgress::new(
                                operation_type,
                                total_bytes,
                                total_files,
                            );

                            progress.start_time = start_time;
                            progress.update(current_bytes, current_file.clone(), files_completed);

                            if let Some(bps) = throughput_bps {
                                progress.throughput_bps = Some(bps);
                            }

                            app.ui
                                .active_file_operations
                                .insert(operation_id.clone(), progress);
                        }

                        if let Some(ref mut loading) = app.ui.loading {
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

                            loading.completed = Some(files_completed as u64);
                            loading.total = Some(total_files as u64);
                        }
                    }
                }

                app.ui.request_redraw(RedrawFlag::All);
            }

            // Enhanced directory scan processing
            Action::DirectoryScanUpdate { path, update } => {
                debug!("Directory scan update for path: {:?}", path);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                if app.fs.active_pane().cwd == path {
                    match update {
                        ScanUpdate::Entry(entry) => {
                            trace!("Adding incremental entry: {:?}", entry.name);
                            app.fs.active_pane_mut().add_incremental_entry(entry);
                            app.ui.request_redraw(RedrawFlag::All);
                        }

                        ScanUpdate::Completed(count) => {
                            info!("Directory scan completed with {} entries", count);
                            let entries: Vec<ObjectInfo> = app.fs.active_pane().entries.clone();

                            app.fs
                                .active_pane_mut()
                                .complete_incremental_loading(entries);
                            app.fs.add_recent_dir(path.clone());

                            // Start background size calculation tasks
                            let action_tx: UnboundedSender<Action> = app.action_tx.clone();
                            let entries_for_size: Vec<ObjectInfo> =
                                app.fs.active_pane().entries.clone();

                            for entry in entries_for_size {
                                if entry.is_dir {
                                    crate::tasks::size_task::calculate_size_task(
                                        path.clone(),
                                        entry,
                                        action_tx.clone(),
                                    );
                                }
                            }

                            app.ui.request_redraw(RedrawFlag::All);
                        }

                        ScanUpdate::Error(e) => {
                            warn!("Directory scan error: {}", e);
                            let current_pane: &mut PaneState = app.fs.active_pane_mut();
                            current_pane.is_loading = false;
                            current_pane.is_incremental_loading = false;

                            let err_msg: String = format!("Error scanning directory: {e}");
                            current_pane.last_error = Some(err_msg.clone());
                            app.set_error(err_msg);
                            app.ui.request_redraw(RedrawFlag::All);
                        }
                    }
                }
            }

            // Development/debug actions
            Action::ToggleShowHidden => {
                debug!("Toggling hidden files visibility");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_show_hidden();
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::SimulateLoading => {
                debug!("Simulating loading state");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                app.ui.loading = Some(LoadingState {
                    message: "Simulated loading...".into(),
                    progress: None,
                    spinner_frame: 0,
                    current_item: Some("demo.txt".into()),
                    completed: Some(0),
                    total: Some(100),
                });

                app.ui.overlay = UIOverlay::Loading;
                app.ui.request_redraw(RedrawFlag::All);
            }

            // Legacy actions - maintained for compatibility
            Action::Sort(_) => {
                info!("Sort action should now be command-driven (:sort)");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let active_pane: &mut PaneState = app.fs.active_pane_mut();

                active_pane.sort = match active_pane.sort {
                    EntrySort::NameAsc => EntrySort::NameDesc,

                    EntrySort::NameDesc => EntrySort::SizeAsc,

                    EntrySort::SizeAsc => EntrySort::SizeDesc,

                    EntrySort::SizeDesc => EntrySort::ModifiedAsc,

                    EntrySort::ModifiedAsc => EntrySort::ModifiedDesc,

                    EntrySort::ModifiedDesc | EntrySort::Custom(_) => EntrySort::NameAsc,
                };

                let sort_criteria: String = active_pane.sort.to_string();
                app.sort_entries(&sort_criteria);
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::Filter(_) => {
                info!("Filter action should now be command-driven (:filter)");
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let active_pane: &mut PaneState = app.fs.active_pane_mut();

                active_pane.filter = match active_pane.filter {
                    EntryFilter::All => EntryFilter::FilesOnly,
                    EntryFilter::FilesOnly => EntryFilter::DirsOnly,
                    EntryFilter::DirsOnly
                    | EntryFilter::Extension(_)
                    | EntryFilter::Pattern(_)
                    | EntryFilter::Custom(_) => EntryFilter::All,
                };

                let filter_criteria: String = active_pane.filter.to_string();
                app.filter_entries(&filter_criteria);
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::UpdateObjectInfo { parent_dir, info } => {
                trace!("Updating object info for {:?}", info.path);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.update_object_info(parent_dir, info);
                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::ShowInputPrompt(prompt_type) => {
                info!("Showing input prompt: {:?}", prompt_type);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.show_input_prompt(prompt_type);
                app.ui.request_redraw(RedrawFlag::All);
            }

            // ENHANCED INPUT PROMPT HANDLING - All TODOs implemented
            Action::SubmitInputPrompt(input) => {
                info!("Submitting input prompt: '{}'", input);
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let prompt_type: Option<InputPromptType> = app.ui.input_prompt_type.clone();
                app.ui.hide_input_prompt();

                match prompt_type {
                    Some(InputPromptType::CreateFile) => {
                        drop(app);
                        Box::pin(self.dispatch_action(Action::CreateFileWithName(input))).await;
                    }

                    Some(InputPromptType::CreateDirectory) => {
                        drop(app);
                        Box::pin(self.dispatch_action(Action::CreateDirectoryWithName(input)))
                            .await;
                    }

                    Some(InputPromptType::Rename) => {
                        info!("Processing rename prompt with input: '{}'", input);
                        drop(app);
                        Box::pin(self.dispatch_action(Action::RenameEntry(input))).await;
                    }

                    Some(InputPromptType::Search) => {
                        info!("Processing search prompt with input: '{}'", input);
                        drop(app);
                        Box::pin(self.dispatch_action(Action::DirectContentSearch(input))).await;
                    }

                    Some(InputPromptType::GoToPath) => {
                        info!("Processing go-to-path prompt with input: '{}'", input);
                        drop(app);
                        Box::pin(self.dispatch_action(Action::GoToPath(input))).await;
                    }

                    Some(InputPromptType::Custom(prompt_msg)) => {
                        info!(
                            "Processing custom prompt '{}' with input: '{}'",
                            prompt_msg, input
                        );

                        // Show notification with custom prompt result
                        app.ui.show_notification(
                            format!("Custom prompt '{prompt_msg}': {input}"),
                            NotificationLevel::Info,
                            Some(3000), // 3 second auto-dismiss
                        );

                        app.ui.request_redraw(RedrawFlag::All);
                    }

                    // File operation input prompts
                    Some(InputPromptType::CopyDestination) => {
                        info!("Processing copy destination prompt with input: '{}'", input);

                        // Get current selected file path
                        let selected_path: Option<PathBuf> = {
                            let app: MutexGuard<'_, AppState> = self.app.lock().await;

                            app.fs
                                .active_pane()
                                .selected
                                .and_then(|selected_idx: usize| {
                                    app.fs
                                        .active_pane()
                                        .entries
                                        .get(selected_idx)
                                        .map(|entry: &ObjectInfo| entry.path.clone())
                                })
                        };

                        if let Some(source_path) = selected_path {
                            let dest_path: PathBuf = std::path::PathBuf::from(input);
                            drop(app);

                            Box::pin(self.dispatch_action(Action::Copy {
                                source: source_path,
                                dest: dest_path,
                            }))
                            .await;
                        } else {
                            app.ui
                                .show_error("No file selected for copy operation".to_string());
                            app.ui.request_redraw(RedrawFlag::All);
                        }
                    }

                    Some(InputPromptType::MoveDestination) => {
                        info!("Processing move destination prompt with input: '{}'", input);
                        // Get current selected file path
                        let selected_path: Option<PathBuf> = {
                            let app: MutexGuard<'_, AppState> = self.app.lock().await;

                            app.fs
                                .active_pane()
                                .selected
                                .and_then(|selected_idx: usize| {
                                    app.fs
                                        .active_pane()
                                        .entries
                                        .get(selected_idx)
                                        .map(|entry: &ObjectInfo| entry.path.clone())
                                })
                        };

                        if let Some(source_path) = selected_path {
                            let dest_path: PathBuf = std::path::PathBuf::from(input);
                            drop(app);
                            Box::pin(self.dispatch_action(Action::Move {
                                source: source_path,
                                dest: dest_path,
                            }))
                            .await;
                        } else {
                            app.ui
                                .show_error("No file selected for move operation".to_string());

                            app.ui.request_redraw(RedrawFlag::All);
                        }
                    }
                    Some(InputPromptType::RenameFile) => {
                        info!("Processing rename file prompt with input: '{}'", input);
                        // Get current selected file path
                        let selected_path: Option<PathBuf> = {
                            let app: MutexGuard<'_, AppState> = self.app.lock().await;

                            app.fs
                                .active_pane()
                                .selected
                                .and_then(|selected_idx: usize| {
                                    app.fs
                                        .active_pane()
                                        .entries
                                        .get(selected_idx)
                                        .map(|entry: &ObjectInfo| entry.path.clone())
                                })
                        };

                        if let Some(source_path) = selected_path {
                            drop(app);
                            Box::pin(self.dispatch_action(Action::Rename {
                                source: source_path,
                                new_name: input,
                            }))
                            .await;
                        } else {
                            app.ui
                                .show_error("No file selected for rename operation".to_string());

                            app.ui.request_redraw(RedrawFlag::All);
                        }
                    }
                    None => {
                        info!("No prompt type set when submitting input");
                        app.ui.request_redraw(RedrawFlag::All);
                    }
                }
            }

            Action::Tick => {
                // Quiet tick processing with performance monitoring
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let redraw_needed: bool = app.ui.update_notification();

                // Periodic cleanup and optimization
                if self.event_count.is_multiple_of(1000) {
                    trace!("Performing periodic cleanup (event #{}))", self.event_count);
                    // Could add memory cleanup, cache pruning, etc. here
                }

                if redraw_needed {
                    app.ui.request_redraw(RedrawFlag::All);
                }
            }

            // NEW FILE OPERATIONS - Core functionality implementation
            Action::Copy { source, dest } => {
                info!("Starting copy operation: {:?} -> {:?}", source, dest);
                let app: MutexGuard<'_, AppState> = self.app.lock().await;
                let task_tx: UnboundedSender<TaskResult> = app.task_tx.clone();
                let app_handle: Arc<Mutex<AppState>> = self.app.clone();
                drop(app);

                // Create and spawn file operation task
                let cancel_token: CancellationToken = CancellationToken::new();

                let task: FileOperationTask = FileOperationTask::new(
                    FileOperation::Copy {
                        source: source.clone(),
                        dest: dest.clone(),
                    },
                    task_tx,
                    cancel_token.clone(),
                    app_handle,
                );

                // Store cancellation token in UI state for ESC key access
                {
                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                    app.ui
                        .store_cancel_token(task.operation_id.clone(), cancel_token);
                }

                tokio::spawn(async move {
                    if let Err(e) = task.execute().await {
                        eprintln!("Copy operation failed: {e}");
                    }
                });

                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.show_info(format!(
                    "Copying {} to {}",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                    dest.display()
                ));

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::Move { source, dest } => {
                info!("Starting move operation: {:?} -> {:?}", source, dest);
                let app: MutexGuard<'_, AppState> = self.app.lock().await;
                let task_tx: UnboundedSender<TaskResult> = app.task_tx.clone();
                let app_handle: Arc<Mutex<AppState>> = self.app.clone();
                drop(app);

                // Create and spawn file operation task
                let cancel_token: CancellationToken = CancellationToken::new();

                let task: FileOperationTask = FileOperationTask::new(
                    FileOperation::Move {
                        source: source.clone(),
                        dest: dest.clone(),
                    },
                    task_tx,
                    cancel_token.clone(),
                    app_handle,
                );

                // Store cancellation token in UI state for ESC key access
                {
                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                    app.ui
                        .store_cancel_token(task.operation_id.clone(), cancel_token);
                }

                tokio::spawn(async move {
                    if let Err(e) = task.execute().await {
                        eprintln!("Move operation failed: {e}");
                    }
                });

                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                app.ui.show_info(format!(
                    "Moving {} to {}",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                    dest.display()
                ));

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::Rename { source, new_name } => {
                info!("Starting rename operation: {:?} -> {}", source, new_name);
                let app: MutexGuard<'_, AppState> = self.app.lock().await;
                let task_tx: UnboundedSender<TaskResult> = app.task_tx.clone();
                let app_handle: Arc<Mutex<AppState>> = self.app.clone();
                drop(app);

                // Create and spawn file operation task
                let cancel_token: CancellationToken = CancellationToken::new();

                let task: FileOperationTask = FileOperationTask::new(
                    FileOperation::Rename {
                        source: source.clone(),
                        new_name: new_name.clone(),
                    },
                    task_tx,
                    cancel_token.clone(),
                    app_handle,
                );

                // Store cancellation token in UI state for ESC key access
                {
                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                    app.ui
                        .store_cancel_token(task.operation_id.clone(), cancel_token);
                }

                tokio::spawn(async move {
                    if let Err(e) = task.execute().await {
                        eprintln!("Rename operation failed: {e}");
                    }
                });

                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                app.ui.show_info(format!(
                    "Renaming {} to {}",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                    new_name
                ));

                app.ui.request_redraw(RedrawFlag::All);
            }

            Action::CancelFileOperation { operation_id } => {
                info!("Cancelling file operation: {operation_id}");

                // TODO: Implement actual cancellation logic in phase 2.4
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                app.ui
                    .show_info(format!("Cancellation operations {operation_id}"));

                app.ui.request_redraw(RedrawFlag::All);
            }

            // Pass-through actions
            Action::Key(_) | Action::Mouse(_) | Action::Resize(..) | Action::NoOp => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.request_redraw(RedrawFlag::All);
            }
        }

        let execution_time: Duration = start_time.elapsed();

        if execution_time.as_millis() > 10 {
            debug!("Action dispatch took {:.2}ms", execution_time.as_millis());
        }
    }

    /// Unified high-performance key event handling with context awareness
    async fn handle_key_event_performance(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Result<Action, ()> {
        let _start_time = Instant::now();

        // Get UI context for processor
        let (ui_mode, ui_overlay, clipboard_active) = {
            let app = self.app.lock().await;
            (app.ui.mode, app.ui.overlay, app.ui.clipboard_overlay_active)
        };

        let mut app = self.app.lock().await;
        if let Some(processor) = app.key_processor.as_mut()
            && let Some(action) = processor.process_key(key, ui_mode, ui_overlay, clipboard_active)
        {
            // Drop the lock before calling dispatch_eaction, which might lock again
            drop(app);

            // Dispatch action with lock-free atomic operations
            self.dispatch_eaction(action).await;

            return Ok(Action::NoOp); // Handled
        }

        Err(()) // Not handled
    }

    /// Unified lock-free action dispatch with zero allocations
    async fn dispatch_eaction(&mut self, action: EAction) {
        match action.action_type {
            // Clipboard operations
            ActionType::CopyToClipboard => {
                self.handle_copy_to_clipboard_performance().await;
            }
            ActionType::MoveToClipboard => {
                self.handle_move_to_clipboard_performance().await;
            }
            ActionType::PasteFromClipboard => {
                self.handle_paste_from_clipboard_performance().await;
            }

            // Navigation actions
            ActionType::NavigateUp => {
                if action.param1 == 1 {
                    // Clipboard context
                    self.handle_clipboard_navigate_up().await;
                } else {
                    self.dispatch_action(Action::MoveSelectionUp).await;
                }
            }
            ActionType::NavigateDown => {
                if action.param1 == 1 {
                    // Clipboard context
                    self.handle_clipboard_navigate_down().await;
                } else {
                    self.dispatch_action(Action::MoveSelectionDown).await;
                }
            }
            ActionType::NavigatePageUp => {
                self.dispatch_action(Action::PageUp).await;
            }
            ActionType::NavigatePageDown => {
                self.dispatch_action(Action::PageDown).await;
            }
            ActionType::NavigateHome => {
                self.dispatch_action(Action::SelectFirst).await;
            }
            ActionType::NavigateEnd => {
                self.dispatch_action(Action::SelectLast).await;
            }
            ActionType::EnterDirectory => {
                if action.param1 == 1 {
                    // Clipboard context
                    self.handle_clipboard_paste_selected().await;
                } else {
                    self.dispatch_action(Action::EnterSelected).await;
                }
            }
            ActionType::NavigateParent => {
                self.dispatch_action(Action::GoToParent).await;
            }

            // Command mode actions
            ActionType::EnterCommandMode => {
                self.dispatch_action(Action::EnterCommandMode).await;
            }
            ActionType::CommandModeChar => {
                self.handle_command_mode_char(action.param1 as u8 as char)
                    .await;
            }
            ActionType::CommandModeBackspace => {
                self.handle_command_mode_backspace().await;
            }
            ActionType::CommandModeEnter => {
                self.handle_command_mode_enter().await;
            }
            ActionType::CommandModeTab => {
                self.handle_command_mode_tab().await;
            }
            ActionType::CommandModeUpDown => {
                self.handle_command_mode_up_down(action.param1 == 1).await;
            }
            ActionType::ExitCommandMode => {
                self.dispatch_action(Action::ExitCommandMode).await;
            }

            // Overlay toggles
            ActionType::ToggleClipboardOverlay => {
                self.dispatch_action(Action::ToggleClipboardOverlay).await;
            }
            ActionType::ToggleFileNameSearch => {
                self.dispatch_action(Action::ToggleFileNameSearch).await;
            }
            ActionType::ToggleContentSearch => {
                self.dispatch_action(Action::ToggleContentSearch).await;
            }
            ActionType::ToggleHelp => {
                self.dispatch_action(Action::ToggleHelp).await;
            }
            ActionType::CloseOverlay => {
                self.dispatch_action(Action::CloseOverlay).await;
            }

            // Search mode actions
            ActionType::SearchModeChar => {
                self.handle_search_mode_char(action.param1 as u8 as char, action.param2)
                    .await;
            }
            ActionType::SearchModeBackspace => {
                self.handle_search_mode_backspace(action.param2).await;
            }
            ActionType::SearchModeEnter => {
                self.handle_search_mode_enter(action.param2).await;
            }
            ActionType::SearchModeUp => {
                self.handle_search_mode_up(action.param2).await;
            }
            ActionType::SearchModeDown => {
                self.handle_search_mode_down(action.param2).await;
            }

            // File operations
            ActionType::FileOpsShowPrompt => {
                self.handle_file_ops_prompt(action.param1 as u32).await;
            }

            // System actions
            ActionType::Quit => {
                self.dispatch_action(Action::Quit).await;
            }

            ActionType::NoOp => {
                // Do nothing
            }
        }
    }

    // Clipboard overlay navigation handlers
    async fn handle_clipboard_navigate_up(&mut self) {
        debug!("Clipboard overlay: Navigate Up");
        let mut app = self.app.lock().await;
        if app.ui.selected_clipboard_item_index > 0 {
            app.ui.selected_clipboard_item_index -= 1;
            app.ui.request_redraw(RedrawFlag::Overlay);
        }
    }

    async fn handle_clipboard_navigate_down(&mut self) {
        debug!("Clipboard overlay: Navigate Down");
        let mut app = self.app.lock().await;
        let clipboard_size = app.ui.clipboard.len();
        if app.ui.selected_clipboard_item_index < clipboard_size.saturating_sub(1) {
            app.ui.selected_clipboard_item_index += 1;
            app.ui.request_redraw(RedrawFlag::Overlay);
        }
    }

    async fn handle_clipboard_paste_selected(&mut self) {
        debug!("Clipboard overlay: Paste Selected Item");
        let mut app = self.app.lock().await;
        app.ui.close_clipboard_overlay();
        app.ui
            .show_info("Paste operation not yet implemented".to_string());
        app.ui.request_redraw(RedrawFlag::All);
    }

    // Command mode handlers
    async fn handle_command_mode_char(&mut self, c: char) {
        let mut app = self.app.lock().await;
        app.ui.command_palette.input.push(c);
        app.ui.command_palette.update_filter();
        app.ui.command_palette.show_completions_if_available();
        app.ui.request_redraw(RedrawFlag::Command);
    }

    async fn handle_command_mode_backspace(&mut self) {
        let mut app = self.app.lock().await;
        app.ui.command_palette.input.pop();
        app.ui.command_palette.update_filter();
        app.ui.command_palette.show_completions_if_available();
        app.ui.request_redraw(RedrawFlag::Command);
    }

    async fn handle_command_mode_enter(&mut self) {
        let app = self.app.lock().await;
        let input = app.ui.command_palette.input.trim();
        info!("Executing command: '{}'", input);

        if let Some(parsed_action) = app.ui.command_palette.parse_command() {
            debug!("Command parsed successfully: {:?}", parsed_action);
            let action = self.map_command_action_to_action(parsed_action);
            drop(app);
            self.dispatch_action(action).await;
        } else if let Some(cmd) = app
            .ui
            .command_palette
            .filtered
            .get(app.ui.command_palette.selected)
        {
            debug!("Using selected command from list: {:?}", cmd.action);
            let action = self.map_command_action_to_action(cmd.action.clone());
            drop(app);
            self.dispatch_action(action).await;
        } else {
            info!("No valid command to execute, exiting command mode");
            drop(app);
            self.dispatch_action(Action::ExitCommandMode).await;
        }
    }

    async fn handle_command_mode_tab(&mut self) {
        let mut app = self.app.lock().await;
        if app.ui.command_palette.show_completions {
            let before = app.ui.command_palette.input.clone();
            app.ui.command_palette.apply_completion();
            let after = app.ui.command_palette.input.clone();
            info!("Applied completion: '{}' -> '{}'", before, after);
            app.ui.request_redraw(RedrawFlag::Command);
        }
    }

    async fn handle_command_mode_up_down(&mut self, is_down: bool) {
        let mut app = self.app.lock().await;
        if app.ui.command_palette.show_completions {
            if is_down {
                app.ui.command_palette.next_completion();
            } else {
                app.ui.command_palette.prev_completion();
            }
        } else {
            if is_down {
                let max_idx = app.ui.command_palette.filtered.len().saturating_sub(1);
                app.ui.command_palette.selected = app
                    .ui
                    .command_palette
                    .selected
                    .saturating_add(1)
                    .min(max_idx);
            } else {
                app.ui.command_palette.selected = app.ui.command_palette.selected.saturating_sub(1);
            }
        }
        app.ui.request_redraw(RedrawFlag::Command);
    }

    // Search mode handlers
    async fn handle_search_mode_char(&mut self, c: char, overlay_type: u64) {
        let mut app = self.app.lock().await;
        app.ui.input.push(c);

        match overlay_type {
            1 => {
                // ContentSearch
                self.clear_search_results(&mut app);
            }
            2 => {
                // FileNameSearch
                let pattern = app.ui.input.clone();
                drop(app);
                self.dispatch_action(Action::FileNameSearch(pattern)).await;
                return;
            }
            3 => { // Prompt
                // Just update input
            }
            _ => {}
        }
        app.ui.request_redraw(RedrawFlag::Overlay);
    }

    async fn handle_search_mode_backspace(&mut self, overlay_type: u64) {
        let mut app = self.app.lock().await;
        app.ui.input.pop();

        match overlay_type {
            1 => {
                // ContentSearch
                self.clear_search_results(&mut app);
            }
            2 => {
                // FileNameSearch
                let pattern = app.ui.input.clone();
                drop(app);
                self.dispatch_action(Action::FileNameSearch(pattern)).await;
                return;
            }
            3 => { // Prompt
                // Just update input
            }
            _ => {}
        }
        app.ui.request_redraw(RedrawFlag::Overlay);
    }

    async fn handle_search_mode_enter(&mut self, overlay_type: u64) {
        let pattern = {
            let app = self.app.lock().await;
            app.ui.input.clone()
        };

        match overlay_type {
            1 => {
                // ContentSearch
                self.dispatch_action(Action::ContentSearch(pattern)).await;
            }
            2 => {
                // FileNameSearch
                self.dispatch_action(Action::FileNameSearch(pattern)).await;
            }
            3 => {
                // Prompt
                self.dispatch_action(Action::SubmitInputPrompt(pattern))
                    .await;
            }
            _ => {}
        }
    }

    async fn handle_search_mode_up(&mut self, overlay_type: u64) {
        let mut app = self.app.lock().await;
        match overlay_type {
            1 => {
                // ContentSearch
                let result_count = Self::current_result_count(&app);
                if result_count > 0 {
                    let new_idx = app.ui.selected.unwrap_or(0).saturating_sub(1);
                    app.ui.selected = Some(new_idx);
                }
            }
            2 => {
                // FileNameSearch
                let result_count = app.ui.filename_search_results.len();
                if result_count > 0 {
                    app.ui.selected = Some(app.ui.selected.unwrap_or(0).saturating_sub(1));
                }
            }
            _ => {}
        }
        app.ui.request_redraw(RedrawFlag::Overlay);
    }

    async fn handle_search_mode_down(&mut self, overlay_type: u64) {
        let mut app = self.app.lock().await;
        match overlay_type {
            1 => {
                // ContentSearch
                let result_count = Self::current_result_count(&app);
                if result_count > 0 {
                    let new_idx =
                        (app.ui.selected.unwrap_or(0) + 1).min(result_count.saturating_sub(1));
                    app.ui.selected = Some(new_idx);
                }
            }
            2 => {
                // FileNameSearch
                let result_count = app.ui.filename_search_results.len();
                if result_count > 0 {
                    let current = app.ui.selected.unwrap_or(0);
                    app.ui.selected = Some((current + 1).min(result_count.saturating_sub(1)));
                }
            }
            _ => {}
        }
        app.ui.request_redraw(RedrawFlag::Overlay);
    }

    // File operations handler
    async fn handle_file_ops_prompt(&mut self, key_code: u32) {
        // Convert u32 back to char for file operation key
        let prompt_type = match key_code as u8 as char {
            'm' => InputPromptType::MoveDestination,
            'r' => InputPromptType::RenameFile,
            _ => return,
        };
        self.dispatch_action(Action::ShowInputPrompt(prompt_type))
            .await;
    }

    /// Legacy clipboard overlay handler (kept for fallback)
    async fn handle_clipboard_overlay_keys(&self, key: KeyEvent) -> Action {
        // This should rarely be called now that we have unified processing
        debug!("Fallback clipboard overlay handler for key: {:?}", key.code);
        Action::NoOp
    }

    /// Zero-allocation clipboard copy with lock-free operations
    async fn handle_copy_to_clipboard_performance(&mut self) {
        let selected_path = {
            let app = self.app.lock().await;
            app.fs.get_selected_path()
        };

        if let Some(path) = selected_path {
            let mut app = self.app.lock().await;
            if let Some(processor) = app.key_processor.as_mut() {
                match processor.clipboard.add_copy(path).await {
                    Ok(id) => {
                        app.ui.show_info(format!("Copied to clipboard: item {id}"));
                    }
                    Err(e) => {
                        let error_msg = format!("Clipboard copy error: {e}");
                        app.ui.show_error(error_msg);
                    }
                }
            }
        } else {
            let mut app = self.app.lock().await;
            app.ui.show_error("No item selected to copy".to_string());
        }
    }

    async fn handle_move_to_clipboard_performance(&mut self) {
        let selected_path = {
            let app = self.app.lock().await;
            app.fs.get_selected_path()
        };

        if let Some(path) = selected_path {
            let mut app = self.app.lock().await;
            if let Some(processor) = app.key_processor.as_mut() {
                match processor.clipboard.add_move(path).await {
                    Ok(id) => {
                        app.ui.show_info(format!("Marked for move: item {id}"));
                    }
                    Err(e) => {
                        let error_msg = format!("Clipboard move error: {e}");
                        app.ui.show_error(error_msg);
                    }
                }
            }
        } else {
            let mut app = self.app.lock().await;
            app.ui.show_error("No item selected to move".to_string());
        }
    }

    async fn handle_paste_from_clipboard_performance(&mut self) {
        let (clipboard, dest_path) = {
            let app = self.app.lock().await;

            // Safe handling of key_processor - use UI clipboard as fallback
            let clipboard = if let Some(processor) = app.key_processor.as_ref() {
                processor.clipboard.clone()
            } else {
                app.ui.clipboard.clone()
            };

            (clipboard, app.fs.active_pane().cwd.clone())
        };

        // TODO: Implement clipboard.get_all_items() in clipr crate
        let items_to_paste = clipboard.get_all_items().await;

        if items_to_paste.is_empty() {
            let mut app = self.app.lock().await;
            app.ui.show_info("Clipboard is empty.".to_string());
            return;
        }

        let op_count = items_to_paste.len();
        for item in items_to_paste {
            let source = item.source_path.clone().into();
            let dest = dest_path.join(item.source_path.clone());
            let action = match item.operation {
                clipr::ClipBoardOperation::Copy => Action::Copy { source, dest },
                clipr::ClipBoardOperation::Move => Action::Move { source, dest },
            };
            self.dispatch_action(action).await;
        }

        if op_count > 0 {
            let mut app = self.app.lock().await;
            app.ui.show_info(format!("Pasting {op_count} item(s)..."));
        }

        // TODO: Implement clipboard.clear_on_paste() in clipr crate
        clipboard.clear_on_paste().await;
    }

    /// Lock-free error handling without allocations
    #[allow(dead_code)]
    async fn handle_clipboard_error_performance(
        &mut self,
        error: ClipError,
        operation: &'static str,
    ) {
        let mut app = self.app.lock().await;

        app.ui
            .show_error(format!("Clipboard {operation} error: {error}"));
    }
}
