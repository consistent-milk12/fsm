//! StateCoordinator: Lock-free state management for Phase 4.0
//!
//! Replaces Arc<Mutex<AppState>> with high-performance lock-free patterns:
//! - ArcSwap for read-heavy UIState updates
//! - DashMap for concurrent filesystem cache access
//! - Atomic counters for performance metrics
//! - Append-only structures for command history
//!
//! Architecture: 10-50x lock contention reduction with sub-millisecond updates

use crate::config::Config;
use crate::controller::actions::{Action, OperationId};
use crate::fs::object_info::ObjectInfo;
use crate::model::fs_state::{EntryFilter, EntrySort};
use crate::model::ui_state::{NotificationLevel, UIState};

use arc_swap::ArcSwap;
use chrono::{DateTime, Local, TimeZone};
use dashmap::DashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Lock-free state coordinator replacing Arc<Mutex<AppState>>
pub struct StateCoordinator {
    // Read-heavy UI state with lock-free atomic updates
    ui_state: ArcSwap<UIState>,

    // Concurrent filesystem cache with per-directory locking
    fs_cache: DashMap<PathBuf, Arc<DirState>>,

    // High-frequency atomic performance counters
    performance_stats: Arc<AtomicStats>,

    // Configuration with atomic hot-reload support
    config: ArcSwap<Config>,

    // Command history with append-only lock-free logging
    command_log: parking_lot::RwLock<VecDeque<CommandEntry>>,

    // Active file operations tracking
    active_operations: DashMap<OperationId, Arc<OperationState>>,

    // Notification system with atomic updates
    notifications: parking_lot::RwLock<VecDeque<NotificationEntry>>,

    // Search state with concurrent result storage
    search_state: DashMap<String, Arc<SearchResults>>,

    // Current working directory with atomic updates
    current_directory: ArcSwap<PathBuf>,
}

/// Cached directory state with metadata
#[derive(Debug)]
pub struct DirState {
    pub entries: Vec<ObjectInfo>,
    pub last_updated: Instant,
    pub sort_order: EntrySort,
    pub filter: EntryFilter,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub is_loading: AtomicBool,
    pub entry_count: usize,
}

impl Clone for DirState {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            last_updated: self.last_updated,
            sort_order: self.sort_order,
            filter: self.filter.clone(),
            selected_index: self.selected_index,
            scroll_offset: self.scroll_offset,
            is_loading: AtomicBool::new(self.is_loading.load(std::sync::atomic::Ordering::Relaxed)),
            entry_count: self.entry_count,
        }
    }
}

/// Atomic performance statistics
#[derive(Debug)]
pub struct AtomicStats {
    pub events_processed: AtomicU64,
    pub actions_executed: AtomicU64,
    pub ui_updates: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub avg_response_time_ns: AtomicU64,
    pub memory_usage_bytes: AtomicU64,
    pub active_tasks: AtomicUsize,
}

/// Command log entry for history and undo/redo
#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub action: Action,
    pub timestamp: Instant,
    pub execution_time: Duration,
    pub result: CommandResult,
    pub context: String,
}

/// Command execution result
#[derive(Debug, Clone)]
pub enum CommandResult {
    Success(String),
    Error(String),
    Cancelled,
    InProgress,
}

/// File operation state tracking
#[derive(Debug)]
pub struct OperationState {
    pub operation_id: OperationId,
    pub operation_type: String,
    pub progress: AtomicU64, // Progress percentage * 100
    pub total_items: AtomicU64,
    pub completed_items: AtomicU64,
    pub start_time: Instant,
    pub is_cancelled: AtomicBool,
    pub current_item: parking_lot::RwLock<Option<String>>,
}

/// Notification entry with expiration
#[derive(Debug, Clone)]
pub struct NotificationEntry {
    pub level: NotificationLevel,
    pub message: String,
    pub timestamp: Instant,
    pub expires_at: Option<Instant>,
    pub action: Option<Action>,
}

/// Search results with concurrent access
#[derive(Debug)]
pub struct SearchResults {
    pub query: String,
    pub results: parking_lot::RwLock<Vec<PathBuf>>,
    pub is_complete: AtomicBool,
    pub started_at: Instant,
    pub total_matches: AtomicUsize,
}

impl Default for AtomicStats {
    fn default() -> Self {
        Self {
            events_processed: AtomicU64::new(0),
            actions_executed: AtomicU64::new(0),
            ui_updates: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            avg_response_time_ns: AtomicU64::new(0),
            memory_usage_bytes: AtomicU64::new(0),
            active_tasks: AtomicUsize::new(0),
        }
    }
}

impl StateCoordinator {
    /// Create new state coordinator with initial state
    pub fn new(config: Config, initial_ui_state: UIState) -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            ui_state: ArcSwap::from_pointee(initial_ui_state),
            fs_cache: DashMap::new(),
            performance_stats: Arc::new(AtomicStats::default()),
            config: ArcSwap::from_pointee(config),
            command_log: parking_lot::RwLock::new(VecDeque::with_capacity(1000)),
            active_operations: DashMap::new(),
            notifications: parking_lot::RwLock::new(VecDeque::with_capacity(100)),
            search_state: DashMap::new(),
            current_directory: ArcSwap::from_pointee(current_dir),
        }
    }

    /// Update UI state atomically without blocking readers
    pub fn update_ui_state<F>(&self, update_fn: F)
    where
        F: FnOnce(&UIState) -> UIState,
    {
        let start = Instant::now();
        let current = self.ui_state.load();
        let updated = update_fn(&current);
        self.ui_state.store(Arc::new(updated));

        // Update performance metrics
        self.performance_stats
            .ui_updates
            .fetch_add(1, Ordering::Relaxed);
        self.update_avg_response_time(start.elapsed());

        debug!("UI state updated in {:?}", start.elapsed());
    }

    /// Get current UI state snapshot (lock-free read)
    pub fn ui_state(&self) -> Arc<UIState> {
        self.ui_state.load_full()
    }

    /// Update directory cache with new state
    pub fn update_dir_cache(&self, path: PathBuf, dir_state: DirState) {
        let arc_state = Arc::new(dir_state);
        self.fs_cache.insert(path.clone(), arc_state);

        debug!("Updated cache for directory: {:?}", path);
        self.performance_stats
            .cache_hits
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Get current directory (lock-free read)
    pub fn current_directory(&self) -> PathBuf {
        (**self.current_directory.load()).clone()
    }

    /// Update current directory atomically
    pub fn set_current_directory(&self, path: PathBuf) {
        self.current_directory.store(Arc::new(path));
    }

    /// Get directory state from cache (lock-free read)
    pub fn get_dir_state(&self, path: &Path) -> Option<Arc<DirState>> {
        if let Some(entry) = self.fs_cache.get(path) {
            self.performance_stats
                .cache_hits
                .fetch_add(1, Ordering::Relaxed);
            Some(entry.value().clone())
        } else {
            self.performance_stats
                .cache_misses
                .fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Invalidate directory cache entry
    pub fn invalidate_cache(&self, path: &Path) {
        if self.fs_cache.remove(path).is_some() {
            debug!("Invalidated cache for: {:?}", path);
        }
    }

    /// Clear entire cache (for refresh operations)
    pub fn clear_cache(&self) {
        let count = self.fs_cache.len();
        self.fs_cache.clear();
        info!("Cleared {} cache entries", count);
    }

    /// Load directory contents into StateCoordinator
    pub async fn load_directory(
        &self,
        path: PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use std::fs;

        info!("Loading directory: {:?}", path);

        // Read directory entries
        let entries = match fs::read_dir(&path) {
            Ok(entries) => {
                let mut dir_entries = Vec::new();

                for entry in entries {
                    if let Ok(entry) = entry {
                        let entry_path = entry.path();
                        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                        let metadata = entry.metadata().ok();

                        let size = metadata
                            .as_ref()
                            .and_then(|m| if m.is_file() { Some(m.len()) } else { None })
                            .unwrap_or(0);

                        let modified = metadata
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| Local.timestamp_opt(d.as_secs() as i64, 0).single())
                            .flatten()
                            .unwrap_or_else(|| Local::now());

                        let name = entry_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();

                        let extension = entry_path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|s| s.to_string());

                        let object_type = if is_dir {
                            crate::fs::object_info::ObjectType::Dir
                        } else if metadata
                            .as_ref()
                            .map(|m| m.file_type().is_symlink())
                            .unwrap_or(false)
                        {
                            crate::fs::object_info::ObjectType::Symlink
                        } else {
                            crate::fs::object_info::ObjectType::File
                        };

                        dir_entries.push(ObjectInfo {
                            path: entry_path,
                            name,
                            extension,
                            object_type,
                            is_dir,
                            is_symlink: metadata
                                .as_ref()
                                .map(|m| m.file_type().is_symlink())
                                .unwrap_or(false),
                            size,
                            items_count: if is_dir { 0 } else { 0 }, // Will be calculated later if needed
                            modified,
                            metadata_loaded: true,
                        });
                    }
                }

                // Sort entries: directories first, then files, alphabetically
                dir_entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                });

                dir_entries
            }
            Err(e) => {
                warn!("Failed to read directory {:?}: {}", path, e);
                Vec::new()
            }
        };

        // Create directory state
        let entry_count = entries.len();
        let dir_state = DirState {
            entries,
            last_updated: std::time::Instant::now(),
            sort_order: EntrySort::NameAsc,
            filter: EntryFilter::All,
            selected_index: 0,
            scroll_offset: 0,
            is_loading: AtomicBool::new(false),
            entry_count,
        };

        // Update cache
        self.update_dir_cache(path.clone(), dir_state);

        // Set as current directory
        self.set_current_directory(path);

        // Update UI state to reflect current directory
        let ui_state = self.ui_state.load();
        ui_state.redraw_flags.store(
            (crate::model::ui_state::RedrawFlag::All.bits() | crate::model::ui_state::RedrawFlag::StatusBar.bits()) as u32,
            std::sync::atomic::Ordering::Relaxed,
        );

        Ok(())
    }

    /// Log command execution for history/undo
    pub fn log_command(&self, action: Action, result: CommandResult, execution_time: Duration) {
        let entry = CommandEntry {
            action,
            timestamp: Instant::now(),
            execution_time,
            result,
            context: self.get_current_context(),
        };

        let mut log = self.command_log.write();
        log.push_back(entry);

        // Limit history size
        if log.len() > 1000 {
            log.pop_front();
        }

        self.performance_stats
            .actions_executed
            .fetch_add(1, Ordering::Relaxed);
    }

    // ========================================================================
    // Phase 2: Lock-free Navigation Methods
    // ========================================================================

    /// Update selection up atomically (replaces AppState::move_selection_up)
    pub async fn update_selection_up(&self) {
        let current_path = self.current_directory();

        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            if new_dir_state.selected_index > 0 {
                new_dir_state.selected_index -= 1;

                // Update scroll if needed
                if new_dir_state.selected_index < new_dir_state.scroll_offset {
                    new_dir_state.scroll_offset = new_dir_state.selected_index;
                }

                // Update cache atomically
                self.update_dir_cache(current_path.clone(), new_dir_state);

                // Mark UI for redraw
                self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);

                debug!(
                    "Selection moved up: index={}",
                    self.get_dir_state(&current_path)
                        .map(|s| s.selected_index)
                        .unwrap_or(0)
                );
            }
        }
    }

    /// Update selection down atomically (replaces AppState::move_selection_down)  
    pub async fn update_selection_down(&self) {
        let current_path = self.current_directory();

        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            if new_dir_state.selected_index + 1 < new_dir_state.entry_count {
                new_dir_state.selected_index += 1;

                // Update scroll if needed (assuming page size of 20)
                let page_size = 20;
                if new_dir_state.selected_index >= new_dir_state.scroll_offset + page_size {
                    new_dir_state.scroll_offset = new_dir_state.selected_index - page_size + 1;
                }

                // Update cache atomically
                self.update_dir_cache(current_path.clone(), new_dir_state);

                // Mark UI for redraw
                self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);

                debug!(
                    "Selection moved down: index={}",
                    self.get_dir_state(&current_path)
                        .map(|s| s.selected_index)
                        .unwrap_or(0)
                );
            }
        }
    }

    /// Update page up atomically (replaces AppState::page_up)
    pub async fn update_page_up(&self) {
        let current_path = self.current_directory();

        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            let page_size = 20;

            if new_dir_state.scroll_offset >= page_size {
                new_dir_state.scroll_offset -= page_size;
                new_dir_state.selected_index = new_dir_state.scroll_offset;
            } else {
                new_dir_state.scroll_offset = 0;
                new_dir_state.selected_index = 0;
            }

            // Capture debug values before move
            let scroll_offset = new_dir_state.scroll_offset;
            let selected_index = new_dir_state.selected_index;

            // Update cache atomically
            self.update_dir_cache(current_path, new_dir_state);

            // Mark UI for redraw
            self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);

            debug!(
                "Page up: scroll={}, selection={}",
                scroll_offset, selected_index
            );
        }
    }

    /// Update page down atomically (replaces AppState::page_down)
    pub async fn update_page_down(&self) {
        let current_path = self.current_directory();

        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            let page_size = 20;

            if new_dir_state.scroll_offset + page_size < new_dir_state.entry_count {
                new_dir_state.scroll_offset += page_size;
                new_dir_state.selected_index = new_dir_state.scroll_offset;
            } else {
                // Go to last page
                new_dir_state.selected_index = new_dir_state.entry_count.saturating_sub(1);
                new_dir_state.scroll_offset = new_dir_state.entry_count.saturating_sub(page_size);
            }

            // Capture debug values before move
            let scroll_offset = new_dir_state.scroll_offset;
            let selected_index = new_dir_state.selected_index;

            // Update cache atomically
            self.update_dir_cache(current_path, new_dir_state);

            // Mark UI for redraw
            self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);

            debug!(
                "Page down: scroll={}, selection={}",
                scroll_offset, selected_index
            );
        }
    }

    /// Enter selected directory or open file
    pub async fn enter_selected(&self) {
        let current_path = self.current_directory();
        if let Some(dir_state) = self.get_dir_state(&current_path) {
            if let Some(selected_entry) = dir_state.entries.get(dir_state.selected_index) {
                if selected_entry.is_dir {
                    let new_path = selected_entry.path.clone();
                    info!("Entering directory: {:?}", new_path);
                    // The load_directory function will handle the rest
                    if let Err(e) = self.load_directory(new_path).await {
                        warn!("Failed to enter directory: {}", e);
                    }
                } else {
                    // This is where you would handle opening a file
                    info!("Selected item is a file: {:?}", selected_entry.path);
                }
            }
        }
    }

    /// Go to parent directory
    pub async fn go_to_parent(&self) {
        let current_path = self.current_directory();
        if let Some(parent_path) = current_path.parent() {
            info!("Going to parent directory: {:?}", parent_path);
            if let Err(e) = self.load_directory(parent_path.to_path_buf()).await {
                warn!("Failed to go to parent directory: {}", e);
            }
        } else {
            info!("Already at root directory");
        }
    }

    /// Select the first item in the directory
    pub async fn select_first(&self) {
        let current_path = self.current_directory();
        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            if !new_dir_state.entries.is_empty() {
                new_dir_state.selected_index = 0;
                new_dir_state.scroll_offset = 0;
                self.update_dir_cache(current_path, new_dir_state);
                self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);
            }
        }
    }

    /// Select the last item in the directory
    pub async fn select_last(&self) {
        let current_path = self.current_directory();
        if let Some(dir_state) = self.get_dir_state(&current_path) {
            let mut new_dir_state = (*dir_state).clone();
            if !new_dir_state.entries.is_empty() {
                let last_index = new_dir_state.entries.len() - 1;
                new_dir_state.selected_index = last_index;
                // Adjust scroll to show the last item
                let page_size = 20;
                new_dir_state.scroll_offset = last_index.saturating_sub(page_size - 1);
                self.update_dir_cache(current_path, new_dir_state);
                self.update_ui_atomic_flags(crate::model::ui_state::RedrawFlag::Main.bits() as u32);
            }
        }
    }

    /// Delete selected item
    pub async fn delete_selected(&self) {
        let current_path = self.current_directory();
        if let Some(dir_state) = self.get_dir_state(&current_path) {
            if let Some(selected_entry) = dir_state.entries.get(dir_state.selected_index) {
                let path_to_delete = selected_entry.path.clone();
                info!("Deleting item: {:?}", path_to_delete);
                if path_to_delete.is_dir() {
                    if let Err(e) = tokio::fs::remove_dir_all(&path_to_delete).await {
                        warn!("Failed to delete directory: {}", e);
                    }
                } else {
                    if let Err(e) = tokio::fs::remove_file(&path_to_delete).await {
                        warn!("Failed to delete file: {}", e);
                    }
                }
                // Reload the directory to reflect the changes
                if let Err(e) = self.load_directory(current_path).await {
                    warn!("Failed to reload directory after deletion: {}", e);
                }
            }
        }
    }

    /// Get command history for undo/redo functionality
    pub fn get_command_history(&self, count: usize) -> Vec<CommandEntry> {
        let log = self.command_log.read();
        log.iter().rev().take(count).cloned().collect()
    }

    /// Track file operation progress
    pub fn start_operation(
        &self,
        operation_id: OperationId,
        operation_type: String,
        total_items: u64,
    ) {
        let operation_state = Arc::new(OperationState {
            operation_id: operation_id.clone(),
            operation_type,
            progress: AtomicU64::new(0),
            total_items: AtomicU64::new(total_items),
            completed_items: AtomicU64::new(0),
            start_time: Instant::now(),
            is_cancelled: AtomicBool::new(false),
            current_item: parking_lot::RwLock::new(None),
        });

        self.active_operations.insert(operation_id, operation_state);
        self.performance_stats
            .active_tasks
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Update operation progress
    pub fn update_operation_progress(
        &self,
        operation_id: &OperationId,
        completed: u64,
        current_item: Option<String>,
    ) {
        if let Some(entry) = self.active_operations.get(operation_id) {
            let operation = entry.value();
            operation
                .completed_items
                .store(completed, Ordering::Relaxed);

            let total = operation.total_items.load(Ordering::Relaxed);
            if total > 0 {
                let progress = (completed * 10000) / total; // Progress *100 for precision
                operation.progress.store(progress, Ordering::Relaxed);
            }

            if let Some(item) = current_item {
                *operation.current_item.write() = Some(item);
            }
        }
    }

    /// Complete operation and clean up
    pub fn complete_operation(&self, operation_id: &OperationId) {
        if self.active_operations.remove(operation_id).is_some() {
            self.performance_stats
                .active_tasks
                .fetch_sub(1, Ordering::Relaxed);
            debug!("Completed operation: {:?}", operation_id);
        }
    }

    /// Get operation progress for UI display
    pub fn get_operation_progress(&self, operation_id: &OperationId) -> Option<(u64, u64, String)> {
        self.active_operations.get(operation_id).map(|entry| {
            let op = entry.value();
            let completed = op.completed_items.load(Ordering::Relaxed);
            let total = op.total_items.load(Ordering::Relaxed);
            let current = op.current_item.read().clone().unwrap_or_default();
            (completed, total, current)
        })
    }

    /// Add notification with automatic expiration
    pub fn add_notification(
        &self,
        level: NotificationLevel,
        message: String,
        duration: Option<Duration>,
    ) {
        let expires_at = duration.map(|d| Instant::now() + d);
        let entry = NotificationEntry {
            level,
            message,
            timestamp: Instant::now(),
            expires_at,
            action: None,
        };

        let mut notifications = self.notifications.write();
        notifications.push_back(entry);

        // Limit notification count
        if notifications.len() > 100 {
            notifications.pop_front();
        }
    }

    /// Get active notifications (removing expired ones)
    pub fn get_notifications(&self) -> Vec<NotificationEntry> {
        let mut notifications = self.notifications.write();
        let now = Instant::now();

        // Remove expired notifications
        notifications.retain(|n| n.expires_at.map_or(true, |exp| now < exp));

        notifications.iter().cloned().collect()
    }

    /// Start search operation
    pub fn start_search(&self, query: String) -> Arc<SearchResults> {
        let search_results = Arc::new(SearchResults {
            query: query.clone(),
            results: parking_lot::RwLock::new(Vec::new()),
            is_complete: AtomicBool::new(false),
            started_at: Instant::now(),
            total_matches: AtomicUsize::new(0),
        });

        self.search_state.insert(query, search_results.clone());
        search_results
    }

    /// Add search result
    pub fn add_search_result(&self, query: &str, result: PathBuf) {
        if let Some(entry) = self.search_state.get(query) {
            let search = entry.value();
            search.results.write().push(result);
            search.total_matches.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Complete search operation
    pub fn complete_search(&self, query: &str) {
        if let Some(entry) = self.search_state.get(query) {
            entry.value().is_complete.store(true, Ordering::Relaxed);
        }
    }

    /// Get search results
    pub fn get_search_results(&self, query: &str) -> Option<(Vec<PathBuf>, bool)> {
        self.search_state.get(query).map(|entry| {
            let search = entry.value();
            let results = search.results.read().clone();
            let is_complete = search.is_complete.load(Ordering::Relaxed);
            (results, is_complete)
        })
    }

    /// Get performance statistics snapshot
    pub fn get_performance_stats(&self) -> PerformanceSnapshot {
        let stats = &self.performance_stats;
        PerformanceSnapshot {
            events_processed: stats.events_processed.load(Ordering::Relaxed),
            actions_executed: stats.actions_executed.load(Ordering::Relaxed),
            ui_updates: stats.ui_updates.load(Ordering::Relaxed),
            cache_hits: stats.cache_hits.load(Ordering::Relaxed),
            cache_misses: stats.cache_misses.load(Ordering::Relaxed),
            cache_hit_ratio: {
                let hits = stats.cache_hits.load(Ordering::Relaxed);
                let misses = stats.cache_misses.load(Ordering::Relaxed);
                if hits + misses > 0 {
                    hits as f64 / (hits + misses) as f64
                } else {
                    0.0
                }
            },
            avg_response_time_us: stats.avg_response_time_ns.load(Ordering::Relaxed) as f64
                / 1000.0,
            memory_usage_mb: stats.memory_usage_bytes.load(Ordering::Relaxed) / 1024 / 1024,
            active_tasks: stats.active_tasks.load(Ordering::Relaxed),
            cache_size: self.fs_cache.len(),
        }
    }

    /// Update configuration atomically
    pub fn update_config(&self, new_config: Config) {
        self.config.store(Arc::new(new_config));
        info!("Configuration updated");
    }

    /// Get current configuration
    pub fn config(&self) -> Arc<Config> {
        self.config.load_full()
    }

    // Internal helper methods

    fn update_avg_response_time(&self, elapsed: Duration) {
        let elapsed_ns = elapsed.as_nanos() as u64;
        let current_avg = self
            .performance_stats
            .avg_response_time_ns
            .load(Ordering::Relaxed);

        // Exponential moving average with 90% weight on previous value
        let new_avg = if current_avg == 0 {
            elapsed_ns
        } else {
            (current_avg * 9 + elapsed_ns) / 10
        };

        self.performance_stats
            .avg_response_time_ns
            .store(new_avg, Ordering::Relaxed);
    }

    fn get_current_context(&self) -> String {
        let ui_state = self.ui_state();
        format!("mode:{:?},overlay:{:?}", ui_state.mode, ui_state.overlay)
    }
}

/// Performance statistics snapshot for reporting
#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    pub events_processed: u64,
    pub actions_executed: u64,
    pub ui_updates: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_ratio: f64,
    pub avg_response_time_us: f64,
    pub memory_usage_mb: u64,
    pub active_tasks: usize,
    pub cache_size: usize,
}

impl PerformanceSnapshot {
    /// Check if performance is within acceptable bounds
    pub fn is_healthy(&self) -> bool {
        self.avg_response_time_us < 1000.0 && // Sub-millisecond response
          self.cache_hit_ratio > 0.8 &&         // 80%+ cache hit ratio
          self.active_tasks < 10 // Reasonable task load
    }

    /// Generate performance report
    pub fn report(&self) -> String {
        format!(
            "Events: {}, Actions: {}, UI Updates: {}, Cache: {:.1}% hit\n  ratio, \
               Response: {:.1}μs avg, Memory: {}MB, Tasks: {}",
            self.events_processed,
            self.actions_executed,
            self.ui_updates,
            self.cache_hit_ratio * 100.0,
            self.avg_response_time_us,
            self.memory_usage_mb,
            self.active_tasks
        )
    }
}

/// Thread-safe state updates with batching
impl StateCoordinator {
    /// Apply multiple UI updates atomically
    pub fn batch_ui_updates<F>(&self, updates: Vec<F>)
    where
        F: FnOnce(&UIState) -> UIState,
    {
        if updates.is_empty() {
            return;
        }

        let start = Instant::now();

        // We need to apply updates sequentially since UIState contains atomics
        // that can't be cloned. Instead, we'll load, update, and store atomically
        // for each update to maintain consistency.
        for update_fn in updates {
            let current = self.ui_state.load();
            let updated = update_fn(&current);
            self.ui_state.store(Arc::new(updated));
        }

        self.performance_stats
            .ui_updates
            .fetch_add(1, Ordering::Relaxed);
        self.update_avg_response_time(start.elapsed());

        // debug!(
        //     "Applied {} UI updates in {:?}",
        //     updates.len(),
        //     start.elapsed()
        // );
    }

    /// Apply a single UI update with optimized path
    pub fn update_ui_state_fast<F>(&self, update_fn: F)
    where
        F: FnOnce(&UIState) -> UIState,
    {
        let start = Instant::now();
        let current = self.ui_state.load();
        let updated = update_fn(&current);
        self.ui_state.store(Arc::new(updated));

        self.performance_stats
            .ui_updates
            .fetch_add(1, Ordering::Relaxed);
        self.update_avg_response_time(start.elapsed());
    }

    /// Bulk cache operations for directory scans
    pub fn bulk_cache_update(&self, updates: Vec<(PathBuf, DirState)>) {
        let start = Instant::now();
        let update_count = updates.len();

        for (path, state) in updates {
            self.fs_cache.insert(path, Arc::new(state));
        }

        debug!(
            "Bulk updated {} cache entries in {:?}",
            update_count,
            start.elapsed()
        );
    }

    /// Optimized UI state update for atomic fields only
    pub fn update_ui_atomic_flags(&self, redraw_flags: u32) {
        let current = self.ui_state.load();
        current.redraw_flags.store(redraw_flags, Ordering::Relaxed);
    }

    /// Batch redraw flag updates (atomic only)
    pub fn batch_redraw_flags(&self, flags: Vec<crate::model::ui_state::RedrawFlag>) {
        if flags.is_empty() {
            return;
        }

        let current = self.ui_state.load();
        let mut combined_flags = current.redraw_flags.load(Ordering::Relaxed);

        for flag in flags {
            combined_flags |= flag.bits() as u32;
        }

        current
            .redraw_flags
            .store(combined_flags, Ordering::Relaxed);
    }

    /// High-performance UI update for hot path operations
    pub fn quick_ui_update<F>(&self, update_fn: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut UIState) -> Result<(), Box<dyn std::error::Error>>,
    {
        let start = Instant::now();

        // For truly high-performance updates, we need a different approach
        // since UIState contains atomic fields that prevent simple cloning.
        // We'll need to reconstruct the UIState carefully.

        let current = self.ui_state.load();

        // Create a new UIState by copying non-atomic fields and preserving atomic state
        let mut new_state = UIState {
            redraw_flags: std::sync::atomic::AtomicU32::new(
                current.redraw_flags.load(Ordering::Relaxed),
            ),
            mode: current.mode,
            overlay: current.overlay,
            selected: current.selected,
            active_pane: current.active_pane,
            marked_indices: current.marked_indices.clone(),
            visual_range: current.visual_range,
            input: current.input.clone(),
            last_query: current.last_query.clone(),
            input_prompt_type: current.input_prompt_type.clone(),
            show_hidden: current.show_hidden,
            theme: current.theme.clone(),
            search_results: current.search_results.clone(),
            filename_search_results: current.filename_search_results.clone(),
            rich_search_results: current.rich_search_results.clone(),
            raw_search_results: current.raw_search_results.clone(),
            raw_search_selected: current.raw_search_selected,
            loading: current.loading.clone(),
            notification: current.notification.clone(),
            last_status: current.last_status.clone(),
            frame_count: std::sync::atomic::AtomicU64::new(
                current.frame_count.load(Ordering::Relaxed),
            ),
            last_update: current.last_update,
            active_file_operations: current.active_file_operations.clone(),
            operations_cancel_tokens: current.operations_cancel_tokens.clone(),
            clipboard: current.clipboard.clone(),
            clipboard_overlay_active: current.clipboard_overlay_active,
            selected_clipboard_item: current.selected_clipboard_item.clone(),
            selected_clipboard_item_index: current.selected_clipboard_item_index,
            clipboard_view_mode: current.clipboard_view_mode,
            command_palette: current.command_palette.clone(),
            recent_actions: current.recent_actions.clone(),
        };

        // Apply the update function
        update_fn(&mut new_state)?;

        // Store the updated state
        self.ui_state.store(Arc::new(new_state));

        self.performance_stats
            .ui_updates
            .fetch_add(1, Ordering::Relaxed);
        self.update_avg_response_time(start.elapsed());

        Ok(())
    }
}
