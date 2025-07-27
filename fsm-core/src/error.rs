//! Enhanced AppError with comprehensive error handling and clipboard support

use clipr::ClipError;
use serde_json;
use std::{io, path::PathBuf};
use thiserror::Error;

/// Unified error type for all file manager operations including clipboard.
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
        operation: String,
        path: PathBuf,
        reason: String,
    },

    /// Directory navigation errors
    #[error("Navigation failed: cannot access {path:?}: {reason}")]
    NavigationFailed { path: PathBuf, reason: String },

    /// UI component errors
    #[error("UI component error in {component}: {message}")]
    UiComponent { component: String, message: String },

    /// Input validation errors
    #[error("Invalid input: {field} - {message}")]
    InvalidInput { field: String, message: String },

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
        operation: String,
        key: String,
        reason: String,
    },

    /// Clipboard operation errors
    #[error("Clipboard operation '{operation}' failed: {reason}")]
    ClipboardOperation { operation: String, reason: String },

    /// Clipboard item not found
    #[error("Clipboard item not found: {item_id}")]
    ClipboardItemNotFound { item_id: u64 },

    /// Clipboard serialization/persistence error
    #[error("Clipboard persistence failed: {reason}")]
    ClipboardPersistence { reason: String },

    /// Clipboard memory mapping error
    #[error("Clipboard memory mapping failed: {reason}")]
    ClipboardMemoryMap { reason: String },

    /// Action dispatcher errors
    #[error("Action dispatch failed for {action}: {reason}")]
    ActionDispatch { action: String, reason: String },

    /// State coordinator lock errors
    #[error("State lock error in {component}: {reason}")]
    StateLock { component: String, reason: String },

    /// Rendering errors
    #[error("Render error in {component}: {reason}")]
    Render { component: String, reason: String },

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

    /// Create a search failure error
    pub fn search_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        AppError::SearchFailed {
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
        AppError::FileOperationFailed {
            operation: operation.into(),
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a navigation failure error
    pub fn navigation_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        AppError::NavigationFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a UI component error
    pub fn ui_component_error<S1: Into<String>, S2: Into<String>>(
        component: S1,
        message: S2,
    ) -> Self {
        AppError::UiComponent {
            component: component.into(),
            message: message.into(),
        }
    }

    /// Create an input validation error
    pub fn invalid_input<S1: Into<String>, S2: Into<String>>(field: S1, message: S2) -> Self {
        AppError::InvalidInput {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Create a task failure error
    pub fn task_failed<S: Into<String>>(task_id: u64, reason: S) -> Self {
        AppError::TaskFailed {
            task_id,
            reason: reason.into(),
        }
    }

    /// Create a task timeout error
    pub fn task_timeout<S: Into<String>>(task_type: S, timeout_secs: u64) -> Self {
        AppError::TaskTimeout {
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
        AppError::CacheOperation {
            operation: operation.into(),
            key: key.into(),
            reason: reason.into(),
        }
    }

    /// Create a clipboard operation error
    pub fn clipboard_operation<S1, S2>(operation: S1, reason: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        AppError::ClipboardOperation {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    /// Create a clipboard item not found error
    pub fn clipboard_item_not_found(item_id: u64) -> Self {
        AppError::ClipboardItemNotFound { item_id }
    }

    /// Create a clipboard persistence error
    pub fn clipboard_persistence<S: Into<String>>(reason: S) -> Self {
        AppError::ClipboardPersistence {
            reason: reason.into(),
        }
    }

    /// Create a clipboard memory mapping error
    pub fn clipboard_memory_map<S: Into<String>>(reason: S) -> Self {
        AppError::ClipboardMemoryMap {
            reason: reason.into(),
        }
    }

    /// Create an action dispatch error
    pub fn action_dispatch<S1, S2>(action: S1, reason: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        AppError::ActionDispatch {
            action: action.into(),
            reason: reason.into(),
        }
    }

    /// Create a state lock error
    pub fn state_lock<S1, S2>(component: S1, reason: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        AppError::StateLock {
            component: component.into(),
            reason: reason.into(),
        }
    }

    /// Create a render error
    pub fn render_error<S1, S2>(component: S1, reason: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        AppError::Render {
            component: component.into(),
            reason: reason.into(),
        }
    }
}

// Optimized Clone implementation
impl Clone for AppError {
    fn clone(&self) -> Self {
        match self {
            AppError::Io(e) => AppError::Io(io::Error::new(e.kind(), e.to_string())),
            AppError::FsMetadata { path, source } => AppError::FsMetadata {
                path: path.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            },
            AppError::PermissionDenied(path) => AppError::PermissionDenied(path.clone()),
            AppError::NotFound(path) => AppError::NotFound(path.clone()),
            AppError::Cache(msg) => AppError::Cache(msg.clone()),
            AppError::Config(e) => AppError::Other(format!("Config error: {e}")),
            AppError::ConfigIo { path, source } => AppError::ConfigIo {
                path: path.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            },
            AppError::Serde(e) => AppError::Other(format!("Serde error: {e}")),
            AppError::ExternalCmd { cmd, code, stderr } => AppError::ExternalCmd {
                cmd: cmd.clone(),
                code: *code,
                stderr: stderr.clone(),
            },
            AppError::Ripgrep(msg) => AppError::Ripgrep(msg.clone()),
            AppError::SearchFailed { path, reason } => AppError::SearchFailed {
                path: path.clone(),
                reason: reason.clone(),
            },
            AppError::FileOperationFailed {
                operation,
                path,
                reason,
            } => AppError::FileOperationFailed {
                operation: operation.clone(),
                path: path.clone(),
                reason: reason.clone(),
            },
            AppError::NavigationFailed { path, reason } => AppError::NavigationFailed {
                path: path.clone(),
                reason: reason.clone(),
            },
            AppError::UiComponent { component, message } => AppError::UiComponent {
                component: component.clone(),
                message: message.clone(),
            },
            AppError::InvalidInput { field, message } => AppError::InvalidInput {
                field: field.clone(),
                message: message.clone(),
            },
            AppError::TaskFailed { task_id, reason } => AppError::TaskFailed {
                task_id: *task_id,
                reason: reason.clone(),
            },
            AppError::TaskTimeout {
                task_type,
                timeout_secs,
            } => AppError::TaskTimeout {
                task_type: task_type.clone(),
                timeout_secs: *timeout_secs,
            },
            AppError::CacheOperation {
                operation,
                key,
                reason,
            } => AppError::CacheOperation {
                operation: operation.clone(),
                key: key.clone(),
                reason: reason.clone(),
            },
            AppError::ClipboardOperation { operation, reason } => AppError::ClipboardOperation {
                operation: operation.clone(),
                reason: reason.clone(),
            },
            AppError::ClipboardItemNotFound { item_id } => {
                AppError::ClipboardItemNotFound { item_id: *item_id }
            }
            AppError::ClipboardPersistence { reason } => AppError::ClipboardPersistence {
                reason: reason.clone(),
            },
            AppError::ClipboardMemoryMap { reason } => AppError::ClipboardMemoryMap {
                reason: reason.clone(),
            },
            AppError::ActionDispatch { action, reason } => AppError::ActionDispatch {
                action: action.clone(),
                reason: reason.clone(),
            },
            AppError::StateLock { component, reason } => AppError::StateLock {
                component: component.clone(),
                reason: reason.clone(),
            },
            AppError::Render { component, reason } => AppError::Render {
                component: component.clone(),
                reason: reason.clone(),
            },
            AppError::Cancelled => AppError::Cancelled,
            AppError::Terminal(msg) => AppError::Terminal(msg.clone()),
            AppError::Resize(msg) => AppError::Resize(msg.clone()),
            AppError::Plugin(msg) => AppError::Plugin(msg.clone()),
            AppError::Other(msg) => AppError::Other(msg.clone()),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

// Clipboard error conversions
impl From<ClipError> for AppError {
    fn from(e: ClipError) -> Self {
        match e {
            ClipError::ItemNotFound(id) => AppError::clipboard_item_not_found(id),
            ClipError::SerializationError(msg) => AppError::clipboard_persistence(msg.to_string()),
            ClipError::MemoryMapError { .. } => AppError::clipboard_memory_map(e.to_string()),
            _ => AppError::clipboard_operation("unknown", e.to_string()),
        }
    }
}
