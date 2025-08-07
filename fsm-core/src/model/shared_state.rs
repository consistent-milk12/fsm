//! `shared_state.rs`
//!
//! Core application state container with fine-grained locking architecture.
//! Solves mutex contention crisis by separating UI, FS, and business logic into independent mutexes.

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::{
    config::Config,
    controller::actions::Action,
    controller::event_loop::TaskResult,
    model::{
        app_state::AppState, fs_state::FSState, metadata_manager::MetadataManager,
        ui_state::UIState,
    },
};

/// DEPRECATED: Combined state guard for compatibility with legacy render function
/// Provides old AppState-like interface while using fine-grained locking underneath
pub struct DeprecatedSharedStateGuard<'a> {
    pub app: std::sync::MutexGuard<'a, AppState>,
    pub ui: std::sync::MutexGuard<'a, UIState>,
    pub fs: std::sync::MutexGuard<'a, FSState>,
}

/// Main application state container with fine-grained locking
///
/// Architecture:
/// - app_state: Business logic (channels, tasks, history) - minimal mutex scope
/// - ui_state: UI rendering state - independent mutex, never blocks BG tasks
/// - fs_state: Filesystem navigation state - independent mutex, never blocks UI
/// - metadata: Lock-free DashMap access via Arc (no mutex needed)
pub struct SharedState {
    /// Core business logic and channels (minimal mutex scope)
    pub app_state: Arc<Mutex<AppState>>,

    /// UI rendering state (independent mutex - UI renders never block BG tasks)
    pub ui_state: Arc<Mutex<UIState>>,

    /// Filesystem navigation state (independent mutex - FS ops never block UI)
    pub fs_state: Arc<Mutex<FSState>>,

    /// Metadata access (lock-free DashMap, no mutex needed)
    pub metadata: Arc<MetadataManager>,

    /// Configuration (immutable, no mutex needed)
    pub config: Arc<Config>,
}

impl SharedState {
    pub fn new(
        config: Arc<Config>,
        metadata: Arc<MetadataManager>,
        ui_state: UIState,
        fs_state: FSState,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        action_tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        let app_state = AppState::new(config.clone(), metadata.clone(), task_tx, action_tx);

        Self {
            app_state: Arc::new(Mutex::new(app_state)),
            ui_state: Arc::new(Mutex::new(ui_state)),
            fs_state: Arc::new(Mutex::new(fs_state)),
            metadata,
            config,
        }
    }

    /// Try to lock app state without blocking (for render path)
    pub fn try_lock_app(&self) -> Option<std::sync::MutexGuard<'_, AppState>> {
        self.app_state.try_lock().ok()
    }

    /// Try to lock UI state without blocking (for background tasks)
    pub fn try_lock_ui(&self) -> Option<std::sync::MutexGuard<'_, UIState>> {
        self.ui_state.try_lock().ok()
    }

    /// Try to lock FS state without blocking (for UI renders)
    pub fn try_lock_fs(&self) -> Option<std::sync::MutexGuard<'_, FSState>> {
        self.fs_state.try_lock().ok()
    }

    /// Lock app state (for business logic operations)
    pub fn lock_app(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state.lock().unwrap()
    }

    /// Lock UI state (for rendering operations)
    pub fn lock_ui(&self) -> std::sync::MutexGuard<'_, UIState> {
        self.ui_state.lock().unwrap()
    }

    /// Lock FS state (for navigation operations)
    pub fn lock_fs(&self) -> std::sync::MutexGuard<'_, FSState> {
        self.fs_state.lock().unwrap()
    }

    /// Try to lock entire shared state - DEPRECATED: Use fine-grained locking instead
    /// This method exists for compatibility with main.rs render function
    /// Returns None if any component is locked (promotes concurrent architecture)
    pub fn try_lock(&self) -> Option<DeprecatedSharedStateGuard<'_>> {
        // Try all locks non-blocking - if any fail, return None
        let app_guard = self.app_state.try_lock().ok()?;
        let ui_guard = self.ui_state.try_lock().ok()?;
        let fs_guard = self.fs_state.try_lock().ok()?;

        Some(DeprecatedSharedStateGuard {
            app: app_guard,
            ui: ui_guard,
            fs: fs_guard,
        })
    }

    /// Get object info from metadata registry (lock-free access)
    pub fn get_object_info(
        &self,
        object_id: u64,
    ) -> Option<Arc<crate::fs::object_info::ObjectInfo>> {
        self.metadata.get_by_id(object_id)
    }

    /// Update object info in the metadata registry
    pub fn update_object_info(
        &self,
        _parent_dir: &std::path::PathBuf,
        info: &crate::fs::object_info::ObjectInfo,
    ) {
        // Insert the ObjectInfo into the metadata manager
        self.metadata.insert(info.clone());

        // Mark UI as dirty to trigger re-render
        if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard.mark_dirty(crate::model::ui_state::Component::All);
        }
    }

    /// Update multiple object infos in batch
    pub async fn update_object_info_batch(
        &self,
        _parent_dir: &std::path::PathBuf,
        objects: Vec<crate::fs::object_info::ObjectInfo>,
    ) {
        for info in objects {
            self.metadata.insert(info);
        }

        // Mark UI as dirty to trigger re-render
        if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard.mark_dirty(crate::model::ui_state::Component::All);
        }
    }

    /// Get access to the metadata registry - NOTE: This interface is deprecated
    /// Use metadata.get_by_id() or metadata.get() instead for lock-free access
    pub fn registry(&self) -> &Arc<crate::model::metadata_manager::MetadataManager> {
        &self.metadata
    }

    /// Get access to the cache manager - NOTE: No separate cache in new architecture
    /// Metadata operations go directly through MetadataManager for simplified design
    pub fn cache(&self) -> &Arc<crate::model::metadata_manager::MetadataManager> {
        &self.metadata
    }

    // Navigation Methods

    /// Enter the currently selected directory or open the file
    pub async fn enter_selected_directory(&self) -> Result<(), Box<dyn std::error::Error>> {
        let selected_entry = {
            let ui_guard = self.lock_ui();
            let fs_guard = self.lock_fs();
            let active_pane = fs_guard.active_pane();

            if let Some(selected_idx) = ui_guard.selected {
                active_pane.entries.get(selected_idx).cloned()
            } else {
                return Ok(());
            }
        };

        if let Some(entry) = selected_entry
            && let Some(obj_info) = self.metadata.get_by_id(entry.id)
        {
            if obj_info.is_dir {
                self.enter_directory(obj_info.path.as_ref().clone()).await?;
            } else {
                self.open_file_with_editor(obj_info.path.as_ref().clone())
                    .await?;
            }
        }

        Ok(())
    }

    /// Go to the parent directory of the current active pane
    pub async fn go_to_parent_directory(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_dir = {
            let fs_guard = self.lock_fs();
            fs_guard.active_pane().cwd.clone()
        };

        if let Some(parent) = current_dir.parent() {
            self.enter_directory(parent.to_path_buf()).await?;
        } else if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard.show_info("Already at root.".to_string());
        }

        Ok(())
    }

    /// Reload the current directory
    pub async fn reload_directory(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_dir = {
            let fs_guard = self.lock_fs();
            fs_guard.active_pane().cwd.clone()
        };

        self.enter_directory(current_dir).await?;
        Ok(())
    }

    /// Navigate to a specific path
    pub async fn navigate_to_path(
        &self,
        path_str: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = std::path::PathBuf::from(path_str);
        if path.exists() && path.is_dir() {
            self.enter_directory(path).await?;
        } else if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard.show_error(format!("Invalid directory path: {}", path.display()));
        }
        Ok(())
    }

    // File Operation Methods

    /// Create a new file in the current directory
    pub async fn create_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard.show_input_prompt(crate::controller::actions::InputPromptType::CreateFile);
        }
        Ok(())
    }

    /// Create a new directory in the current directory
    pub async fn create_directory(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(mut ui_guard) = self.ui_state.try_lock() {
            ui_guard
                .show_input_prompt(crate::controller::actions::InputPromptType::CreateDirectory);
        }
        Ok(())
    }

    /// Create a file with a specific name
    pub async fn create_file_with_name(
        &self,
        name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_dir = {
            let fs_guard = self.lock_fs();
            fs_guard.active_pane().cwd.clone()
        };

        let file_path = current_dir.join(&name);

        // Use spawn_blocking to avoid blocking the async runtime
        let file_path_clone = file_path.clone();
        let result =
            tokio::task::spawn_blocking(move || std::fs::File::create(&file_path_clone)).await?;

        match result {
            Ok(_) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_info(format!("Created file: {}", name));
                }
                self.reload_directory().await?;
            }
            Err(e) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_error(format!("Failed to create file: {}", e));
                }
            }
        }

        Ok(())
    }

    /// Create a directory with a specific name
    pub async fn create_directory_with_name(
        &self,
        name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_dir = {
            let fs_guard = self.lock_fs();
            fs_guard.active_pane().cwd.clone()
        };

        let dir_path = current_dir.join(&name);

        // Use spawn_blocking to avoid blocking the async runtime
        let dir_path_clone = dir_path.clone();
        let result =
            tokio::task::spawn_blocking(move || std::fs::create_dir(&dir_path_clone)).await?;

        match result {
            Ok(_) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_info(format!("Created directory: {}", name));
                }
                self.reload_directory().await?;
            }
            Err(e) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_error(format!("Failed to create directory: {}", e));
                }
            }
        }

        Ok(())
    }

    /// Delete the currently selected entry
    pub async fn delete_entry(&self) -> Result<(), Box<dyn std::error::Error>> {
        let selected_entry = {
            let ui_guard = self.lock_ui();
            let fs_guard = self.lock_fs();
            let active_pane = fs_guard.active_pane();

            if let Some(selected_idx) = ui_guard.selected {
                active_pane.entries.get(selected_idx).cloned()
            } else {
                return Ok(());
            }
        };

        if let Some(entry) = selected_entry
            && let Some(obj_info) = self.metadata.get_by_id(entry.id)
        {
            let path = obj_info.path.as_ref();

            // Use spawn_blocking to avoid blocking the async runtime
            let path_clone = path.to_path_buf();
            let is_dir = obj_info.is_dir;
            let result = tokio::task::spawn_blocking(move || {
                if is_dir {
                    std::fs::remove_dir_all(&path_clone)
                } else {
                    std::fs::remove_file(&path_clone)
                }
            })
            .await?;

            match result {
                Ok(_) => {
                    if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                        ui_guard.show_info(format!(
                            "Deleted: {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                    self.reload_directory().await?;
                }
                Err(e) => {
                    if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                        ui_guard.show_error(format!("Failed to delete: {}", e));
                    }
                }
            }
        }

        Ok(())
    }

    /// Rename the currently selected entry
    pub async fn rename_selected_entry(
        &self,
        new_name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let selected_entry = {
            let ui_guard = self.lock_ui();
            let fs_guard = self.lock_fs();
            let active_pane = fs_guard.active_pane();

            if let Some(selected_idx) = ui_guard.selected {
                active_pane.entries.get(selected_idx).cloned()
            } else {
                return Ok(());
            }
        };

        if let Some(entry) = selected_entry
            && let Some(obj_info) = self.metadata.get_by_id(entry.id)
        {
            let old_path = obj_info.path.as_ref();
            let new_path = old_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(&new_name);

            // Use spawn_blocking to avoid blocking the async runtime
            let old_path_clone = old_path.to_path_buf();
            let new_path_clone = new_path.clone();
            let result = tokio::task::spawn_blocking(move || {
                std::fs::rename(&old_path_clone, &new_path_clone)
            })
            .await?;

            match result {
                Ok(_) => {
                    if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                        ui_guard.show_info(format!("Renamed to: {}", new_name));
                    }
                    self.reload_directory().await?;
                }
                Err(e) => {
                    if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                        ui_guard.show_error(format!("Failed to rename: {}", e));
                    }
                }
            }
        }

        Ok(())
    }

    // Helper Methods

    /// Enter a directory (internal implementation)
    async fn enter_directory(
        &self,
        path: std::path::PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Update FS state
        {
            let mut fs_guard = self.lock_fs();
            let active_pane = fs_guard.active_pane_mut();
            active_pane.cwd = path.clone();
            active_pane.entries.clear();
            active_pane.selected = Some(0);
        }

        // Update UI state
        {
            let mut ui_guard = self.lock_ui();
            ui_guard.selected = Some(0);
            ui_guard.mark_dirty(crate::model::ui_state::Component::All);
        }

        // Perform directory scanning and populate entries
        self.scan_directory_and_update_entries(&path).await?;

        Ok(())
    }

    /// Open a file with external editor (VS Code)
    async fn open_file_with_editor(
        &self,
        file_path: std::path::PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::process::Command;

        let path_str = file_path.to_string_lossy().to_string();

        let result = tokio::spawn(async move {
            let mut cmd = Command::new("code");
            cmd.arg(&path_str);

            match cmd.spawn() {
                Ok(_) => Ok(path_str),
                Err(e) => Err(format!("Failed to open file with code: {}", e)),
            }
        })
        .await;

        match result {
            Ok(Ok(path)) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_info(format!(
                        "Opened {} in VS Code",
                        std::path::Path::new(&path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file")
                    ));
                }
            }
            Ok(Err(e)) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_error(format!("Failed to open file: {}", e));
                }
            }
            Err(e) => {
                if let Ok(mut ui_guard) = self.ui_state.try_lock() {
                    ui_guard.show_error(format!("Failed to open file: {}", e));
                }
            }
        }

        Ok(())
    }

    /// Scan directory and update entries using the metadata manager
    pub async fn scan_directory_and_update_entries(
        &self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries: Vec<crate::model::object_registry::SortableEntry> = Vec::new();

        // Read directory entries using tokio::fs for async operation
        let mut read_dir = tokio::fs::read_dir(path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();

            // Skip hidden files for now (could be configurable later)
            if let Some(file_name) = entry_path.file_name()
                && file_name.to_string_lossy().starts_with('.')
            {
                continue;
            }

            // Create ObjectInfo from path and insert into metadata manager
            match crate::fs::object_info::ObjectInfo::from_path_async(&entry_path).await {
                Ok(object_info) => {
                    // Insert into metadata manager and get SortableEntry
                    let (_object_id, sortable_entry) = self.metadata.insert(object_info);
                    entries.push(sortable_entry);
                }
                Err(e) => {
                    // Log error but continue with other entries
                    tracing::debug!(
                        "Failed to create ObjectInfo for {}: {}",
                        entry_path.display(),
                        e
                    );
                }
            }
        }

        // Sort entries: directories first, then alphabetically
        entries.sort_by(|a, b| {
            let a_info = self.metadata.get_by_id(a.id);
            let b_info = self.metadata.get_by_id(b.id);

            match (a_info, b_info) {
                (Some(a_obj), Some(b_obj)) => {
                    if a_obj.is_dir && !b_obj.is_dir {
                        std::cmp::Ordering::Less
                    } else if !a_obj.is_dir && b_obj.is_dir {
                        std::cmp::Ordering::Greater
                    } else {
                        a_obj.name.cmp(&b_obj.name)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        // Update FS state with the new entries
        {
            let mut fs_guard = self.lock_fs();
            let active_pane = fs_guard.active_pane_mut();
            active_pane.set_entries(entries);
            active_pane.is_loading = false;
        }

        // Mark UI as dirty to trigger re-render
        {
            let mut ui_guard = self.lock_ui();
            ui_guard.mark_dirty(crate::model::ui_state::Component::All);
        }

        tracing::info!("Directory scan completed for: {}", path.display());

        Ok(())
    }
}
