//! Central profiling data collector and aggregator
//! Coordinates system monitoring, Arc tracking, and async profiling

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use serde::{Serialize, Deserialize};
use tracing::{debug, warn};

use super::{
    system_monitor::{SystemMonitor, SystemMetrics},
    exporters::{DataExporter, ExportFormat},
};

/// Complete profiling data snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilingSnapshot {
    pub timestamp: u64,
    pub duration_seconds: f64,
    pub system_metrics: SystemMetrics,
    pub arc_metrics: ArcMetrics,
    pub async_metrics: AsyncMetrics,
    pub custom_events: Vec<CustomEvent>,
}

/// Arc/Mutex contention and performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArcMetrics {
    pub lock_acquisitions: u64,
    pub total_wait_time_ms: u64,
    pub max_wait_time_ms: u64,
    pub avg_wait_time_ms: f64,
    pub contention_events: u64,
    pub deadlock_near_misses: u64,
    pub concurrent_access_patterns: HashMap<String, u64>,
}

/// Async task and operation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncMetrics {
    pub tasks_spawned: u64,
    pub tasks_completed: u64,
    pub total_task_time_ms: u64,
    pub avg_task_time_ms: f64,
    pub max_task_time_ms: u64,
    pub operations_by_type: HashMap<String, OperationStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    pub count: u64,
    pub total_time_ms: u64,
    pub avg_time_ms: f64,
    pub max_time_ms: u64,
    pub min_time_ms: u64,
}

/// Custom profiling event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEvent {
    pub timestamp: u64,
    pub name: String,
    pub duration_ms: u64,
    pub metadata: Option<String>,
    pub category: String,
}

/// Central profiling data collector
pub struct ProfileCollector {
    start_time: Instant,
    system_monitor: Mutex<SystemMonitor>,
    
    // Arc metrics tracking
    arc_metrics: Arc<Mutex<ArcMetrics>>,
    
    // Async metrics tracking
    async_metrics: Arc<Mutex<AsyncMetrics>>,
    
    // Custom events
    custom_events: Arc<Mutex<Vec<CustomEvent>>>,
    
    // Export configuration
    export_formats: Vec<ExportFormat>,
    export_interval: Duration,
    last_export: Mutex<Instant>,
}

impl ProfileCollector {
    /// Create a new profile collector with default settings
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            system_monitor: Mutex::new(SystemMonitor::with_default_interval()),
            arc_metrics: Arc::new(Mutex::new(ArcMetrics::default())),
            async_metrics: Arc::new(Mutex::new(AsyncMetrics::default())),
            custom_events: Arc::new(Mutex::new(Vec::new())),
            export_formats: vec![ExportFormat::Json, ExportFormat::Csv],
            export_interval: Duration::from_secs(30),
            last_export: Mutex::new(Instant::now()),
        }
    }

    /// Record a custom profiling event
    pub fn record_event(&self, name: &str, duration_ms: u64, metadata: Option<&str>) {
        let event = CustomEvent {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            name: name.to_string(),
            duration_ms,
            metadata: metadata.map(String::from),
            category: "custom".to_string(),
        };

        if let Ok(mut events) = self.custom_events.lock() {
            events.push(event);
        } else {
            warn!("Failed to acquire lock for custom events recording");
        }
    }

    /// Record Arc lock operation metrics
    pub fn record_arc_operation(&self, operation: &str, wait_time_ms: u64, contended: bool) {
        if let Ok(mut metrics) = self.arc_metrics.lock() {
            metrics.lock_acquisitions += 1;
            metrics.total_wait_time_ms += wait_time_ms;
            metrics.max_wait_time_ms = metrics.max_wait_time_ms.max(wait_time_ms);
            metrics.avg_wait_time_ms = metrics.total_wait_time_ms as f64 / metrics.lock_acquisitions as f64;
            
            if contended {
                metrics.contention_events += 1;
            }
            
            *metrics.concurrent_access_patterns.entry(operation.to_string()).or_insert(0) += 1;
        } else {
            warn!("Failed to acquire lock for Arc metrics recording");
        }
    }

    /// Record async task metrics
    pub fn record_async_operation(&self, operation_type: &str, duration_ms: u64, completed: bool) {
        if let Ok(mut metrics) = self.async_metrics.lock() {
            if completed {
                metrics.tasks_completed += 1;
            } else {
                metrics.tasks_spawned += 1;
            }

            metrics.total_task_time_ms += duration_ms;
            metrics.avg_task_time_ms = metrics.total_task_time_ms as f64 / metrics.tasks_completed.max(1) as f64;
            metrics.max_task_time_ms = metrics.max_task_time_ms.max(duration_ms);

            let stats = metrics.operations_by_type.entry(operation_type.to_string()).or_insert(OperationStats {
                count: 0,
                total_time_ms: 0,
                avg_time_ms: 0.0,
                max_time_ms: 0,
                min_time_ms: u64::MAX,
            });

            stats.count += 1;
            stats.total_time_ms += duration_ms;
            stats.avg_time_ms = stats.total_time_ms as f64 / stats.count as f64;
            stats.max_time_ms = stats.max_time_ms.max(duration_ms);
            stats.min_time_ms = stats.min_time_ms.min(duration_ms);
        } else {
            warn!("Failed to acquire lock for async metrics recording");
        }
    }

    /// Get current profiling snapshot
    pub fn get_snapshot(&self) -> Result<ProfilingSnapshot, Box<dyn std::error::Error + '_>> {
        let system_metrics = self.system_monitor.lock()?.get_metrics();
        let arc_metrics = self.arc_metrics.lock()?.clone();
        let async_metrics = self.async_metrics.lock()?.clone();
        let custom_events = self.custom_events.lock()?.clone();

        let duration_seconds = self.start_time.elapsed().as_secs_f64();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64;

        Ok(ProfilingSnapshot {
            timestamp,
            duration_seconds,
            system_metrics,
            arc_metrics,
            async_metrics,
            custom_events,
        })
    }

    /// Export current profiling data to configured formats
    pub fn export_data(&self) -> Result<(), Box<dyn std::error::Error + '_>> {
        let snapshot = self.get_snapshot()?;
        
        for format in &self.export_formats {
            match DataExporter::export(&snapshot, format) {
                Ok(path) => {
                    debug!("Exported profiling data to: {}", path.display());
                }
                Err(e) => {
                    warn!("Failed to export profiling data in format {:?}: {}", format, e);
                }
            }
        }

        // Update last export time
        if let Ok(mut last_export) = self.last_export.lock() {
            *last_export = Instant::now();
        }

        Ok(())
    }

    /// Check if it's time to export data automatically
    pub fn should_export(&self) -> bool {
        if let Ok(last_export) = self.last_export.lock() {
            last_export.elapsed() >= self.export_interval
        } else {
            false
        }
    }

    /// Get Arc metrics handle for direct access
    pub fn arc_metrics(&self) -> Arc<Mutex<ArcMetrics>> {
        self.arc_metrics.clone()
    }

    /// Get async metrics handle for direct access
    pub fn async_metrics(&self) -> Arc<Mutex<AsyncMetrics>> {
        self.async_metrics.clone()
    }
}

impl Default for ArcMetrics {
    fn default() -> Self {
        Self {
            lock_acquisitions: 0,
            total_wait_time_ms: 0,
            max_wait_time_ms: 0,
            avg_wait_time_ms: 0.0,
            contention_events: 0,
            deadlock_near_misses: 0,
            concurrent_access_patterns: HashMap::new(),
        }
    }
}

impl Default for AsyncMetrics {
    fn default() -> Self {
        Self {
            tasks_spawned: 0,
            tasks_completed: 0,
            total_task_time_ms: 0,
            avg_task_time_ms: 0.0,
            max_task_time_ms: 0,
            operations_by_type: HashMap::new(),
        }
    }
}

impl Default for ProfileCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_creation() {
        let collector = ProfileCollector::new();
        assert!(collector.start_time.elapsed().as_secs() < 1);
    }

    #[test]
    fn test_custom_event_recording() {
        let collector = ProfileCollector::new();
        collector.record_event("test_event", 100, Some("test metadata"));
        
        let snapshot = collector.get_snapshot().unwrap();
        assert_eq!(snapshot.custom_events.len(), 1);
        assert_eq!(snapshot.custom_events[0].name, "test_event");
        assert_eq!(snapshot.custom_events[0].duration_ms, 100);
    }

    #[test]
    fn test_arc_operation_recording() {
        let collector = ProfileCollector::new();
        collector.record_arc_operation("lock_ui", 50, true);
        collector.record_arc_operation("lock_fs", 25, false);
        
        let arc_metrics = collector.arc_metrics.lock().unwrap();
        assert_eq!(arc_metrics.lock_acquisitions, 2);
        assert_eq!(arc_metrics.contention_events, 1);
        assert_eq!(arc_metrics.total_wait_time_ms, 75);
    }
}