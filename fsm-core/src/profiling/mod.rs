//! Profiling infrastructure for Arc architecture performance analysis
//!
//! This module provides comprehensive profiling capabilities specifically designed
//! for monitoring the Arc<Mutex<T>> architecture performance and validating the
//! Crisis #2 resolution (97.2% error reduction).

#[cfg(feature = "profiling")]
pub mod collector;

#[cfg(feature = "profiling")]
pub mod system_monitor;

#[cfg(feature = "profiling")]
pub mod arc_monitor;

#[cfg(feature = "profiling")]
pub mod async_profiler;

#[cfg(feature = "profiling")]
pub mod exporters;

#[cfg(feature = "profiling")]
pub mod macros;

#[cfg(feature = "profiling")]
pub use collector::ProfileCollector;

#[cfg(feature = "profiling")]
pub use system_monitor::SystemMonitor;

#[cfg(feature = "profiling")]
pub use arc_monitor::ArcMonitor;

#[cfg(feature = "profiling")]
pub use async_profiler::AsyncProfiler;

#[cfg(feature = "profiling")]
pub use exporters::{DataExporter, ExportFormat};

// Public API that's always available but no-ops when profiling is disabled
pub fn init_profiling() -> ProfileGuard {
    #[cfg(feature = "profiling")]
    {
        ProfileGuard::new()
    }
    #[cfg(not(feature = "profiling"))]
    {
        ProfileGuard::disabled()
    }
}

/// Guard that ensures profiling data is exported on drop
pub struct ProfileGuard {
    #[cfg(feature = "profiling")]
    collector: Option<ProfileCollector>,
}

impl ProfileGuard {
    #[cfg(feature = "profiling")]
    fn new() -> Self {
        Self {
            collector: Some(ProfileCollector::new()),
        }
    }

    #[cfg(not(feature = "profiling"))]
    fn disabled() -> Self {
        Self {}
    }

    /// Record a profiling event (no-op if profiling disabled)
    pub fn record_event(&self, _name: &str, _duration_ms: u64, _metadata: Option<&str>) {
        #[cfg(feature = "profiling")]
        if let Some(collector) = &self.collector {
            collector.record_event(_name, _duration_ms, _metadata);
        }
    }

    /// Export current profiling data (no-op if profiling disabled)
    pub fn export_data(&self) {
        #[cfg(feature = "profiling")]
        if let Some(collector) = &self.collector
            && let Err(e) = collector.export_data()
        {
            tracing::warn!("Failed to export profiling data: {}", e);
        }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        self.export_data();
    }
}
