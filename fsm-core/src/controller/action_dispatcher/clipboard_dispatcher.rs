//! Clipboard action dispatcher with high-performance operations

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::controller::Action;
use crate::controller::actions::OperationId;
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

        let result = self.copy_to_clipboard_async(path).await;

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.show_success("Item copied to clipboard");
                    }));
                Ok(DispatchResult::Continue)
            }
            Err(e) => {
                let error_msg = format!("Copy failed: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&error_msg);
                    }));
                Ok(DispatchResult::Continue)
            }
        }
    }

    /// Handle clipboard cut operation  
    #[instrument(level = "trace", skip(self))]
    async fn handle_cut(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Cutting to clipboard: {:?}", path);

        let result = self.cut_to_clipboard_async(path).await;

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.show_success("Item cut to clipboard");
                    }));
                Ok(DispatchResult::Continue)
            }
            Err(e) => {
                let error_msg = format!("Cut failed: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&error_msg);
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
                        ui.move_clipboard_selection_up();
                    }));
            }
            ClipboardNavigation::Down => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.move_clipboard_selection_down();
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

        for path in paths {
            if self.copy_to_clipboard_async(path).await.is_ok() {
                success_count += 1;
            }
        }

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if success_count == total_count {
                    ui.show_success(&format!("{} items copied to clipboard", success_count));
                } else {
                    ui.show_warning(&format!(
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

        for path in paths {
            if self.cut_to_clipboard_async(path).await.is_ok() {
                success_count += 1;
            }
        }

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                if success_count == total_count {
                    ui.show_success(&format!("{} items cut to clipboard", success_count));
                } else {
                    ui.show_warning(&format!(
                        "{}/{} items cut successfully",
                        success_count, total_count
                    ));
                }
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle paste to current directory
    async fn handle_paste_to_current(&self) -> Result<DispatchResult> {
        // Get current directory from filesystem state
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
                ui.show_info(&format!("Paste operation to {} initiated", dest_display));
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle clipboard item selection  
    async fn handle_select_clipboard_item(&self, index: usize) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.selected_clipboard_item_index = index;
                ui.request_redraw(RedrawFlag::Overlay);
            }));
        Ok(DispatchResult::Continue)
    }

    /// Handle remove item from clipboard
    async fn handle_remove_from_clipboard(&self, item_id: u64) -> Result<DispatchResult> {
        let result = self.remove_clipboard_item_async(item_id).await;

        match result {
            Ok(_) => {
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.show_success("Item removed from clipboard");
                    }));
            }
            Err(e) => {
                let error_msg = format!("Failed to remove item: {}", e);
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.show_error(&error_msg);
                    }));
            }
        }

        Ok(DispatchResult::Continue)
    }

    /// Handle clear clipboard
    async fn handle_clear_clipboard(&self) -> Result<DispatchResult> {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.clear_clipboard();
                ui.show_success("Clipboard cleared");
            }));
        Ok(DispatchResult::Continue)
    }

    /// Handle paste specific clipboard item
    async fn handle_paste_clipboard_item(
        &self,
        item_id: u64,
        destination: PathBuf,
    ) -> Result<DispatchResult> {
        debug!("Pasting clipboard item {} to: {:?}", item_id, destination);

        // TODO: Implement actual paste operation for specific item
        let dest_display = destination.display().to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_info(&format!("Pasting item {} to {}", item_id, dest_display));
            }));

        Ok(DispatchResult::Continue)
    }

    /// Handle execute clipboard paste operation
    async fn handle_execute_clipboard_paste(
        &self,
        operation_id: OperationId, // Simplified operation ID
        item_ids: Vec<u64>,
        destination: PathBuf,
    ) -> Result<DispatchResult> {
        debug!(
            "Executing clipboard paste operation {} with {} items",
            operation_id,
            item_ids.len()
        );

        // TODO: Implement actual batch paste operation
        let item_count = item_ids.len();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.show_info(&format!(
                    "Executing paste operation for {} items",
                    item_count
                ));
            }));

        Ok(DispatchResult::Continue)
    }

    /// Async clipboard operations (placeholder implementations)
    async fn copy_to_clipboard_async(&self, _path: PathBuf) -> Result<()> {
        // TODO: Implement actual clipboard copy operation
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(())
    }

    async fn cut_to_clipboard_async(&self, _path: PathBuf) -> Result<()> {
        // TODO: Implement actual clipboard cut operation
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Ok(())
    }

    async fn remove_clipboard_item_async(&self, _item_id: u64) -> Result<()> {
        // TODO: Implement actual clipboard item removal
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        Ok(())
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
            Action::RemoveFromClipboard(item_id) => {
                self.handle_remove_from_clipboard(item_id).await
            }
            Action::PasteClipboardItem {
                item_id,
                destination,
            } => self.handle_paste_clipboard_item(item_id, destination).await,
            Action::ExecuteClipboardPaste {
                operation_id,
                item_ids,
                destination,
            } => {
                self.handle_execute_clipboard_paste(operation_id, item_ids, destination)
                    .await
            }
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
                | Action::RemoveFromClipboard(_)
                | Action::ClearClipboard
                | Action::PasteClipboardItem { .. }
                | Action::ExecuteClipboardPaste { .. }
        )
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "clipboard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
    use std::sync::{Mutex, RwLock};

    // Mock StateProvider for testing
    struct MockStateProvider {
        ui_state: Arc<RwLock<UIState>>,
        fs_state: Arc<Mutex<FSState>>,
        app_state: Arc<Mutex<AppState>>,
    }

    impl StateProvider for MockStateProvider {
        fn ui_state(&self) -> Arc<RwLock<UIState>> {
            self.ui_state.clone()
        }

        fn update_ui_state(&self, update: Box<dyn FnOnce(&mut UIState) + Send>) {
            if let Ok(mut ui) = self.ui_state.write() {
                update(&mut ui);
            }
        }

        fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
            self.fs_state.lock().unwrap()
        }

        fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
            self.app_state.lock().unwrap()
        }

        fn request_redraw(&self, _flag: RedrawFlag) {}
        fn needs_redraw(&self) -> bool {
            false
        }
        fn clear_redraw(&self) {}
    }

    fn create_test_dispatcher() -> ClipboardDispatcher {
        let state_provider = Arc::new(MockStateProvider {
            ui_state: Arc::new(RwLock::new(UIState::default())),
            fs_state: Arc::new(Mutex::new(FSState::default())),
            app_state: Arc::new(Mutex::new(AppState::default())),
        });

        ClipboardDispatcher::new(state_provider)
    }

    #[tokio::test]
    async fn test_clipboard_copy() {
        let mut dispatcher = create_test_dispatcher();

        let path = PathBuf::from("/test/file.txt");
        let result = dispatcher.handle(Action::Copy(path)).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), DispatchResult::Continue));
    }

    #[tokio::test]
    async fn test_clipboard_toggle() {
        let mut dispatcher = create_test_dispatcher();

        let result = dispatcher.handle(Action::ToggleClipboard).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), DispatchResult::Continue));
    }

    #[test]
    fn test_can_handle() {
        let dispatcher = create_test_dispatcher();

        assert!(dispatcher.can_handle(&Action::Copy(PathBuf::from("/test"))));
        assert!(dispatcher.can_handle(&Action::ToggleClipboard));
        assert!(!dispatcher.can_handle(&Action::Quit));
    }
}
