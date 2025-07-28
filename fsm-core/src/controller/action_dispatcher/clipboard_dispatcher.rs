// fsm-core/src/controller/action_dispatcher/clipboard_dispatcher.rs
// Streamlined clipboard operations with optimized async handling

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument};

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::*;

#[derive(Clone)]
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

        let result = {
            let binding = self.state_provider.ui_state();
            let mut ui = binding
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.copy_path(path).await
        };

        match result {
            Ok(_) => self.success("Item copied to clipboard"),
            Err(e) => self.error(&format!("Copy failed: {}", e)),
        }

        Ok(DispatchResult::Continue)
    }

    #[instrument(level = "trace", skip(self))]
    async fn handle_cut(&self, path: PathBuf) -> Result<DispatchResult> {
        debug!("Cutting: {:?}", path);

        let result = {
            let binding = self.state_provider.ui_state();
            let mut ui = binding
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.cut_path(path).await
        };

        match result {
            Ok(_) => self.success("Item cut to clipboard"),
            Err(e) => self.error(&format!("Cut failed: {}", e)),
        }

        Ok(DispatchResult::Continue)
    }

    async fn handle_paste(&self) -> Result<DispatchResult> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        // TODO: Implement actual paste operation
        self.info(&format!("Paste to {}", current_dir.display()));
        Ok(DispatchResult::Continue)
    }

    async fn handle_clear_clipboard(&self) -> Result<DispatchResult> {
        {
            let binding = self.state_provider.ui_state();
            let mut ui = binding
                .write()
                .map_err(|_| anyhow::anyhow!("UI state lock poisoned"))?;
            ui.clipboard.clear().await;
        }

        self.success("Clipboard cleared");
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

    fn error(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
    }

    fn info(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.info(&msg);
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

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "clipboard"
    }
}
