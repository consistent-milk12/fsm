//! src/controller/event_loop.rs
//! ============================================================================
//! # Controller: Async Event & State Coordination
//!
//! - Manages terminal input, background task updates, and state transitions.
//! - Mutates AppState/UIState, signals UI redraw via redraw flag.
//! - Never calls UI rendering directly; fully decoupled for immediate-mode TUI.

use crate::controller::actions::{Action, InputPromptType};
use crate::fs::dir_scanner::ScanUpdate;
use crate::fs::object_info::ObjectInfo;
use crate::model::app_state::AppState;
use crate::model::command_palette::CommandAction;
use crate::model::fs_state::{EntryFilter, EntrySort, PaneState};
use crate::model::ui_state::{LoadingState, UIMode, UIOverlay};
use crate::tasks::search_task::RawSearchResult;
use crossterm::event::{Event as TermEvent, EventStream, KeyCode, KeyModifiers};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, MutexGuard, mpsc};
use tracing::debug;

/// Result from a background async task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskResult {
    pub task_id: u64,
    pub result: Result<String, String>,
    pub progress: Option<f64>,
    pub current_item: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<String>,
}

pub struct Controller {
    pub app: Arc<Mutex<AppState>>,
    task_rx: mpsc::UnboundedReceiver<TaskResult>,
    event_stream: EventStream,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

impl Controller {
    pub fn new(
        app: Arc<Mutex<AppState>>,
        task_rx: mpsc::UnboundedReceiver<TaskResult>,
        action_rx: mpsc::UnboundedReceiver<Action>,
    ) -> Self {
        Self {
            app,
            task_rx,
            event_stream: EventStream::new(),
            action_rx,
        }
    }

    /// Asynchronously returns the next action, waiting for user input or background task results.
    pub async fn next_action(&mut self) -> Option<Action> {
        tokio::select! {
            Some(Ok(event)) = self.event_stream.next() =>{
                let action = self.handle_terminal_event(event).await;

                debug!("Received terminal event: {:?}", action);

                Some(action)
            }

            Some(task_result) = self.task_rx.recv() => {
                debug!("Received task result: {:?}", task_result);
                Some(Action::TaskResult(task_result))
            }

            Some(action) = self.action_rx.recv() => {
                debug!("Received action: {:?}", action);

                Some(action)
            }

            else => None,
        }
    }

    /// Maps a raw terminal event to a high-level application Action.
    async fn handle_terminal_event(&self, event: TermEvent) -> Action {
        let app: MutexGuard<'_, AppState> = self.app.lock().await;
        let current_overlay: UIOverlay = app.ui.overlay;
        let current_mode: UIMode = app.ui.mode;

        drop(app); // Release the lock as soon as possible

        match event {
            TermEvent::Key(key_event) => {
                // Handle Esc key globally to close any overlay or notification
                if key_event.code == KeyCode::Esc {
                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                    if app.ui.notification.is_some() {
                        app.ui.dismiss_notification();
                        app.redraw = true;
                        drop(app);
                        return Action::NoOp;
                    }

                    drop(app);

                    if current_overlay != UIOverlay::None {
                        return Action::CloseOverlay;
                    } else if current_mode == UIMode::Command {
                        return Action::ExitCommandMode;
                    } else {
                        return Action::Quit;
                    }
                }

                // Any key can dismiss notifications
                {
                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                    if app.ui.notification.is_some() {
                        app.ui.dismiss_notification();
                        app.redraw = true;
                        // Don't return Action::NoOp here, continue processing the key
                    }
                }

                if current_mode == UIMode::Command {
                    // Handle command mode input
                    match key_event.code {
                        KeyCode::Char(c) => {
                            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                            app.ui.command_palette.input.push(c);
                            app.ui.command_palette.update_filter();
                            Action::NoOp
                        }

                        KeyCode::Backspace => {
                            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                            app.ui.command_palette.input.pop();
                            app.ui.command_palette.update_filter();
                            Action::NoOp
                        }

                        KeyCode::Up => {
                            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                            app.ui.command_palette.selected =
                                app.ui.command_palette.selected.saturating_sub(1);
                            Action::NoOp
                        }

                        KeyCode::Down => {
                            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                            app.ui.command_palette.selected = app
                                .ui
                                .command_palette
                                .selected
                                .saturating_add(1)
                                .min(app.ui.command_palette.filtered.len().saturating_sub(1));

                            Action::NoOp
                        }

                        KeyCode::Enter => {
                            let app: MutexGuard<'_, AppState> = self.app.lock().await;
                            // First try to parse the input as a direct command
                            if let Some(parsed_action) = app.ui.command_palette.parse_command() {
                                match parsed_action {
                                    CommandAction::OpenConfig => {
                                        // TODO: Implement opening config
                                        Action::ExitCommandMode
                                    }

                                    CommandAction::Reload => Action::ReloadDirectory,

                                    CommandAction::NewFile => Action::CreateFile,

                                    CommandAction::NewFolder => Action::CreateDirectory,

                                    CommandAction::NewFileWithName(name) => {
                                        Action::CreateFileWithName(name)
                                    }

                                    CommandAction::NewFolderWithName(name) => {
                                        Action::CreateDirectoryWithName(name)
                                    }
                                    CommandAction::SearchContent => Action::ToggleContentSearch,

                                    CommandAction::SearchContentWithPattern(pattern) => {
                                        Action::DirectContentSearch(pattern)
                                    }

                                    CommandAction::Custom(_s) => {
                                        // Handle custom commands
                                        Action::ExitCommandMode
                                    }
                                }
                            } else if let Some(cmd) = app
                                .ui
                                .command_palette
                                .filtered
                                .get(app.ui.command_palette.selected)
                            {
                                // Fall back to selected command from list
                                match cmd.action.clone() {
                                    CommandAction::OpenConfig => {
                                        // TODO: Implement opening config
                                        Action::ExitCommandMode
                                    }

                                    CommandAction::Reload => Action::ReloadDirectory,

                                    CommandAction::NewFile => Action::CreateFile,

                                    CommandAction::NewFolder => Action::CreateDirectory,

                                    CommandAction::NewFileWithName(name) => {
                                        Action::CreateFileWithName(name)
                                    }

                                    CommandAction::NewFolderWithName(name) => {
                                        Action::CreateDirectoryWithName(name)
                                    }

                                    CommandAction::SearchContent => Action::ToggleContentSearch,

                                    CommandAction::SearchContentWithPattern(pattern) => {
                                        Action::DirectContentSearch(pattern)
                                    }

                                    CommandAction::Custom(_s) => {
                                        // Handle custom commands
                                        Action::ExitCommandMode
                                    }
                                }
                            } else {
                                Action::ExitCommandMode
                            }
                        }

                        _ => Action::NoOp,
                    }
                } else if current_overlay != UIOverlay::None {
                    // If an overlay is active, only process keys relevant to that overlay
                    match current_overlay {
                        UIOverlay::FileNameSearch => {
                            match key_event.code {
                                KeyCode::Char(c) => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    app.ui.input.push(c);

                                    // Trigger live search
                                    Action::FileNameSearch(app.ui.input.clone())
                                }

                                KeyCode::Backspace => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    app.ui.input.pop();

                                    // Trigger live search
                                    Action::FileNameSearch(app.ui.input.clone())
                                }

                                KeyCode::Enter => {
                                    let app: MutexGuard<'_, AppState> = self.app.lock().await;

                                    // If we have recursive search results, open the selected one
                                    if !app.filename_search_results.is_empty()
                                        && let Some(selected_idx) = app.ui.selected
                                        && let Some(selected_entry) =
                                            app.filename_search_results.get(selected_idx)
                                    {
                                        return Action::OpenFile(selected_entry.path.clone());
                                    }

                                    // If no recursive results but we have a search term, trigger search
                                    if !app.ui.input.trim().is_empty() {
                                        return Action::FileNameSearch(app.ui.input.clone());
                                    }

                                    // Otherwise close overlay
                                    Action::CloseOverlay
                                }

                                KeyCode::Up => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    let result_count = app.filename_search_results.len();
                                    if result_count > 0 {
                                        app.ui.selected =
                                            Some(app.ui.selected.unwrap_or(0).saturating_sub(1));
                                    }

                                    Action::NoOp
                                }

                                KeyCode::Down => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    let result_count: usize = app.filename_search_results.len();
                                    if result_count > 0 {
                                        let current: usize = app.ui.selected.unwrap_or(0);
                                        app.ui.selected =
                                            Some((current + 1).min(result_count.saturating_sub(1)));
                                    }

                                    Action::NoOp
                                }

                                _ => Action::NoOp,
                            }
                        }
                        UIOverlay::ContentSearch => {
                            match key_event.code {
                                KeyCode::Char(c) => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    app.ui.input.push(c);

                                    // Clear previous results when typing
                                    app.search_results.clear();
                                    app.rich_search_results.clear();
                                    app.raw_search_results = None;
                                    app.ui.last_query = None;
                                    Action::NoOp
                                }

                                KeyCode::Backspace => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                    app.ui.input.pop();

                                    // Clear previous results when backspacing
                                    app.search_results.clear();
                                    app.rich_search_results.clear();
                                    app.raw_search_results = None;
                                    app.ui.last_query = None;

                                    Action::NoOp
                                }

                                KeyCode::Enter => {
                                    let app: MutexGuard<'_, AppState> = self.app.lock().await;

                                    // Handle Shift+Enter for raw search results
                                    if key_event.modifiers.contains(KeyModifiers::SHIFT)
                                        && let Some(ref raw_results) = app.raw_search_results
                                        && let Some(selected_idx) = app.ui.selected
                                        && let Some(selected_line) =
                                            raw_results.lines.get(selected_idx)
                                        && let Some((file_path, _line_num)) =
                                            RawSearchResult::parse_file_info_with_base(
                                                selected_line,
                                                &raw_results.base_directory,
                                            )
                                    {
                                        return Action::OpenFile(file_path);
                                    }

                                    if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                                        return Action::NoOp;
                                    }

                                    // Regular Enter - check for existing results first
                                    // If we have raw search results and a selection, open the file
                                    if let Some(ref raw_results) = app.raw_search_results
                                        && !raw_results.lines.is_empty()
                                        && let Some(selected_idx) = app.ui.selected
                                        && let Some(selected_line) =
                                            raw_results.lines.get(selected_idx)
                                        && let Some((file_path, _line_num)) =
                                            RawSearchResult::parse_file_info_with_base(
                                                selected_line,
                                                &raw_results.base_directory,
                                            )
                                    {
                                        return Action::OpenFile(file_path);
                                    } else if let Some(ref raw_results) = app.raw_search_results
                                        && !raw_results.lines.is_empty()
                                    {
                                        // If we have results but couldn't parse, just return NoOp
                                        return Action::NoOp;
                                    }

                                    // If we have rich search results and a selection, try to parse and open the file
                                    if !app.rich_search_results.is_empty()
                                        && let Some(selected_idx) = app.ui.selected
                                        && let Some(selected_line) =
                                            app.rich_search_results.get(selected_idx)
                                    {
                                        // For rich search results, we need to get the base directory from app state
                                        let base_dir: PathBuf = app.fs.active_pane().cwd.clone();

                                        if let Some((file_path, _line_num)) =
                                            RawSearchResult::parse_file_info_with_base(
                                                selected_line,
                                                &base_dir,
                                            )
                                        {
                                            return Action::OpenFile(file_path);
                                        }
                                    }

                                    // If we have simple search results and a selection, open the file
                                    if !app.search_results.is_empty()
                                        && let Some(selected_idx) = app.ui.selected
                                        && let Some(selected_result) =
                                            app.search_results.get(selected_idx)
                                    {
                                        return Action::OpenFile(selected_result.path.clone());
                                    }

                                    // Otherwise, perform search
                                    Action::ContentSearch(app.ui.input.clone())
                                }

                                KeyCode::Up => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                                    let result_count: usize =
                                        if let Some(ref raw_results) = app.raw_search_results {
                                            raw_results.lines.len()
                                        } else if !app.rich_search_results.is_empty() {
                                            app.rich_search_results.len()
                                        } else {
                                            app.search_results.len()
                                        };

                                    if result_count > 0 {
                                        app.ui.selected =
                                            Some(app.ui.selected.unwrap_or(0).saturating_sub(1));
                                    }

                                    Action::NoOp
                                }

                                KeyCode::Down => {
                                    let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                                    let result_count: usize =
                                        if let Some(ref raw_results) = app.raw_search_results {
                                            raw_results.lines.len()
                                        } else if !app.rich_search_results.is_empty() {
                                            app.rich_search_results.len()
                                        } else {
                                            app.search_results.len()
                                        };

                                    if result_count > 0 {
                                        let current: usize = app.ui.selected.unwrap_or(0);
                                        app.ui.selected =
                                            Some((current + 1).min(result_count.saturating_sub(1)));
                                    }

                                    Action::NoOp
                                }

                                _ => Action::NoOp,
                            }
                        }

                        UIOverlay::Prompt => match key_event.code {
                            KeyCode::Char(c) => {
                                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                app.ui.input.push(c);
                                Action::NoOp
                            }
                            KeyCode::Backspace => {
                                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                                app.ui.input.pop();
                                Action::NoOp
                            }
                            KeyCode::Enter => {
                                let app: MutexGuard<'_, AppState> = self.app.lock().await;
                                let input = app.ui.input.trim().to_string();
                                if !input.is_empty() {
                                    Action::SubmitInputPrompt(input)
                                } else {
                                    Action::CloseOverlay
                                }
                            }
                            _ => Action::NoOp,
                        },

                        UIOverlay::SearchResults => {
                            match key_event.code {
                                KeyCode::Backspace => Action::CloseOverlay,
                                _ => Action::NoOp, // Ignore other keys in search results overlay
                            }
                        }

                        _ => Action::NoOp, // For other overlays (Help, Loading, etc.), ignore all keys except Esc
                    }
                } else {
                    // No overlay is active, process general hotkeys
                    match (key_event.code, key_event.modifiers) {
                        (KeyCode::Char('?'), _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                            Action::ToggleHelp
                        }

                        (KeyCode::Char(':'), _) => Action::EnterCommandMode,

                        (KeyCode::Char('l'), KeyModifiers::CONTROL) => Action::SimulateLoading,

                        (KeyCode::Char('.'), KeyModifiers::CONTROL) => Action::ToggleShowHidden,

                        (KeyCode::Up, _) => Action::MoveSelectionUp,

                        (KeyCode::Down, _) => Action::MoveSelectionDown,

                        (KeyCode::PageUp, _) => Action::PageUp,

                        (KeyCode::PageDown, _) => Action::PageDown,

                        (KeyCode::Home, _) => Action::SelectFirst,

                        (KeyCode::End, _) => Action::SelectLast,

                        (KeyCode::Char('/'), _) => Action::ToggleFileNameSearch,

                        (KeyCode::Enter, _) => Action::EnterSelected,

                        (KeyCode::Backspace, _) => Action::GoToParent,

                        (KeyCode::Char('d'), KeyModifiers::NONE) => Action::Delete,

                        (KeyCode::Char('n'), KeyModifiers::NONE) => {
                            Action::ShowInputPrompt(InputPromptType::CreateFile)
                        }

                        (KeyCode::Char('f'), KeyModifiers::NONE) => {
                            Action::ShowInputPrompt(InputPromptType::CreateDirectory)
                        }

                        (KeyCode::Char('s'), KeyModifiers::NONE) => {
                            Action::Sort("name_asc".to_string())
                        }

                        (KeyCode::Char('F'), KeyModifiers::NONE) => {
                            Action::Filter("all".to_string())
                        }

                        (KeyCode::Char('q'), _) => Action::Quit,

                        _ => Action::Key(key_event), // Pass through unhandled key events
                    }
                }
            }

            TermEvent::Mouse(mouse_event) => Action::Mouse(mouse_event),

            TermEvent::Resize(x, y) => Action::Resize(x, y),

            _ => Action::Tick, // Default to tick for unhandled events
        }
    }

    /// Dispatches an action to update the application state.
    pub async fn dispatch_action(&self, action: Action) {
        debug!("Dispatching action: {:?}", action);
        match action {
            Action::Quit => {
                // Handled in main loop for graceful shutdown
            }

            Action::ToggleHelp => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_help_overlay();
                app.redraw = true;
            }

            Action::EnterCommandMode => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.enter_command_mode();
                app.redraw = true;
            }

            Action::ExitCommandMode => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.exit_command_mode();
                app.redraw = true;
            }

            Action::ToggleFileNameSearch => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_filename_search_overlay();
                app.redraw = true;
            }

            Action::ToggleContentSearch => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_content_search_overlay();
                app.redraw = true;
            }

            Action::FileNameSearch(pattern) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.filename_search(pattern);
                app.redraw = true;
            }

            Action::ContentSearch(pattern) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.start_content_search(pattern);
                app.redraw = true;
            }

            Action::DirectContentSearch(pattern) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.start_content_search(pattern);

                // Exit command mode after starting search
                app.ui.exit_command_mode();
                app.redraw = true;
            }

            Action::ShowSearchResults(results) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.search_results = results;

                // Don't change overlay if we're already in ContentSearch mode
                if app.ui.overlay != UIOverlay::ContentSearch {
                    app.ui.set_overlay(UIOverlay::SearchResults);
                } else {
                    // Initialize selection for ContentSearch results
                    if !app.search_results.is_empty() {
                        app.ui.selected = Some(0);
                    }
                }

                app.redraw = true;
            }

            Action::ShowFilenameSearchResults(results) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.filename_search_results = results;
                app.redraw = true;
            }

            Action::ShowRichSearchResults(results) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.rich_search_results = results;

                // Initialize selection if we're in ContentSearch mode
                if app.ui.overlay == UIOverlay::ContentSearch && !app.rich_search_results.is_empty()
                {
                    app.ui.selected = Some(0);
                }

                app.redraw = true;
            }

            Action::ShowRawSearchResults(results) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.raw_search_results = Some(results);
                app.raw_search_selected = 0;

                // Initialize selection if we're in ContentSearch mode
                if app.ui.overlay == UIOverlay::ContentSearch {
                    app.ui.selected = Some(0);
                }

                app.redraw = true;
            }

            Action::SimulateLoading => {
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
                app.redraw = true;
            }

            Action::ToggleShowHidden => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.toggle_show_hidden();
                app.redraw = true;
            }

            Action::TaskResult(task_result) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                // If a loading overlay is active, update its fields.
                if let Some(ref mut loading) = app.ui.loading {
                    if let Some(progress) = task_result.progress {
                        loading.progress = Some(progress);
                    }
                    if let Some(ref item) = task_result.current_item {
                        loading.current_item = Some(item.clone());
                    }
                    if let Some(done) = task_result.completed {
                        loading.completed = Some(done);
                    }
                    if let Some(total) = task_result.total {
                        loading.total = Some(total);
                    }
                    if let Some(msg) = task_result.message {
                        loading.message = msg;
                    }

                    loading.spinner_frame = loading.spinner_frame.wrapping_add(1);
                }

                // On completion (progress == 1.0), hide overlay.
                if let Some(p) = task_result.progress
                    && (p - 1.0).abs() < f64::EPSILON
                {
                    app.ui.loading = None;

                    // Optionally close overlay if UIOverlay::Loading
                    if app.ui.overlay == UIOverlay::Loading {
                        app.ui.overlay = UIOverlay::None;
                    }
                }

                // Always update AppState's task table.
                app.complete_task(
                    task_result.task_id,
                    Some(match &task_result.result {
                        Ok(s) => s.clone(),
                        Err(e) => format!("Error: {e}"),
                    }),
                );

                app.redraw = true;
            }

            Action::MoveSelectionUp => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().move_selection_up();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::MoveSelectionDown => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().move_selection_down();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::PageUp => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().page_up();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::PageDown => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().page_down();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::SelectFirst => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().select_first();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::SelectLast => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.fs.active_pane_mut().select_last();

                // Update UI state to match pane state
                app.ui.selected = app.fs.active_pane().selected;
                app.redraw = true;
            }

            Action::EnterSelected => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.enter_selected_directory().await;
                app.redraw = true;
            }

            Action::GoToParent => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.go_to_parent_directory().await;
                app.redraw = true;
            }

            Action::Delete => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.delete_entry().await;
                app.redraw = true;
            }

            Action::CreateFile => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_file().await;
                app.redraw = true;
            }

            Action::CreateDirectory => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_directory().await;
                app.redraw = true;
            }

            Action::CreateFileWithName(name) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_file_with_name(name).await;
                app.redraw = true;
            }

            Action::CreateDirectoryWithName(name) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.create_directory_with_name(name).await;
                app.redraw = true;
            }

            Action::Sort(_) => {
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
                app.redraw = true;
            }

            Action::Filter(_) => {
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

                let filter_criteria = active_pane.filter.to_string();
                app.filter_entries(&filter_criteria);
                app.redraw = true;
            }

            Action::UpdateObjectInfo { parent_dir, info } => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.update_object_info(parent_dir, info);
                app.redraw = true;
            }

            Action::CloseOverlay => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.close_all_overlays();
                app.redraw = true;
            }

            Action::ReloadDirectory => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.reload_directory().await;
                app.redraw = true;
            }

            Action::OpenFile(path) => {
                // Launch external editor with the file
                let path_str: String = path.to_string_lossy().to_string();

                tokio::spawn(async move {
                    let mut cmd: Command = Command::new("code");
                    cmd.arg(&path_str);
                    if let Err(e) = cmd.spawn() {
                        tracing::error!("Failed to open file with code: {}", e);
                    }
                });

                // Close the overlay after opening
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.close_all_overlays();
                app.redraw = true;
            }

            Action::DirectoryScanUpdate { path, update } => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;

                // Only process updates for the current directory
                if app.fs.active_pane().cwd == path {
                    match update {
                        ScanUpdate::Entry(entry) => {
                            app.fs.active_pane_mut().add_incremental_entry(entry);
                            app.redraw = true;
                        }

                        ScanUpdate::Completed(_count) => {
                            // Final sort and completion
                            let entries: Vec<ObjectInfo> = app.fs.active_pane().entries.clone();

                            app.fs
                                .active_pane_mut()
                                .complete_incremental_loading(entries);

                            // Add recent directory
                            app.fs.add_recent_dir(path.clone());

                            // Start size calculation tasks for directories
                            let action_tx: mpsc::UnboundedSender<Action> = app.action_tx.clone();
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

                            app.redraw = true;
                        }

                        ScanUpdate::Error(e) => {
                            let current_pane: &mut PaneState = app.fs.active_pane_mut();
                            current_pane.is_loading = false;
                            current_pane.is_incremental_loading = false;

                            let err_msg: String = format!("Error scanning directory: {e}");
                            current_pane.last_error = Some(err_msg.clone());
                            app.set_error(err_msg);
                            app.redraw = true;
                        }
                    }
                }
            }

            Action::Tick => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                // Handle auto-dismiss notifications
                if app.ui.update_notification() {
                    app.redraw = true;
                }
            }

            Action::ShowInputPrompt(prompt_type) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.ui.show_input_prompt(prompt_type);
                app.redraw = true;
            }

            Action::SubmitInputPrompt(input) => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                let prompt_type: Option<InputPromptType> = app.ui.input_prompt_type;
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
                    None => {
                        app.redraw = true;
                    }
                }
            }

            Action::Key(_) | Action::Mouse(_) | Action::Resize(..) | Action::NoOp => {
                let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
                app.redraw = true;
            }
        }
    }
}
