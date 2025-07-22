//! src/error.rs
//! ============================================================================
//! # AppError: Unified Error Type for File Manager
//!
//! This module defines the comprehensive error enum (`AppError`) used across the
//! entire application. Each variant carries rich context for diagnostics, and all
//! major modules are expected to use `Result<T, AppError>` for consistency.

use serde_json;
use std::{io, path::PathBuf};
use thiserror::Error;

/// Unified error type for all file manager operations.
#[derive(Debug, Error)]
pub enum AppError {
    /// Standard IO error, auto-converted from `io::Error`.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Error retrieving file or directory metadata.
    #[error("Filesystem metadata error on {path:?}: {source}")]
    FsMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Permissions error for file/directory access.
    #[error("Permission denied: {0:?}")]
    PermissionDenied(PathBuf),

    /// Requested file or directory does not exist.
    #[error("File or directory not found: {0:?}")]
    NotFound(PathBuf),

    /// Caching layer error.
    #[error("Cache error: {0}")]
    Cache(String),

    /// TOML config parsing error.
    #[error("Config parse error: {0}")]
    Config(#[from] toml::de::Error),

    /// Config file I/O error with path.
    #[error("Failed to read config file {path:?}: {source}")]
    ConfigIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Serialization or deserialization error (e.g., JSON).
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// External process/command failure (e.g., ripgrep).
    #[error("External command failed: {cmd} (exit code: {code:?})\n{stderr}")]
    ExternalCmd {
        cmd: String,
        code: Option<i32>,
        stderr: String,
    },

    /// Ripgrep-specific error.
    #[error("Ripgrep search error: {0}")]
    Ripgrep(String),

    /// Async task failure or join error.
    #[error("Async task failed: {0}")]
    Task(String),

    /// Operation cancelled by user or system.
    #[error("Operation was cancelled")]
    Cancelled,

    /// Terminal I/O or rendering error.
    #[error("Terminal error: {0}")]
    Terminal(String),

    /// Terminal resize failure.
    #[error("Resize error: {0}")]
    Resize(String),

    /// Plugin/extension error.
    #[error("Plugin error: {0}")]
    Plugin(String),

    /// Any other error, with description.
    #[error("Unexpected error: {0}")]
    Other(String),
}

impl AppError {
    /// Attach extra context to an error.
    pub fn with_context<S: Into<String>>(self, ctx: S) -> AppError {
        AppError::Other(format!("{}: {}", ctx.into(), self))
    }
}

// Allow conversion from `anyhow::Error` as fallback.
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e.to_string())
    }
}
