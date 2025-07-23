//! src/fs/object_info.rs
//! ============================================================================
//! # ObjectInfo: Rich Filesystem Entry Metadata
//!
//! Cross-platform, async-friendly abstraction for a file or directory entry.
//! Integrates with ObjectTable (for TUI), moka cache, and async tasks.

use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{FileType, Metadata};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

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
    pub items_count: usize,        // Number of items in dir, 0 for files
    pub modified: DateTime<Local>, // For display/sorting
    /// Whether full metadata has been loaded
    pub metadata_loaded: bool,
}

/// Lightweight version with just basic info for immediate display
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightObjectInfo {
    pub path: PathBuf,
    pub name: String,
    pub extension: Option<String>,
    pub object_type: ObjectType,
    pub is_dir: bool,
    pub is_symlink: bool,
}

impl ObjectInfo {
    /// Create a lightweight object with just basic info (fast)
    pub async fn from_path_light(path: &Path) -> std::io::Result<LightObjectInfo> {
        let metadata: Metadata = tokio::fs::metadata(path).await?;
        let file_type: FileType = metadata.file_type();

        let name: String = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("Unknown")
            .to_string();

        let extension = if file_type.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|ext| ext.to_lowercase())
        } else {
            None
        };

        let is_dir: bool = file_type.is_dir();
        let is_symlink: bool = file_type.is_symlink();

        let object_type: ObjectType = if is_dir {
            ObjectType::Dir
        } else if is_symlink {
            ObjectType::Symlink
        } else if let Some(ext) = &extension {
            ObjectType::Other(ext.to_uppercase())
        } else {
            ObjectType::File
        };

        Ok(LightObjectInfo {
            path: path.to_path_buf(),
            name,
            extension,
            object_type,
            is_dir,
            is_symlink,
        })
    }

    /// Upgrade a lightweight object to full ObjectInfo with metadata (slow)
    pub async fn from_light_info(light: LightObjectInfo) -> std::io::Result<Self> {
        let metadata: Metadata = tokio::fs::symlink_metadata(&light.path).await?;

        let size: u64 = if light.is_dir { 0 } else { metadata.len() };

        let items_count: usize = if light.is_dir {
            match tokio::fs::read_dir(&light.path).await {
                Ok(mut entries) => {
                    let mut count = 0;
                    while let Ok(Some(_)) = entries.next_entry().await {
                        count += 1;
                    }
                    count
                }
                Err(_) => 0,
            }
        } else {
            0
        };

        let modified: SystemTime = metadata.modified().unwrap_or_else(|e| {
            warn!("Failed to get modified time for {:?}: {}", light.path, e);
            UNIX_EPOCH
        });

        let modified_dt: Duration = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0));

        let modified_dt: DateTime<Local> = Local
            .timestamp_opt(modified_dt.as_secs() as i64, modified_dt.subsec_nanos())
            .single()
            .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().unwrap());

        debug!(
            "ObjectInfo for {}: modified_dt = {}",
            light.path.display(),
            modified_dt.format("%Y-%m-%d")
        );

        Ok(ObjectInfo {
            path: light.path,
            name: light.name,
            extension: light.extension,
            object_type: light.object_type,
            is_dir: light.is_dir,
            is_symlink: light.is_symlink,
            size,
            items_count,
            modified: modified_dt,
            metadata_loaded: true,
        })
    }

    /// Create from lightweight info with placeholder metadata (for immediate display)
    pub fn with_placeholder_metadata(light: LightObjectInfo) -> Self {
        // Try to get actual modified  even for placeholder
        let modified: SystemTime = std::fs::metadata(&light.path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);

        let modified_dt = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0));

        let modified_dt = Local
            .timestamp_opt(modified_dt.as_secs() as i64, modified_dt.subsec_nanos())
            .single()
            .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().unwrap());

        ObjectInfo {
            path: light.path,
            name: light.name,
            extension: light.extension,
            object_type: light.object_type,
            is_dir: light.is_dir,
            is_symlink: light.is_symlink,
            size: 0,
            items_count: 0,
            modified: modified_dt,
            metadata_loaded: false,
        }
    }

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
            .map(|d: Duration| {
                Utc.timestamp_opt(d.as_secs() as i64, d.subsec_nanos())
                    .single()
                    .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap())
            })
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap());

        Ok(Self {
            path: path.to_path_buf(),
            name,
            extension,
            object_type,
            is_dir,
            is_symlink,
            size,
            items_count,
            modified: modified.into(),
            metadata_loaded: true,
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
            modified: chrono::Local.timestamp_opt(0, 0).unwrap(),
            metadata_loaded: false,
        }
    }
}
