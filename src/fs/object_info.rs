//! src/fs/object_info.rs
//! ============================================================================
//! # ObjectInfo: Rich Filesystem Entry Metadata
//!
//! Cross-platform, async-friendly abstraction for a file or directory entry.
//! Integrates with ObjectTable (for TUI), moka cache, and async tasks.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{FileType, Metadata};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Enum for object type, matching the table logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectType {
    Dir,
    File,
    Symlink,
    Other(String), // File extension or special case
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectType::Dir => write!(f, "Dir"),
            ObjectType::File => write!(f, "File"),
            ObjectType::Symlink => write!(f, "Symlink"),
            ObjectType::Other(ext) => write!(f, "{ext}"),
        }
    }
}

/// Core metadata struct for file or directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectInfo {
    pub path: PathBuf,
    pub name: String,
    pub extension: Option<String>,
    pub object_type: ObjectType,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub items_count: usize,      // Number of items in dir, 0 for files
    pub modified: DateTime<Utc>, // For display/sorting
}

impl ObjectInfo {
    /// Build from path and standard metadata. (You can adapt for async if needed.)
    pub async fn from_path(path: &Path) -> std::io::Result<Self> {
        use chrono::TimeZone;
        use tokio::fs;

        let metadata: Metadata = fs::symlink_metadata(path).await?;
        let file_type: FileType = metadata.file_type();
        let is_dir: bool = file_type.is_dir();
        let is_symlink: bool = file_type.is_symlink();

        let name: String = path
            .file_name()
            .map(|n: &OsStr| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from(""));

        let extension: Option<String> = path
            .extension()
            .map(|e: &OsStr| e.to_string_lossy().into_owned());

        let object_type: ObjectType = if is_dir {
            ObjectType::Dir
        } else if is_symlink {
            ObjectType::Symlink
        } else if let Some(ref ext) = extension {
            ObjectType::Other(ext.to_ascii_uppercase())
        } else {
            ObjectType::File
        };

        // Item count for directories is calculated in a background task.
        let items_count: usize = 0;

        // File size
        let size: u64 = if is_dir { 0 } else { metadata.len() };

        // Modification time, fall back to epoch on error
        let modified: DateTime<Utc> = metadata
            .modified()
            .ok()
            .and_then(|t: SystemTime| t.duration_since(UNIX_EPOCH).ok())
            .map(|d: Duration| Utc.timestamp_opt(d.as_secs_f32() as i64, 0).unwrap())
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());

        Ok(Self {
            path: path.to_path_buf(),
            name,
            extension,
            object_type,
            is_dir,
            is_symlink,
            size,
            items_count,
            modified,
        })
    }

    /// Human-friendly file size.
    pub fn size_human(&self) -> String {
        bytesize::ByteSize::b(self.size).to_string()
    }
}

// --- Default (empty entry, for error stubs/caching) ---
impl Default for ObjectInfo {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            name: String::new(),
            extension: None,
            object_type: ObjectType::File,
            is_dir: false,
            is_symlink: false,
            size: 0,
            items_count: 0,
            modified: chrono::Utc.timestamp_opt(0, 0).unwrap(),
        }
    }
}
