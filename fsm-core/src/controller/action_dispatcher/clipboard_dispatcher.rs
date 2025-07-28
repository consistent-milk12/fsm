//! Clipboard action dispatcher with high-performance operations

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// High-performance clipboard operations dispatcher
#[derive(Clone)]
pub struct ClipboardDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl ClipboardDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Handle clipboard copy operation
    #[instrument(level = "trace", skip(self))]
    async fn handle_copy(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Copying to clipboard: {:?}", path);

        // Use StateProvider's direct UI state access
        let ui_state = self.state_provider.ui_state();
        let result = {
            let mut ui = ui_state
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.copy_path(path).await
        };

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.success("Item copied to clipboard");
                    }));
                Ok(DispatchResult::Continue)
            }
            Err(e) => {
                let error_msg = format!("Copy failed: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.error(&error_msg);
                    }));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle clipboard cut operation  
    #[instrument(level = "trace", skip(self))]
    async fn handle_cut(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Cutting to clipboard: {:?}", path);

        let ui_state = self.state_provider.ui_state();
        let result = {
            let mut ui = ui_state
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.cut_path(path).await
        };

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.success("Item cut to clipboard");
                    }));
                Ok(DispatchResult::Continue)
            }
            Err(e) => {
                let error_msg = format!("Cut failed: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.error(&error_msg);
                    }));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Toggle clipboard overlay display
    async fn handle_toggle_clipboard(&self) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.toggle_clipboard_overlay();
            }));
        Ok(DispatchResult::Continue)
    }

    /// Navigate clipboard selection
    async fn handle_clipboard_navigation(
        &self,
        direction: ClipboardNavigation,
    ) -> Result<DispatchResult> {
        match direction {
            ClipboardNavigation::Up => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.clipboard_up();
                    }));
            }
            ClipboardNavigation::Down => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.clipboard_down();
                    }));
            }
        }
        Ok(DispatchResult::Continue)
    }

    /// Handle multiple file copy to clipboard
    async fn handle_copy_multiple(&self, paths: Vec<PathBuf>) -> Result<DispatchResult> {
        debug!("Copying {} items to clipboard", paths.len());

        let mut success_count = 0;
        let total_count = paths.len();
        let ui_state = self.state_provider.ui_state();

        for path in paths {
            let result = {
                let mut ui = ui_state
                    .write()
                    .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
                ui.copy_path(path).await
            };

            if result.is_ok() {
                success_count += 1;
            }
        }

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if success_count == total_count {
                    ui.success(&format!("{} items copied to clipboard", success_count));
                } else {
                    ui.warn(&format!(
                        "{}/{} items copied successfully",
                        success_count, total_count
                    ));
                }
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle multiple file cut to clipboard
    async fn handle_cut_multiple(&self, paths: Vec<PathBuf>) -> Result<DispatchResult> {
        debug!("Cutting {} items to clipboard", paths.len());

        let mut success_count = 0;
        let total_count = paths.len();
        let ui_state = self.state_provider.ui_state();

        for path in paths {
            let result = {
                let mut ui = ui_state
                    .write()
                    .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
                ui.cut_path(path).await
            };

            if result.is_ok() {
                success_count += 1;
            }
        }

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if success_count == total_count {
                    ui.success(&format!("{} items cut to clipboard", success_count));
                } else {
                    ui.warn(&format!(
                        "{}/{} items cut successfully",
                        success_count, total_count
                    ));
                }
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle paste to current directory
    async fn handle_paste_to_current(&self) -> Result<DispatchResult> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };
        self.handle_paste_to_directory(current_dir).await
    }

    /// Handle paste to specific directory
    async fn handle_paste_to_directory(&self, destination: PathBuf) -> Result<DispatchResult> {
        debug!("Pasting clipboard to: {:?}", destination);

        // TODO: Implement actual paste operation with file operations
        let dest_display = destination.display().to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.info(&format!("Paste operation to {} initiated", dest_display));
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle clipboard item selection  
    async fn handle_select_clipboard_item(&self, index: usize) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.selected_clipboard_item_idx = index;
                ui.request_redraw(RedrawFlag::Overlay);
            }));
        Ok(DispatchResult::Continue)
    }

    async fn handle_clear_clipboard(&self) -> Result<DispatchResult> {
        let ui_state = self.state_provider.ui_state();

        // Perform async operation without holding the lock
        let result: Result<(), anyhow::Error> = {
            let mut ui = ui_state
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;

            // Call async method on mutable reference
            ui.clipboard.clear().await;
            Ok(())
        };

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.success("Clipboard cleared");
                    }));
            }
            Err(e) => {
                let error_msg = format!("Failed to clear clipboard: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.error(&error_msg);
                    }));
            }
        }

        Ok(DispatchResult::Continue)
    }
    
    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::Copy(path) => self.handle_copy(path).await,
            Action::Cut(path) => self.handle_cut(path).await,
            Action::CopyMultiple(paths) => self.handle_copy_multiple(paths).await,
            Action::CutMultiple(paths) => self.handle_cut_multiple(paths).await,
            Action::ToggleClipboard => self.handle_toggle_clipboard().await,
            Action::ClipboardUp => {
                self.handle_clipboard_navigation(ClipboardNavigation::Up)
                    .await
            }
            Action::ClipboardDown => {
                self.handle_clipboard_navigation(ClipboardNavigation::Down)
                    .await
            }
            Action::ClearClipboard => self.handle_clear_clipboard().await,
            Action::Paste => self.handle_paste_to_current().await,
            Action::PasteToDirectory(dest) => self.handle_paste_to_directory(dest).await,
            Action::SelectClipboardItem(index) => self.handle_select_clipboard_item(index).await,
            _ => Ok(DispatchResult::NotHandled),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ClipboardNavigation {
    Up,
    Down,
}

impl ActionMatcher for ClipboardDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::Copy(_)
                | Action::Cut(_)
                | Action::CopyMultiple(_)
                | Action::CutMultiple(_)
                | Action::Paste
                | Action::PasteToDirectory(_)
                | Action::ToggleClipboard
                | Action::ClipboardUp
                | Action::ClipboardDown
                | Action::SelectClipboardItem(_)
                | Action::ClearClipboard
        )
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "clipboard"
    }
}
