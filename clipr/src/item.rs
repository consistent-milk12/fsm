use crate::error::{ClipError, ClipResult};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardItem {
    pub id: String,
    pub source_path: PathBuf,
    pub operation: ClipboardOperation,
    pub metadata: FileMetadata,
    pub added_at: Instant,
    pub status: ItemStatus,
}

impl ClipboardItem {
    pub fn new_copy(path: PathBuf) -> ClipResult<Self> {
        let metadata = FileMetadata::from_path(&path)?;

        Ok(Self {
            id: Uuid::new_v4().to_string(),
            source_path: path,
            operation: ClipboardOperation::Copy,
            metadata,
            added_at: Instant::now(),
            status: ItemStatus::Ready,
        })
    }

    pub fn new_move(path: PathBuf) -> ClipResult<Self> {
        let metadata = FileMetadata::from_path(&path)?;

        Ok(Self {
            id: Uuid::new_v4().to_string(),
            source_path: path,
            operation: ClipboardOperation::Move,
            metadata,
            added_at: Instant::now(),
            status: ItemStatus::Ready,
        })
    }

    pub fn display_name(&self) -> &str {
        self.source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
    }

    pub fn operation_tag(&self) -> &'static str {
        match self.operation {
            ClipboardOperation::Copy => "C",
            ClipboardOperation::Move => "M",
        }
    }

    pub fn is_expired(&self, max_age: std::time::Duration) -> bool {
        self.added_at.elapsed() > max_age
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOperation {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Ready,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMetadata {
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<SystemTime>,
    pub permissions: FilePermissions,
    pub file_type: FileType,
}

impl FileMetadata {
    pub fn from_path(path: &PathBuf) -> ClipResult<Self> {
        let metadata = std::fs::metadata(path)
            .map_err(|e| ClipError::MetadataError(path.clone(), e.to_string()))?;

        Ok(Self {
            size: metadata.len(),
            is_dir: metadata.is_dir(),
            modified: metadata.modified().ok(),
            permissions: FilePermissions::from_metadata(&metadata),
            file_type: FileType::from_path(path),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilePermissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl FilePermissions {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();

        Self {
            readable: mode & 0o400 != 0,
            writable: mode & 0o200 != 0,
            executable: mode & 0o100 != 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    Other,
}

impl FileType {
    fn from_path(path: &PathBuf) -> Self {
        if path.is_dir() {
            Self::Directory
        } else if path.is_symlink() {
            Self::Symlink
        } else if path.is_file() {
            Self::RegularFile
        } else {
            Self::Other
        }
    }
}
