//! System-level performance monitoring using sysinfo
//! Provides memory, CPU, and I/O metrics for Arc architecture analysis

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, System};
use tracing::debug;

/// System performance metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub timestamp: u64,
    pub memory: MemoryMetrics,
    pub cpu: CpuMetrics,
    pub process: ProcessMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub used_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f32,
    pub core_count: usize,
    pub frequency_mhz: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessMetrics {
    pub memory_mb: u64,
    pub cpu_percent: f32,
    pub thread_count: usize,
}

/// System performance monitor with configurable sampling
pub struct SystemMonitor {
    system: System,
    last_update: Instant,
    update_interval: Duration,
    process_pid: Pid,
}

impl SystemMonitor {
    /// Create a new system monitor with specified update interval
    pub fn new(update_interval: Duration) -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        let process_pid = Pid::from(std::process::id() as usize);

        Self {
            system,
            last_update: Instant::now(),
            update_interval,
            process_pid,
        }
    }

    /// Create a system monitor with default 1-second update interval
    pub fn with_default_interval() -> Self {
        Self::new(Duration::from_secs(1))
    }

    /// Get current system metrics, refreshing if needed
    pub fn get_metrics(&mut self) -> SystemMetrics {
        let now = Instant::now();

        // Only refresh if enough time has passed
        if now.duration_since(self.last_update) >= self.update_interval {
            self.system.refresh_all();
            self.last_update = now;
        }

        let memory = self.collect_memory_metrics();
        let cpu = self.collect_cpu_metrics();
        let process = self.collect_process_metrics();

        SystemMetrics {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            memory,
            cpu,
            process,
        }
    }

    /// Force refresh system data regardless of interval
    pub fn force_refresh(&mut self) {
        self.system.refresh_all();
        self.last_update = Instant::now();
    }

    /// Check if memory usage is critical (>90% or <100MB available)
    pub fn is_memory_critical(&mut self) -> bool {
        let metrics: SystemMetrics = self.get_metrics();
        metrics.memory.used_percent > 90.0 || metrics.memory.available_mb < 100
    }

    /// Check if CPU usage is high (>80%)
    pub fn is_cpu_high(&mut self) -> bool {
        let metrics: SystemMetrics = self.get_metrics();
        metrics.cpu.usage_percent > 80.0
    }

    fn collect_memory_metrics(&self) -> MemoryMetrics {
        let total_bytes: u64 = self.system.total_memory();
        let available_bytes: u64 = self.system.available_memory();
        let used_bytes: u64 = self.system.used_memory();

        let total_mb: u64 = total_bytes / 1024 / 1024;
        let available_mb: u64 = available_bytes / 1024 / 1024;
        let used_mb: u64 = used_bytes / 1024 / 1024;
        let used_percent: f32 = (used_mb as f32 / total_mb as f32) * 100.0;

        MemoryMetrics {
            total_mb,
            used_mb,
            available_mb,
            used_percent,
        }
    }

    fn collect_cpu_metrics(&self) -> CpuMetrics {
        let cpus = self.system.cpus();
        let usage_percent = if cpus.is_empty() {
            0.0
        } else {
            self.system.global_cpu_usage()
        };

        let core_count = cpus.len();
        let frequency_mhz = cpus.first().map(|cpu| cpu.frequency()).unwrap_or(0);

        CpuMetrics {
            usage_percent,
            core_count,
            frequency_mhz,
        }
    }

    fn collect_process_metrics(&self) -> ProcessMetrics {
        if let Some(process) = self.system.process(self.process_pid) {
            let memory_mb = process.memory() / 1024 / 1024;
            let cpu_percent = process.cpu_usage();
            let thread_count = process.tasks().map(|tasks| tasks.len()).unwrap_or(1);

            ProcessMetrics {
                memory_mb,
                cpu_percent,
                thread_count,
            }
        } else {
            debug!("Could not find current process in system monitor");
            ProcessMetrics {
                memory_mb: 0,
                cpu_percent: 0.0,
                thread_count: 0,
            }
        }
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::with_default_interval()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_monitor_creation() {
        let monitor = SystemMonitor::with_default_interval();
        assert_eq!(monitor.update_interval, Duration::from_secs(1));
    }

    #[test]
    fn test_metrics_collection() {
        let mut monitor = SystemMonitor::with_default_interval();
        let metrics = monitor.get_metrics();

        // Basic sanity checks
        assert!(metrics.memory.total_mb > 0);
        assert!(metrics.cpu.core_count > 0);
        assert!(metrics.timestamp > 0);
    }
}
