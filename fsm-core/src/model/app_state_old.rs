//! ``src/model/app_state.rs``
//! ============================================================================
//! # `AppState`: Advanced, Power-User-Oriented Application State
//!
//! `AppState` unifies all persistent and transient state for the terminal file
//! manager. Its design is optimized for power users, async workflows, and advanced
//! batch/file operations. This state struct makes it easy to support:
//! - Multiple concurrent tasks (search, copy, batch delete/rename)
//! - Undo/redo for destructive operations
//! - Multi-selection and mark/stage workflows
//! - Plugins and extensibility
//! - UI overlays, detailed history/status reporting, and responsive TUI
//!
//! ## Highlights
//! - Shares core handles (cache, config) as `Arc` for async/background use
//! - Clean sub-state for filesystem, UI, tasks, plugin registry, and history
//! - Robust message/status/error management
//! - Extensible for plugins, user scripting, and power tools

use crate::{
    model::metadata_manager::MetadataManager, 
    fs::dir_scanner::scan_dir_streaming_with_background_metadata
};
use crate::config::Config;
use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use crate::fs::object_info::ObjectInfo;
use crate::model::fs_state::{FSState, PaneState};
use crate::model::object_registry::SortableEntry;
use crate::model::ui_state::{Component, UIState};
use crate::tasks::filename_search_task::FilenameSearchTask;

use std::{cmp::Ordering, collections::{HashMap, HashSet, VecDeque}, path::PathBuf};
use std::io::Error;
use std::path::{Path};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use dashmap::mapref::one::Ref;
use tokio::{process::Command, task::JoinError};
use tokio::sync::mpsc;
use tracing::{debug, info, warn, Instrument};

/// Represents a pending or running asynchronous task (search, copy, delete, etc.).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_id: u64,
    pub description: String,
    pub started_at: Instant,
    pub is_completed: bool,
    pub result: Option<String>, // Could be success message, error, etc.
    // For progress-based/overlay reporting:
    pub progress: Option<f64>,
    pub current_item: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// Shared application state - Contains only metadata and business logic
/// UI and FS state moved to separate Arc<Mutex> containers for fine-grained locking
pub struct AppState {
    // --- Core Configuration and Services ---
    pub config: Arc<Config>,
    pub metadata: Arc<MetadataManager>,

    // --- Communication Channels ---
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub action_tx: mpsc::UnboundedSender<Action>,

    // --- Business Logic State ---
    /// Marked files/directories by path for batch operations
    pub marked: HashSet<PathBuf>,
    
    /// Application history for undo/redo operations
    pub history: VecDeque<AppHistoryEvent>,
    
    /// Plugin registry and information
    pub plugins: HashMap<String, PluginInfo>,
    
    /// Active background tasks
    pub tasks: HashMap<u64, TaskInfo>,
    
    /// Last error message (distinct from UI notifications)
    pub last_error: Option<String>,
    
    /// Application startup time for analytics
    pub started_at: Instant,
}


#[derive(Debug, Clone)]
pub enum AppHistoryEvent {
    Delete { paths: Vec<String> },
    Move { from: String, to: String },
    Rename { from: String, to: String },
    Copy { from: String, to: String },
    // Extend with plugin hooks, batch ops, custom undo, etc.
}

/// Info about a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    pub loaded: bool,
    // Optionally: plugin type, API version, path, config, etc.
}

impl AppState {
    /// Construct a new, ready-to-use `AppState`.
    #[must_use]
    pub fn new(
        config: Arc<Config>,
        metadata: Arc<MetadataManager>,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        Self {
            // Core Configuration and Services
            config,
            metadata,

            // Communication Channels
            task_tx,
            action_tx,

            // Business Logic State
            marked: HashSet::new(),
            history: VecDeque::new(),
            plugins: HashMap::new(),
            tasks: HashMap::new(),
            last_error: None,
            started_at: Instant::now(),
        }
    }

    /// Mark a file or directory by its canonical path for batch operations.
    pub fn mark_entry(&mut self, path: impl Into<PathBuf>) {
        self.marked.insert(path.into());
    }

    /// Unmark a previously marked entry.
    pub fn unmark_entry(&mut self, path: &Path) {
        self.marked.remove(path);
    }

    /// Clear all marked entries.
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Add a reversible event to the history stack (for undo/redo).
    pub fn push_history(&mut self, event: AppHistoryEvent) {
        self.history.push_back(event);
        
        if self.history.len() > 128 {
            self.history.pop_front();
        }
    }

    /// Register a plugin for later use.
    pub fn register_plugin(&mut self, info: PluginInfo) {
        self.plugins.insert(info.name.clone(), info);
        
        self.ui.mark_dirty(Component::All);
    }

    /// Add or update a running/pending async task.
    pub fn add_task(&mut self, task: TaskInfo) {
        info!("Adding task: {}", task.description);
        
        self.tasks.insert(task.task_id, task);
        
        self.ui.mark_dirty(Component::All);
    }

    /// Update task completion/result.
    pub fn complete_task(&mut self, task_id: u64, result: Option<String>) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            info!("Completing task: {}", task.description);
        
            task.is_completed = true;
            task.result = result;
        
            self.ui.mark_dirty(Component::All);
        }
    }

    /// Set the latest error message (display in UI).
    pub fn set_error(&mut self, msg: impl Into<String>) {
        let msg_str: String = msg.into();
        
        warn!("Setting error: {}", msg_str);
        
        self.last_error = Some(msg_str.clone());
        self.ui.show_error(msg_str);
        self.ui.mark_dirty(Component::All);
    }

    /// Set the latest info/status message (display in UI).
    pub fn set_status(&mut self, msg: impl Into<String>) {
        let msg_str: String = msg.into();
        
        info!("Setting status: {}", msg_str);
        
        self.ui.last_status = Some(msg_str.clone());
        self.ui.show_info(msg_str);
        self.ui.mark_dirty(Component::All);
    }

    /// Show a success notification
    pub fn show_success(&mut self, msg: impl Into<String>) {
        let success_msg = msg.into();
        self.ui.show_success(success_msg.clone());
        
        info!("Success: {}", success_msg);
        self.ui.mark_dirty(Component::All);
    }

    /// Show a warning notification
    pub fn show_warning(&mut self, msg: impl Into<String>) {
        let warning_msg = msg.into();
        self.ui.show_warning(warning_msg.clone());
        
        info!("Warning: {}", warning_msg);
        self.ui.mark_dirty(Component::All);
    }

    /// Clear error and status messages.
    pub fn clear_msgs(&mut self) {
        self.last_error = None;
        self.ui.last_status = None;
        self.ui.mark_dirty(Component::All);
    }

    /// Navigate to a new directory, updating the active pane.
    pub async fn enter_directory(&mut self, path: PathBuf) {
        info!("Entering directory: {}", path.display());
        
        // Pre-warm cache before directory change for better performance
        let warm_start = std::time::Instant::now();
        let warmed_count = self.cache.warm_for_navigation(&path).await
            .unwrap_or_else(|e| {
                tracing::warn!("Navigation cache warming failed: {}", e);
                0
            });
        
        if warmed_count > 0 {
            let warm_duration = warm_start.elapsed();
            info!(
                marker = "CACHE_OPERATION",
                operation_type = "navigation_pre_warming",
                warmed_count = warmed_count,
                warm_duration_us = warm_duration.as_micros(),
                target_path = %path.display(),
                "Pre-navigation cache warming completed"
            );
        }
        
        let canonical_path: Arc<PathBuf> = match tokio::fs::canonicalize(&path).await {
            Ok(p) => p.into(),
        
            Err(e) => {
                self.set_error(format!("Invalid path: {}: {}", path.display(), e));
                self.ui.mark_dirty(Component::All);
        
                return;
            }
        };

        let current_pane: &mut PaneState = self.fs.active_pane_mut();
        current_pane.cwd.clone_from(&canonical_path);
        current_pane.is_loading = true;
        
        self.ui.mark_dirty(Component::All);

        // Use streaming directory scan for better responsiveness
        self.enter_directory_streaming(canonical_path).await;
    }

    /// Go to the parent directory of the current active pane.
    pub async fn go_to_parent_directory(&mut self) {
        let current_pane_cwd: PathBuf = self.fs.active_pane().cwd.clone();
        
        if let Some(parent) = current_pane_cwd.parent() {
            info!("Going to parent directory: {}", parent.display());
        
            self.enter_directory(parent.to_path_buf()).await;
        } else {
            info!("Already at root, cannot go to parent.");
        
            self.set_status("Already at root.");
        }
        
        self.ui.mark_dirty(Component::All);
    }

    pub async fn reload_directory(&mut self) {
        let current_dir: PathBuf = self.fs.active_pane().cwd.clone();
        
        self.enter_directory(current_dir).await;
    }

    /// Enter directory using streaming scan for better responsiveness
    async fn enter_directory_streaming(&mut self, path: Arc<PathBuf>) {
        let current_pane: &mut PaneState = self.fs.active_pane_mut();
        current_pane.start_incremental_loading();

        let (
            mut rx,
            _handle
        ) = scan_dir_streaming_with_background_metadata(
            path.clone(),
            self.ui.show_hidden,
            10, // Batch size for yielding
            self.action_tx.clone(),
            self.config.profiling.clone(),
            self.cache.clone()
        )
        .await;

        // Spawn task to handle streaming updates
        let action_tx: mpsc::UnboundedSender<Action> = self.action_tx.clone();
        let scan_path: Arc<PathBuf> = path.clone();

        let value: Arc<PathBuf> = scan_path.clone();
        
        tokio::spawn(
            async move {
                while let Some(update) = rx.recv().await {
                    let _ = action_tx.send(Action::DirectoryScanUpdate {
                        path: value.clone(),
                        update,
                    });
                }
            }
            .instrument(tracing::info_span!(
                "dir_scan_stream_processing",
                operation_type = "dir_scan_stream_processing",
                path = %scan_path.display()
            )),
        );

        self.ui.mark_dirty(Component::All);
    }

    /// Enter the currently selected directory or open the file.
    pub async fn enter_selected_directory(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();

        if let Some(selected_idx) = self.ui.selected
            && let Some(selected_entry) = active_pane.entries.get(selected_idx)
        {
            // Clone Arc<PathBuf> before registry lookup ends to avoid borrow conflicts  
            let path_and_type: Option<(Arc<PathBuf>, bool)> = self
                .registry
                .get(selected_entry.id)
                .map(
                    |obj: Ref<'_, u64, ObjectInfo>| -> (Arc<PathBuf>, bool) 
                    {
                        (obj.path.clone(), obj.is_dir)
                    }
                 );

            if let Some((path, is_dir)) = path_and_type {
                if path.is_file() {
                    self.open_file_with_editor(path.clone().to_path_buf()).await;
                } else if is_dir {
                    self.enter_directory(path.clone().to_path_buf()).await;
                } else {
                    self.set_error(format!("Cannot open: {}", path.display()));
                }
            }
        }

        self.ui.mark_dirty(Component::All);
    }

    /// Open a file with external editor (VS Code)
    pub async fn open_file_with_editor(&mut self, file_path: PathBuf) {
        let path_str: String = file_path.to_string_lossy().to_string();
        
        let open_result: Result<Result<String, String>, JoinError> = tokio::spawn(
            async move 
            {
                let mut cmd: Command = Command::new("code");
                cmd.arg(&path_str);
                
                match cmd.spawn() 
                {
                    Ok(_) => Ok(path_str),
                
                    Err(e) => Err(format!("Failed to open file with code: {e}")),
                }
            }
        )
        .await;

        match open_result {
            Ok(Ok(path)) => {
                self.show_success(format!(
                    "Opened {} in VS Code",
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                ));
            }

            Ok(Err(e)) => {
                self.set_error(e);
            }

            Err(e) => {
                self.set_error(format!("Task failed: {e}"));
            }
        }
    }

    pub async fn delete_entry(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();

        if let Some(selected_idx) = self.ui.selected
            && let Some(selected_entry) = active_pane.entries.get(selected_idx)
        {
            // Clone Arc<PathBuf> and is_dir before registry lookup ends to avoid borrow conflicts
            let path_and_type: Option<(Arc<PathBuf>, bool)> = self
                .registry
                .get(selected_entry.id)
                .map(
                    |obj: Ref<'_, u64, ObjectInfo>| -> (Arc<PathBuf>, bool)
                    { 
                        (obj.path.clone(), obj.is_dir) 
                    }
                );

            if let Some((path, is_dir)) = path_and_type {
                let result: Result<(), Error> = if is_dir {
                    tokio::fs::remove_dir_all(&**path).await
                } else {
                    tokio::fs::remove_file(&**path).await
                };

                if let Err(e) = result {
                    self.set_error(format!("Failed to delete {}: {}", path.display(), e));
                } else {
                    self.show_success(format!("Deleted {}", path.display()));
                    self.reload_directory().await;
                }
            }
        }
    }

    pub async fn create_file(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_file_path: PathBuf = active_pane.cwd.join("new_file.txt");

        if let Err(e) = tokio::fs::File::create(&new_file_path).await {
            self.set_error(format!("Failed to create file: {e}"));
        } else {
            self.show_success(format!("Created file: {}", new_file_path.display()));
            self.reload_directory().await;
        }
    }

    pub async fn create_directory(&mut self) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_dir_path: PathBuf = active_pane.cwd.join("new_directory");

        if let Err(e) = tokio::fs::create_dir(&new_dir_path).await {
            self.set_error(format!("Failed to create directory: {e}"));
        } else {
            self.show_success(format!("Created directory: {}", new_dir_path.display()));
            self.reload_directory().await;
        }
    }

    pub async fn create_file_with_name(&mut self, name: String) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_file_path: PathBuf = active_pane.cwd.join(&name);

        if let Err(e) = tokio::fs::File::create(&new_file_path).await {
            self.set_error(format!("Failed to create file '{name}': {e}"));
        } else {
            self.show_success(format!("Created file: {name}"));
            self.reload_directory().await;
        }
    }

    pub async fn create_directory_with_name(&mut self, name: String) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        let new_dir_path: PathBuf = active_pane.cwd.join(&name);
        
        if let Err(e) = tokio::fs::create_dir(&new_dir_path).await {
            self.set_error(format!("Failed to create directory '{name}': {e}"));
        } else {
            self.show_success(format!("Created directory: {name}"));
            self.reload_directory().await;
        }
    }

    /// Rename the currently selected entry
    pub async fn rename_selected_entry(&mut self, new_name: String) {
        let active_pane: PaneState = self.fs.active_pane().clone();
        if let Some(selected_idx) = self.ui.selected
            && let Some(selected_entry) = active_pane.entries.get(selected_idx)
        {
            // Clone Arc<PathBuf> before registry lookup ends to avoid borrow conflicts
            let old_path_opt = self.registry.get(selected_entry.id)
                .map(|obj| obj.path.clone());

            if let Some(old_path) = old_path_opt {
                let parent_dir: &Path = old_path.parent().unwrap_or(&active_pane.cwd);
                let new_path: PathBuf = parent_dir.join(&new_name);

                if let Err(e) = tokio::fs::rename(&**old_path, &new_path).await {
                    self.set_error(format!("Failed to rename to '{new_name}': {e}"));
                } else {
                    self.show_success(format!("Renamed to '{new_name}'"));
                    self.reload_directory().await;
                }
            } else {
                self.set_error("No entry selected for renaming".to_string());
            }
        } else {
            self.set_error("No entry selected for renaming".to_string());
        }
    }

    /// Navigate to the specified path
    pub async fn navigate_to_path(&mut self, path_str: String) {
        let path: PathBuf = PathBuf::from(path_str.trim());

        // Expand tilde for home directory
        let expanded_path: PathBuf = if path.starts_with("~") {
            if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
                home.join(path.strip_prefix("~").unwrap_or(&path))
            } else {
                path
            }
        } else if path.is_relative() {
            // Make relative paths absolute from current directory
            self.fs.active_pane().cwd.join(path)
        } else {
            path
        };

        if expanded_path.exists() {
            if expanded_path.is_dir() {
                info!("Navigating to directory: {:?}", expanded_path);
                self.enter_directory(expanded_path).await;
            } else {
                self.set_error(format!(
                    "Path is not a directory: {}",
                    expanded_path.display()
                ));
            }
        } else {
            self.set_error(format!("Path does not exist: {}", expanded_path.display()));
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::missing_panics_doc)]
    /// Perform a file name search (recursive, background task)
    pub fn filename_search(&mut self, pattern: &str) {
        let trimmed_pattern: &str = pattern.trim();
        if trimmed_pattern.is_empty() {
            debug!("Filename search called with empty pattern, ignoring");
            return;
        }

        info!(
            "Starting filename search for pattern: '{}'",
            trimmed_pattern
        );

        // Cancel any existing filename search tasks to prevent conflicts
        let existing_searches: Vec<u64> = self
            .tasks
            .iter()
            .filter(|(_, task)| -> bool 
                {
                    task
                        .description
                        .contains("Filename search") && !task.is_completed
                }
            )
            .map(
                |(id, _)| -> u64 
                {
                     *id 
                }
            )
            .collect();

        if !existing_searches.is_empty() {
            info!(
                "Cancelling {} existing filename search tasks: {:?}",
                existing_searches.len(),
                existing_searches
            );
            for task_id in existing_searches {
                if let Some(task) = self.tasks.get_mut(&task_id) {
                    task.is_completed = true;
                    task.result = Some("Cancelled by new search".to_string());
                }
            }
        }

        // Clear previous results and start new search
        let previous_results_count: usize = self.ui.filename_search_results.len();
        self.ui.filename_search_results.clear();
        if previous_results_count > 0 {
            debug!("Cleared {} previous search results", previous_results_count);
        }

        // Generate unique task ID (use timestamp + random component to avoid collisions)
        let task_id: u64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        debug!("Generated task ID {} for filename search", task_id);

        let task: TaskInfo = TaskInfo {
            task_id,
            description: format!("Filename search for '{trimmed_pattern}'"),
            started_at: Instant::now(),
            is_completed: false,
            result: None,
            progress: None,
            current_item: None,
            completed: None,
            total: None,
            message: None,
        };
        self.tasks.insert(task_id, task);

        // Start background filename search task
        let current_dir = self.fs.active_pane().cwd.clone();
        info!(
            "Launching filename search task {} in directory: {}",
            task_id,
            current_dir.display()
        );

        FilenameSearchTask::filename_search_task(
            task_id,
            trimmed_pattern.to_string(),
            current_dir,
            self.task_tx.clone(),
            self.action_tx.clone(),
            self.cache.clone(),
            self.registry.clone(),
        );

        self.ui.mark_dirty(Component::All);
        debug!("Filename search task {} initiated successfully", task_id);
    }

    /// Start a content search using ripgrep
    pub fn start_content_search(&mut self, pattern: String) {
        if pattern.trim().is_empty() {
            return;
        }

        self.ui.search_results.clear();
        self.ui.rich_search_results.clear();
        self.ui.last_query = Some(pattern.clone());

        // Keep the ContentSearch overlay active and show search state
        let task_id: u64 = self.tasks.len() as u64;
        let task: TaskInfo = TaskInfo {
            task_id,
            description: format!("Content search for '{pattern}'"),
            started_at: Instant::now(),
            is_completed: false,
            result: None,
            progress: None,
            current_item: None,
            completed: None,
            total: None,
            message: None,
        };
        self.add_task(task);

        // Start ripgrep search task
        let path = self.fs.active_pane().cwd.clone();
        let task_tx = self.task_tx.clone();
        let action_tx = self.action_tx.clone();

        crate::tasks::search_task::search_task(task_id, pattern, path, task_tx, action_tx);
    }

    /// Updates an `ObjectInfo` in the registry with new data from a background task.
    pub fn update_object_info(&mut self, parent_dir: &PathBuf, info: &ObjectInfo) {
        // Update the registry with new ObjectInfo
        let object_id = self.registry.insert(info.clone());
        
        // Update SortableEntry in relevant pane if needed
        if let Some(pane) = self
            .fs
            .panes
            .iter_mut()
            .find(|p: &&mut PaneState| -> bool 
                {
                    &p.cwd == parent_dir
                }
            )
        {
            // Find matching SortableEntry by id and update if registry changed
            for entry in &mut pane.entries {
                if entry.id == object_id {
                    // Refresh SortableEntry with updated ObjectInfo
                    *entry = SortableEntry::from_object_info(info, object_id);
                    break;
                }
            }
        }
            
        debug!(
            "Updating object info for {}: modified = {}",
            info.path.display(),
            info.format_date("%Y-%m-%d")
        );
        
        self.ui.mark_dirty(Component::All);
    }

    /// Process batch of `ObjectInfo` updates efficiently (single mutex lock per batch) 
    pub async fn update_object_info_batch(&mut self, parent_dir: &PathBuf, objects: Vec<ObjectInfo>)
    {
        for (i, info) in objects.iter().enumerate()
        {
            self.update_object_info(parent_dir, info);

            // Yield every 10 entries to prevent event loop blocking
            if (i % 10 == 0) && (i > 0)
            {
                tokio::task::yield_now().await;
            }
        }
    }

    pub fn sort_entries(&mut self, sort_criteria: &str, metadata: &MetadataManager) {
        let active_pane: &mut PaneState = self.fs.active_pane_mut();
        match sort_criteria {
            "name_asc" => active_pane
                .entries
                .sort_by(
                    |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        // Get actual file names from metadata for proper string comparison
                        let name_a = metadata.get_by_id(a.id).map(|info| &info.name).unwrap_or("");
                        let name_b = metadata.get_by_id(b.id).map(|info| &info.name).unwrap_or("");
                        name_a.cmp(name_b)
                    }
                ),
            
            "name_desc" => active_pane
                .entries
                .sort_by(
                    |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        // Get actual file names from metadata for proper string comparison
                        let name_a = metadata.get_by_id(a.id).map(|info| &info.name).unwrap_or("");
                        let name_b = metadata.get_by_id(b.id).map(|info| &info.name).unwrap_or("");
                        name_b.cmp(name_a)
                    }
                ),
            
            "size_asc" => active_pane
                .entries
                .sort_by(
                    |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        a.size.cmp(&b.size)
                    }
                ),
            
            "size_desc" => active_pane
                .entries
                .sort_by(
                    |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        b.size.cmp(&a.size)
                    }
                ),
            
            "modified_asc" => active_pane
                .entries
                .sort_by(|a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        a.modified.cmp(&b.modified)
                    }
                ),
            
            "modified_desc" => active_pane
                .entries
                .sort_by(
                    |a: &SortableEntry, b: &SortableEntry| -> Ordering 
                    {
                        b.modified.cmp(&a.modified)
                    }
                ),
     
            _ => {}
        }
      
        self.ui.mark_dirty(Component::All);
    }

    pub fn filter_entries(&mut self, filter_criteria: &str) {
        let active_pane: &mut PaneState = self.fs.active_pane_mut();
        // Filter SortableEntry by checking name via registry lookup
        let entries: Vec<SortableEntry> = active_pane.entries.clone();
        
        active_pane.entries = entries
            .into_iter()
            .filter(|entry: &SortableEntry| -> bool 
                {
                    // Look up ObjectInfo from registry to check name
                    if let Some(object_info) = self.registry.get(entry.id) {
                        object_info.name.contains(filter_criteria)
                    } else {
                        false // Skip entries not found in registry
                    }
                }
            )
            .collect();
        
        self.ui.mark_dirty(Component::All);
    }
}

#[expect(clippy::missing_fields_in_debug, reason = "Expected")]
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("task_tx", &"Transfer Channel")
            .field("action_tx", &"Receiver Channel")
            .field("config", &"Config")
            .field("cache", &"ObjectInfoCache")
            .field("fs", &self.fs)
            .field("ui", &self.ui)
            .field("marked", &self.marked)
            .field("history", &self.history)
            .field("tasks", &self.tasks)
            .field("plugins", &self.plugins)
            .field("last_error", &self.last_error)
            .field("last_status", &self.ui.last_status)
            .field("started_at", &self.started_at)
            .finish()
    }
}
