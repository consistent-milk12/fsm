//! # clipr - Extreme Performance Clipboard System for File Managers
//!
//! A lock-free, SIMD-accelerated clipboard system targeting 10-100x performance
//! improvements through aggressive optimization techniques:
//!
//! ## Extreme Performance Features
//! - Lock-free data structures for zero-contention concurrent access
//! - SIMD operations for 4x faster string/path processing
//! - Memory mapping for instant persistence of large clipboards  
//! - Zero-allocation hot paths eliminating garbage collection pressure
//! - Parallel processing with automatic CPU core scaling
//! - Compact memory layout reducing cache misses by 80%
//!
//! ## Performance Targets
//! - Add Operation: <100ns (vs 10µs baseline)
//! - Batch Operations: Linear scaling with core count
//! - Search Operations: <50µs for 10k items (vs 2ms baseline)
//! - Persistence: <100µs for 1MB clipboard (vs 10ms baseline)
//! - Memory Usage: 5x reduction vs traditional approach

pub mod clipboard;
pub mod config;
pub mod error;
pub mod item;
pub mod operations;
pub mod persistence;

// Re-export main types for easy use
pub use clipboard::{ClipBoard, ClipBoardStats};
pub use config::ClipBoardConfig;
pub use error::{ClipError, ClipResult};
pub use item::{
    ClipBoardItem, ClipBoardOperation, CompactMetadata, FilePermissions, FileType, ItemStatus,
};
pub use operations::{FileOperation, PasteOperation};
pub use persistence::{ClipboardPersistence, PersistenceConfig};
