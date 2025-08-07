//! Core error handling module
//!
//! • High-performance, stack-friendly design
//! • Near-zero allocation in hot paths (`SmallVec + CompactString`)
//! • First-class `tracing` JSON-layer integration
//! • `#[non_exhaustive]` for forward-compatible extension
use std::io::{self, ErrorKind};

use compact_str::CompactString;
use smallvec::{smallvec, SmallVec};
use thiserror::Error;
use tracing::{event, Level};

/// Convenient alias carrying our unified error type
pub type CoreResult<T> = Result<T, CoreError>;

/// Primary error enumeration (grouped by concern)
#[non_exhaustive] // allow adding variants without breaking callers
#[derive(Error, Debug)]
pub enum CoreError {
    // ────────────────────────────────────────────────────────────
    // Input validation
    // ────────────────────────────────────────────────────────────
    #[error("Invalid input: {field} - {message}")]
    InvalidInput {
        field:   CompactString,
        message: CompactString,
    },

    // ------------------------------------------------------------
    // Invalid State
    // ------------------------------------------------------------
    #[error("Invalid state: {message}")]
    InvalidState 
    {
        message: CompactString,
    },

    // ────────────────────────────────────────────────────────────
    // Path-related failures
    // ────────────────────────────────────────────────────────────
    #[error("Path not found: {0}")]
    PathNotFound(CompactString),

    #[error("Path access denied: {0}")]
    PathAccessDenied(CompactString),

    // ────────────────────────────────────────────────────────────
    // Command execution
    // ────────────────────────────────────────────────────────────
    #[error("Command unavailable: {command}")]
    CommandUnavailable {
        command: CompactString,
    },

    #[error("Command failed: {command} - {kind:?}")]
    CommandFailed {
        command: CompactString,
        kind:    ErrorKind,
    },

    // ────────────────────────────────────────────────────────────
    // Task management
    // ────────────────────────────────────────────────────────────
    #[error("Task {task_id} failed: {reason}")]
    TaskFailed {
        task_id: u64,
        reason:  CompactString,
    },

    #[error("Task {task_id} timed out after {timeout_ms} ms")]
    TaskTimeout {
        task_id:   u64,
        timeout_ms: u64,
    },

    // ────────────────────────────────────────────────────────────
    // Search operations
    // ────────────────────────────────────────────────────────────
    #[error("Search failed: {reason}")]
    SearchFailed {
        reason: CompactString,
    },

    #[error("Search stream error: {kind:?}")]
    SearchStreamError {
        kind: ErrorKind,
    },

    // ────────────────────────────────────────────────────────────
    // File-system
    // ────────────────────────────────────────────────────────────
    #[error("FS operation failed: {kind:?}")]
    FileSystem {
        kind:   ErrorKind,
        #[source]                       // keep causal chain intact
        source: Box<io::Error>,
    },

    #[error("Metadata error: {path} - {kind:?}")]
    Metadata {
        path: CompactString,
        kind: ErrorKind,
    },

    // ────────────────────────────────────────────────────────────
    // Parsing
    // ────────────────────────────────────────────────────────────
    #[error("Parse error: {input} - expected {expected}")]
    ParseError {
        input:    CompactString,
        expected: CompactString,
    },

    // ────────────────────────────────────────────────────────────
    // Tracing context
    // ────────────────────────────────────────────────────────────
    #[error("Span context missing: {operation}")]
    SpanContextMissing {
        operation: CompactString,
    },

    // ------------------------------------------------------------
    // Process spawn
    // ------------------------------------------------------------
    #[error("Process spawn error: {command}")]
    ProcessSpawn
    {
        command: CompactString,

        #[source]
        source: Box<io::Error>,
    },

    /// Caching layer error.
    #[error("Cache error: {0}")]
    Cache(CompactString),

    /// Any other error, with description.
    #[error("Unexpected error: {0}")]
    Other(CompactString),
}

impl Clone for CoreError
{
    fn clone(&self) -> Self {
        Self::Other(CompactString::const_new("Shouldn't be cloned. This is just a quick fix."))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Fast classification helpers
// ────────────────────────────────────────────────────────────────────────────
impl CoreError {
    /// Determine whether downstream logic may safely recover
    #[inline]
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self, Self::PathNotFound(_)
            | Self::CommandUnavailable { .. }
            | Self::TaskTimeout { .. }
            | Self::ParseError { .. }
        )
    }

    /// Decide if an automated retry is sensible
    #[inline]
    #[must_use]
    pub const fn should_retry(&self) -> bool {
        matches!(
            self, Self::TaskTimeout { .. }
            | Self::SearchStreamError {
                kind: ErrorKind::Interrupted | ErrorKind::TimedOut,
            }
            | Self::FileSystem {
                kind: ErrorKind::Interrupted | ErrorKind::TimedOut,
                ..
            })
    }

    // ────────────────────────────────────────────────────────────
    // Attribute helpers – used for JSON log grouping
    // ────────────────────────────────────────────────────────────
    #[inline]
    #[must_use]
    pub const fn operation_type(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. }                       => "input_validation",
            
            Self::PathNotFound(_) |
                    Self::PathAccessDenied(_)               => "path_access",
            
            Self::CommandUnavailable { .. } |
                    Self::CommandFailed { .. }              => "command_execution",
            
            Self::TaskFailed { .. } |
                    Self::TaskTimeout { .. }                => "task_management",
            
            Self::SearchFailed { .. } |
                    Self::SearchStreamError { .. }          => "search_operation",
            
            Self::FileSystem { .. } |
                    Self::Metadata { .. }                   => "file_system",
            
            Self::ParseError { .. }                         => "data_parsing",
            
            Self::SpanContextMissing { .. }                 => "tracing_context",
            
            Self::InvalidState { .. }                       => "invalid_state",
            
            Self::ProcessSpawn { .. }                       => "process_spawn",
            
            Self::Cache(_)                                  => "Moka Cache Error",

            Self::Other(_)                                  => "unknown_error",
        }
    }

    // ────────────────────────────────────────────────────────────
    // Structured-field extraction (SmallVec avoids heap ≤4 items)
    // ────────────────────────────────────────────────────────────
    #[inline]
    #[must_use]
    pub fn extract_trace_fields(
        &self,
    ) -> SmallVec<[(&'static str, CompactString); 4]> {
        match self {
            Self::InvalidInput { field, message } => smallvec![
                ("field",   field.clone()),
                ("message", message.clone()),
            ],

            Self::TaskFailed { task_id, reason } => smallvec![
                ("task_id",
                 CompactString::from(task_id.to_string())),
                ("reason",  reason.clone()),
            ],

            Self::CommandFailed { command, kind } => smallvec![
                ("command",    command.clone()),
                ("error_kind",
                 CompactString::from(format!("{kind:?}"))),
            ],

            Self::PathNotFound(path) => smallvec![
                ("path", path.clone()),
            ],

            Self::SearchFailed { reason } => smallvec![
                ("reason", reason.clone()),
            ],

            _ => smallvec![], // cold variants: no extra fields
        }
    }

    // ────────────────────────────────────────────────────────────
    // Single-call JSON-layer emission
    // ────────────────────────────────────────────────────────────
    #[must_use]
    pub fn trace(self) -> Self {
        // gather minimal structured data
        let extra: SmallVec<[(&'static str, CompactString); 4]> = self.extract_trace_fields();

        // fire a single `tracing` event – JsonLayer will flatten
        event!(
            Level::ERROR,
            marker       = self.error_marker(),
            op_type      = self.operation_type(),
            error        = %self,             // Display impl
            recoverable  = self.is_recoverable(),
            retry        = self.should_retry(),
            extra_len    = extra.len(),
            extra        = ?extra,            // debug-print vec
        );

        // propagate error unchanged for caller handling
        self
    }

    // ────────────────────────────────────────────────────────────
    // Lightweight smart-constructors (stack-allocated)
    // ────────────────────────────────────────────────────────────
    #[inline]
    #[must_use]
    pub fn invalid_input(field: &str, message: &str) -> Self 
    {
        Self::InvalidInput 
        {
            field:   CompactString::new(field),
            message: CompactString::new(message),
        }
    }

    #[inline]
    #[must_use]
    pub fn invalid_state(message: &str) -> Self
    {
        Self::InvalidState
        {
            message: CompactString::new(message)
        }
    }

    #[inline]
    #[must_use]
    pub fn command_unavailable(command: &str) -> Self 
    {
        Self::CommandUnavailable 
        {
            command: CompactString::new(command),
        }
    }

    #[inline]
    #[must_use]
    pub fn process_spawn(command: &str, e: std::io::Error) -> Self
    {
        Self::ProcessSpawn {
            command: CompactString::new(command),
            source: Box::new(e),
        }
    }

    #[inline]
    #[must_use]
    pub fn task_failed(task_id: u64, reason: &str) -> Self 
    {
        Self::TaskFailed 
        {
            task_id,
            reason: CompactString::new(reason),
        }
    }

    #[inline]
    #[must_use]
    pub fn search_failed(reason: &str) -> Self 
    {
        Self::SearchFailed 
        {
            reason: CompactString::new(reason),
        }
    }

    #[inline]
    #[must_use]
    pub fn path_not_found(path: &str) -> Self 
    {
        Self::PathNotFound(CompactString::new(path))
    }


    // ────────────────────────────────────────────────────────────
    // Internal marker generator – keeps log keys stable
    // ────────────────────────────────────────────────────────────
    #[inline]
    #[must_use]
    const fn error_marker(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. }        => "ERROR_INPUT_VALIDATION",
           
            Self::PathNotFound(_)            => "ERROR_PATH_NOT_FOUND",
           
            Self::PathAccessDenied(_)        => "ERROR_PATH_ACCESS_DENIED",
           
            Self::CommandUnavailable { .. }  => "ERROR_COMMAND_UNAVAILABLE",
           
            Self::CommandFailed { .. }       => "ERROR_COMMAND_FAILED",
           
            Self::TaskFailed { .. }          => "ERROR_TASK_FAILED",
           
            Self::TaskTimeout { .. }         => "ERROR_TASK_TIMEOUT",
           
            Self::SearchFailed { .. }        => "ERROR_SEARCH_FAILED",
           
            Self::SearchStreamError { .. }   => "ERROR_SEARCH_STREAM",
           
            Self::FileSystem { .. }          => "ERROR_FILE_SYSTEM",
           
            Self::Metadata { .. }            => "ERROR_METADATA",
           
            Self::ParseError { .. }          => "ERROR_PARSE_FAILED",
           
            Self::SpanContextMissing { .. }  => "ERROR_SPAN_CONTEXT_MISSING",

            Self::InvalidState { .. }        => "ERROR_INVALID_SYSTEM_STATE",
            
            Self::ProcessSpawn { .. }        => "ERROR_PROCESS_SPAWN",

            Self::Cache(_)                   => "ERROR_MOKA_CACHE",
            
            Self::Other(_)                   => "ERROR_UNKNOWN"
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Loss-free mapping from std::io::Error
// ────────────────────────────────────────────────────────────────────────────
impl From<io::Error> for CoreError {
    fn from(err: io::Error) -> Self {
        Self::FileSystem {
            kind:   err.kind(),
            source: Box::new(err),
        }
    }
}

impl From<std::sync::Arc<Self>> for CoreError
{
    fn from(value: std::sync::Arc<Self>) -> Self {
        let core_err: Self = value.into();
        
        core_err
    }
}