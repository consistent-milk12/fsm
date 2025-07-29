//! fsm-core/src/controller/action_dispatcher/ui_dispatcher.rs
//! UI state and overlay management with tracing

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, instrument, trace};

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Dispatcher for UI controls and overlays.
#[derive(Clone, Debug)]
pub struct UIControlDispatcher {
    /// Provides access to shared UI state.
    state_provider: Arc<dyn StateProvider>,
}

impl UIControlDispatcher {
    /// Create a new UIControlDispatcher.
    #[instrument(level = "debug", skip(state_provider))]
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Toggle help, search, or close overlays.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    fn handle_overlay_toggle(&self, action: &Action) -> Option<DispatchResult> {
        // Determine new overlay and appropriate redraw scope
        let (new_overlay, redraw_flag) = match action {
            Action::ToggleHelp => {
                let current = {
                    let lock = self.state_provider.ui_state();
                    let ui = lock.read().expect("UI lock poisoned");
                    ui.overlay
                };
                let next = if current == UIOverlay::Help {
                    UIOverlay::None
                } else {
                    UIOverlay::Help
                };
                (next, RedrawFlag::Overlay)
            }
            Action::ToggleFileNameSearch => {
                let current = {
                    let lock = self.state_provider.ui_state();
                    let ui = lock.read().expect("UI lock poisoned");
                    ui.overlay
                };
                let next = if current == UIOverlay::FileNameSearch {
                    UIOverlay::None
                } else {
                    UIOverlay::FileNameSearch
                };
                // switching search may require full redraw
                (next, RedrawFlag::All)
            }
            Action::CloseOverlay => (UIOverlay::None, RedrawFlag::All),
            _ => {
                trace!("handle_overlay_toggle: action not relevant");
                return None;
            }
        };

        debug!(?new_overlay, ?redraw_flag, "toggling overlay");
        // Update UI state atomically
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = new_overlay;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                if let UIOverlay::None = new_overlay {
                    ui.input_prompt_type = None;
                }
                ui.request_redraw(redraw_flag);
            }));

        Some(DispatchResult::Continue)
    }

    /// Enter command‑mode prompt overlay.
    #[instrument(level = "debug", skip(self))]
    fn handle_command_mode(&self) -> DispatchResult {
        debug!("entering command mode overlay");
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

    /// Show a specific input prompt overlay.
    #[instrument(level = "debug", skip(self, prompt_type), fields(prompt_type = ?prompt_type))]
    fn handle_input_prompt(&self, prompt_type: &InputPromptType) -> DispatchResult {
        debug!("showing input prompt: {:?}", prompt_type);
        let prompt_type = prompt_type.clone();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.overlay = UIOverlay::Prompt;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                ui.input_prompt_type = Some(prompt_type.clone());
                ui.request_redraw(RedrawFlag::All);
            }));
        DispatchResult::Continue
    }

    /// Update the prompt buffer with user input.
    #[instrument(level = "trace", skip(self, input), fields(input = input))]
    fn handle_input_update(&self, input: &str) -> DispatchResult {
        debug!("updating prompt buffer");
        let buf = input.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.prompt_set(&buf);
                ui.request_redraw(RedrawFlag::Overlay);
            }));
        DispatchResult::Continue
    }

    /// Tick action triggers a main redraw.
    #[instrument(level = "trace", skip(self))]
    fn handle_tick(&self) -> DispatchResult {
        trace!("handling Tick → main redraw");
        self.state_provider.request_redraw(RedrawFlag::Main);
        DispatchResult::Continue
    }

    /// Main async handler entry.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        // Overlay toggles have highest priority here
        if let Some(r) = self.handle_overlay_toggle(&action) {
            return Ok(r);
        }

        // Dispatch based on action variant
        let result = match action {
            Action::EnterCommandMode => self.handle_command_mode(),
            Action::ShowInputPrompt(prompt_type) => self.handle_input_prompt(&prompt_type),
            Action::UpdateInput(input) => self.handle_input_update(&input),
            Action::Tick => self.handle_tick(),
            Action::Quit => {
                info!("received Quit → terminating");
                return Ok(DispatchResult::Terminate);
            }
            _ => {
                trace!(
                    marker = "ACTION_NOT_HANDLED",
                    operation_type = "action_dispatch",
                    "UIControlDispatcher: action not handled"
                );
                DispatchResult::NotHandled
            }
        };

        Ok(result)
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

    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::High
    }

    fn name(&self) -> &'static str {
        "ui_control"
    }
}
