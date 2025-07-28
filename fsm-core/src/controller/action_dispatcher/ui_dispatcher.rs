//! action_dispatchers/ui_control.rs
//! UI state and overlay management

use anyhow::Result;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// UI control dispatcher for overlays and interface state
#[derive(Clone)]
pub struct UIControlDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl UIControlDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Handle overlay toggles efficiently
    fn handle_overlay_toggle(&self, action: &Action) -> Option<DispatchResult> {
        let (new_overlay, redraw_flag) = match action {
            Action::ToggleHelp => {
                let current = {
                    let ui_state = self.state_provider.ui_state();
                    let ui = ui_state.read().expect("UI state lock poisoned");
                    ui.overlay.clone()
                };

                let new_overlay = if current == UIOverlay::Help {
                    UIOverlay::None
                } else {
                    UIOverlay::Help
                };

                (new_overlay, RedrawFlag::Overlay)
            }

            Action::ToggleFileNameSearch => {
                let current = {
                    let ui_state = self.state_provider.ui_state();
                    let ui = ui_state.read().expect("UI state lock poisoned");
                    ui.overlay.clone()
                };

                let new_overlay = if current == UIOverlay::FileNameSearch {
                    UIOverlay::None
                } else {
                    UIOverlay::FileNameSearch
                };
                (new_overlay, RedrawFlag::All)
            }

            Action::CloseOverlay => (UIOverlay::None, RedrawFlag::All),

            _ => return None,
        };

        let overlay_clone = new_overlay.clone();
        let action_clone = action.clone(); // if Action is Clone

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = overlay_clone;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;

                if matches!(action_clone, Action::CloseOverlay) {
                    ui.input_prompt_type = None;
                }

                ui.request_redraw(redraw_flag);
            }));

        Some(DispatchResult::Continue)
    }

    /// Handle input prompts
    fn handle_input_prompt(&self, prompt_type: &InputPromptType) -> DispatchResult {
        let prompt_type = prompt_type.clone();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = UIOverlay::Prompt;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                ui.input_prompt_type = Some(prompt_type);
                ui.request_redraw(RedrawFlag::All);
            }));

        DispatchResult::Continue
    }

    /// Handle input updates
    fn handle_input_update(&self, input: &str) -> DispatchResult {
        let input = input.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.prompt_set(&input);
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

    /// Handle command mode entry
    fn handle_command_mode(&self) -> DispatchResult {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::Prompt;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                ui.input_prompt_type = Some(InputPromptType::Custom("command".to_string()));
                ui.request_redraw(RedrawFlag::All);
            }));

        DispatchResult::Continue
    }
    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        // Fast path for overlay toggles
        if let Some(result) = self.handle_overlay_toggle(&action) {
            return Ok(result);
        }

        match action {
            Action::EnterCommandMode => Ok(self.handle_command_mode()),

            Action::ShowInputPrompt(prompt_type) => Ok(self.handle_input_prompt(&prompt_type)),

            Action::UpdateInput(input) => Ok(self.handle_input_update(&input)),

            Action::Tick => {
                self.state_provider.request_redraw(RedrawFlag::Main);
                Ok(DispatchResult::Continue)
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }
}

impl ActionMatcher for UIControlDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::ToggleHelp
                | Action::ToggleFileNameSearch
                | Action::CloseOverlay
                | Action::EnterCommandMode
                | Action::ShowInputPrompt(_)
                | Action::UpdateInput(_)
                | Action::Tick
        )
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::High // UI responsiveness is important
    }

    fn dynamic_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::Tick => ActionPriority::Low,
            Action::CloseOverlay => ActionPriority::High,
            _ => self.priority(),
        }
    }

    fn name(&self) -> &'static str {
        "ui_control"
    }

    fn can_disable(&self) -> bool {
        false // UI control is essential
    }
}
