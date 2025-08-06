//! ``src/fs/dir_scanner.rs``
//!
//! # `Directory Scanner`: Asynchronous Filesystem Listing
//!
//! Provides an asynchronous function to scan a directory and return a sorted
//! list of `ObjectInfo` entries. Designed for non-blocking UI updates.

use crate::{cache::cache_manager::ObjectInfoCache,
    config::ProfilingConfig, 
    controller::actions::Action, 
    logging::ProfilingData, 
    tasks::metadata_task::batch_load_metadata_task
};
use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
use std::{cmp::Ordering, ffi::OsStr, path::{Path, PathBuf}, sync::Arc, time::Duration};
use std::time::{Instant, SystemTime};
use tokio::{fs::{self, DirEntry, ReadDir}, sync::mpsc::{UnboundedReceiver, UnboundedSender}};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, instrument, Span};

/// Scans the given directory asynchronously and returns a sorted list of `ObjectInfo`.
///
/// # Arguments
/// * `path` - The path to the directory to scan.
/// * `show_hidden` - Whether to include hidden files/directories (starting with '.').
/// * `profiling_config` - The configuration for performance profiling.
/// * `cache` - The `ObjectInfo` cache for performance optimization
#[expect(clippy::too_many_lines, reason = "Expected verbosity due to logging")]
#[instrument(
    skip(profiling_config, cache),
    fields(
        operation_type = "scan_dir_cached",
        path = %path.display(),
        show_hidden = show_hidden,
        entry_count = tracing::field::Empty,
        cache_hits = tracing::field::Empty,
        cache_misses = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        memory_delta_kb = tracing::field::Empty,
    )
)]
pub async fn scan_dir(
    path: &Path,
    show_hidden: bool,
    profiling_config: &ProfilingConfig,
    cache: &ObjectInfoCache
) -> Result<Vec<ObjectInfo>, AppError> {
    let start_time: Instant = Instant::now();
    let start_mem: Option<i64> = ProfilingData::get_current_memory_kb();

    let mut entries: Vec<ObjectInfo> = Vec::new();
    let mut read_dir: ReadDir = fs::read_dir(path).await?;

    // Cache performance tracking
    let mut cache_hits: u32 = 0;
    let mut cache_misses: u32 = 0;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path: PathBuf = entry.path();

        let file_name: &str = entry_path
            .file_name()
            .and_then(|s: &std::ffi::OsStr| s.to_str())
            .unwrap_or("");

        if !show_hidden && file_name.starts_with('.') {
            continue;
        }

        // Cache-integrated ObjectInfo loading with performance tracking
        let cache_lookup_start = Instant::now();

        // Cache-integrated ObjectInfo loading
        match cache.get_or_load_path(
            &entry_path, 
            || 
            { 
                cache_misses += 1;

                tracing::debug!(
                    path = %entry_path.display(),
                    "Cache miss - loading from filesystem"
                );

                ObjectInfo::from_path_direct(&entry_path)
            }
        )
        .await 
        {
            Ok(info) => {
                let lookup_duration = cache_lookup_start.elapsed();

                if lookup_duration.as_micros() < 100
                {
                    // Fast lookup indicates cache hit
                    cache_hits += 1;

                    tracing::debug!(
                        path = %entry_path.display(),
                        lookup_time_us = lookup_duration.as_micros(),
                        "Cache hit - fast retrieval"
                    );
                }

                entries.push(info);
            }
            
            Err(e) => {
                // Log the error but continue processing other entries
                tracing::debug!("Failed to get ObjectInfo for {entry_path:?}: {e}");
            }
        }
    }

    // Sort entries: directories first, then alphabetically by name
    entries.sort_by(|a: &ObjectInfo, b: &ObjectInfo| {
        if a.is_dir && !b.is_dir {
            Ordering::Less
        } else if !a.is_dir && b.is_dir {
            Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    let duration: Duration = start_time.elapsed();
    let profiling_data: ProfilingData = ProfilingData::collect_profiling_data_conditional(
         start_mem, duration, profiling_config
    );

    // Record performance metrics in span
    let current_span = Span::current();
    current_span.record("entry_count", entries.len());
    current_span.record("cache_hits", cache_hits);
    current_span.record("cache_misses", cache_misses);
    current_span.record("duration_ms", duration.as_millis());

    if let Some(memory_delta) = profiling_data.memory_delta_kb {
        current_span.record("memory_delta_kb", memory_delta);
    }

    // Calculate cache efficiency
    let total_lookups: u32 = cache_hits + cache_misses;
    let cache_hit_rate: f64 = if total_lookups > 0 {
        (f64::from(cache_hits) / f64::from(total_lookups)) * 100.0
    } else {
        0.0
    };

    if let Some(duration_ns) = profiling_data.operation_duration_ns {
        let perf_category = if duration.as_millis() < 10 
        { 
            "excellent" 
        } else if duration.as_millis() < 50 
        { 
            "good" 
        }
        else 
        { 
            "needs_optimization" 
        };

        info!(
            marker = "PERF_DIRECTORY_SCAN",
            operation_type = "scan_dir_cached",
            duration_ns = duration_ns,
            duration_ms = duration.as_millis(),
            memory_delta_kb = profiling_data.memory_delta_kb.unwrap_or(0),
            entry_count = entries.len(),
            cache_hits = cache_hits,
            cache_misses = cache_misses,
            cache_hit_rate = format!("{:.1}%", cache_hit_rate),
            performance_category = perf_category,
            "Directory scan completed - {} entries, {:.1}% cache hit rate in {:?}",
            entries.len(), cache_hit_rate, duration
        );
    }

    Ok(entries)
}

/// Represents a scanning progress update
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanUpdate {
    /// A new entry was discovered
    Entry(ObjectInfo),
    /// Scanning completed with final count
    Completed(usize),
    /// An error occurred while scanning
    Error(String),
}

/// Scans directory with streaming updates and two-phase metadata loading
///
/// # Arguments
/// * `path` - The path to the directory to scan
/// * `show_hidden` - Whether to include hidden files/directories
/// * `batch_size` - Number of entries to process before yielding (for responsiveness)
/// * `action_tx` - Channel to send metadata loading tasks
/// * `profiling_config` - The configuration for performance profiling.
/// * `cache` - The `ObjectInfo` cache for performance optimization.
/// 
/// # Returns
/// * A receiver channel that will receive `ScanUpdate` messages
/// * A sender for the final sorted results
#[allow(clippy::unused_async, reason = "async move occurs inside tokio::spawn")]
#[instrument(
    skip(action_tx, profiling_config, cache)
    fields(
        operation_type = "scan_dir_streaming",
        path = %path.display(),
        show_hidden = show_hidden,
        batch_size = batch_size,
    )
)]
pub async fn scan_dir_streaming_with_background_metadata(
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    action_tx: UnboundedSender<Action>,
    profiling_config: ProfilingConfig,
    cache: Arc<ObjectInfoCache>
) -> (
    UnboundedReceiver<ScanUpdate>,
    JoinHandle<Result<Vec<ObjectInfo>, AppError>>,
) {
    let (
        tx, 
        rx
    ) = mpsc::unbounded_channel();

    let handle: JoinHandle<Result<Vec<ObjectInfo>, AppError>> = tokio::spawn(
        async move 
        {
            let scanner: DirectoryScanner = DirectoryScanner::new(
                path, 
                show_hidden, 
                batch_size, 
                action_tx, 
                tx, 
                profiling_config,
                cache
            );
            
            scanner.scan().await
        }
    );

    (rx, handle)
}

struct DirectoryScanner {
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    action_tx: UnboundedSender<Action>,
    tx: UnboundedSender<ScanUpdate>,
    profiling_config: ProfilingConfig,
    cache: Arc<ObjectInfoCache>,
}

impl DirectoryScanner {
    const fn new(
        path: PathBuf,
        show_hidden: bool,
        batch_size: usize,
        action_tx: UnboundedSender<Action>,
        tx: UnboundedSender<ScanUpdate>,
        profiling_config: ProfilingConfig,
        cache: Arc<ObjectInfoCache>,
    ) -> Self {
        Self {
            path,
            show_hidden,
            batch_size,
            action_tx,
            tx,
            profiling_config,
            cache
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "Expected loss")]
    #[instrument(
        skip(self),
        fields(
            operation_type = "streaming_directory_scan",
            path = %self.path.display(),
            batch_size = self.batch_size,
            entry_count = tracing::field::Empty,
            cache_hits = tracing::field::Empty,
            cache_misses = tracing::field::Empty,
            phase1_duration_ms = tracing::field::Empty,
            total_duration_ms = tracing::field::Empty,
        )
    )]
    async fn scan(self) -> Result<Vec<ObjectInfo>, AppError> {
        let start_time: Instant = Instant::now();
        let start_mem: Option<i64> = ProfilingData::get_current_memory_kb();

        let mut entries: Vec<ObjectInfo> = Vec::new();
        let mut light_entries: Vec<LightObjectInfo> = Vec::new();

        // Phase 1: Quick scan for basic info
        let phase1_start = Instant::now();
        self.perform_quick_scan(&mut entries, &mut light_entries).await?;
        let phase1_duration = phase1_start.elapsed();

        // Sort entries by directory first, then alphabetically
        Self::sort_entries(&mut entries);

        // Send completion notification
        let _ = self.tx.send(ScanUpdate::Completed(entries.len()));

        // Phase 2: Start background metadata loading
        self.start_background_metadata_loading(light_entries);

        let duration: Duration = start_time.elapsed();
        let profiling_data: ProfilingData = ProfilingData::collect_profiling_data_conditional(
            start_mem,
            duration,
            &self.profiling_config
        );

        // Record performance metrics
        Span::current()
            .record("entry_count", entries.len())
            .record("phase1_duration_ms", phase1_duration.as_millis())
            .record("total_duration_ms", duration.as_millis());

        if let Some(duration_ns) = profiling_data.operation_duration_ns {
            let perf_category = if duration.as_millis() < 20 
            { 
                "excellent" 
            } else if duration.as_millis() < 100 
            { 
                "good" 
            }
            else 
            { 
                "needs_optimization" 
            }; 

            info!(
                  marker = "PERF_DIRECTORY_SCAN",
                  operation_type = "scan_dir_streaming_cached",
                  duration_ns = duration_ns,
                  duration_ms = duration.as_millis(),
                  memory_delta_kb = profiling_data.memory_delta_kb.unwrap_or(0),
                  entry_count = entries.len(),
                  phase1_duration_ms = phase1_duration.as_millis(),
                  phase1_percentage = format!("{:.1}%", (phase1_duration.as_millis() as f64 / duration.as_millis() as f64) * 100.0),
                  performance_category = perf_category,
                  "Streaming directory scan completed - {} entries in {:?} (phase1:{:?})",
                  entries.len(), duration, phase1_duration
              );
        }

        Ok(entries)
    }

    #[expect(clippy::cast_precision_loss, reason = "Expected loss")]
    #[instrument(
        skip(self, entries, light_entries),
        fields(
            operation_type = "quick_scan_phase",
            processed_count = tracing::field::Empty,
            cache_optimization_rate = tracing::field::Empty,
        )
    )]
    async fn perform_quick_scan(
        &self,
        entries: &mut Vec<ObjectInfo>,
        light_entries: &mut Vec<LightObjectInfo>,
    ) -> Result<(), AppError> {
        let mut read_dir: ReadDir = self.initialize_directory_reader().await?;
        let mut processed: u64 = 0u64;
        let mut cache_optimized: u64 = 0u64;

        while let Some(entry_result) = read_dir.next_entry().await.transpose() {
            let entry: DirEntry = match entry_result {
                Ok(e) => e,

                Err(e) => {
                    let app_error: AppError = AppError::from(e);

                    let _ = self.tx.send(ScanUpdate::Error(app_error.to_string()));

                    continue;
                }
            };

            if self.should_skip_entry(&entry) {
                continue;
            }

            let was_cache_optimized = self
                .process_directory_entry(
                    entry,
                    entries,
                    light_entries
                )
                .await;

            if was_cache_optimized.0 {
                processed += 1;

                if was_cache_optimized.1 {
                    cache_optimized += 1;
                }

                // Yield control periodically for responsiveness
                if processed.is_multiple_of(self.batch_size as u64) {
                    tokio::task::yield_now().await;
                }
            } else {
                // Receiver dropped, stop scanning
                break;
            }
        }

        // Record cache optimization rate
        let current_span = Span::current();
        current_span.record("processed_count", processed);

        if processed > 0 {
            let optimization_rate = (cache_optimized as f64 / processed as f64) * 100.0;
            current_span.record("cache_optimization_rate", format!("{optimization_rate:.1}%"));

            tracing::debug!(
                processed = processed,
                cache_optimized = cache_optimized,
                optimization_rate = format!("{:.1}%", optimization_rate),
                "Quick scan phase completed with cache optimization"
            );
        }

        Ok(())
    }

    async fn initialize_directory_reader(&self) -> Result<ReadDir, AppError> {
        match fs::read_dir(&self.path).await {
            Ok(read_dir) => Ok(read_dir),
            
            Err(e) => {
                let app_error: AppError = AppError::from(e);
                
                let _ = self.tx.send(ScanUpdate::Error(app_error.to_string()));
                
                Err(app_error)
            }
        }
    }

    fn should_skip_entry(&self, entry: &DirEntry) -> bool {
        if self.show_hidden {
            return false;
        }

        let entry_path: PathBuf = entry.path();
        let file_name: &str = entry_path
            .file_name()
            .and_then(|s: &OsStr| -> Option<&str> 
                {
                    s.to_str()
                }
            )
            .unwrap_or("");

        file_name.starts_with('.')
    }

    // Returns (success, was_cache_hit)
    async fn process_directory_entry(
        &self,
        entry: DirEntry,
        entries: &mut Vec<ObjectInfo>,
        light_entries: &mut Vec<LightObjectInfo>,
    ) -> (bool, bool) {
        let entry_path: PathBuf = entry.path();

        match LightObjectInfo::from_path(&entry_path).await {
            Ok(light_info) => {
                // Create placeholder with cache check for existing full metadata
                let cache_check_start = Instant::now();
                let (placeholder_info, was_cache_hit) = if let Some(cached_info) = self
                    .cache.get_by_path(&entry_path)
                    .await
                     {
                    let cache_lookup_time = cache_check_start.elapsed();
                    tracing::debug!(
                        path = %entry_path.display(),
                        lookup_time_us = cache_lookup_time.as_micros(),
                        "Cache hit during streaming scan"
                    );
                    (cached_info, true)
                } else {
                    let placeholder = ObjectInfo {
                        path: light_info.path.clone(),
                        modified: SystemTime::UNIX_EPOCH, // Placeholder
                        name: light_info.name.clone(),
                        extension: light_info.extension.clone(),
                        size: 0,
                        items_count: 0,
                        is_dir: light_info.is_dir,
                        is_symlink: light_info.is_symlink,
                        metadata_loaded: false,
                    };
                    tracing::debug!(
                        path = %entry_path.display(),
                        "Cache miss - using placeholder for streaming scan"
                    );
                    (placeholder, false)
                };

                // Send streaming update immediately
                if self
                    .tx
                    .send(
                        ScanUpdate::Entry(placeholder_info.clone())
                    )
                    .is_err()
                {
                    return (false, false); // Receiver dropped
                }

                entries.push(placeholder_info);
                light_entries.push(light_info);

                (true, was_cache_hit)
            }

            Err(e) => {
                tracing::debug!(
                    path = %entry_path.display(),
                    error = %e,
                    "Failed to get basic info for entry"
                );

                let _ = self.tx.send(ScanUpdate::Error(e.to_string()));

                (true, false)
            }
        }
    }

    fn sort_entries(entries: &mut [ObjectInfo]) {
        entries
            .sort_by(
                |a: &ObjectInfo, b: &ObjectInfo| -> Ordering 
                {
                    if a.is_dir && !b.is_dir {
                        Ordering::Less
                    } else if !a.is_dir && b.is_dir {
                        Ordering::Greater
                    } else {
                        a.name.cmp(&b.name)
                    }
                }
            );
    }

    #[instrument(
        skip(self, light_entries),
        fields(
            operation_type = "start_background_metadata_loading",
            entry_count = light_entries.len(),
            batch_size = 5,
        )
    )]
    fn start_background_metadata_loading(&self, light_entries: Vec<LightObjectInfo>) {
        if !light_entries.is_empty() {
            batch_load_metadata_task(
                self.path.clone(),
                light_entries,
                self.action_tx.clone(),
                5, // Metadata batch size
                self.cache.clone(),
            );
        }
    }
}