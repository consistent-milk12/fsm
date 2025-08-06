
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
use std::{path::PathBuf, sync::Arc};
use tokio::{sync::mpsc, task::{self as TokioScheduler, JoinError}};
use tracing::{info, warn};
use walkdir::WalkDir;

/// Spawns a Tokio task to calculate the recursive size and direct item count for a directory.
///
/// Size is calculated recursively (all files in subdirectories), but item count only
/// includes direct children (files + folders in the immediate directory).
///
/// Results are cached for performance and UI responsiveness.
/// Once calculated, it sends an `Action::UpdateObjectInfo` to the main event loop
/// to update the UI.
pub fn calculate_size_task(
    parent_dir: PathBuf,
    mut object_info: ObjectInfo,
    action_tx: mpsc::UnboundedSender<Action>,
    cache: Arc<ObjectInfoCache>,
) {
    if !object_info.is_dir {
        return;
    }

    let path: PathBuf = object_info.path.clone();

    tokio::spawn(async move {
        // Check cache first for existing size calculation
        if let Some(cached_info) = cache.get_by_path(&path).await
            && (cached_info.size > 0 || cached_info.items_count > 0) 
        {
            info!("Using cached size for directory: {} (size: {}, items: {})",
                path.display(), cached_info.size, cached_info.items_count);

            // Send cached result immediately
            let _ = action_tx.send(Action::UpdateObjectInfo {
                parent_dir,
                info: cached_info,
            });
        
            return;
        }

        info!("Calculating new size for directory: {}", path.display());

        // Perform expensive calculation in blocking task
        let path_clone: PathBuf = path.clone();
        let result: Result<(u64, usize), JoinError> = TokioScheduler::spawn_blocking(
            move || -> (u64, usize)
            {
                let mut total_size: u64 = 0;
                let mut items_count: usize = 0;

                // Calculate recursive size for files, but only count direct children
                for entry in WalkDir::new(&path_clone)
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

        match result {
            Ok((size, items)) => {
                // Update object info with calculated values
                object_info.size = size;
                object_info.items_count = items as u64;
                object_info.metadata_loaded = true;

                info!(
                    "Size calculation completed for {0}: {size} bytes, {items} items",
                    path.display()
                );

                // Cache the result for future use
                cache.insert_path(&path, object_info.clone()).await;

                // Send updated info to UI
                let _ = action_tx.send(Action::UpdateObjectInfo {
                    parent_dir,
                    info: object_info,
                });
            }
            
            Err(e) => {
                warn!("Size calculation failed for {path:?}: {e}");
            }
        }
    });
}