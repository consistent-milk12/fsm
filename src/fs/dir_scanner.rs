//! src/fs/dir_scanner.rs
//! ============================================================================
//! # Directory Scanner: Asynchronous Filesystem Listing
//!
//! Provides an asynchronous function to scan a directory and return a sorted
//! list of `ObjectInfo` entries. Designed for non-blocking UI updates.

use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::mpsc;

/// Scans the given directory asynchronously and returns a sorted list of `ObjectInfo`.
///
/// # Arguments
/// * `path` - The path to the directory to scan.
/// * `show_hidden` - Whether to include hidden files/directories (starting with '.').
pub async fn scan_dir(path: &Path, show_hidden: bool) -> Result<Vec<ObjectInfo>, AppError> {
    let mut entries: Vec<ObjectInfo> = Vec::new();
    let mut read_dir: fs::ReadDir = fs::read_dir(path).await?;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path: PathBuf = entry.path();
        let file_name: &str = entry_path
            .file_name()
            .and_then(|s: &std::ffi::OsStr| s.to_str())
            .unwrap_or("");

        if !show_hidden && file_name.starts_with(".") {
            continue;
        }

        match ObjectInfo::from_path(&entry_path).await {
            Ok(info) => entries.push(info),
            Err(e) => {
                // Log the error but continue processing other entries
                tracing::warn!("Failed to get ObjectInfo for {:?}: {}", entry_path, e);
            }
        }
    }

    // Sort entries: directories first, then alphabetically by name
    entries.sort_by(|a, b| {
        if a.is_dir && !b.is_dir {
            std::cmp::Ordering::Less
        } else if !a.is_dir && b.is_dir {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    Ok(entries)
}

/// Represents a scanning progress update
#[derive(Debug, Clone, PartialEq)]
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
///
/// # Returns
/// * A receiver channel that will receive `ScanUpdate` messages
/// * A sender for the final sorted results
pub async fn scan_dir_streaming_with_background_metadata(
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    action_tx: mpsc::UnboundedSender<crate::controller::actions::Action>,
) -> (
    mpsc::UnboundedReceiver<ScanUpdate>,
    tokio::task::JoinHandle<Result<Vec<ObjectInfo>, AppError>>,
) {
    let (tx, rx) = mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let mut entries: Vec<ObjectInfo> = Vec::new();
        let mut light_entries: Vec<LightObjectInfo> = Vec::new();
        let mut processed = 0;

        let read_dir_result = fs::read_dir(&path).await;
        let mut read_dir = match read_dir_result {
            Ok(rd) => rd,
            Err(e) => {
                let app_error = AppError::from(e);
                let _ = tx.send(ScanUpdate::Error(app_error.to_string()));
                return Err(app_error);
            }
        };

        // Phase 1: Quick scan for basic info
        while let Some(entry_result) = read_dir.next_entry().await.transpose() {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    let app_error = AppError::from(e);
                    let _ = tx.send(ScanUpdate::Error(app_error.to_string()));
                    continue;
                }
            };

            let entry_path = entry.path();
            let file_name = entry_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if !show_hidden && file_name.starts_with(".") {
                continue;
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    // Create ObjectInfo with placeholder metadata for immediate display
                    let placeholder_info =
                        ObjectInfo::with_placeholder_metadata(light_info.clone());

                    // Send streaming update immediately
                    if tx
                        .send(ScanUpdate::Entry(placeholder_info.clone()))
                        .is_err()
                    {
                        // Receiver dropped, stop scanning
                        break;
                    }

                    entries.push(placeholder_info);
                    light_entries.push(light_info);
                    processed += 1;

                    // Yield control periodically for responsiveness
                    if processed % batch_size == 0 {
                        tokio::task::yield_now().await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get basic info for {:?}: {}", entry_path, e);
                    let _ = tx.send(ScanUpdate::Error(e.to_string()));
                }
            }
        }

        // Sort entries: directories first, then alphabetically by name
        entries.sort_by(|a, b| {
            if a.is_dir && !b.is_dir {
                std::cmp::Ordering::Less
            } else if !a.is_dir && b.is_dir {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        // Send completion notification
        let _ = tx.send(ScanUpdate::Completed(entries.len()));

        // Phase 2: Start background metadata loading
        if !light_entries.is_empty() {
            crate::tasks::metadata_task::batch_load_metadata_task(
                path.clone(),
                light_entries,
                action_tx,
                5, // Metadata batch size
            );
        }

        Ok(entries)
    });

    (rx, handle)
}

/// Original streaming scanner (kept for compatibility)
pub async fn scan_dir_streaming(
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
) -> (
    mpsc::UnboundedReceiver<ScanUpdate>,
    tokio::task::JoinHandle<Result<Vec<ObjectInfo>, AppError>>,
) {
    let (tx, rx) = mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let mut entries: Vec<ObjectInfo> = Vec::new();
        let mut processed = 0;

        let read_dir_result = fs::read_dir(&path).await;
        let mut read_dir = match read_dir_result {
            Ok(rd) => rd,
            Err(e) => {
                let app_error = AppError::from(e);
                let _ = tx.send(ScanUpdate::Error(app_error.to_string()));
                return Err(app_error);
            }
        };

        while let Some(entry_result) = read_dir.next_entry().await.transpose() {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    let app_error = AppError::from(e);
                    let _ = tx.send(ScanUpdate::Error(app_error.to_string()));
                    continue;
                }
            };

            let entry_path = entry.path();
            let file_name = entry_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if !show_hidden && file_name.starts_with(".") {
                continue;
            }

            match ObjectInfo::from_path(&entry_path).await {
                Ok(info) => {
                    // Send streaming update
                    if tx.send(ScanUpdate::Entry(info.clone())).is_err() {
                        // Receiver dropped, stop scanning
                        break;
                    }
                    entries.push(info);
                    processed += 1;

                    // Yield control periodically for responsiveness
                    if processed % batch_size == 0 {
                        tokio::task::yield_now().await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get ObjectInfo for {:?}: {}", entry_path, e);
                    let _ = tx.send(ScanUpdate::Error(e.to_string()));
                }
            }
        }

        // Sort entries: directories first, then alphabetically by name
        entries.sort_by(|a, b| {
            if a.is_dir && !b.is_dir {
                std::cmp::Ordering::Less
            } else if !a.is_dir && b.is_dir {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        // Send completion notification
        let _ = tx.send(ScanUpdate::Completed(entries.len()));

        Ok(entries)
    });

    (rx, handle)
}
