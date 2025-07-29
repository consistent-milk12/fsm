//! Handles system-related actions like monitoring and process management.

use anyhow::Result;
use std::sync::Arc;
use sysinfo::{Pid, System};
use tracing::{error, info, instrument};

use crate::controller::action_dispatcher::{ActionMatcher, ActionPriority, DispatchResult};
use crate::controller::actions::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::{RedrawFlag, UIOverlay};

/// Dispatches system‑level actions (monitor toggles, kill).
#[derive(Clone, Debug)]
pub struct SystemHandler {
    /// Shared UI state provider.
    state_provider: Arc<dyn StateProvider>,
}

impl SystemHandler {
    /// Construct a new SystemHandler with the given state provider.
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Toggle the system monitor overlay on or off.
    #[instrument(level = "info", name = "toggle_system_monitor")]
    fn toggle_system_monitor_overlay(&self) {
        info!(marker = "SYSTEM_MONITOR_TOGGLE", "Toggling system monitor");

        self.state_provider.update_ui_state(Box::new(|ui| {
            // Flip between SystemMonitor and None.
            ui.overlay = if ui.overlay == UIOverlay::SystemMonitor {
                UIOverlay::None
            } else {
                UIOverlay::SystemMonitor
            };

            // Request only overlay redraw.
            ui.request_redraw(RedrawFlag::Overlay);
        }));
    }

    /// Toggle the process monitor overlay on or off.
    #[instrument(level = "info", name = "toggle_process_monitor")]
    fn toggle_process_monitor_overlay(&self) {
        info!(
            marker = "PROCESS_MONITOR_TOGGLE",
            "Toggling process monitor"
        );

        self.state_provider.update_ui_state(Box::new(|ui| {
            // Flip between ProcessMonitor and None.
            ui.overlay = if ui.overlay == UIOverlay::SystemMonitor {
                UIOverlay::None
            } else {
                UIOverlay::SystemMonitor
            };

            // Request only overlay redraw.
            ui.request_redraw(RedrawFlag::Overlay);
        }));
    }

    /// Attempt to kill a process by PID.
    #[instrument(level = "info", name = "kill_process", fields(pid))]
    fn kill_process(&self, pid: u32) {
        info!(
            marker = "PROCESS_KILL_ATTEMPT",
            pid, "Attempting to kill process"
        );

        let mut sys = System::new();

        // Refresh process info before attempting kill.
        if sys.refresh_process(Pid::from_u32(pid)) {
            if let Some(process) = sys.process(Pid::from_u32(pid)) {
                if process.kill() {
                    info!(marker = "PROCESS_KILL_SUCCESS", pid, "Killed process");
                } else {
                    error!(marker = "PROCESS_KILL_FAILED", pid, "Kill call failed");
                }
            } else {
                error!(marker = "PROCESS_KILL_FAILED", pid, "Process disappeared");
            }
        } else {
            error!(marker = "PROCESS_KILL_FAILED", pid, "Process not found");
        }
    }
}

impl ActionMatcher for SystemHandler {
    /// Which actions this handler will process.
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::ToggleSystemMonitor | Action::ToggleProcessMonitor | Action::KillProcess(_)
        )
    }

    /// Handle an incoming action and return whether to continue or terminate.
    #[instrument(level = "debug", name = "system_handle")]
    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::ToggleSystemMonitor => {
                self.toggle_system_monitor_overlay();
                Ok(DispatchResult::Continue)
            }
            Action::ToggleProcessMonitor => {
                self.toggle_process_monitor_overlay();
                Ok(DispatchResult::Continue)
            }
            Action::KillProcess(pid) => {
                self.kill_process(pid);
                Ok(DispatchResult::Continue)
            }
            _ => Ok(DispatchResult::NotHandled),
        }
    }

    /// Normal priority among dispatchers.
    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    /// Name of this dispatcher.
    fn name(&self) -> &'static str {
        "system"
    }

    /// Never disable system handler.
    fn can_disable(&self) -> bool {
        false
    }
}
