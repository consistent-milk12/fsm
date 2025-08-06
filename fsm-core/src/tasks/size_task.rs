
// fsm-core/src/tasks/size_task.rs

//! ``src/tasks/size_task.rs``
//! ============================================================================
//! # Size Task: Background Directory Size Calculation with Cache Integration
//!
//! This module provides a background task for calculating the recursive size
//! and direct item count of directories without blocking the UI. Includes
//! cache integration for performance optimization.

use crate::cache::cache_manager::ObjectInfoCache;
use crate::controller::actions::Action;
use crate::fs::object_info::ObjectInfo;
use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};
use tokio::{sync::mpsc, task::{self as TokioScheduler, JoinError}};
use tracing::{info, warn, instrument, Instrument};
use walkdir::WalkDir;

/// Spawns a Tokio task to calculate the recursive size and direct item count for a directory.
///
/// Size is calculated recursively (all files in subdirectories), but item count only
/// includes direct children (files + folders in the immediate directory).
///
/// Results are cached for performance and UI responsiveness.
/// Once calculated, it sends an `Action::UpdateObjectInfo` to the main event loop
/// to update the UI.
#[expect(clippy::too_many_lines, reason = "Expected due to logging")]
#[instrument(
    skip(action_tx, cache, object_info),
    fields(
        path = %object_info.path.display(),
        is_dir = object_info.is_dir
    )
)]
pub fn calculate_size_task(
    parent_dir: Arc<PathBuf>,
    mut object_info: ObjectInfo,
    action_tx: mpsc::UnboundedSender<Action>,
    cache: Arc<ObjectInfoCache>,
) {
    if !object_info.is_dir {
        return;
    }

    let path: Arc<PathBuf> = object_info.path.clone();
    let path_display: String = path.display().to_string();

    tokio::spawn(
        async move {
            let task_start = Instant::now();
            
            info!(
                marker = "SIZE_TASK",
                operation_type = "size_calculation_start",
                path = %path.display(),
                "Starting directory size calculation"
            );

            // Check cache first for existing size calculation
            let cache_check_start = Instant::now();
            if let Some(cached_info) = cache.get_by_path(&*path).await
                && (cached_info.size > 0 || cached_info.items_count > 0) 
            {
                let cache_check_duration = cache_check_start.elapsed();
                info!(
                    marker = "SIZE_TASK",
                    operation_type = "size_cache_hit",
                    path = %path.display(),
                    cached_size = cached_info.size,
                    cached_items = cached_info.items_count,
                    cache_lookup_duration_us = cache_check_duration.as_micros(),
                    "Using cached size for directory"
                );

                // Send cached result immediately
                let _ = action_tx.send(Action::UpdateObjectInfo {
                    parent_dir,
                    info: cached_info,
                });
            
                return;
            }

            let cache_check_duration = cache_check_start.elapsed();
            info!(
                marker = "SIZE_TASK",
                operation_type = "size_cache_miss",
                path = %path.display(),
                cache_lookup_duration_us = cache_check_duration.as_micros(),
                "Cache miss - calculating new size for directory"
            );

            // Perform expensive calculation in blocking task
            let calculation_start = Instant::now();
            let path_for_calc = path.clone();
           
            let result: Result<(u64, usize), JoinError> = TokioScheduler::spawn_blocking(
                move || -> (u64, usize) {
                    let mut total_size: u64 = 0;
                    let mut items_count: usize = 0;
                    
                    // Calculate recursive size for files, but only count direct children
                    for entry in WalkDir::new(&**path_for_calc)
                        .min_depth(1)
                        .into_iter()
                        .filter_map(Result::ok)
                    {
                        if entry.depth() == 1 {
                            items_count += 1;
                        }

                        if let Ok(metadata) = entry.metadata() {
                            total_size += metadata.len();
                        }
                    }

                    (total_size, items_count)
                }
            )
            .await;

            let calculation_duration = calculation_start.elapsed();

            match result {
                Ok((size, items)) => {
                    // Update object info with calculated values
                    object_info.size = size;
                    object_info.items_count = items as u64;
                    object_info.metadata_loaded = true;

                    info!(
                        marker = "SIZE_TASK",
                        operation_type = "size_calculation_success",
                        calculated_size = size,
                        calculated_items = items,
                        calculation_duration_ms = calculation_duration.as_millis(),
                        total_duration_ms = task_start.elapsed().as_millis(),
                        "Size calculation completed successfully"
                    );

                    // Cache the result for future use
                    let cache_insert_start: Instant = Instant::now();
                    cache.insert_path((**path).to_path_buf(), object_info.clone()).await;
                    let cache_insert_duration: Duration = cache_insert_start.elapsed();
                    
                    tracing::debug!(
                        marker = "SIZE_TASK",
                        operation_type = "size_cache_insert",
                        path = %path.display(),
                        cache_insert_duration_us = cache_insert_duration.as_micros(),
                        "Cached size calculation result"
                    );

                    // Send updated info to UI
                    let _ = action_tx.send(Action::UpdateObjectInfo {
                        parent_dir,
                        info: object_info,
                    });
                }
                
                Err(e) => {
                    warn!(
                        marker = "SIZE_TASK",
                        operation_type = "size_calculation_failure",
                        path = %path.display(),
                        calculation_duration_ms = calculation_duration.as_millis(),
                        error = %e,
                        "Size calculation failed"
                    );
                }
            }
        }
        .instrument(tracing::info_span!(
            "size_calculation",
            operation_type = "size_calculation",
            path = %path_display
        ))
    );
}