//! action_dispatchers/ui_control.rs
//! UI state and overlay management

use anyhow::Result;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use super::{ActionHandler, DispatchResult};
use crate::UIState;
use crate::controller::actions::InputPromptType;
use crate::controller::{Action, state_coordinator::StateCoordinator};
use crate::model::{RedrawFlag, UIOverlay};

/// UI control dispatcher for overlays and interface state
pub struct UIControlDispatcher {
    state: Arc<StateCoordinator>,
}

impl UIControlDispatcher {
    pub fn new(state: Arc<StateCoordinator>) -> Self {
        Self { state }
    }

    /// Handle overlay toggles efficiently
    fn handle_overlay_toggle(&self, action: &Action) -> Option<DispatchResult> {
        let (new_overlay, redraw_flag) = match action {
            Action::ToggleHelp => {
                let current: UIOverlay = {
                    let ui_state: Arc<RwLock<UIState>> = self.state.ui_state();
                    let ui: RwLockReadGuard<'_, UIState> =
                        ui_state.read().expect("UI state lock poisoned");
                    ui.overlay.clone()
                };

                let new_overlay: UIOverlay = if current == UIOverlay::Help {
                    UIOverlay::None
                } else {
                    UIOverlay::Help
                };

                (new_overlay, RedrawFlag::Overlay)
            }

            Action::ToggleFileNameSearch => {
                let current: UIOverlay = {
                    let ui_state: Arc<RwLock<UIState>> = self.state.ui_state();
                    let ui: RwLockReadGuard<'_, UIState> =
                        ui_state.read().expect("UI state lock poisoned");
                    ui.overlay.clone()
                };

                let new_overlay: UIOverlay = if current == UIOverlay::FileNameSearch {
                    UIOverlay::None
                } else {
                    UIOverlay::FileNameSearch
                };
                (new_overlay, RedrawFlag::All)
            }

            Action::CloseOverlay => (UIOverlay::None, RedrawFlag::All),

            _ => return None,
        };

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = new_overlay;
                ui.clear_input();

                if matches!(action, Action::CloseOverlay) {
                    ui.input_prompt_type = None;
                }

                ui.request_redraw(redraw_flag);
            }));

        Some(DispatchResult::Continue)
    }

    /// Handle input prompts
    fn handle_input_prompt(&self, prompt_type: &InputPromptType) -> DispatchResult {
        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = UIOverlay::Prompt;
                ui.clear_input();

                ui.input_prompt_type = Some(prompt_type.clone());
                ui.request_redraw(RedrawFlag::All);
            }));

        DispatchResult::Continue
    }

    /// Handle input updates
    fn handle_input_update(&self, input: String) -> DispatchResult {
        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.set_input(&input);
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

    /// Handle command mode entry
    fn handle_command_mode(&self) -> DispatchResult {
        self.state.update_ui_state(Box::new(|ui: &mut UIState| {
            ui.overlay = UIOverlay::Prompt;
            ui.clear_input();

            ui.input_prompt_type = Some(InputPromptType::Custom("command".to_string()));
            ui.request_redraw(RedrawFlag::All);
        }));

        DispatchResult::Continue
    }
}

impl ActionHandler for UIControlDispatcher {
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

    async fn handle(&mut self, action: &Action) -> Result<DispatchResult> {
        // Fast path for overlay toggles
        if let Some(result) = self.handle_overlay_toggle(&action) {
            return Ok(result);
        }

        match action {
            Action::EnterCommandMode => Ok(self.handle_command_mode()),

            Action::ShowInputPrompt(prompt_type) => Ok(self.handle_input_prompt(prompt_type)),

            Action::UpdateInput(input) => Ok(self.handle_input_update(input.to_string())),

            Action::Tick => {
                self.state.request_redraw(RedrawFlag::Main);

                Ok(DispatchResult::Continue)
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn priority(&self) -> u8 {
        20
    } // High priority for responsive UI

    fn name(&self) -> &'static str {
        "ui_control"
    }
}
