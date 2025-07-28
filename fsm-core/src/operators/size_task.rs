//! High-performance directory size calculation with progress reporting

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use crate::fs::object_info::ObjectInfo;

/// Spawn directory size calculation task
#[allow(unused)]
pub fn spawn_size_calculation(
    task_id: u64,
    parent_dir: PathBuf,
    mut object_info: ObjectInfo,
    task_tx: UnboundedSender<TaskResult>,
) {
    if !object_info.is_dir {
        return;
    }

    let path = object_info.path.clone();
    debug!("Starting size calculation for: {}", path.display());

    tokio::spawn(async move {
        let start_time = Instant::now();

        let result = tokio::task::spawn_blocking({
            let path = path.clone();
            move || calculate_directory_size(&path)
        })
        .await;

        match result {
            Ok(Ok((total_size, items_count))) => {
                let exec = start_time.elapsed();

                if total_size > 0 || items_count > 0 {
                    object_info.size = total_size;
                    object_info.items_count = items_count;

                    info!(
                        "Size calculation completed for {}: {} bytes, {} items ({:?})",
                        path.display(),
                        total_size,
                        items_count,
                        exec
                    );
                }

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Ok(()),
                    msg: Some(format!(
                        "Calculated size: {total_size} bytes, {items_count} items"
                    )),
                    exec,
                };

                let _ = task_tx.send(task_result);
            }
            Ok(Err(e)) => {
                warn!("Size calculation failed for {}: {}", path.display(), e);

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Io(e))),
                    msg: Some(format!("Size calculation failed for {}", path.display())),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
            }
            Err(e) => {
                warn!(
                    "Size calculation task panicked for {}: {}",
                    path.display(),
                    e
                );

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Io(std::io::Error::other(format!(
                        "Task panicked: {e}"
                    ))))),
                    msg: Some(format!(
                        "Size calculation task failed for {}",
                        path.display()
                    )),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
            }
        }
    });
}

/// Calculate directory size and item count efficiently
fn calculate_directory_size(path: &PathBuf) -> Result<(u64, usize), std::io::Error> {
    use std::fs;

    let mut total_size: u64 = 0u64;
    let mut items_count: usize = 0usize;

    // Use walkdir for efficient recursive traversal
    let walker = walkdir::WalkDir::new(path)
        .min_depth(1) // Skip the root directory itself
        .into_iter()
        .filter_map(|e| e.ok()); // Skip errors to continue processing

    for entry in walker {
        let entry_path: &Path = entry.path();

        if entry_path.is_file() {
            // Add file size to total
            if let Ok(metadata) = fs::metadata(entry_path) {
                total_size += metadata.len();
            }
        }
    }

    // Count only direct children (files + directories)
    if let Ok(entries) = fs::read_dir(path) {
        for _entry in entries.filter_map(Result::ok) {
            items_count += 1;
        }
    }

    Ok((total_size, items_count))
}

/// Spawn progressive size calculation with cancellation support
#[allow(unused)]
pub fn spawn_progressive_size_calculation(
    task_id: u64,
    parent_dir: PathBuf,
    mut object_info: ObjectInfo,
    task_tx: UnboundedSender<TaskResult>,
    report_interval: Duration,
) {
    if !object_info.is_dir {
        return;
    }

    let path = object_info.path.clone();
    debug!(
        "Starting progressive size calculation for: {}",
        path.display()
    );

    tokio::spawn(async move {
        let start_time = Instant::now();
        let mut _last_report = Instant::now();

        let result = tokio::task::spawn_blocking({
            let path = path.clone();
            let task_tx = task_tx.clone();
            let task_id = task_id;

            move || -> Result<(u64, usize), std::io::Error> {
                use std::fs;

                let mut total_size = 0u64;
                let mut items_count = 0usize;
                let mut file_count = 0u64;

                let walker = walkdir::WalkDir::new(&path)
                    .min_depth(1)
                    .into_iter()
                    .filter_map(|e| e.ok());

                for entry in walker {
                    let entry_path = entry.path();

                    if entry_path.is_file() {
                        if let Ok(metadata) = fs::metadata(entry_path) {
                            total_size += metadata.len();
                            file_count += 1;

                            // TODO: FIX PERCENTAGE
                            // Report progress periodically (every 100 files or 10MB)
                            if file_count % 100 == 0 || total_size % (10 * 1024 * 1024) == 0 {
                                let progress = TaskResult::Progress {
                                    task_id,
                                    pct: 0.0,
                                    msg: Some(format!(
                                        "Scanned {file_count} files, {total_size} bytes"
                                    )),
                                };

                                let _ = task_tx.send(progress);
                            }
                        }
                    }
                }

                // Count direct children
                if let Ok(entries) = fs::read_dir(&path) {
                    for _entry in entries.filter_map(Result::ok) {
                        items_count += 1;
                    }
                }

                Ok((total_size, items_count))
            }
        })
        .await;

        match result {
            Ok(Ok((total_size, items_count))) => {
                let exec = start_time.elapsed();

                if total_size > 0 || items_count > 0 {
                    object_info.size = total_size;
                    object_info.items_count = items_count;

                    info!(
                        "Progressive size calculation completed for {}: {} bytes, {} items ({:?})",
                        path.display(),
                        total_size,
                        items_count,
                        exec
                    );
                }

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Ok(()),
                    msg: Some(format!(
                        "Final size: {total_size} bytes, {items_count} items"
                    )),
                    exec,
                };

                let _ = task_tx.send(task_result);
            }
            Ok(Err(e)) => {
                warn!(
                    "Progressive size calculation failed for {}: {}",
                    path.display(),
                    e
                );

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Io(e))),
                    msg: Some(format!("Size calculation failed for {}", path.display())),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
            }
            Err(e) => {
                warn!(
                    "Progressive size calculation task panicked for {}: {}",
                    path.display(),
                    e
                );

                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Io(std::io::Error::other(format!(
                        "Task panicked: {e}"
                    ))))),
                    msg: Some(format!(
                        "Size calculation task failed for {}",
                        path.display()
                    )),
                    exec: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
            }
        }
    });
}
