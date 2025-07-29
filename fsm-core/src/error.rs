//! Enhanced AppError with practical tracing integration

use clipr::ClipError;
use serde_json;
use std::{io, path::PathBuf};
use thiserror::Error;
use tracing::Span;

/// Enhanced error type with tracing support
#[derive(Debug, Error)]
pub enum AppError {
    /// Standard IO error
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Filesystem metadata error
    #[error("Filesystem metadata error on {path:?}: {source}")]
    FsMetadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Permission denied
    #[error("Permission denied: {0:?}")]
    PermissionDenied(PathBuf),

    /// File not found
    #[error("File or directory not found: {0:?}")]
    NotFound(PathBuf),

    /// Cache error
    #[error("Cache error: {0}")]
    Cache(String),

    /// Config error
    #[error("Config parse error: {0}")]
    Config(#[from] toml::de::Error),

    /// Config file I/O error
    #[error("Failed to read config file {path:?}: {source}")]
    ConfigIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// External command failure
    #[error("External command failed: {cmd} (exit code: {code:?})\n{stderr}")]
    ExternalCmd {
        cmd: String,
        code: Option<i32>,
        stderr: String,
    },

    /// Ripgrep error
    #[error("Ripgrep search error: {0}")]
    Ripgrep(String),

    /// Search operation error
    #[error("Search failed in {path:?}: {reason}")]
    SearchFailed { path: PathBuf, reason: String },

    /// File operation error
    #[error("File operation '{operation}' failed on {path:?}: {reason}")]
    FileOperationFailed {
        operation: String,
        path: PathBuf,
        reason: String,
    },

    /// Navigation error
    #[error("Navigation failed: cannot access {path:?}: {reason}")]
    NavigationFailed { path: PathBuf, reason: String },

    /// UI component error
    #[error("UI component error in {component}: {message}")]
    UiComponent { component: String, message: String },

    /// Input validation error
    #[error("Invalid input: {field} - {message}")]
    InvalidInput { field: String, message: String },

    /// Task failure
    #[error("Task {task_id} failed: {reason}")]
    TaskFailed { task_id: u64, reason: String },

    /// Task timeout
    #[error("Task {task_type} timed out after {timeout_secs}s")]
    TaskTimeout {
        task_type: String,
        timeout_secs: u64,
    },

    /// Cache operation error
    #[error("Cache operation failed: {operation} on key '{key}': {reason}")]
    CacheOperation {
        operation: String,
        key: String,
        reason: String,
    },

    /// Clipboard operation error
    #[error("Clipboard operation '{operation}' failed: {reason}")]
    ClipboardOperation { operation: String, reason: String },

    /// Clipboard item not found
    #[error("Clipboard item not found: {item_id}")]
    ClipboardItemNotFound { item_id: u64 },

    /// Clipboard persistence error
    #[error("Clipboard persistence failed: {reason}")]
    ClipboardPersistence { reason: String },

    /// Clipboard memory mapping error
    #[error("Clipboard memory mapping failed: {reason}")]
    ClipboardMemoryMap { reason: String },

    /// Action dispatch error
    #[error("Action dispatch failed for {action}: {reason}")]
    ActionDispatch { action: String, reason: String },

    /// State lock error
    #[error("State lock error in {component}: {reason}")]
    StateLock { component: String, reason: String },

    /// Render error
    #[error("Render error in {component}: {reason}")]
    Render { component: String, reason: String },

    /// Operation cancelled
    #[error("Operation was cancelled")]
    Cancelled,

    /// Terminal error
    #[error("Terminal error: {0}")]
    Terminal(String),

    /// Resize error
    #[error("Resize error: {0}")]
    Resize(String),

    /// Plugin error
    #[error("Plugin error: {0}")]
    Plugin(String),

    /// Other error
    #[error("Unexpected error: {0}")]
    Other(String),
}

impl AppError {
    /// Log error with tracing context
    pub fn trace_error(self, span: &Span) -> Self {
        // Record error in current span
        span.record("error", tracing::field::display(&self));
        span.record("error_type", std::any::type_name::<Self>());

        // Log structured error
        tracing::error!(
            error = %self,
            error_type = std::any::type_name::<Self>(),
            "Error occurred"
        );

        self
    }

    /// Add context to error
    pub fn with_context<S: Into<String>>(self, ctx: S) -> AppError {
        AppError::Other(format!("{}: {}", ctx.into(), self))
    }

    // Convenience constructors
    pub fn search_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        AppError::SearchFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

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

    pub fn navigation_failed<P: Into<PathBuf>, S: Into<String>>(path: P, reason: S) -> Self {
        AppError::NavigationFailed {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn ui_component_error<S1: Into<String>, S2: Into<String>>(
        component: S1,
        message: S2,
    ) -> Self {
        AppError::UiComponent {
            component: component.into(),
            message: message.into(),
        }
    }

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
}

// Standard conversions
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

impl From<ClipError> for AppError {
    fn from(e: ClipError) -> Self {
        match e {
            ClipError::ItemNotFound(id) => AppError::ClipboardItemNotFound { item_id: id },
            ClipError::SerializationError(msg) => AppError::ClipboardPersistence {
                reason: msg.to_string(),
            },
            ClipError::MemoryMapError { .. } => AppError::ClipboardMemoryMap {
                reason: e.to_string(),
            },
            _ => AppError::ClipboardOperation {
                operation: "unknown".to_string(),
                reason: e.to_string(),
            },
        }
    }
}

/// Result type alias
pub type AppResult<T> = Result<T, AppError>;

/// Trait for adding tracing to results
pub trait TracedResult<T> {
    /// Add tracing context to result
    fn trace_err(self, operation: &str) -> AppResult<T>;

    /// Add tracing with custom fields
    fn trace_err_with<F>(self, operation: &str, field_fn: F) -> AppResult<T>
    where
        F: FnOnce(&Span);
}

impl<T> TracedResult<T> for AppResult<T> {
    fn trace_err(self, operation: &str) -> AppResult<T> {
        match self {
            Ok(value) => Ok(value),
            Err(error) => {
                tracing::error!(
                    operation = operation,
                    error = %error,
                    "Operation failed"
                );
                Err(error)
            }
        }
    }

    fn trace_err_with<F>(self, operation: &str, field_fn: F) -> AppResult<T>
    where
        F: FnOnce(&Span),
    {
        match self {
            Ok(value) => Ok(value),
            Err(error) => {
                let span = tracing::error_span!("error_context", operation = operation);
                let _enter = span.enter();
                field_fn(&span);

                tracing::error!(
                    operation = operation,
                    error = %error,
                    "Operation failed with context"
                );

                Err(error)
            }
        }
    }
}

/// Convenient error creation macros
#[macro_export]
macro_rules! trace_error {
    ($error:expr) => {{
        let span = tracing::Span::current();
        $error.trace_error(&span)
    }};
    ($error:expr, $($field:tt)*) => {{
        tracing::error!(
            error = %$error,
            $($field)*,
            "Error occurred"
        );
        $error
    }};
}
