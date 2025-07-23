//! src/fs/dir_scanner.rs
//! ============================================================================
//! # Directory Scanner: Asynchronous Filesystem Listing
//!
//! Provides an asynchronous function to scan a directory and return a sorted
//! list of `ObjectInfo` entries. Designed for non-blocking UI updates.

use crate::error::AppError;
use crate::fs::object_info::ObjectInfo;
use std::path::{Path, PathBuf};
use tokio::fs;

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
