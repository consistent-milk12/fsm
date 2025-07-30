//! Enhanced UI dispatcher with render system integration and clean architecture compliance
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{Span, info, instrument, trace, warn};

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Enhanced dispatcher for UI controls, overlays, and render system integration
#[derive(Clone, Debug)]
pub struct UIControlDispatcher {
    state_provider: Arc<dyn StateProvider>,
    frame_metrics: Arc<std::sync::Mutex<FrameMetrics>>,
}

#[derive(Debug, Default)]
struct FrameMetrics {
    total_frames: u64,
    render_errors: u64,
    immediate_renders: u64,
    last_frame_time: Option<Instant>,
}

impl UIControlDispatcher {
    #[instrument(level = "debug", skip(state_provider))]
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self {
            state_provider,
            frame_metrics: Arc::new(std::sync::Mutex::new(FrameMetrics::default())),
        }
    }

    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        // Handle render system actions first (highest priority)
        if let Some(result) = self.handle_render_actions(&action).await? {
            return Ok(result);
        }

        // Handle overlay toggles
        if let Some(r) = self.handle_overlay_toggle(&action) {
            return Ok(r);
        }

        // Handle other UI control actions
        let result = match action {
            Action::EnterCommandMode => self.handle_command_mode(),

            Action::ExitCommandMode => self.handle_exit_command_mode(),

            Action::CreateFile => self.handle_input_prompt(&InputPromptType::CreateFile),

            Action::CreateDirectory => self.handle_input_prompt(&InputPromptType::CreateDirectory),

            Action::ShowInputPrompt(prompt_type) => self.handle_input_prompt(&prompt_type),

            Action::UpdateInput(input) => self.handle_input_update(&input),

            Action::Tick => self.handle_tick(),

            Action::Quit => {
                info!(
                    marker = "QUIT_ACTION_PROCESSED",
                    operation_type = "application_control",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    "Application quit signal received"
                );

                return Ok(DispatchResult::Terminate);
            }

            _ => {
                trace!(
                    marker = "ACTION_NOT_HANDLED",
                    operation_type = "action_dispatch",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    action = action.description(),
                    "UIControlDispatcher: action not handled"
                );
                DispatchResult::NotHandled
            }
        };

        Ok(result)
    }

    /// Enhanced render system integration with proper TSV tracing
    #[instrument(
          name = "handle_render_actions",
          skip(self),
          fields(
              marker = "ACTION_DISPATCH_START",
              operation_type = "render_system",
              frame_count = tracing::field::Empty,
              trigger_source = tracing::field::Empty,
              duration_us = tracing::field::Empty,
              cache_hit = false,
              area_width = "NULL",
              area_height = "NULL"
          )
      )]
    async fn handle_render_actions(&self, action: &Action) -> Result<Option<DispatchResult>> {
        let span = Span::current();
        let start_time = Instant::now();

        match action {
            Action::TriggerImmediateRender {
                trigger_source,
                frame_count,
                timestamp: _,
            } => {
                span.record("frame_count", *frame_count);
                span.record("trigger_source", format!("{trigger_source:?}"));

                info!(
                    marker = "RENDER_NOTIFY_RECEIVED",
                    operation_type = "immediate_render",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    frame_count = *frame_count,
                    trigger_source = format!("{:?}", trigger_source),
                    "Immediate render notification received"
                );

                // Update frame metrics
                if let Ok(mut metrics) = self.frame_metrics.lock() {
                    metrics.immediate_renders += 1;
                    metrics.total_frames = (*frame_count).max(metrics.total_frames);
                    metrics.last_frame_time = Some(Instant::now());
                }

                // Handle notification polling and state update
                info!(
                    marker = "IMMEDIATE_RENDER_START",
                    operation_type = "immediate_render",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    frame_count = *frame_count,
                    "Processing immediate render notification"
                );

                // Update UI state for notification polling (clean architecture compliance)
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
                        ui.poll_notification();
                    }));

                // Request full redraw through StateProvider interface
                self.state_provider.request_redraw(RedrawFlag::All);

                let duration = start_time.elapsed();
                span.record("duration_us", duration.as_micros());

                info!(
                    marker = "IMMEDIATE_RENDER_COMPLETE",
                    operation_type = "immediate_render",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = duration.as_micros(),
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    frame_count = *frame_count,
                    "Immediate render notification processed 
  successfully"
                );

                Ok(Some(DispatchResult::Continue))
            }

            Action::HandleRenderError {
                error,
                frame_count,
                error_source,
                recovery_action,
                ..
            } => {
                // Update error metrics
                if let Ok(mut metrics) = self.frame_metrics.lock() {
                    metrics.render_errors += 1;
                }

                warn!(
                    marker = "RENDER_ERROR_RECEIVED",
                    operation_type = "render_error_handling",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = "NULL",
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    frame_count = *frame_count,
                    error_source = error_source,
                    error = error,
                    "Render error received for handling"
                );

                // Update UI state with error notification
                let error_msg = error.clone();
                let frame_num = *frame_count;
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.error(format!("Render error (frame {frame_num}): {error_msg}"));
                    }));

                // Execute recovery action if provided
                if let Some(recovery) = recovery_action {
                    info!(
                        marker = "RENDER_ERROR_RECOVERY_START",
                        operation_type = "render_error_recovery",
                        current_path = "NULL",
                        target_path = "NULL",
                        entries_count = "NULL",
                        selected_index = "NULL",
                        duration_us = "NULL",
                        cache_hit = false,
                        area_width = "NULL",
                        area_height = "NULL",
                        recovery_action = recovery.description(),
                        "Executing render error recovery action"
                    );

                    // Recursive dispatch for recovery action using Box::pin to avoid infinite sized future
                    let mut dispatcher_clone = self.clone();
                    let recovery_action = recovery.clone();
                    return Ok(Some(
                        Box::pin(dispatcher_clone.handle(*recovery_action)).await?,
                    ));
                }

                let duration = start_time.elapsed();
                warn!(
                    marker = "RENDER_NOTIFY_FAILED",
                    operation_type = "render_error_handling",
                    current_path = "NULL",
                    target_path = "NULL",
                    entries_count = "NULL",
                    selected_index = "NULL",
                    duration_us = duration.as_micros(),
                    cache_hit = false,
                    area_width = "NULL",
                    area_height = "NULL",
                    frame_count = *frame_count,
                    "Render error handled without recovery"
                );

                Ok(Some(DispatchResult::Continue))
            }

            _ => Ok(None),
        }
    }

    /// Enhanced overlay toggle with proper TSV tracing
    #[instrument(level = "debug", skip(self, action), fields(action = 
  ?action))]
    fn handle_overlay_toggle(&self, action: &Action) -> Option<DispatchResult> {
        let (new_overlay, redraw_flag) = match action {
            Action::ToggleHelp => {
                let current = self.get_current_overlay();
                let next = if current == UIOverlay::Help {
                    UIOverlay::None
                } else {
                    UIOverlay::Help
                };
                (next, RedrawFlag::Overlay)
            }
            Action::ToggleFileNameSearch => {
                let current = self.get_current_overlay();
                let next = if current == UIOverlay::FileNameSearch {
                    UIOverlay::None
                } else {
                    UIOverlay::FileNameSearch
                };
                (next, RedrawFlag::All)
            }
            Action::ToggleContentSearch => {
                let current = self.get_current_overlay();
                let next = if current == UIOverlay::ContentSearch {
                    UIOverlay::None
                } else {
                    UIOverlay::ContentSearch
                };
                (next, RedrawFlag::All)
            }
            Action::ToggleSystemMonitor => {
                let current = self.get_current_overlay();
                let next = if current == UIOverlay::SystemMonitor {
                    UIOverlay::None
                } else {
                    UIOverlay::SystemMonitor
                };
                (next, RedrawFlag::Overlay)
            }
            Action::ToggleClipboard => {
                let current = self.get_current_overlay();
                let next = if current == UIOverlay::ClipBoard {
                    UIOverlay::None
                } else {
                    UIOverlay::ClipBoard
                };
                (next, RedrawFlag::Overlay)
            }
            Action::CloseOverlay => (UIOverlay::None, RedrawFlag::All),
            _ => return None,
        };

        info!(
            marker = "OVERLAY_TOGGLE",
            operation_type = "ui_control",
            current_path = "NULL",
            target_path = "NULL",
            entries_count = "NULL",
            selected_index = "NULL",
            duration_us = "NULL",
            cache_hit = false,
            area_width = "NULL",
            area_height = "NULL",
            overlay = format!("{:?}", new_overlay),
            redraw_scope = format!("{:?}", redraw_flag),
            "Overlay toggle processed"
        );

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

    fn get_current_overlay(&self) -> UIOverlay {
        let lock = self.state_provider.ui_state();
        let ui = lock.read().expect("UI lock poisoned");
        ui.overlay
    }

    #[instrument(level = "debug", skip(self))]
    fn handle_command_mode(&self) -> DispatchResult {
        info!(
            marker = "COMMAND_MODE_ENTER",
            operation_type = "ui_control",
            current_path = "NULL",
            target_path = "NULL",
            entries_count = "NULL",
            selected_index = "NULL",
            duration_us = "NULL",
            cache_hit = false,
            area_width = "NULL",
            area_height = "NULL",
            "Entering command mode"
        );

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

    fn handle_exit_command_mode(&self) -> DispatchResult {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::None;
                ui.prompt_buffer.clear();
                ui.prompt_cursor = 0;
                ui.input_prompt_type = None;
                ui.request_redraw(RedrawFlag::All);
            }));

        DispatchResult::Continue
    }

    #[instrument(level = "debug", skip(self, prompt_type), 
  fields(prompt_type = ?prompt_type))]
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

    #[instrument(level = "trace", skip(self, input), fields(input = 
  input))]
    fn handle_input_update(&self, input: &str) -> DispatchResult {
        let buf = input.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.prompt_set(&buf);
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

    #[instrument(level = "trace", skip(self))]
    fn handle_tick(&self) -> DispatchResult {
        trace!(
            marker = "TICK_MAIN_REDRAW",
            operation_type = "ui_control",
            current_path = "NULL",
            target_path = "NULL",
            entries_count = "NULL",
            selected_index = "NULL",
            duration_us = "NULL",
            cache_hit = false,
            area_width = "NULL",
            area_height = "NULL",
            "Tick action processed"
        );

        self.state_provider.request_redraw(RedrawFlag::Main);
        DispatchResult::Continue
    }
}

impl ActionMatcher for UIControlDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::ToggleHelp
                | Action::ToggleSystemMonitor
                | Action::ToggleFileNameSearch
                | Action::ToggleContentSearch
                | Action::ToggleClipboard
                | Action::CloseOverlay
                | Action::EnterCommandMode
                | Action::ExitCommandMode
                | Action::CreateFile
                | Action::CreateDirectory
                | Action::ShowInputPrompt(_)
                | Action::UpdateInput(_)
                | Action::Tick
                | Action::Quit
                | Action::TriggerImmediateRender { .. }
                | Action::HandleRenderError { .. }
        )
    }

    #[instrument(level = "debug", skip(self, action), fields(action = 
  ?action))]
    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Critical // Highest priority for render system
    }

    fn name(&self) -> &'static str {
        "ui_control_enhanced"
    }
}
