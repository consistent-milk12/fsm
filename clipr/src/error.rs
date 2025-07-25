//! Extreme performance error handling optimized for zero-allocation hot paths

use compact_str::CompactString;
use std::io::ErrorKind;
use thiserror::Error;

pub type ClipResult<T> = Result<T, ClipError>;

/// High-performance error types with minimal allocations
#[derive(Error, Debug, Clone)]
pub enum ClipError {
    #[error("ClipBoard item not found: {0}")]
    ItemNotFound(u64), // Use u64 ID instead of String for performance

    #[error("Duplicate item already in clipboard: {path}")]
    DuplicateItem { path: CompactString }, // CompactString for memory efficiency

    #[error("ClipBoard is full (max {max} items)")]
    ClipBoardFull { max: usize },

    #[error("Failed to read metadata for {path}: {kind:?}")]
    MetadataError {
        path: CompactString,
        kind: ErrorKind, // Store ErrorKind instead of String for performance
    },

    #[error("Invalid file path: {0}")]
    InvalidPath(CompactString),

    #[error("Operation not supported: {0}")]
    UnsupportedOperation(CompactString),

    #[error("ClipBoard configuration error: {0}")]
    ConfigError(CompactString),

    #[error("File system error: {kind:?}")]
    FileSystemError { kind: ErrorKind },

    #[error("Memory mapping error: {kind:?}")]
    MemoryMapError { kind: ErrorKind },

    #[error("Serialization error: {0}")]
    SerializationError(CompactString),

    #[error("Lock-free operation failed (retry recommended)")]
    LockFreeRetry,

    #[error("SIMD operation not supported on this CPU")]
    SimdUnsupported,
}

impl ClipError {
    /// Fast inline recovery check for hot paths
    #[inline(always)]
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            ClipError::ItemNotFound(_)
                | ClipError::DuplicateItem { .. }
                | ClipError::ClipBoardFull { .. }
                | ClipError::LockFreeRetry
        )
    }

    /// Check if error indicates a transient condition that should be retried
    #[inline(always)]
    pub fn should_retry(&self) -> bool {
        matches!(self, ClipError::LockFreeRetry)
    }

    /// Create metadata error with path conversion optimization
    #[inline]
    pub fn metadata_error(path: &std::path::Path, io_error: std::io::Error) -> Self {
        Self::MetadataError {
            path: CompactString::from(path.to_string_lossy()),
            kind: io_error.kind(),
        }
    }

    /// Create duplicate error with path conversion optimization
    #[inline]
    pub fn duplicate_item(path: &std::path::Path) -> Self {
        Self::DuplicateItem {
            path: CompactString::from(path.to_string_lossy()),
        }
    }
}

/// Optimized From implementations avoiding unnecessary allocations
impl From<std::io::Error> for ClipError {
    fn from(err: std::io::Error) -> Self {
        ClipError::FileSystemError { kind: err.kind() }
    }
}

impl From<rmp_serde::encode::Error> for ClipError {
    fn from(err: rmp_serde::encode::Error) -> Self {
        ClipError::SerializationError(CompactString::from(err.to_string()))
    }
}

impl From<rmp_serde::decode::Error> for ClipError {
    fn from(err: rmp_serde::decode::Error) -> Self {
        ClipError::SerializationError(CompactString::from(err.to_string()))
    }
}

#[cfg(feature = "io-uring")]
impl From<io_uring::opcode::Error> for ClipError {
    fn from(err: io_uring::opcode::Error) -> Self {
        ClipError::FileSystemError {
            kind: std::io::ErrorKind::Other,
        }
    }
}
