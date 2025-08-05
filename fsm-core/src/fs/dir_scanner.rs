//! ``src/fs/dir_scanner.rs``
//!
//! # `Directory Scanner`: Asynchronous Filesystem Listing
//!
//! Provides an asynchronous function to scan a directory and return a sorted
//! list of `ObjectInfo` entries. Designed for non-blocking UI updates.

use crate::{config::ProfilingConfig, controller::actions::Action, logging::ProfilingData, tasks::metadata_task::batch_load_metadata_task};
use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
use std::{cmp::Ordering, ffi::OsStr, path::{Path, PathBuf}, time::Duration};
use std::time::{Instant, SystemTime};
use tokio::{fs::{self, DirEntry, ReadDir}, sync::mpsc::{UnboundedReceiver, UnboundedSender}};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::info;

/// Scans the given directory asynchronously and returns a sorted list of `ObjectInfo`.
///
/// # Arguments
/// * `path` - The path to the directory to scan.
/// * `show_hidden` - Whether to include hidden files/directories (starting with '.').
/// * `profiling_config` - The configuration for performance profiling.
pub async fn scan_dir(
    path: &Path,
    show_hidden: bool,
    profiling_config: &ProfilingConfig,
) -> Result<Vec<ObjectInfo>, AppError> {
    let start_time = Instant::now();
    let start_mem = ProfilingData::get_current_memory_kb();

    let mut entries: Vec<ObjectInfo> = Vec::new();
    let mut read_dir: ReadDir = fs::read_dir(path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path: PathBuf = entry.path();

        let file_name: &str = entry_path
            .file_name()
            .and_then(|s: &std::ffi::OsStr| s.to_str())
            .unwrap_or("");

        if !show_hidden && file_name.starts_with('.') {
            continue;
        }

        match ObjectInfo::from_path(&entry_path).await {
            Ok(info) => entries.push(info),

            Err(e) => {
                // Log the error but continue processing other entries
                tracing::info!("Failed to get ObjectInfo for {:?}: {}", entry_path, e);
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
    
    if let Some(duration_ns) = profiling_data.operation_duration_ns {
        info!(
            marker = "PERF_DIRECTORY_SCAN",
            operation_type = "scan_dir",
            duration_ns = duration_ns,
            memory_delta_kb = profiling_data.memory_delta_kb.unwrap_or(0),
            "Directory scan completed in {:?}",
            duration
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
///
/// # Returns
/// * A receiver channel that will receive `ScanUpdate` messages
/// * A sender for the final sorted results
#[allow(clippy::unused_async, reason = "async move occurs inside tokio::spawn")]
pub async fn scan_dir_streaming_with_background_metadata(
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    action_tx: UnboundedSender<Action>,
    profiling_config: ProfilingConfig,
) -> (
    UnboundedReceiver<ScanUpdate>,
    JoinHandle<Result<Vec<ObjectInfo>, AppError>>,
) {
    let (
        tx, 
        rx
    ): (UnboundedSender<ScanUpdate>, UnboundedReceiver<ScanUpdate>) = mpsc::unbounded_channel();

    let handle: JoinHandle<Result<Vec<ObjectInfo>, AppError>> = tokio::spawn(
        async move 
        {
            let scanner: DirectoryScanner = DirectoryScanner::new(
                path, 
                show_hidden, 
                batch_size, 
                action_tx, 
                tx, 
                profiling_config
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
}

impl DirectoryScanner {
    const fn new(
        path: PathBuf,
        show_hidden: bool,
        batch_size: usize,
        action_tx: UnboundedSender<Action>,
        tx: UnboundedSender<ScanUpdate>,
        profiling_config: ProfilingConfig,
    ) -> Self {
        Self {
            path,
            show_hidden,
            batch_size,
            action_tx,
            tx,
            profiling_config,
        }
    }

    async fn scan(self) -> Result<Vec<ObjectInfo>, AppError> {
        let start_time: Instant = Instant::now();
        let start_mem: Option<i64> = ProfilingData::get_current_memory_kb();

        let mut entries: Vec<ObjectInfo> = Vec::new();
        let mut light_entries: Vec<LightObjectInfo> = Vec::new();

        // Phase 1: Quick scan for basic info
        self.perform_quick_scan(&mut entries, &mut light_entries)
            .await?;

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

        if let Some(duration_ns) = profiling_data.operation_duration_ns {
            info!(
                marker = "PERF_DIRECTORY_SCAN",
                operation_type = "scan_dir_streaming",
                duration_ns = duration_ns,
                memory_delta_kb = profiling_data.memory_delta_kb.unwrap_or(0),
                "Streaming directory scan completed in {:?}",
                duration
            );
        }

        Ok(entries)
    }

    async fn perform_quick_scan(
        &self,
        entries: &mut Vec<ObjectInfo>,
        light_entries: &mut Vec<LightObjectInfo>,
    ) -> Result<(), AppError> {
        let mut read_dir: ReadDir = self.initialize_directory_reader().await?;
        let mut processed: u64 = 0u64;

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

            if self
                .process_directory_entry(
                    entry,
                    entries,
                    light_entries
                )
                .await
            {
                processed += 1;

                // Yield control periodically for responsiveness
                if processed.is_multiple_of(self.batch_size as u64) {
                    tokio::task::yield_now().await;
                }
            } else {
                // Receiver dropped, stop scanning
                break;
            }
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

    async fn process_directory_entry(
        &self,
        entry: DirEntry,
        entries: &mut Vec<ObjectInfo>,
        light_entries: &mut Vec<LightObjectInfo>,
    ) -> bool {
        let entry_path: PathBuf = entry.path();

        match LightObjectInfo::from_path(&entry_path).await {
            Ok(light_info) => {
                let placeholder_info: ObjectInfo = ObjectInfo {
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

                // Send streaming update immediately
                if self
                    .tx
                    .send(
                        ScanUpdate::Entry(placeholder_info.clone())
                    )
                    .is_err()
                {
                    return false; // Receiver dropped
                }

                entries.push(placeholder_info);
                light_entries.push(light_info);

                true
            }
            Err(e) => {
                tracing::info!("Failed to get basic info for {:?}: {}", entry_path, e);

                let _ = self.tx.send(ScanUpdate::Error(e.to_string()));

                true
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

    fn start_background_metadata_loading(&self, light_entries: Vec<LightObjectInfo>) {
        if !light_entries.is_empty() {
            batch_load_metadata_task(
                self.path.clone(),
                light_entries,
                self.action_tx.clone(),
                5, // Metadata batch size
            );
        }
    }
}