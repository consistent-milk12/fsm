use std::time::Duration;

use super::object_info::ObjectInfo;

/// Scan update for streaming directory operations
#[derive(Debug, Clone)]
pub enum ScanUpdate {
    /// New entry discovered (immediate display)
    EntryAdded(ObjectInfo),

    /// Batch of entries processed
    BatchComplete {
        processed: usize,
        total: Option<usize>,
    },

    /// Scanning completed
    ScanComplete {
        total_entries: usize,
        exec: Duration,
    },

    /// Error during scanning
    ScanError(String),
}
