// fsm-core/src/controller/action_dispatcher/ui_dispatcher.rs
// UI state and overlay management

use anyhow::Result;
use std::sync::Arc;

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

#[derive(Clone, Debug)]
pub struct UIControlDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl UIControlDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    fn handle_overlay_toggle(&self, action: &Action) -> Option<DispatchResult> {
        let (new_overlay, redraw_flag) = match action {
            Action::ToggleHelp => {
                let current = {
                    let binding = self.state_provider.ui_state();

                    let ui = binding.read().expect("UI lock poisoned");

                    ui.overlay
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
                    let binding = self.state_provider.ui_state();

                    let ui = binding.read().expect("UI lock poisoned");
                    ui.overlay
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

        let overlay = new_overlay;
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = overlay;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                if matches!(overlay, UIOverlay::None) {
                    ui.input_prompt_type = None;
                }
                ui.request_redraw(redraw_flag);
            }));

        Some(DispatchResult::Continue)
    }

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

    fn handle_input_update(&self, input: &str) -> DispatchResult {
        let input = input.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.prompt_set(&input);
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

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

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
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
            Action::Quit => Ok(DispatchResult::Terminate),
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
                | Action::Quit
        )
    }

    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        match self {
            _ => ActionPriority::High,
        }
    }

    fn name(&self) -> &'static str {
        "ui_control"
    }
}
