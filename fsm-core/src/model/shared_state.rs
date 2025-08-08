//! `shared_state.rs`
//!
//! Core application state container with fine-grained locking architecture.
//! Solves mutex contention crisis by separating UI, FS, and business logic into independent mutexes.

use compact_str::CompactString;
use futures::{StreamExt, stream};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, task};
use tracing::{Instrument, Span, field, instrument};

use crate::{
    config::Config,
    controller::{actions::Action, event_loop::TaskResult},
    fs::object_info::ObjectInfo,
    model::{
        PaneState,
        app_state::AppState,
        fs_state::FSState,
        metadata_manager::MetadataManager,
        object_registry::SortableEntry,
        ui_state::{Component, UIState},
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
/// ## Architecture:
/// - app_state: Business logic (channels, tasks, history) - minimal mutex scope
/// - ui_state: UI rendering state - independent mutex, never blocks BG tasks
/// - fs_state: Filesystem navigation state - independent mutex, never blocks UI
/// - metadata: Lock-free DashMap access via Arc (no mutex needed)
///
/// ## 🔒 **MANDATORY GLOBAL LOCK ORDERING - DEADLOCK PREVENTION**
/// 
/// **CRITICAL**: When acquiring multiple locks, ALWAYS follow this order:
/// ```
/// 1. FS_STATE  →  2. UI_STATE  →  3. APP_STATE
/// ```
///
/// ### ✅ **SAFE PATTERNS**:
/// ```rust
/// // Pattern 1: Independent scopes (PREFERRED)
/// let data = { self.lock_fs().some_data.clone() };
/// { self.lock_ui().update(data); }
///
/// // Pattern 2: Sequential FS→UI (when needed)
/// let mut fs = self.lock_fs();   // FS first
/// drop(fs);                      // Explicit drop
/// let mut ui = self.lock_ui();   // UI second
/// ```
///
/// ### ❌ **DEADLOCK PATTERNS - FORBIDDEN**:
/// ```rust
/// let ui = self.lock_ui();   // UI first
/// let fs = self.lock_fs();   // FS second - DEADLOCK RISK!
/// ```
///
/// ### 📋 **ENFORCEMENT CHECKLIST**:
/// - [ ] All multi-lock functions follow FS→UI→APP order
/// - [ ] Use independent scopes when possible (safer)
/// - [ ] Never hold locks across `.await` points
/// - [ ] Document any exceptions with architectural justification
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
        task_tx: mpsc::Sender<TaskResult>,
        action_tx: mpsc::Sender<Action>,
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
    ///
    /// ⚠️  **LOCK ORDER**: When acquiring multiple locks, acquire APP_STATE LAST (after FS→UI)
    pub fn lock_app(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state.lock().unwrap()
    }

    /// Lock UI state (for rendering operations)
    ///
    /// ⚠️  **LOCK ORDER**: When acquiring multiple locks, acquire UI_STATE SECOND (after FS)
    pub fn lock_ui(&self) -> std::sync::MutexGuard<'_, UIState> {
        self.ui_state.lock().unwrap()
    }

    /// Lock FS state (for navigation operations)
    /// 
    /// ⚠️  **LOCK ORDER**: When acquiring multiple locks, acquire FS_STATE FIRST
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
            ui_guard.mark_dirty(Component::All);
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
            ui_guard.mark_dirty(Component::All);
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
        // Read selected index without holding FS lock
        let selected_idx: Option<usize> = {
            let ui_guard = self.lock_ui();
            ui_guard.selected
        };

        let selected_entry = if let Some(idx) = selected_idx {
            let fs_guard = self.lock_fs();
            fs_guard.active_pane().entries.get(idx).cloned()
        } else {
            None
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

    // ─────────────────────────────────────────────────────────────────────────────
    // Create a file with a specific name
    // ─────────────────────────────────────────────────────────────────────────────
    #[instrument(skip(self), fields(operation_type = "fs_create_file"))]
    pub async fn create_file_with_name(
        &self,
        name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Read cwd without holding the lock across await
        let cwd: PathBuf = {
            let fs = self.lock_fs();
            fs.active_pane().cwd.clone()
        };

        let file_path = cwd.join(&name);

        // Do the blocking create off the runtime
        let create_res = task::spawn_blocking({
            let file_path = file_path.clone();
            move || std::fs::File::create(&file_path)
        })
        .in_current_span()
        .await;

        match create_res {
            Ok(Ok(_)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_info(format!("Created file: {name}"));
                }
                // Don’t hold any locks while awaiting
                self.reload_directory().await?;
            }
            Ok(Err(e)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Failed to create file: {e}"));
                }
            }
            Err(join_err) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!(
                        "Create task panicked/joined with error: {join_err}"
                    ));
                }
            }
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Create a directory with a specific name
    // ─────────────────────────────────────────────────────────────────────────────
    #[instrument(skip(self), fields(operation_type = "fs_create_dir"))]
    pub async fn create_directory_with_name(
        &self,
        name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cwd: PathBuf = {
            let fs = self.lock_fs();
            fs.active_pane().cwd.clone()
        };

        let dir_path = cwd.join(&name);

        let mkdir_res = task::spawn_blocking({
            let dir_path = dir_path.clone();
            move || std::fs::create_dir(&dir_path)
        })
        .in_current_span()
        .await;

        match mkdir_res {
            Ok(Ok(_)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_info(format!("Created directory: {name}"));
                }
                self.reload_directory().await?;
            }
            Ok(Err(e)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Failed to create directory: {e}"));
                }
            }
            Err(join_err) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Create dir task error: {join_err}"));
                }
            }
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Delete the currently selected entry
    // ─────────────────────────────────────────────────────────────────────────────
    #[instrument(skip(self), fields(operation_type = "fs_delete"))]
    pub async fn delete_entry(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Snapshot selection without holding multiple locks
        let selected_idx: Option<usize> = {
            let ui = self.lock_ui();
            ui.selected
        };

        let id: Option<u64> = if let Some(idx) = selected_idx {
            let fs = self.lock_fs();
            fs.active_pane().entries.get(idx).map(|e| e.id)
        } else {
            None
        };

        let Some(id) = id else {
            return Ok(());
        };

        // Lock-free metadata lookup → own the PathBuf so blocking task owns it
        let (path, is_dir) = match self.metadata.get_by_id(id) {
            Some(info) => (PathBuf::from(info.path.as_ref()), info.is_dir),
            None => return Ok(()),
        };

        // Do removal off runtime
        let del_res = task::spawn_blocking({
            let path = path.clone();
            move || {
                if is_dir {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                }
            }
        })
        .in_current_span()
        .await;

        match del_res {
            Ok(Ok(())) => {
                // Invalidate metadata entry if present
                self.metadata.invalidate(&path);

                if let Ok(mut ui) = self.ui_state.try_lock() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("item");
                    ui.show_info(format!("Deleted: {name}"));
                }
                self.reload_directory().await?;
            }
            Ok(Err(e)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Failed to delete {}: {e}", path.display()));
                }
            }
            Err(join_err) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Delete task error: {join_err}"));
                }
            }
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Rename the currently selected entry
    // ─────────────────────────────────────────────────────────────────────────────
    #[instrument(skip(self), fields(operation_type = "fs_rename"))]
    pub async fn rename_selected_entry(
        &self,
        new_name: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Snapshot selected index without holding FS+UI together
        let selected_idx: Option<usize> = {
            let ui = self.lock_ui();
            ui.selected
        };

        // Snapshot parent dir and entry id under FS lock
        let (id, parent_dir): (u64, PathBuf) = if let Some(idx) = selected_idx {
            let fs = self.lock_fs();
            let pane = fs.active_pane();
            if let Some(entry) = pane.entries.get(idx) {
                (entry.id, pane.cwd.clone())
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        let Some(obj) = self.metadata.get_by_id(id) else {
            return Ok(());
        };
        let old_path = PathBuf::from(obj.path.as_ref());
        let new_path = old_path
            .parent()
            .unwrap_or(parent_dir.as_path())
            .join(&new_name);

        let ren_res = task::spawn_blocking({
            let old_path = old_path.clone();
            let new_path = new_path.clone();
            move || std::fs::rename(&old_path, &new_path)
        })
        .in_current_span()
        .await;

        match ren_res {
            Ok(Ok(())) => {
                // Invalidate old path to prevent stale metadata
                self.metadata.invalidate(&old_path);

                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_info(format!("Renamed to: {new_name}"));
                }
                self.reload_directory().await?;
            }
            Ok(Err(e)) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Failed to rename: {e}"));
                }
            }
            Err(join_err) => {
                if let Ok(mut ui) = self.ui_state.try_lock() {
                    ui.show_error(format!("Rename task error: {join_err}"));
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
            ui_guard.mark_dirty(Component::All);
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
    ///
    /// - Bounded concurrency (CONCURRENCY)
    /// - Precomputed sort keys (no registry lookups in comparator)
    /// - No locks held across `.await`
    /// - Tracing with ENTER/EXIT + entry_count
    #[instrument(
        skip(self),
        fields(
            operation_type = "directory_scan",
            path = %path.display(),
            entry_count = field::Empty
        )
    )]
    pub async fn scan_directory_and_update_entries(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        const CONCURRENCY: usize = 16;

        // Read show_hidden flag once
        let show_hidden: bool = { self.lock_ui().show_hidden };

        // Snapshot previous ids before clearing entries
        let prev_ids: std::collections::HashSet<u64> = {
            let fs = self.lock_fs();
            fs.active_pane().entries.iter().map(|e| e.id).collect()
        };

        // --- Phase 0: flip UI/FS into "loading" without holding locks across .await ---
        {
            let mut fs = self.lock_fs();
            let pane = fs.active_pane_mut();
            pane.is_loading = true;
            pane.entries.clear();
            // Avoid confusing keyboard nav while loading
            pane.selected = None;
        }
        {
            let mut ui = self.lock_ui();
            ui.mark_dirty(Component::All);
        }

        // --- Phase 1: collect candidate paths (async, sequential read_dir) ---
        let mut rd = tokio::fs::read_dir(path).await?;
        let mut paths: Vec<PathBuf> = Vec::with_capacity(256);

        while let Some(entry) = rd.next_entry().await? {
            let p = entry.path();

            // Skip hidden files unless show_hidden is enabled
            if !show_hidden
                && let Some(name) = p.file_name()
                && name.to_string_lossy().starts_with('.')
            {
                continue;
            }

            paths.push(p);
        }

        // --- Phase 2: build ObjectInfo concurrently, insert into registry, capture sort keys ---
        #[derive(Debug)]
        struct KeyedEntry {
            entry: SortableEntry,
            is_dir: bool,
            name_key: CompactString,
        }

        let metadata: Arc<MetadataManager> = self.metadata.clone();
        let keyed: Vec<KeyedEntry> = stream::iter(paths.into_iter())
            .map(|p: PathBuf| {
                let metadata: Arc<MetadataManager> = metadata.clone();
                async move {
                    match ObjectInfo::from_path_async(&p).await {
                        Ok(info) => {
                            let is_dir = info.is_dir;
                            let name_key = info.name.to_ascii_lowercase();
                            let (_id, sortable) = metadata.insert(info);
                            Ok::<KeyedEntry, ()>(KeyedEntry {
                                entry: sortable,
                                is_dir,
                                name_key,
                            })
                        }
                        Err(e) => {
                            tracing::debug!("Failed to create ObjectInfo for {}: {e}", p.display());
                            Err(())
                        }
                    }
                }
            })
            .buffer_unordered(CONCURRENCY)
            .filter_map(|res| async move { res.ok() })
            .collect()
            .await;

        // --- Phase 3: sort ---
        let mut keyed: Vec<KeyedEntry> = keyed;
        keyed.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name_key.cmp(&b.name_key),
        });

        let entries: Vec<SortableEntry> = keyed.into_iter().map(|k| k.entry).collect();

        // Record final count on the span for tracing/analytics
        Span::current().record("entry_count", entries.len() as i64);

        // Compute new id set for pruning before publishing
        let new_ids: std::collections::HashSet<u64> = entries.iter().map(|e| e.id).collect();
        let dir_id = self.metadata.dir_id_for_path(path);

        // --- Phase 4: publish results back to FS/UI ---
        {
            let mut fs: std::sync::MutexGuard<'_, FSState> = self.lock_fs();
            let pane: &mut PaneState = fs.active_pane_mut();
            pane.set_entries(entries);
            pane.is_loading = false;
            if !pane.entries.is_empty() {
                pane.selected = Some(0);
            }
        }
        {
            let mut ui = self.lock_ui();
            ui.mark_dirty(Component::All);
        }

        // Update dir index and prune stale entries (best-effort, after UI publish)
        {
            self.metadata.update_dir_index(dir_id, &new_ids);
            let stale: std::collections::HashSet<u64> =
                prev_ids.difference(&new_ids).copied().collect();
            if !stale.is_empty() {
                self.metadata.prune_stale_entries(&stale, dir_id);
            }
        }

        tracing::info!("Directory scan completed for: {}", path.display());
        Ok(())
    }
}
