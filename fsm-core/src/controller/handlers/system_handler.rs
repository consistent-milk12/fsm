//! Handles system-related actions like monitoring and process management.

use std::sync::Arc;

use sysinfo::{Pid, System};
use tracing::{error, info};

use crate::controller::action_dispatcher::{ActionMatcher, ActionPriority, DispatchResult};
use crate::controller::actions::Action;
use crate::controller::state_provider::StateProvider;
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct SystemHandler {
    state_provider: Arc<dyn StateProvider>,
}

impl SystemHandler {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    pub async fn handle_action(&self, action: &Action) {
        match action {
            Action::ToggleSystemMonitor => self.toggle_system_monitor_overlay(),
            Action::ToggleProcessMonitor => self.toggle_process_monitor_overlay(),
            Action::KillProcess(pid) => self.kill_process(*pid),
            _ => {}
        }
    }

    fn toggle_system_monitor_overlay(&self) {
        info!(
            marker = "SYSTEM_MONITOR_TOGGLE",
            "Toggling system monitor overlay"
        );
        self.state_provider.update_ui_state(Box::new(|ui| {
            ui.show_system_monitor = !ui.show_system_monitor;
            if ui.show_system_monitor {
                ui.show_process_monitor = false;
            }
        }));
    }

    fn toggle_process_monitor_overlay(&self) {
        info!(
            marker = "PROCESS_MONITOR_TOGGLE",
            "Toggling process monitor overlay"
        );
        self.state_provider.update_ui_state(Box::new(|ui| {
            ui.show_process_monitor = !ui.show_process_monitor;
            if ui.show_process_monitor {
                ui.show_system_monitor = false;
            }
        }));
    }

    fn kill_process(&self, pid: u32) {
        info!(
            marker = "PROCESS_KILL_ATTEMPT",
            pid = pid,
            "Attempting to kill process"
        );
        let mut sys = System::new();
        if sys.refresh_process(Pid::from_u32(pid)) {
            if sys.process(Pid::from_u32(pid)).unwrap().kill() {
                info!(
                    marker = "PROCESS_KILL_SUCCESS",
                    pid = pid,
                    "Successfully killed process"
                );
            } else {
                error!(
                    marker = "PROCESS_KILL_FAILED",
                    pid = pid,
                    "Failed to kill process"
                );
            }
        } else {
            error!(
                marker = "PROCESS_KILL_FAILED",
                pid = pid,
                "Process not found"
            );
        }
    }
}

impl ActionMatcher for SystemHandler {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::ToggleSystemMonitor | Action::ToggleProcessMonitor | Action::KillProcess(_)
        )
    }

    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle_action(&action).await;
        Ok(DispatchResult::Continue)
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "system"
    }

    fn dynamic_priority(&self, _action: &Action) -> ActionPriority {
        ActionPriority::Normal
    }

    fn can_disable(&self) -> bool {
        false
    }
}
