//! ``src/tasks/metadata_task.rs``
//! ============================================================================
//! # Background Metadata Loading Task
//!
//! Loads expensive metadata (size, modification time, item count) in the background
//! for better UI responsiveness with cache integration.

use crate::{cache::cache_manager::ObjectInfoCache, controller::actions::Action};
use crate::fs::object_info::{LightObjectInfo};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, Instrument};

/// Spawn a background task to load full metadata for an entry
#[instrument(skip(action_tx, cache), fields(path = %light_info.path.display()))]
pub fn load_metadata_task(
    parent_dir: PathBuf,
    light_info: LightObjectInfo,
    action_tx: mpsc::UnboundedSender<Action>,
    cache: Arc<ObjectInfoCache>,
) {
    let light_info_path: Arc<PathBuf> = light_info.path.clone();
    tokio::spawn(
        async move {
            info!(
                marker = "METADATA_TASK",
                operation_type = "single_metadata_load",
                path = %light_info.path.display(),
                "Starting single metadata load"
            );

            // Check cache first, then load if not present
            let load_start = std::time::Instant::now();
            match cache.get_or_load_path(
                &**light_info.path, 
                || light_info.clone().into_full_info()
            ).await {
                Ok(full_info) => {
                    let load_duration = load_start.elapsed();
                    info!(
                        marker = "METADATA_TASK",
                        operation_type = "metadata_load_success",
                        path = %full_info.path.display(),
                        load_duration_ms = load_duration.as_millis(),
                        "Metadata loaded successfully"
                    );

                    let _ = action_tx.send(Action::UpdateObjectInfo {
                        parent_dir: Arc::new(parent_dir),
                        info: full_info,
                    });
                }
            
                Err(e) => {
                    let load_duration = load_start.elapsed();
                    info!(
                        marker = "METADATA_TASK",
                        operation_type = "metadata_load_failure",
                        path = %light_info.path.display(),
                        load_duration_ms = load_duration.as_millis(),
                        error = %e,
                        "Failed to load metadata"
                    );
                }
            }
        }
        .instrument(tracing::info_span!(
            "single_metadata_load",
            operation_type = "single_metadata_load",
            path = %light_info_path.display()
        ))
    );
}

/// Batch load metadata for multiple entries
#[expect(clippy::cast_precision_loss, reason = "Expected loss")]
#[instrument(
    skip(light_entries, action_tx, cache),
    fields(
        parent_dir = %parent_dir.display(),
        entry_count = light_entries.len(),
        batch_size = batch_size
    )
)]
pub fn batch_load_metadata_task(
    parent_dir: Arc<PathBuf>,
    light_entries: Vec<LightObjectInfo>,
    action_tx: mpsc::UnboundedSender<Action>,
    batch_size: usize,
    cache: Arc<ObjectInfoCache>
) {
    let entry_count = light_entries.len();
    let parent_dir_display = parent_dir.display().to_string();
    
    tokio::spawn(
        async move {
            let batch_start = std::time::Instant::now();
            info!(
                marker = "METADATA_TASK",
                operation_type = "batch_metadata_load_start",
                parent_dir = %parent_dir.display(),
                entry_count = entry_count,
                batch_size = batch_size,
                "Starting batch metadata load"
            );

            let mut count: usize = 0;
            let mut success_count: usize = 0;
            let mut error_count: usize = 0;
            let mut cache_hits: usize = 0;
            let mut cache_misses: usize = 0;

            let value = parent_dir.clone();

            for light_info in light_entries {
                let light_info_path = light_info.path.clone();
                let item_start = std::time::Instant::now();
                
                match cache.get_or_load_path(
                    &**light_info_path, 
                    || {
                        cache_misses += 1;
                        light_info.into_full_info()
                    }
                ).await {
                    Ok(full_info) => {
                        let item_duration = item_start.elapsed();
                        
                        // Fast lookup indicates cache hit
                        if item_duration.as_micros() < 100 {
                            cache_hits += 1;
                        }
                        
                        debug!(
                            marker = "METADATA_TASK",
                            operation_type = "metadata_item_success",
                            path = %full_info.path.display(),
                            load_duration_us = item_duration.as_micros(),
                            "Metadata item loaded successfully"
                        );
                        
                        let _ = action_tx.send(Action::UpdateObjectInfo {
                            parent_dir: value.clone(),
                            info: full_info,
                        });
                        
                        success_count += 1;
                    }

                    Err(e) => {
                        let item_duration = item_start.elapsed();
                        debug!(
                            marker = "METADATA_TASK",
                            operation_type = "metadata_item_failure",
                            path = %light_info_path.display(),
                            load_duration_us = item_duration.as_micros(),
                            error = %e,
                            "Failed to load metadata item"
                        );
                        
                        error_count += 1;
                    }
                }

                count += 1;

                // Yield control periodically to avoid blocking
                if count.is_multiple_of(batch_size) {
                    tokio::task::yield_now().await;
                }
            }

            let batch_duration = batch_start.elapsed();
            let cache_hit_rate = if (cache_hits + cache_misses) > 0 {
                (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
            } else {
                0.0
            };

            info!(
                marker = "METADATA_TASK",
                operation_type = "batch_metadata_load_complete",
                parent_dir = %value.display(),
                total_count = count,
                success_count = success_count,
                error_count = error_count,
                cache_hits = cache_hits,
                cache_misses = cache_misses,
                cache_hit_rate = format!("{:.1}%", cache_hit_rate),
                batch_duration_ms = batch_duration.as_millis(),
                "Completed batch metadata loading"
            );
        }
        .instrument(tracing::info_span!(
            "batch_metadata_load",
            operation_type = "batch_metadata_load",
            parent_dir = %parent_dir_display,
            entry_count = entry_count
        ))
    );
}
