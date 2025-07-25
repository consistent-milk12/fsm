//! # clipr - Advanced Clipboard System for File Managers
//! 
//! A comprehensive clipboard system providing persistent copy/move operations
//! with rich metadata, visual indicators, and configurable behavior.
//!
//! ## Key Features
//! - Persistent clipboard operations (copy/move)
//! - Rich file metadata and status tracking  
//! - Configurable limits and expiration
//! - Clean API for file manager integration
//! - Comprehensive error handling

pub mod clipboard;
pub mod item;
pub mod operations;
pub mod error;
pub mod config;

// Re-export main types for easy use
pub use clipboard::{Clipboard, ClipboardStats};
pub use item::{ClipboardItem, ClipboardOperation, ItemStatus, FileMetadata, FilePermissions, FileType};
pub use operations::{PasteOperation, FileOperation};
pub use error::{ClipError, ClipResult};
pub use config::ClipboardConfig;