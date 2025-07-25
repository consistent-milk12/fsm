use std::path::PathBuf;
use thiserror::Error;

pub type ClipResult<T> = Result<T, ClipError>;

#[derive(Error, Debug, Clone)]
pub enum ClipError {
    #[error("Clipboard item not found: {0}")]
    ItemNotFound(String),

    #[error("Duplicate item already in clipboard: {0}")]
    DuplicateItem(PathBuf),

    #[error("Clipboard is full (max {max} items)")]
    ClipboardFull { max: usize },

    #[error("Failed to read metadata for {0}: {1}")]
    MetadataError(PathBuf, String),

    #[error("Invalid file path: {0}")]
    InvalidPath(PathBuf),

    #[error("Operation not supported: {0}")]
    UnsupportedOperation(String),

    #[error("Clipboard configuration error: {0}")]
    ConfigError(String),

    #[error("File system error: {0}")]
    FileSystemError(String),
}

impl ClipError {
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            ClipError::ItemNotFound(_)
                | ClipError::DuplicateItem(_)
                | ClipError::ClipboardFull { .. }
        )
    }
}

impl From<std::io::Error> for ClipError {
    fn from(err: std::io::Error) -> Self {
        ClipError::FileSystemError(err.to_string())
    }
}
