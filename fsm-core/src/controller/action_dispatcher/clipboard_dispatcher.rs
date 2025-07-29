// fsm-core/src/controller/action_dispatcher/clipboard_dispatcher.rs
// Send-safe clipboard operations with proper async handling

use anyhow::Result;
use clipr::{FileOperation, PasteOperation};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument, warn};

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::*;

/// Categorized error types for better user feedback
#[derive(Debug)]
enum PasteErrorType {
    SourceNotFound,
    PermissionDenied,
    InsufficientSpace,
    DestinationExists,
    NetworkError,
    Other(String),
}

impl PasteErrorType {
    /// Categorize error from string message for better user experience
    fn from_error_message(msg: &str) -> Self {
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
    fn user_message(&self) -> &str {
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
    fn detailed_message(&self) -> String {
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

    #[instrument(level = "trace", skip(self))]
    async fn handle_copy(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Copying: {:?}", path);

        // Get clipboard without holding UI lock across await
        let clipboard = {
            let ui_state = self.state_provider.ui_state();
            let ui = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.clipboard.clone()
        };

        // Perform async operation
        match clipboard.add_copy(path).await {
            Ok(_) => self.success("Item copied to clipboard"),
            Err(e) => self.error(&format!("Copy failed: {}", e)),
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(level = "trace", skip(self))]
    async fn handle_cut(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Cutting: {:?}", path);

        let clipboard = {
            let ui_state = self.state_provider.ui_state();
            let ui = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.clipboard.clone()
        };

        match clipboard.add_move(path).await {
            Ok(_) => self.success("Item cut to clipboard"),
            Err(e) => self.error(&format!("Cut failed: {}", e)),
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(level = "trace", skip(self))]
    async fn handle_paste(&self) -> Result<DispatchResult> {
        let (current_dir, clipboard) = {
            let fs = self.state_provider.fs_state();
            let ui_state = self.state_provider.ui_state();
            let ui = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            (fs.active_pane().cwd.clone(), ui.clipboard.clone())
        };

        let items = clipboard.get_all_items().await;
        if items.is_empty() {
            self.error("Clipboard is empty");
            return Ok(DispatchResult::Continue);
        }

        debug!("Pasting {} items to {:?}", items.len(), current_dir);

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
            self.error(&error_msg);
            return Ok(DispatchResult::Continue);
        }

        // Execute paste operations with concurrent processing for better performance
        let mut success_count = 0;
        let mut operation_errors = Vec::new();

        // Process operations in batches for optimal performance
        let batch_size = std::cmp::min(paste_ops.len(), num_cpus::get() * 2);
        let total_batches = (paste_ops.len() + batch_size - 1) / batch_size;
        
        // Show progress for large operations
        if paste_ops.len() > 5 {
            self.success(&format!("Starting paste of {} items...", paste_ops.len()));
        }
        
        for (batch_idx, batch) in paste_ops.chunks(batch_size).enumerate() {
            // Create futures for concurrent execution within batch
            let futures: Vec<_> = batch
                .iter()
                .enumerate()
                .map(|(idx, paste_op)| async move {
                    let global_idx = batch_idx * batch_size + idx;
                    let result = match &paste_op.file_operation {
                        FileOperation::Copy {
                            source,
                            dest,
                            preserve_attrs,
                            ..
                        } => {
                            self.execute_copy_operation(source.as_str(), dest.as_str(), *preserve_attrs)
                                .await
                        }
                        FileOperation::Move {
                            source,
                            dest,
                            atomic_move,
                            ..
                        } => {
                            self.execute_move_operation(source.as_str(), dest.as_str(), *atomic_move)
                                .await
                        }
                    };
                    
                    (global_idx, paste_op, result)
                })
                .collect();

            // Await all operations in this batch concurrently
            let batch_results = futures::future::join_all(futures).await;
            
            // Process results
            for (idx, paste_op, result) in batch_results {
                match result {
                    Ok(_) => {
                        success_count += 1;
                        debug!(
                            "Successfully executed paste operation {}/{}",
                            idx + 1,
                            paste_ops.len()
                        );
                    }
                    Err(e) => {
                        let error_type = PasteErrorType::from_error_message(&e.to_string());
                        let user_msg = format!(
                            "{}: {}",
                            paste_op.file_operation.operation_name(),
                            error_type.user_message()
                        );
                        operation_errors.push(user_msg);
                        
                        // Log detailed error for debugging
                        warn!(
                            "Paste operation failed for {:?}: {}",
                            paste_op.source_path,
                            error_type.detailed_message()
                        );
                    }
                }
            }
            
            // Show intermediate progress for large operations
            if paste_ops.len() > 10 && batch_idx + 1 < total_batches {
                let completed = (batch_idx + 1) * batch_size;
                debug!("Completed batch {}/{} ({} items)", batch_idx + 1, total_batches, completed);
            }
        }

        // Clear move items after successful paste and provide detailed feedback
        if success_count > 0 {
            clipboard.clear_on_paste().await;

            let success_msg = if operation_errors.is_empty() {
                format!("Successfully pasted {} items", success_count)
            } else {
                format!(
                    "Pasted {}/{} items. Errors: {}",
                    success_count,
                    paste_ops.len(),
                    operation_errors.join(", ")
                )
            };

            if operation_errors.is_empty() {
                self.success(&success_msg);
            } else {
                // Show as warning since some operations succeeded
                self.error(&success_msg);
            }
        } else {
            let error_msg = if operation_errors.is_empty() {
                "All paste operations failed".to_string()
            } else {
                format!("Paste failed: {}", operation_errors.join(", "))
            };
            self.error(&error_msg);
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(level = "trace", skip(self))]
    async fn handle_clear_clipboard(&self) -> Result<DispatchResult> {
        let clipboard = {
            let ui_state = self.state_provider.ui_state();
            let ui = ui_state
                .read()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.clipboard.clone()
        };

        // Use optimized bulk clear operation
        match clipboard.clear_all().await {
            Ok(cleared_count) => {
                let msg = if cleared_count == 0 {
                    "Clipboard was already empty".to_string()
                } else {
                    format!("Cleared {} items from clipboard", cleared_count)
                };
                self.success(&msg);
                debug!("Successfully cleared {} items from clipboard", cleared_count);
            }
            Err(e) => {
                self.error(&format!("Failed to clear clipboard: {}", e));
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

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::Copy(path) => self.handle_copy(path).await,
            Action::Cut(path) => self.handle_cut(path).await,
            Action::Paste => self.handle_paste().await,
            Action::ClearClipboard => self.handle_clear_clipboard().await,
            Action::ToggleClipboardOverlay => self.handle_toggle_overlay().await,
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
            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn success(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    /// Execute copy operation with proper error handling and attribute preservation
    async fn execute_copy_operation(
        &self,
        source: &str,
        dest: &str,
        preserve_attrs: bool,
    ) -> Result<()> {
        use std::path::Path;
        use tokio::fs;

        let source_path = Path::new(source);
        let dest_path = Path::new(dest);

        // Check if source exists
        if !source_path.exists() {
            return Err(anyhow::anyhow!("Source file does not exist: {}", source));
        }

        // Ensure destination directory exists
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create destination directory: {}", e))?;
        }

        // Perform the copy operation
        fs::copy(source_path, dest_path)
            .await
            .map_err(|e| anyhow::anyhow!("Copy operation failed: {}", e))?;

        // Preserve attributes if requested
        if preserve_attrs {
            if let Ok(metadata) = fs::metadata(source_path).await {
                let _ = fs::set_permissions(dest_path, metadata.permissions()).await;
            }
        }

        Ok(())
    }

    /// Execute move operation with atomic guarantees when possible
    async fn execute_move_operation(
        &self,
        source: &str,
        dest: &str,
        atomic_move: bool,
    ) -> Result<()> {
        use std::path::Path;
        use tokio::fs;

        let source_path = Path::new(source);
        let dest_path = Path::new(dest);

        // Check if source exists
        if !source_path.exists() {
            return Err(anyhow::anyhow!("Source file does not exist: {}", source));
        }

        // Ensure destination directory exists
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create destination directory: {}", e))?;
        }

        if atomic_move {
            // Try atomic rename first (works on same filesystem)
            match fs::rename(source_path, dest_path).await {
                Ok(_) => Ok(()),
                Err(_) => {
                    // Fallback to copy + delete for cross-filesystem moves
                    fs::copy(source_path, dest_path).await.map_err(|e| {
                        anyhow::anyhow!("Move operation (copy phase) failed: {}", e)
                    })?;

                    fs::remove_file(source_path).await.map_err(|e| {
                        anyhow::anyhow!("Move operation (cleanup phase) failed: {}", e)
                    })?;

                    Ok(())
                }
            }
        } else {
            // Simple rename operation
            fs::rename(source_path, dest_path)
                .await
                .map_err(|e| anyhow::anyhow!("Move operation failed: {}", e))
        }
    }

    fn error(&self, msg: &str) {
        let msg = msg.to_string();
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
            Action::Copy(_)
                | Action::Cut(_)
                | Action::Paste
                | Action::ClearClipboard
                | Action::ToggleClipboardOverlay
                | Action::ClipboardUp
                | Action::ClipboardDown
                | Action::SelectClipboardItem(_)
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
