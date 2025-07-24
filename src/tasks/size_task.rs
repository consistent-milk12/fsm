//! src/tasks/size_task.rs
//! ============================================================================
//! # Size Task: Background Directory Size Calculation
//!
//! This module provides a background task for calculating the recursive size
//! and direct item count of directories without blocking the UI.

use crate::controller::actions::Action;
use crate::fs::object_info::ObjectInfo;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{info, warn};
use walkdir::WalkDir;

/// Spawns a Tokio task to calculate the recursive size and direct item count for a directory.
/// Size is calculated recursively (all files in subdirectories), but item count only
/// includes direct children (files + folders in the immediate directory).
///
/// Once calculated, it sends an `Action::UpdateObjectInfo` to the main event loop
/// to update the UI.
pub fn calculate_size_task(
    parent_dir: PathBuf,
    mut object_info: ObjectInfo,
    action_tx: mpsc::UnboundedSender<Action>,
) {
    if !object_info.is_dir {
        return;
    }

    let path: PathBuf = object_info.path.clone();
    info!(
        "Spawning size calculation task for directory: {}",
        &path.display()
    );

    // This will be moved, hence we need a clone.
    let m_path: PathBuf = path.clone();

    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let mut total_size: u64 = 0;
            let mut items_count: usize = 0;

            let tmp: PathBuf = path.clone();
            // Calculate recursive size for files, but only count direct children
            for entry in WalkDir::new(&tmp)
                .min_depth(1)
                .into_iter()
                .filter_map(Result::ok)
            {
                if let Ok(metadata) = entry.metadata()
                    && metadata.is_file()
                {
                    total_size += metadata.len();
                }
            }

            // Count only direct children (files + directories)
            if let Ok(entries) = std::fs::read_dir(&tmp) {
                for _entry in entries.filter_map(Result::ok) {
                    items_count += 1;
                }
            }

            (total_size, items_count)
        })
        .await;

        match result {
            Ok((total_size, items_count)) => {
                if total_size > 0 || items_count > 0 {
                    object_info.size = total_size;
                    object_info.items_count = items_count;

                    let action = Action::UpdateObjectInfo {
                        parent_dir,
                        info: object_info,
                    };
                    if let Err(e) = action_tx.send(action) {
                        warn!("Failed to send object info update: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to calculate directory size for {}: {}",
                    m_path.display(),
                    e
                );
            }
        }
    });
}
