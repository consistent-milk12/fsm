//! src/error.rs
//! ============================================================================
//! # `Self`: Unified Error Type for File Manager
//!
//! This module defines the comprehensive error enum (`Self`) used across the
//! entire application. Each variant carries rich context for diagnostics, and all
//! major modules are expected to use `Result<T, Self>` for consistency.

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

    /// Search operation specific errors
    #[error("Search failed in {path:?}: {reason}")]
    SearchFailed { path: PathBuf, reason: String },

    /// File operation specific errors  
    #[error("File operation '{operation}' failed on {path:?}: {reason}")]
    FileOperationFailed {
        operation: String, // "create", "delete", "rename", etc.
        path: PathBuf,
        reason: String,
    },

    /// Directory navigation errors
    #[error("Navigation failed: cannot access {path:?}: {reason}")]
    NavigationFailed { path: PathBuf, reason: String },

    /// UI component errors
    #[error("UI component error in {component}: {message}")]
    UiComponent {
        component: String, // "ObjectTable", "SearchOverlay", etc.
        message: String,
    },

    /// Input validation errors
    #[error("Invalid input: {field} - {message}")]
    InvalidInput {
        field: String, // "filename", "search_pattern", etc.
        message: String,
    },

    /// Task management errors  
    #[error("Task {task_id} failed: {reason}")]
    TaskFailed { task_id: u64, reason: String },

    /// Background task timeout
    #[error("Task {task_type} timed out after {timeout_secs}s")]
    TaskTimeout {
        task_type: String,
        timeout_secs: u64,
    },

    /// Cache operation errors (more specific than generic Cache)
    #[error("Cache operation failed: {operation} on key '{key}': {reason}")]
    CacheOperation {
        operation: String, // "get", "insert", "evict", etc.
        key: String,
        reason: String,
    },

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
    #[must_use]
    /// Attach extra context to an error.
    pub fn with_context<S: Into<String>>(self, ctx: S) -> Self {
        Self::Other(format!("{}: {}", ctx.into(), self))
    }

    /// Create a search failure error
    pub fn search_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        Self::SearchFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a file operation failure error
    pub fn file_operation_failed<S1, P, S2>(operation: S1, path: P, reason: S2) -> Self
    where
        S1: Into<String>,
        P: Into<PathBuf>,
        S2: Into<String>,
    {
        Self::FileOperationFailed {
            operation: operation.into(),
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a navigation failure error
    pub fn navigation_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        Self::NavigationFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a UI component error
    pub fn ui_component_error<S1: Into<String>, S2: Into<String>>(
        component: S1,
        message: S2,
    ) -> Self {
        Self::UiComponent {
            component: component.into(),
            message: message.into(),
        }
    }

    /// Create an input validation error
    pub fn invalid_input<S1: Into<String>, S2: Into<String>>(field: S1, message: S2) -> Self {
        Self::InvalidInput {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Create a task failure error
    pub fn task_failed<S: Into<String>>(task_id: u64, reason: S) -> Self {
        Self::TaskFailed {
            task_id,
            reason: reason.into(),
        }
    }

    /// Create a task timeout error
    pub fn task_timeout<S: Into<String>>(task_type: S, timeout_secs: u64) -> Self {
        Self::TaskTimeout {
            task_type: task_type.into(),
            timeout_secs,
        }
    }

    /// Create a cache operation error
    pub fn cache_operation_failed<S1, S2, S3>(operation: S1, key: S2, reason: S3) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self::CacheOperation {
            operation: operation.into(),
            key: key.into(),
            reason: reason.into(),
        }
    }
}

// Manual Clone implementation to handle non-Clone fields
impl Clone for AppError {
    fn clone(&self) -> Self {
        match self {
            Self::Io(e) => Self::Io(io::Error::new(e.kind(), e.to_string())),
            Self::FsMetadata { path, source } => Self::FsMetadata {
                path: path.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            },
            Self::PermissionDenied(path) => Self::PermissionDenied(path.clone()),
            Self::NotFound(path) => Self::NotFound(path.clone()),
            Self::Cache(msg) => Self::Cache(msg.clone()),
            Self::Config(e) => Self::Other(format!("Config error: {e}")),
            Self::ConfigIo { path, source } => Self::ConfigIo {
                path: path.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            },
            Self::Serde(e) => Self::Other(format!("Serde error: {e}")),

            Self::ExternalCmd { cmd, code, stderr } => Self::ExternalCmd {
                cmd: cmd.clone(),
                code: *code,
                stderr: stderr.clone(),
            },
            Self::Ripgrep(msg) => Self::Ripgrep(msg.clone()),
            Self::SearchFailed { path, reason } => Self::SearchFailed {
                path: path.clone(),
                reason: reason.clone(),
            },
            Self::FileOperationFailed {
                operation,
                path,
                reason,
            } => Self::FileOperationFailed {
                operation: operation.clone(),
                path: path.clone(),
                reason: reason.clone(),
            },
            Self::NavigationFailed { path, reason } => Self::NavigationFailed {
                path: path.clone(),
                reason: reason.clone(),
            },
            Self::UiComponent { component, message } => Self::UiComponent {
                component: component.clone(),
                message: message.clone(),
            },
            Self::InvalidInput { field, message } => Self::InvalidInput {
                field: field.clone(),
                message: message.clone(),
            },
            Self::TaskFailed { task_id, reason } => Self::TaskFailed {
                task_id: *task_id,
                reason: reason.clone(),
            },
            Self::TaskTimeout {
                task_type,
                timeout_secs,
            } => Self::TaskTimeout {
                task_type: task_type.clone(),
                timeout_secs: *timeout_secs,
            },
            Self::CacheOperation {
                operation,
                key,
                reason,
            } => Self::CacheOperation {
                operation: operation.clone(),
                key: key.clone(),
                reason: reason.clone(),
            },
            Self::Cancelled => Self::Cancelled,
            Self::Terminal(msg) => Self::Terminal(msg.clone()),
            Self::Resize(msg) => Self::Resize(msg.clone()),
            Self::Plugin(msg) => Self::Plugin(msg.clone()),
            Self::Other(msg) => Self::Other(msg.clone()),
        }
    }
}

// Allow conversion from `anyhow::Error` as fallback.
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e.to_string())
    }
}
