//! Optimized metadata loading with batch processing and caching
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info};

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};

/// Spawn single metadata loading task
#[allow(unused)]
pub fn spawn_metadata_load(
    task_id: u64,
    parent_dir: PathBuf,
    light_info: LightObjectInfo,
    task_tx: UnboundedSender<TaskResult>,
) {
    tokio::spawn(async move {
        let start_time: Instant = Instant::now();
        let path: PathBuf = light_info.path.clone();

        match ObjectInfo::from_light_info(light_info).await {
            Ok(full_info) => {
                debug!("Loaded metadata for: {}", path.display());

                let task_result: TaskResult = TaskResult::Generic {
                    task_id,
                    result: Ok(()),
                    message: Some(format!("Metadata loaded for {}", path.display())),
                    execution_time: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);

                // Note: In the new architecture, metadata updates would be handled
                // differently - possibly through a dedicated metadata cache or
                // direct state updates via the action dispatcher
            }
            Err(e) => {
                debug!("Failed to load metadata for {}: {e}", path.display());

                let task_result: TaskResult = TaskResult::Generic {
                    task_id,
                    result: Err(AppError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))),
                    message: Some(format!("Metadata load failed for {}", path.display())),
                    execution_time: start_time.elapsed(),
                };

                let _ = task_tx.send(task_result);
            }
        }
    });
}

/// Spawn batch metadata loading task with yield points
pub fn spawn_batch_metadata_load(
    task_id: u64,
    parent_dir: PathBuf,
    light_entries: Vec<LightObjectInfo>,
    task_tx: UnboundedSender<TaskResult>,
    batch_size: usize,
) {
    tokio::spawn(async move {
        let start_time: Instant = Instant::now();
        let total_entries: usize = light_entries.len();

        debug!(
            "Starting batch metadata load for {total_entries} entries in {}",
            parent_dir.display()
        );

        let mut processed: usize = 0;
        let mut successful: i32 = 0;

        for (index, light_info) in light_entries.into_iter().enumerate() {
            let path: PathBuf = light_info.path.clone();

            match ObjectInfo::from_light_info(light_info).await {
                Ok(_full_info) => {
                    successful += 1;
                    debug!("Loaded metadata for: {}", path.display());
                }
                Err(e) => {
                    debug!("Failed to load metadata for {}: {}", path.display(), e);
                }
            }

            processed += 1;

            // Report progress periodically
            if processed % batch_size == 0 || processed == total_entries {
                let progress_result: TaskResult = TaskResult::Progress {
                    task_id,
                    current: processed as u64,
                    total: total_entries as u64,
                    message: Some(format!(
                        "Loaded {} of {} metadata entries",
                        processed, total_entries
                    )),
                };

                let _ = task_tx.send(progress_result);
            }

            // Yield control periodically
            if index % batch_size == 0 {
                tokio::task::yield_now().await;
            }
        }

        let execution_time: Duration = start_time.elapsed();
        info!(
            "Batch metadata loading completed: {}/{} successful in {:?}",
            successful, total_entries, execution_time
        );

        let completion_result: TaskResult = TaskResult::Generic {
            task_id,
            result: Ok(()),
            message: Some(format!(
                "Batch metadata completed: {successful}/{total_entries} successful",
            )),
            execution_time,
        };

        let _ = task_tx.send(completion_result);
    });
}
