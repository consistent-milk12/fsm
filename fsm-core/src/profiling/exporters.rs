//! Data exporters for profiling data in various formats
//! Supports JSON, CSV, and flamegraph formats for different analysis needs

use csv::Writer;
use serde_json;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, warn};

use super::collector::{ArcMetrics, ProfilingSnapshot};

/// Supported export formats
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    Flamegraph,
}

/// Data exporter utility
pub struct DataExporter;

impl DataExporter {
    /// Export profiling snapshot to specified format
    pub fn export(
        snapshot: &ProfilingSnapshot,
        format: &ExportFormat,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Ensure logs directory exists
        let logs_dir = Path::new("logs");
        if !logs_dir.exists() {
            std::fs::create_dir_all(logs_dir)?;
        }

        match format {
            ExportFormat::Json => Self::export_json(snapshot),
            ExportFormat::Csv => Self::export_csv(snapshot),
            ExportFormat::Flamegraph => Self::export_flamegraph(snapshot),
        }
    }

    /// Export to structured JSON format
    fn export_json(snapshot: &ProfilingSnapshot) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let filename = format!("profiling-{}.json", timestamp);
        let path = Path::new("logs").join(&filename);

        let json = serde_json::to_string_pretty(snapshot)?;

        let mut file = File::create(&path)?;
        file.write_all(json.as_bytes())?;

        debug!(
            "Exported JSON profiling data: {} events, {} bytes",
            snapshot.custom_events.len(),
            json.len()
        );

        Ok(path)
    }

    /// Export to CSV format for spreadsheet analysis
    fn export_csv(snapshot: &ProfilingSnapshot) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let filename = format!("profiling-{}.csv", timestamp);
        let path = Path::new("logs").join(&filename);

        let file = File::create(&path)?;
        let mut writer = Writer::from_writer(file);

        // Write headers
        writer.write_record([
            "timestamp",
            "event_type",
            "name",
            "duration_ms",
            "category",
            "metadata",
            "system_memory_mb",
            "system_cpu_percent",
            "process_memory_mb",
            "arc_lock_acquisitions",
            "arc_contention_events",
            "arc_avg_wait_ms",
            "async_tasks_completed",
            "async_avg_task_ms",
        ])?;

        // Write system metrics row
        writer.write_record([
            &snapshot.timestamp.to_string(),
            "system_metrics",
            "system_snapshot",
            "0", // duration not applicable
            "system",
            "",
            &snapshot.system_metrics.memory.used_mb.to_string(),
            &snapshot.system_metrics.cpu.usage_percent.to_string(),
            &snapshot.system_metrics.process.memory_mb.to_string(),
            &snapshot.arc_metrics.lock_acquisitions.to_string(),
            &snapshot.arc_metrics.contention_events.to_string(),
            &snapshot.arc_metrics.avg_wait_time_ms.to_string(),
            &snapshot.async_metrics.tasks_completed.to_string(),
            &snapshot.async_metrics.avg_task_time_ms.to_string(),
        ])?;

        // Write custom events
        for event in &snapshot.custom_events {
            writer.write_record([
                &event.timestamp.to_string(),
                "custom_event",
                &event.name,
                &event.duration_ms.to_string(),
                &event.category,
                event.metadata.as_deref().unwrap_or(""),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "", // System metrics not applicable for individual events
            ])?;
        }

        // Write Arc operation patterns
        for (operation, count) in &snapshot.arc_metrics.concurrent_access_patterns {
            writer.write_record([
                &snapshot.timestamp.to_string(),
                "arc_operation",
                operation,
                "0", // duration aggregated
                "arc_pattern",
                &format!("access_count:{}", count),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
            ])?;
        }

        // Write async operation statistics
        for (op_type, stats) in &snapshot.async_metrics.operations_by_type {
            writer.write_record([
                &snapshot.timestamp.to_string(),
                "async_operation",
                op_type,
                &stats.avg_time_ms.to_string(),
                "async_stats",
                &format!(
                    "count:{},max:{},min:{}",
                    stats.count, stats.max_time_ms, stats.min_time_ms
                ),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
            ])?;
        }

        writer.flush()?;

        debug!(
            "Exported CSV profiling data: {} total records",
            1 + snapshot.custom_events.len()
                + snapshot.arc_metrics.concurrent_access_patterns.len()
                + snapshot.async_metrics.operations_by_type.len()
        );

        Ok(path)
    }

    /// Export to flamegraph format for async span visualization
    fn export_flamegraph(
        snapshot: &ProfilingSnapshot,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let filename = format!("flamegraph-{}.folded", timestamp);
        let path = Path::new("logs").join(&filename);

        let mut file = File::create(&path)?;

        // Generate folded format for flamegraph visualization
        // Format: stack_trace samples

        // System-level stack
        writeln!(
            file,
            "system;memory_usage {} {}",
            snapshot.system_metrics.memory.used_mb, 1
        )?;
        writeln!(
            file,
            "system;cpu_usage {} {}",
            snapshot.system_metrics.cpu.usage_percent as u64, 1
        )?;

        // Arc operations stack
        if snapshot.arc_metrics.lock_acquisitions > 0 {
            writeln!(
                file,
                "arc;lock_operations {} {}",
                snapshot.arc_metrics.avg_wait_time_ms as u64,
                snapshot.arc_metrics.lock_acquisitions
            )?;

            if snapshot.arc_metrics.contention_events > 0 {
                writeln!(
                    file,
                    "arc;lock_operations;contention {} {}",
                    snapshot.arc_metrics.avg_wait_time_ms as u64,
                    snapshot.arc_metrics.contention_events
                )?;
            }
        }

        // Async operations stack
        for (op_type, stats) in &snapshot.async_metrics.operations_by_type {
            writeln!(
                file,
                "async;{} {} {}",
                op_type, stats.avg_time_ms as u64, stats.count
            )?;
        }

        // Custom events stack
        let mut event_aggregates: std::collections::HashMap<String, (u64, u64)> =
            std::collections::HashMap::new();
        for event in &snapshot.custom_events {
            let entry = event_aggregates.entry(event.name.clone()).or_insert((0, 0));
            entry.0 += event.duration_ms;
            entry.1 += 1;
        }

        for (event_name, (total_duration, count)) in event_aggregates {
            writeln!(
                file,
                "custom;{} {} {}",
                event_name,
                total_duration / count,
                count
            )?;
        }

        debug!(
            "Exported flamegraph profiling data with {} unique stacks",
            1 + 1
                + snapshot.arc_metrics.concurrent_access_patterns.len()
                + snapshot.async_metrics.operations_by_type.len()
                + snapshot.custom_events.len()
        );

        Ok(path)
    }

    /// Export Arc-specific metrics to dedicated JSON file
    pub fn export_arc_metrics(
        arc_metrics: &ArcMetrics,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let filename = format!("arc-metrics-{}.json", timestamp);
        let path = Path::new("logs").join(&filename);

        let json = serde_json::to_string_pretty(arc_metrics)?;

        let mut file = File::create(&path)?;
        file.write_all(json.as_bytes())?;

        debug!(
            "Exported Arc-specific metrics: {} lock acquisitions, {} contentions",
            arc_metrics.lock_acquisitions, arc_metrics.contention_events
        );

        Ok(path)
    }

    /// List available profiling export files
    pub fn list_exports() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let logs_dir = Path::new("logs");
        if !logs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut exports = Vec::new();
        for entry in std::fs::read_dir(logs_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(filename) = path.file_name()
                && let Some(filename_str) = filename.to_str()
                && (filename_str.starts_with("profiling-")
                    || filename_str.starts_with("arc-metrics-")
                    || filename_str.starts_with("flamegraph-"))
            {
                exports.push(path);
            }
        }

        exports.sort();

        Ok(exports)
    }

    /// Clean up old profiling files (older than specified days)
    pub fn cleanup_old_exports(max_age_days: u64) -> Result<usize, Box<dyn std::error::Error>> {
        let cutoff =
            SystemTime::now() - std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);
        let exports = Self::list_exports()?;
        let mut removed_count = 0;

        for export_path in exports {
            if let Ok(metadata) = export_path.metadata()
                && let Ok(created) = metadata.created()
                && created < cutoff
            {
                match std::fs::remove_file(&export_path) {
                    Ok(()) => {
                        removed_count += 1;
                        debug!("Removed old profiling export: {}", export_path.display());
                    }

                    Err(e) => {
                        warn!(
                            "Failed to remove old profiling export {}: {}",
                            export_path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(removed_count)
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::profiling::collector::{
//         CpuMetrics, MemoryMetrics, ProcessMetrics, ProfilingSnapshot, SystemMetrics,
//     };

//     fn create_test_snapshot() -> ProfilingSnapshot {
//         ProfilingSnapshot {
//             timestamp: 1234567890,
//             duration_seconds: 60.0,
//             system_metrics: SystemMetrics {
//                 timestamp: 1234567890,
//                 memory: MemoryMetrics {
//                     total_mb: 16000,
//                     used_mb: 8000,
//                     available_mb: 8000,
//                     used_percent: 50.0,
//                 },
//                 cpu: CpuMetrics {
//                     usage_percent: 25.0,
//                     core_count: 8,
//                     frequency_mhz: 3200,
//                 },
//                 process: ProcessMetrics {
//                     memory_mb: 256,
//                     cpu_percent: 10.0,
//                     thread_count: 4,
//                 },
//             },
//             arc_metrics: ArcMetrics::default(),
//             async_metrics: AsyncMetrics::default(),
//             custom_events: vec![],
//         }
//     }

//     #[test]
//     fn test_export_formats() {
//         let snapshot = create_test_snapshot();

//         // Test JSON export
//         let json_result = DataExporter::export(&snapshot, &ExportFormat::Json);
//         assert!(json_result.is_ok());

//         // Test CSV export
//         let csv_result = DataExporter::export(&snapshot, &ExportFormat::Csv);
//         assert!(csv_result.is_ok());

//         // Test flamegraph export
//         let flame_result = DataExporter::export(&snapshot, &ExportFormat::Flamegraph);
//         assert!(flame_result.is_ok());
//     }
// }
