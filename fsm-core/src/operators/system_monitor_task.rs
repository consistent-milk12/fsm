//! System monitor task for collecting and dispatching system metrics.

use std::time::Duration;

use sysinfo::System;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;

use crate::controller::actions::{Action, ProcessData, SystemData};

/// System monitor task runner.
pub struct SystemMonitor {
    action_tx: UnboundedSender<Action>,
    sys: System,
}

impl SystemMonitor {
    /// Create a new SystemMonitor.
    pub fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self {
            action_tx,
            sys: System::new_all(),
        }
    }

    /// Start the system monitoring task.
    pub fn start(mut self) {
        tokio::spawn(async move {
            loop {
                self.sys.refresh_all();

                // Collect system data
                let system_data = SystemData {
                    cpu_usage: self.sys.global_cpu_info().cpu_usage(),
                    mem_usage: self.sys.used_memory(),
                    total_mem: self.sys.total_memory(),
                    swap_usage: self.sys.used_swap(),
                    total_swap: self.sys.total_swap(),
                };

                // Collect process data
                let process_data: Vec<ProcessData> = self
                    .sys
                    .processes()
                    .values()
                    .map(|p| ProcessData {
                        pid: p.pid().as_u32(),
                        name: p.name().to_string(),
                        cpu_usage: p.cpu_usage(),
                        memory: p.memory(),
                        status: format!("{:?}", p.status()),
                    })
                    .collect();

                // Dispatch actions
                if self
                    .action_tx
                    .send(Action::SystemMonitorUpdate(system_data))
                    .is_err()
                {
                    break; // Stop if channel is closed
                }
                if self
                    .action_tx
                    .send(Action::ProcessMonitorUpdate(process_data))
                    .is_err()
                {
                    break; // Stop if channel is closed
                }

                sleep(Duration::from_secs(2)).await;
            }
        });
    }
}
