// fsm-core/src/controller/action_dispatcher/clipboard_dispatcher.rs
// Send-safe clipboard operations with proper async handling

use anyhow::Result;
use clipr::{ClipBoard, ClipBoardItem, ClipBoardOperation, ClipError, PasteOperation};
use compact_str::CompactString;
use std::path::PathBuf;
use std::sync::{Arc, MutexGuard, RwLock, RwLockReadGuard};
use std::time::{Instant, SystemTime};

use tracing::{debug, info, instrument, warn};

use std::result::Result as StdResult;

use crate::AppState;
use crate::controller::Action;
use crate::controller::actions::{ClipboardOperationType, ClipboardStats, OperationId};
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::*;

/// Categorized error types for better user feedback
#[derive(Debug)]
pub enum PasteErrorType {
    SourceNotFound,
    PermissionDenied,
    InsufficientSpace,
    DestinationExists,
    NetworkError,
    Other(String),
}

impl PasteErrorType {
    /// Categorize error from string message for better user experience
    pub fn from_error_message(msg: &str) -> Self {
        let msg_lower = msg.to_lowercase();

        if msg_lower.contains("not found") || msg_lower.contains("no such file") {
            Self::SourceNotFound
        } else if msg_lower.contains("permission denied") || msg_lower.contains("access denied") {
            Self::PermissionDenied
        } else if msg_lower.contains("no space") || msg_lower.contains("disk full") {
            Self::InsufficientSpace
        } else if msg_lower.contains("already exists") || msg_lower.contains("file exists") {
            Self::DestinationExists
        } else if msg_lower.contains("network") || msg_lower.contains("connection") {
            Self::NetworkError
        } else {
            Self::Other(msg.to_string())
        }
    }

    /// Get user-friendly error description
    pub fn user_message(&self) -> &str {
        match self {
            Self::SourceNotFound => "Source file no longer exists",
            Self::PermissionDenied => "Permission denied - check file access rights",
            Self::InsufficientSpace => "Insufficient disk space",
            Self::DestinationExists => "Destination file already exists",
            Self::NetworkError => "Network connection error",
            Self::Other(_) => "Operation failed",
        }
    }

    /// Get detailed error message for logging
    pub fn detailed_message(&self) -> String {
        match self {
            Self::Other(msg) => msg.clone(),
            _ => self.user_message().to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClipboardDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl ClipboardDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            // Legacy clipboard operations
            Action::Copy(path) => self.handle_copy(path).await,

            Action::Cut(path) => self.handle_cut(path).await,

            Action::Paste => self.handle_paste().await,

            Action::ClearClipboard => self.handle_clear_clipboard().await,

            Action::ToggleClipboard => self.handle_toggle_overlay().await,

            Action::ClipboardUp => self.handle_navigation(true).await,

            Action::ClipboardDown => self.handle_navigation(false).await,

            Action::SelectClipboardItem(index) => {
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.selected_clipboard_item_idx = index;

                        ui.request_redraw(RedrawFlag::Overlay);
                    }));

                Ok(DispatchResult::Continue)
            }

            // clipr advanced operations
            Action::ClipboardAddBatch {
                paths,
                operations,
                operation_id,
            } => self.handle_batch_add(paths, operations, operation_id).await,

            Action::ClipboardPasteBatch {
                item_ids,
                destination,
                ..
            } => self.handle_batch_paste(item_ids, destination).await,

            Action::ClipboardSearch {
                pattern,
                operation_id,
            } => self.handle_clipboard_search(pattern, operation_id).await,

            Action::ClipboardSelectMultiple {
                item_ids,
                operation_id,
            } => self.handle_multi_select(item_ids, operation_id).await,

            Action::ClipboardRemoveItems {
                item_ids,
                operation_id,
            } => self.handle_remove_items(item_ids, operation_id).await,

            Action::ClipboardOptimizedClear { operation_id } => {
                self.handle_optimized_clear(operation_id).await
            }

            Action::ClipboardGetStats { operation_id } => self.handle_get_stats(operation_id).await,

            Action::ClipboardUpdateCache { operation_id } => {
                self.handle_update_cache(operation_id).await
            }

            Action::ClipboardShowSearchResults {
                results,
                pattern,
                operation_id,
            } => {
                self.handle_show_search_results(results, pattern, operation_id)
                    .await
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }

    #[instrument(
        level = "info",
        skip(self),
        err,
        fields(
            marker = "CLIPBOARD_COPY_START",
            operation_type = "clipboard_copy",
            current_path = %path.display(),
            duration_us = tracing::field::Empty,
            cache_hit = false
        )
    )]
    async fn handle_copy(&self, path: PathBuf) -> Result<DispatchResult> {
        let start = Instant::now();
        let span = tracing::Span::current();

        debug!(
            marker = "CLIPBOARD_COPY_START",
            operation_type = "clipboard_copy",
            current_path = %path.display(),
            "Clipbaord copy operation initiated"
        );

        // CRITICAL FIX: Add path validation before clipboard operations
        if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
            warn!(
                marker = "CLIPBOARD_COPY_INVALID_PATH",
                operation_type = "clipboard_copy",
                current_path = %path.display(),
                "Copy operation failed: empty or invalid path"
            );

            self.error("Cannot copy: Invalid or empty file path");

            return Ok(DispatchResult::Continue);
        }

        // Get clipboard without holding UI lock across await
        let clipboard: ClipBoard = {
            let ui_state: Arc<RwLock<UIState>> = self.state_provider.ui_state();
            let ui: RwLockReadGuard<'_, UIState> = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            ui.clipboard.clone()
        };

        // Additional validation: Check if path exists before metadata read
        if !path.exists() {
            warn!(
                marker = "CLIPBOARD_COPY_NOT_FOUND",
                operation_type = "clipboard_copy",
                current_path = %path.display(),
                "Copy operation failed: File does not exist"
            );

            self.error(&format!("Cannot copy: File does exist: {}", path.display()));
        }

        let path_str: PathBuf = path.clone();

        // Perform async operation
        match clipboard.add_copy(path).await {
            Ok(_) => {
                span.record("marker", "CLIPBOARD_COPY_SUCCESS");
                span.record("duration_us", start.elapsed().as_micros());

                info!(
                    marker = "CLIPBOARD_COPY_SUCCESS",
                    operation_type = "clipboard_copy",
                    current_path = %path_str.display(),
                    duration_us = start.elapsed().as_micros(),
                    "Clipbaord copy completed successfully"
                );

                self.success("Item copied to clipboard");
                self.state_provider.update_ui_state(Box::new(|ui| {
                    ui.request_redraw(RedrawFlag::Overlay);
                }));
                self.state_provider.update_ui_state(Box::new(|ui| {
                    ui.request_redraw(RedrawFlag::Overlay);
                }));
            }

            Err(e) => {
                span.record("marker", "CLIPBOARD_COPY_FAILED");
                span.record("duration_us", start.elapsed().as_micros());

                warn!(
                    marker = "CLIPBOARD_COPY_FAILED",
                    operation_type = "clipboard_copy",
                    current_path = %path_str.display(),
                    duration_us = start.elapsed().as_micros(),
                    error = %e,
                    "Clipboard copy operation failed"
                );

                self.error(&format!("Copy failed: {e}"));
            }
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(
        level = "info",
        skip(self),
        err,
        fields(
            marker = "CLIPBOARD_CUT_START",
            operation_type = "clipboard_cut", 
            current_path = %path.display(),
            duration_us = tracing::field::Empty,
            cache_hit = false
        )
    )]
    async fn handle_cut(&self, path: PathBuf) -> Result<DispatchResult> {
        let start = Instant::now();
        let span = tracing::Span::current();

        debug!(
            marker = "CLIPBOARD_CUT_START",
            operation_type = "clipboard_cut",
            current_path = %path.display(),
            "Clipboard cut operation initiated"
        );

        // CRITICAL: Add path validation before clipboard operations
        if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
            warn!(
                marker = "CLIPBOARD_CUT_INVALID_PATH",
                operation_type = "clipboard_cut",
                current_path = %path.display(),
                "Cut operation failed: empty or invalid path"
            );
            self.error("Cannot cut: invalid or empty file path");
            return Ok(DispatchResult::Continue);
        }

        // Additional validation: Check if path exists before metadata read
        if !path.exists() {
            warn!(
                marker = "CLIPBOARD_CUT_NOT_FOUND",
                operation_type = "clipboard_cut",
                current_path = %path.display(),
                "Cut operation failed: file does not exist"
            );
            self.error(&format!(
                "Cannot cut: file does not exist: {}",
                path.display()
            ));
            return Ok(DispatchResult::Continue);
        }

        let clipboard: ClipBoard = {
            let ui_state: Arc<RwLock<UIState>> = self.state_provider.ui_state();

            let ui: RwLockReadGuard<'_, UIState> = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            ui.clipboard.clone()
        };

        match clipboard.add_move(path.clone()).await {
            Ok(_) => {
                span.record("marker", "CLIPBOARD_CUT_SUCCESS");
                span.record("duration_us", start.elapsed().as_micros());

                info!(
                    marker = "CLIPBOARD_CUT_SUCCESS",
                    operation_type = "clipboard_cut",
                    current_path = %path.display(),
                    duration_us = start.elapsed().as_micros(),
                    "Clipboard cut completed successfully"
                );

                self.success("Item cut to clipboard");
            }

            Err(e) => {
                span.record("marker", "CLIPBOARD_CUT_FAILED");
                span.record("duration_us", start.elapsed().as_micros());

                warn!(
                    marker = "CLIPBOARD_CUT_FAILED",
                    operation_type = "clipboard_cut",
                    current_path = %path.display(),
                    duration_us = start.elapsed().as_micros(),
                    error = %e,
                    "Clipboard cut operation failed"
                );

                self.error(&format!("Cut failed: {e}"));
            }
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(
      level = "info",
      skip(self),
      err,
      fields(
          marker = "CLIPBOARD_PASTE_START",
          operation_type = "clipboard_paste",
          current_path = tracing::field::Empty,
          entries_count = tracing::field::Empty,
          duration_us = tracing::field::Empty,
          cache_hit = false
      )
  )]
    async fn handle_paste(&self) -> Result<DispatchResult> {
        let start = Instant::now();
        let span = tracing::Span::current();

        let (current_dir, clipboard) = {
            let fs = self.state_provider.fs_state();
            let ui_state = self.state_provider.ui_state();
            let ui = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            (fs.active_pane().cwd.clone(), ui.clipboard.clone())
        };

        // Record current directory in span
        span.record(
            "current_path",
            tracing::field::display(&current_dir.display()),
        );

        debug!(
            marker = "CLIPBOARD_PASTE_START",
            operation_type = "clipboard_paste",
            current_path = %current_dir.display(),
            "Clipboard paste operation initiated"
        );

        let items = clipboard.get_all_items().await;
        span.record("entries_count", items.len());

        if items.is_empty() {
            warn!(
                marker = "CLIPBOARD_PASTE_EMPTY",
                operation_type = "clipboard_paste",
                current_path = %current_dir.display(),
                entries_count = 0,
                "Paste operation failed: clipboard is empty"
            );
            self.error("Clipboard is empty");
            return Ok(DispatchResult::Continue);
        }

        info!(
            marker = "CLIPBOARD_PASTE_ITEMS_FOUND",
            operation_type = "clipboard_paste",
            current_path = %current_dir.display(),
            entries_count = items.len(),
            "Found clipboard items for paste operation"
        );

        // Create paste operations using clipr's optimized batch processing
        let paste_results = PasteOperation::create_batch(&items, current_dir.clone());

        // Filter successful operations and collect errors
        let mut paste_ops = Vec::new();
        let mut creation_errors = Vec::new();

        for (idx, result) in paste_results.into_iter().enumerate() {
            match result {
                Ok(op) => paste_ops.push(op),
                Err(e) => {
                    creation_errors.push(format!("Item {}: {}", idx + 1, e));
                }
            }
        }

        if paste_ops.is_empty() {
            let error_msg = if creation_errors.is_empty() {
                "No valid paste operations".to_string()
            } else {
                format!("All operations failed: {}", creation_errors.join(", "))
            };

            span.record("marker", "CLIPBOARD_PASTE_NO_VALID_OPS");
            span.record("duration_us", start.elapsed().as_micros());

            warn!(
                marker = "CLIPBOARD_PASTE_NO_VALID_OPS",
                operation_type = "clipboard_paste",
                current_path = %current_dir.display(),
                entries_count = items.len(),
                duration_us = start.elapsed().as_micros(),
                error = %error_msg,
                "No valid paste operations could be created"
            );

            self.error(&error_msg);
            return Ok(DispatchResult::Continue);
        }

        info!(
            marker = "CLIPBOARD_PASTE_OPERATIONS_CREATED",
            operation_type = "clipboard_paste",
            current_path = %current_dir.display(),
            entries_count = paste_ops.len(),
            "Created paste operations for execution"
        );

        // Process operations in batches for optimal performance
        let batch_size = std::cmp::min(paste_ops.len(), num_cpus::get() * 2);
        let total_batches = paste_ops.len().div_ceil(batch_size);

        // Show progress for large operations
        if paste_ops.len() > 5 {
            info!(
                marker = "CLIPBOARD_PASTE_BATCH_START",
                operation_type = "clipboard_paste",
                current_path = %current_dir.display(),
                entries_count = paste_ops.len(),
                total_batches = total_batches,
                "Starting batch paste operation"
            );
            self.success(&format!("Starting paste of {} items...", paste_ops.len()));
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(level = "trace", skip(self))]
    async fn handle_clear_clipboard(&self) -> Result<DispatchResult> {
        let clipboard: ClipBoard = {
            let ui_state: Arc<RwLock<UIState>> = self.state_provider.ui_state();

            let ui: RwLockReadGuard<'_, UIState> = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            ui.clipboard.clone()
        };

        // Use optimized bulk clear operation
        match clipboard.clear_all().await {
            Ok(cleared_count) => {
                let msg: String = if cleared_count == 0 {
                    "Clipboard was already empty".to_string()
                } else {
                    format!("Cleared {cleared_count} items from clipboard")
                };

                self.success(&msg);

                debug!(
                    marker = "CLIPBOARD_CLEAR_SUCCESS",
                    operation_type = "clipboard_clear",
                    "Successfully cleared {} items from clipboard",
                    cleared_count
                );
            }

            Err(e) => {
                self.error(&format!("Failed to clear clipboard: {e}"));
            }
        }

        Ok(DispatchResult::Continue)
    }

    async fn handle_toggle_overlay(&self) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.toggle_clipboard_overlay();
            }));

        Ok(DispatchResult::Continue)
    }

    async fn handle_navigation(&self, up: bool) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if up {
                    ui.clipboard_up();
                } else {
                    ui.clipboard_down();
                }
            }));

        Ok(DispatchResult::Continue)
    }

    // ===== clipr Advanced Handler Methods =====

    /// Handle batch add operations using clipr's parallel processing
    #[instrument(level = "info", skip(self, paths), fields(operation_id = %operation_id))]
    async fn handle_batch_add(
        &self,
        paths: Vec<PathBuf>,
        operation_type: ClipboardOperationType,
        operation_id: OperationId,
    ) -> Result<DispatchResult> {
        // Get clipboard from AppState (not UIState)
        let clipboard: Arc<ClipBoard> = self.state_provider.app_state().clipboard.clone();

        info!(
            marker = "CLIPBOARD_BATCH_ADD_START",
            operation_type = "clipboard_batch_add",
            items_count = paths.len(),
            operation_id = %operation_id,
            "Starting batch add operation"
        );

        // Use clipr's optimized batch_add_parallel
        let operation_enum: ClipBoardOperation = match operation_type {
            ClipboardOperationType::Copy => ClipBoardOperation::Copy,
            ClipboardOperationType::Move => ClipBoardOperation::Move,
        };

        let results: Vec<StdResult<u64, ClipError>> = clipboard
            .add_batch_parallel(paths.clone(), operation_enum)
            .await;

        let mut success_count: i32 = 0;
        let mut failed_paths: Vec<PathBuf> = Vec::new();

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(_item_id) => {
                    success_count += 1;
                }

                Err(e) => {
                    let failed_path = paths[i].clone();
                    failed_paths.push(failed_path);

                    warn!(
                        marker = "CLIPBOARD_BATCH_ADD_ITEM_FAILED",
                        operation_type = "clipboard_batch_add",
                        current_path = %paths[i].display(),
                        error = %e,
                        "Failed to add item to clipboard"
                    );
                }
            }
        }

        if success_count > 0 {
            info!(
                marker = "CLIPBOARD_BATCH_ADD_SUCCESS",
                operation_type = "clipboard_batch_add",
                entries_count = success_count,
                "Successfully added items to clipboard"
            );
        }

        if !failed_paths.is_empty() {
            self.error(&format!(
                "Failed to add {} items to clipboard",
                failed_paths.len()
            ));
        } else {
            self.success(&format!("Added {success_count} items to clipboard"));
        }

        Ok(DispatchResult::Continue)
    }

    /// Handle batch paste operations using clipr's optimized paste
    #[instrument(level = "info", skip(self, item_ids))]
    async fn handle_batch_paste(
        &mut self,
        item_ids: Vec<u64>,
        destination: PathBuf,
    ) -> Result<DispatchResult> {
        info!(
            marker = "CLIPBOARD_PASTE_BATCH_START",
            operation_type = "clipboard_batch_paste",
            entries_count = item_ids.len(),
            target_path = %destination.display(),
            "Starting batch paste operation"
        );

        let clipboard: &Arc<ClipBoard> = &self.state_provider.app_state().clipboard.clone();
        let mut success_count: i32 = 0;
        let mut failed_operations: Vec<CompactString> = Vec::new();

        for item_id in item_ids {
            // Get paste operation from clipr
            match clipboard
                .get_paste_operation(item_id, destination.clone())
                .await
            {
                Ok(paste_op) => {
                    // Execute the file system operation based on the paste operation details
                    match self.execute_file_operation(&paste_op).await {
                        Ok(_) => {
                            success_count += 1;
                            info!(
                                marker = "CLIPBOARD_PASTE_ITEM_SUCCESS",
                                operation_type = "clipboard_paste_item",
                                current_path = %paste_op.source_path,
                                target_path = %paste_op.destination_path,
                                "Paste operation completed successfully"
                            );
                        }
                        Err(e) => {
                            failed_operations.push(paste_op.source_path.clone());
                            warn!(
                                marker = "CLIPBOARD_PASTE_ITEM_FAILED",
                                operation_type = "clipboard_paste_item",
                                current_path = %paste_op.source_path,
                                target_path = %paste_op.destination_path,
                                error = %e,
                                "Paste operation failed"
                            );
                        }
                    }
                }

                Err(e) => {
                    failed_operations.push(format!("item_{item_id}").into());

                    warn!(
                        marker = "CLIPBOARD_GET_PASTE_OPERATION_FAILED",
                        operation_type = "clipboard_get_paste_operation",
                        entries_count = item_id,
                        error = %e,
                        "Failed to get paste operation for item"
                    );
                }
            }
        }

        // Update UI and provide feedback
        if success_count > 0 {
            self.success(&format!("Successfully pasted {success_count} items"));
        }

        if !failed_operations.is_empty() {
            self.error(&format!(
                "Failed to paste {} items",
                failed_operations.len()
            ));
        }

        info!(
            marker = "CLIPBOARD_PASTE_BATCH_COMPLETE",
            operation_type = "clipboard_batch_paste",
            entries_count = success_count,
            "Batch paste operation completed"
        );

        Ok(DispatchResult::Continue)
    }

    /// Handle SIMD-accelerated clipboard search using clipr
    #[instrument(level = "info", skip(self, pattern), fields(operation_id = %operation_id))]
    async fn handle_clipboard_search(
        &self,
        pattern: String,
        operation_id: OperationId,
    ) -> Result<DispatchResult> {
        info!(
            marker = "CLIPBOARD_SEARCH_START",
            operation_type = "clipboard_search",
            operation_id = %operation_id,
            pattern = %pattern,
            "Starting SIMD clipboard search"
        );

        let start: Instant = Instant::now();

        let clipboard: Arc<ClipBoard> = self.state_provider.app_state().clipboard.clone();

        // Use clipr's SIMD-accelerated find_by_pattern
        let results: Vec<ClipBoardItem> = clipboard.find_by_pattern(&pattern).await;
        let result_count: usize = results.len();

        let result_ids: Vec<u64> = results.iter().map(|item: &ClipBoardItem| item.id).collect();

        info!(
            marker = "CLIPBOARD_SEARCH_COMPLETE",
            operation_type = "clipboard_search",
            operation_id = %operation_id,
            pattern = %pattern,
            results_count = results.len(),
            duration_us = start.elapsed().as_micros(),
            "Clipboard search completed"
        );

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.update_clipboard_search_results(result_ids, 0);
            }));

        info!(
            marker = "CLIPBOARD_SEARCH_COMPLETE",
            operation_type = "clipboard_search",
            entries_count = result_count,
            "Clipboard search completed successfully"
        );

        self.success(&format!("Found {} matching items", results.len()));

        Ok(DispatchResult::Continue)
    }

    async fn execute_file_operation(&self, paste_op: &PasteOperation) -> Result<()> {
        use tokio::fs as TokioFs;

        let source: PathBuf = PathBuf::from(paste_op.source_path.as_str());
        let dest: PathBuf = PathBuf::from(paste_op.destination_path.as_str());

        match paste_op.operation_type {
            ClipBoardOperation::Copy => {
                // Peform file copy
                TokioFs::copy(&source, &dest)
                    .await
                    .map_err(|e| anyhow::Error::new(e).context("Failed to copy file"))?;
            }

            ClipBoardOperation::Move => {
                // Perform file move
                TokioFs::rename(&source, &dest)
                    .await
                    .map_err(|e| anyhow::Error::new(e).context("Failed to move file"))?;
            }
        }

        Ok(())
    }

    /// Handle multi-selection using clipr items
    #[instrument(level = "debug", skip(self, item_ids), fields(operation_id = %operation_id))]
    async fn handle_multi_select(
        &self,
        item_ids: Vec<u64>,
        operation_id: crate::controller::actions::OperationId,
    ) -> Result<DispatchResult> {
        debug!(
            marker = "CLIPBOARD_MULTI_SELECT_START",
            operation_type = "clipboard_multi_select",
            operation_id = %operation_id,
            items_count = item_ids.len(),
            "Processing multi-selection"
        );

        let item_ids_len: usize = item_ids.len();

        // Update UI state with multi-selection
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.clear_clipboard_selection();
                for item_id in item_ids.iter() {
                    ui.select_clipboard_item(*item_id);
                }
            }));

        self.success(&format!("Selected {item_ids_len} items"));

        Ok(DispatchResult::Continue)
    }

    /// Handle removing specific items using clipr
    #[instrument(level = "info", skip(self, item_ids), fields(operation_id = %operation_id))]
    async fn handle_remove_items(
        &self,
        item_ids: Vec<u64>,
        operation_id: OperationId,
    ) -> Result<DispatchResult> {
        info!(
            marker = "CLIPBOARD_REMOVE_ITEM_START",
            operation_type = "clipboard_remove_items",
            entries_count = item_ids.len(),
            "Starting remove items operation"
        );

        let start: Instant = Instant::now();

        let clipboard: Arc<ClipBoard> = self.state_provider.app_state().clipboard.clone();

        info!(
            marker = "CLIPBOARD_REMOVE_ITEMS_START",
            operation_type = "clipboard_remove_items",
            operation_id = %operation_id,
            items_count = item_ids.len(),
            "Starting item removal"
        );

        let mut removed_count: i32 = 0;
        let mut failed_removals = Vec::new();

        // Use clipr's remove_item for each ID
        for item_id in &item_ids {
            match clipboard.remove_item(*item_id).await {
                Ok(removed_item) => {
                    removed_count += 1;

                    info!(
                        marker = "CLIPBOARD_ITEM_REMOVED",
                        operation_type = "clipboard_remove_item",
                        entries_count = *item_id,
                        current_path = %removed_item.source_path,
                        "Item removed successfully"
                    );
                }

                Err(ClipError::ItemNotFound(_)) => {
                    failed_removals.push(*item_id);

                    warn!(
                        marker = "CLIPBOARD_ITEM_NOT_FOUND",
                        operation_type = "clipboard_remove_item",
                        entries_count = *item_id,
                        "Item not found for removal"
                    );
                }

                Err(e) => {
                    failed_removals.push(*item_id);

                    warn!(
                        marker = "CLIPBOARD_ITEM_REMOVE_FAILED",
                        operation_type = "clipboard_remove_item",
                        entries_count = *item_id,
                        error = %e,
                        "Failed to remove item"
                    );
                }
            }
        }

        let removed_ids = item_ids.clone();

        // Update UI to remove items from selection
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                for item_id in removed_ids {
                    ui.deselect_clipboard_item(item_id);
                }
            }));

        {
            let app_state = self.state_provider.app_state();
            let current_stats = clipboard.stats();

            if let Ok(mut stats) = app_state.clipboard_metrics.write() {
                *stats = current_stats.into();
            }
        }

        info!(
            marker = "CLIPBOARD_REMOVE_ITEMS_COMPLETE",
            operation_type = "clipboard_remove_items",
            operation_id = %operation_id,
            duration_us = start.elapsed().as_micros(),
            "Item removal completed"
        );

        if removed_count > 0 {
            self.success(&format!("Removed {removed_count} items"));
        }

        if !failed_removals.is_empty() {
            self.error(&format!(
                "Failed to remove {} items from clipboard",
                failed_removals.len()
            ));
        }

        if removed_count == 0 && !failed_removals.is_empty() {
            self.error("No items were removed from clipboard");
        }

        Ok(DispatchResult::Continue)
    }

    /// Handle optimized clear using clipr's clear_all
    #[instrument(level = "info", skip(self), fields(operation_id = %operation_id))]
    async fn handle_optimized_clear(&self, operation_id: OperationId) -> Result<DispatchResult> {
        let start: Instant = std::time::Instant::now();

        let clipboard: Arc<ClipBoard> = {
            let app_state: MutexGuard<'_, AppState> = self.state_provider.app_state();
            app_state.clipboard.clone()
        };

        info!(
            marker = "CLIPBOARD_OPTIMIZED_CLEAR_START",
            operation_type = "clipboard_optimized_clear",
            operation_id = %operation_id,
            "Starting optimized clear"
        );

        match clipboard.clear_all().await {
            Ok(cleared_count) => {
                info!(
                    marker = "CLIPBOARD_OPTIMIZED_CLEAR_COMPLETE",
                    operation_type = "clipboard_optimized_clear",
                    operation_id = %operation_id,
                    cleared_count = cleared_count,
                    duration_us = start.elapsed().as_micros(),
                    "Optimized clear completed"
                );

                self.success(&format!("Cleared {cleared_count} items"));

                // Update UI state
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.clear_clipboard_selection();
                        ui.request_redraw(crate::model::ui_state::RedrawFlag::All);
                    }));
            }

            Err(e) => {
                warn!(
                    marker = "CLIPBOARD_OPTIMIZED_CLEAR_FAILED",
                    operation_type = "clipboard_optimized_clear",
                    operation_id = %operation_id,
                    error = %e,
                    duration_us = start.elapsed().as_micros(),
                    "Optimized clear failed"
                );
                self.error(&format!("Clear failed: {e}"));
            }
        }

        Ok(DispatchResult::Continue)
    }

    /// Handle getting clipboard statistics using clipr
    #[instrument(level = "debug", skip(self), fields(operation_id = %operation_id))]
    async fn handle_get_stats(
        &self,
        operation_id: crate::controller::actions::OperationId,
    ) -> Result<DispatchResult> {
        let clipboard = {
            let app_state = self.state_provider.app_state();
            app_state.clipboard.clone()
        };

        debug!(
            marker = "CLIPBOARD_GET_STATS_START",
            operation_type = "clipboard_get_stats",
            operation_id = %operation_id,
            "Getting clipboard statistics"
        );

        let stats = clipboard.stats();

        // Update AppState with fresh statistics
        {
            let app_state = self.state_provider.app_state();
            let clipr_stats = ClipboardStats {
                total_items: stats.total_items as u64,
                copy_items: stats.copy_items as u64,
                move_items: stats.move_items as u64,
                total_size_bytes: stats.total_size,
                cache_hit_ratio: stats.cache_hit_rate as f32,
                last_updated: SystemTime::now(),
            };
            app_state.update_clipboard_metrics(clipr_stats);
        }

        debug!(
            marker = "CLIPBOARD_GET_STATS_COMPLETE",
            operation_type = "clipboard_get_stats",
            operation_id = %operation_id,
            total_items = stats.total_items,
            "Statistics retrieved"
        );

        Ok(DispatchResult::Continue)
    }

    /// Handle cache update operations
    #[instrument(level = "debug", skip(self), fields(operation_id = %operation_id))]
    async fn handle_update_cache(
        &self,
        operation_id: crate::controller::actions::OperationId,
    ) -> Result<DispatchResult> {
        debug!(
            marker = "CLIPBOARD_UPDATE_CACHE_START",
            operation_type = "clipboard_update_cache",
            operation_id = %operation_id,
            "Updating clipboard cache"
        );

        // Trigger cache refresh and UI update
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.request_redraw(crate::model::ui_state::RedrawFlag::Overlay);
            }));

        debug!(
            marker = "CLIPBOARD_UPDATE_CACHE_COMPLETE",
            operation_type = "clipboard_update_cache",
            operation_id = %operation_id,
            "Cache update completed"
        );

        Ok(DispatchResult::Continue)
    }

    /// Handle showing search results in UI
    #[instrument(level = "info", skip(self, results, pattern), fields(operation_id = %operation_id))]
    async fn handle_show_search_results(
        &self,
        results: Vec<u64>,
        pattern: String,
        operation_id: crate::controller::actions::OperationId,
    ) -> Result<DispatchResult> {
        info!(
            marker = "CLIPBOARD_SHOW_SEARCH_RESULTS_START",
            operation_type = "clipboard_show_search_results",
            operation_id = %operation_id,
            results_count = results.len(),
            pattern = %pattern,
            "Showing search results"
        );

        let results_len: usize = results.len();

        // Update UI with search results and pattern
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.update_clipboard_search_results(results, 0);
                ui.request_redraw(crate::model::ui_state::RedrawFlag::Overlay);
            }));

        self.success(&format!(
            "Showing {results_len} search results for '{pattern}'"
        ));

        Ok(DispatchResult::Continue)
    }

    fn success(&self, msg: &str) {
        let msg: String = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    fn error(&self, msg: &str) {
        let msg: String = msg.to_string();

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
    }
}

impl ActionMatcher for ClipboardDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            // Legacy clipboard operations
            Action::Copy(_)
                | Action::Cut(_)
                | Action::Paste
                | Action::ClearClipboard
                | Action::ToggleClipboard
                | Action::ClipboardUp
                | Action::ClipboardDown
                | Action::SelectClipboardItem(_)
                // clipr advanced operations
                | Action::ClipboardAddBatch { .. }
                | Action::ClipboardPasteBatch { .. }
                | Action::ClipboardSearch { .. }
                | Action::ClipboardSelectMultiple { .. }
                | Action::ClipboardRemoveItems { .. }
                | Action::ClipboardOptimizedClear { .. }
                | Action::ClipboardGetStats { .. }
                | Action::ClipboardUpdateCache { .. }
                | Action::ClipboardShowSearchResults { .. }
        )
    }

    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "clipboard"
    }
}
