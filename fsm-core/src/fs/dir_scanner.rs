//! High-performance directory scanner with streaming and background metadata

use anyhow::Result;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;
use tracing::debug;

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};

/// Scan update for streaming directory operations
#[derive(Debug, Clone)]
pub enum ScanUpdate {
    /// New entry discovered (immediate display)
    EntryAdded(ObjectInfo),
    /// Batch of entries processed
    BatchComplete {
        processed: usize,
        total: Option<usize>,
    },
    /// Scanning completed
    ScanComplete {
        total_entries: usize,
        exec: Duration,
    },
    /// Error during scanning
    ScanError(String),
}

/// High-performance directory scanner with streaming results
pub fn spawn_directory_scan(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    task_tx: UnboundedSender<TaskResult>,
) -> JoinHandle<Result<Vec<ObjectInfo>>> {
    tokio::spawn(async move {
        let start_time = Instant::now();

        match scan_directory_fast(&path, show_hidden).await {
            Ok(entries) => {
                let task_result = TaskResult::DirectoryLoad {
                    task_id,
                    path: path.clone(),
                    result: Ok(entries.clone()),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);

                Ok(entries)
            }
            Err(e) => {
                let app_error: AppError = AppError::Io(std::io::Error::other(e.to_string()));

                let task_result: TaskResult = TaskResult::DirectoryLoad {
                    task_id,
                    path: path.clone(),
                    result: Err(Arc::new(app_error)),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
                Err(e)
            }
        }
    })
}

/// Fast directory scanning with light metadata only
async fn scan_directory_fast(path: &Path, show_hidden: bool) -> Result<Vec<ObjectInfo>> {
    debug!("Fast scanning directory: {}", path.display());

    let mut entries: Vec<ObjectInfo> = Vec::new();
    let mut read_dir: fs::ReadDir = fs::read_dir(path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path = entry.path();

        // Filter hidden files
        if !show_hidden {
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
        }

        match ObjectInfo::from_path_light(&entry_path).await {
            Ok(light_info) => {
                entries.push(ObjectInfo::with_placeholder_metadata(light_info));
            }
            Err(e) => {
                debug!("Failed to read entry {}: {}", entry_path.display(), e);
            }
        }
    }

    // Sort: directories first, then alphabetical
    entries.sort_by(
        |a: &ObjectInfo, b: &ObjectInfo| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.cmp(&b.name),
        },
    );

    debug!("Scanned {} entries from {}", entries.len(), path.display());
    Ok(entries)
}

/// Streaming directory scanner with progress updates
pub fn spawn_streaming_directory_scan(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    task_tx: UnboundedSender<TaskResult>,
) -> (
    mpsc::UnboundedReceiver<ScanUpdate>,
    JoinHandle<Result<Vec<ObjectInfo>>>,
) {
    let (update_tx, update_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let start_time = Instant::now();
        let mut entries = Vec::new();
        let mut processed = 0;

        let mut read_dir = match fs::read_dir(&path).await {
            Ok(rd) => rd,
            Err(e) => {
                let error_msg = format!("Failed to read directory: {e}");
                let _ = update_tx.send(ScanUpdate::ScanError(error_msg.clone()));

                let app_error = AppError::Io(e);
                let task_result = TaskResult::DirectoryLoad {
                    task_id,
                    path: path.clone(),
                    result: Err(Arc::new(app_error)),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);

                return Err(anyhow::anyhow!("Directory read failed"));
            }
        };

        while let Some(entry_result) = read_dir.next_entry().await.transpose() {
            let entry: fs::DirEntry = match entry_result {
                Ok(e) => e,

                Err(e) => {
                    let error_msg: String = format!("Failed to read entry: {e}");

                    let _ = update_tx.send(ScanUpdate::ScanError(error_msg));

                    continue;
                }
            };

            let entry_path = entry.path();

            // Filter hidden files
            if !show_hidden {
                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue;
                    }
                }
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    let object_info: ObjectInfo = ObjectInfo::with_placeholder_metadata(light_info);

                    // Send immediate update for UI
                    let _ = update_tx.send(ScanUpdate::EntryAdded(object_info.clone()));
                    entries.push(object_info);
                    processed += 1;

                    // Send batch progress
                    if processed % batch_size == 0 {
                        let _ = update_tx.send(ScanUpdate::BatchComplete {
                            processed,
                            total: None,
                        });

                        // TODO: Fix percentage here
                        // Report progress to task system
                        let progress_result: TaskResult = TaskResult::Progress {
                            task_id,
                            pct: processed as f32,
                            msg: Some(format!("Scanned {processed} entries")),
                        };

                        let _ = task_tx.send(progress_result);

                        // Yield for responsiveness
                        tokio::task::yield_now().await;
                    }
                }
                Err(e) => {
                    let error_msg = format!("Failed to read {}: {}", entry_path.display(), e);
                    let _ = update_tx.send(ScanUpdate::ScanError(error_msg));
                }
            }
        }

        // Sort entries
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        let exec = start_time.elapsed();

        // Send completion update
        let _ = update_tx.send(ScanUpdate::ScanComplete {
            total_entries: entries.len(),
            exec,
        });

        // Send task completion
        let task_result: TaskResult = TaskResult::DirectoryLoad {
            task_id,
            path: path.clone(),
            result: Ok(entries.clone()),
            exec,
        };
        let _ = task_tx.send(task_result);

        debug!(
            "Streaming scan completed: {} entries in {:?}",
            entries.len(),
            exec
        );

        Ok(entries)
    });

    (update_rx, handle)
}

/// Two-phase scanner: immediate display + background metadata loading
pub fn spawn_two_phase_directory_scan(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    task_tx: UnboundedSender<TaskResult>,
) -> JoinHandle<Result<Vec<ObjectInfo>>> {
    tokio::spawn(async move {
        let start_time = Instant::now();

        // Phase 1: Quick scan for immediate display
        let (entries, light_entries) = match scan_with_light_metadata(&path, show_hidden).await {
            Ok(result) => result,
            Err(e) => {
                let app_error = AppError::Io(std::io::Error::other(e.to_string()));

                let task_result = TaskResult::DirectoryLoad {
                    task_id,
                    path: path.clone(),
                    result: Err(app_error.into()),
                    exec: start_time.elapsed(),
                };
                let _ = task_tx.send(task_result);

                return Err(e);
            }
        };

        // Send quick results for immediate display
        let quick_result = TaskResult::DirectoryLoad {
            task_id,
            path: path.clone(),
            result: Ok(entries.clone()),
            exec: start_time.elapsed(),
        };
        let _ = task_tx.send(quick_result);

        // Phase 2: Background metadata loading
        if !light_entries.is_empty() {
            crate::operators::metadata_task::spawn_batch_metadata_load(
                task_id + 1000, // Different task ID for metadata
                path.clone(),
                light_entries,
                task_tx.clone(),
                10, // Batch size
            );
        }

        Ok(entries)
    })
}

/// Scan directory and collect both full entries and light metadata
async fn scan_with_light_metadata(
    path: &Path,
    show_hidden: bool,
) -> Result<(Vec<ObjectInfo>, Vec<LightObjectInfo>)> {
    let mut entries = Vec::new();
    let mut light_entries = Vec::new();
    let mut read_dir = fs::read_dir(path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path = entry.path();

        // Filter hidden files
        if !show_hidden {
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
        }

        match ObjectInfo::from_path_light(&entry_path).await {
            Ok(light_info) => {
                let object_info = ObjectInfo::with_placeholder_metadata(light_info.clone());
                entries.push(object_info);
                light_entries.push(light_info);
            }
            Err(e) => {
                debug!("Failed to read entry {}: {}", entry_path.display(), e);
            }
        }
    }

    // Sort entries
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    Ok((entries, light_entries))
}
