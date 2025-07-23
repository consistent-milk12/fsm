//! src/tasks/metadata_task.rs
//! ============================================================================
//! # Background Metadata Loading Task
//!
//! Loads expensive metadata (size, modification time, item count) in the background
//! for better UI responsiveness.

use crate::controller::actions::Action;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Spawn a background task to load full metadata for an entry
pub fn load_metadata_task(
    parent_dir: PathBuf,
    light_info: LightObjectInfo,
    action_tx: mpsc::UnboundedSender<Action>,
) {
    tokio::spawn(async move {
        debug!("Loading metadata for: {}", light_info.path.display());

        match ObjectInfo::from_light_info(light_info).await {
            Ok(full_info) => {
                let _ = action_tx.send(Action::UpdateObjectInfo {
                    parent_dir,
                    info: full_info,
                });
            }
            Err(e) => {
                warn!("Failed to load metadata for {:?}: {}", parent_dir, e);
            }
        }
    });
}

/// Batch load metadata for multiple entries
pub fn batch_load_metadata_task(
    parent_dir: PathBuf,
    light_entries: Vec<LightObjectInfo>,
    action_tx: mpsc::UnboundedSender<Action>,
    batch_size: usize,
) {
    tokio::spawn(async move {
        debug!(
            "Batch loading metadata for {} entries in {}",
            light_entries.len(),
            parent_dir.display()
        );

        let mut count: usize = 0;
        for light_info in light_entries {
            let light_info_path = light_info.path.clone();
            match ObjectInfo::from_light_info(light_info).await {
                Ok(full_info) => {
                    debug!(
                        "Metadata task sending full_info for {}: modified = {}",
                        full_info.path.display(),
                        full_info.modified.format("%Y-%m-%d")
                    );

                    let _ = action_tx.send(Action::UpdateObjectInfo {
                        parent_dir: parent_dir.clone(),
                        info: full_info,
                    });
                }

                Err(e) => {
                    debug!("Failed to load metadata for {:?}: {}", light_info_path, e);
                    warn!("Failed to load metadata for {:?}: {}", parent_dir, e);
                }
            }

            count += 1;

            // Yield control periodically to avoid blocking
            if count % batch_size == 0 {
                tokio::task::yield_now().await;
            }
        }

        debug!(
            "Completed batch metadata loading for {}",
            parent_dir.display()
        );
    });
}
