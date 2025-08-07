//! `src/fs/object_info.rs`
//! ============================================================
//! Cache-friendly filesystem metadata structures for FSM.
//!
//! Goals
//! -----
//! • Sync-first path for the hot metadata loop.
//! • Memory layout ordered large→small for cache locality.
//! • Zero-clone sharing via Arc.
//! • Async helpers reuse the same core builder.
//!
//! Approximate sizes (x86-64, release):
//!   `ObjectInfo`      ≈ 96 B
//!   `LightObjectInfo` ≈ 48 B

// ------------------------------------------------------------
// Standard-library imports
// ------------------------------------------------------------

// Bring Arc into scope for shared ownership.
use std::{fs::ReadDir, sync::Arc};

// Work with filesystem paths.
use std::path::{Path, PathBuf};

// Access OS string views.
use std::ffi::OsStr;

// Obtain file metadata quickly.
use std::fs::{self, FileType, Metadata};

// Handle time values safely.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ------------------------------------------------------------
// Third-party crate imports
// ------------------------------------------------------------

// CompactString for small-string optimisation.
use compact_str::CompactString;

// Byte-size formatting utility.
use bytesize::ByteSize;

// Chrono for timestamp formatting.
use chrono::{DateTime, Local, TimeZone};

// Serde traits for (de)serialization.
use serde::{Deserialize, Serialize};

// Tokio FS for optional async constructors.
use tokio::fs as tokio_fs;

// ------------------------------------------------------------
// Local-crate imports
// ------------------------------------------------------------

// Application-level error enum.
use crate::error_core::CoreError;

// ------------------------------------------------------------
// ObjectType — file, directory, or symlink.
// ------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectType {
    Dir,
    File,
    Symlink,
}

// Provide human-readable display strings.
impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir => write!(f, "Dir"),
            Self::File => write!(f, "File"),
            Self::Symlink => write!(f, "Symlink"),
        }
    }
}

// ------------------------------------------------------------
// ObjectInfo — rich metadata held in the registry & cache.
// ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    // Cache line 1 (64 bytes) - Hot path fields accessed together
    // Shared absolute path - registry lookup hot path
    pub path: Arc<PathBuf>, // 8 bytes

    // Byte length - sorting hot path
    pub size: u64, // 8 bytes

    // Children count - directory operations
    pub items_count: u64, // 8 bytes

    // Last-modification timestamp - sorting hot path
    pub modified: SystemTime, // 16 bytes (u64 + u32)

    // File or directory name - rendering hot path
    pub name: CompactString, // 24 bytes
    // Total: 64 bytes - exactly one cache line

    // Cache line 2 - Less frequently accessed fields
    // Lower-case extension (files only) - occasional access
    pub extension: Option<CompactString>, // 32 bytes

    // Object classification bits - frequent but small
    pub is_dir: bool,     // 1 byte
    pub is_symlink: bool, // 1 byte
    pub metadata_loaded: bool, // 1 byte
                          // 29 bytes padding to cache line boundary
}

// ------------------------------------------------------------
// LightObjectInfo — minimal, instantly built version.
// ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightObjectInfo {
    // Hot path fields - accessed during directory scanning
    pub path: Arc<PathBuf>,               // 8 bytes - most accessed
    pub name: CompactString,              // 24 bytes - frequent rendering
    pub extension: Option<CompactString>, // 32 bytes - occasional
    pub is_dir: bool,                     // 1 byte - frequent classification
    pub is_symlink: bool,                 // 1 byte - less frequent
                                          // 6 bytes padding - but fields in optimal access order
}

// ------------------------------------------------------------
// Helper methods for LightObjectInfo.
// ------------------------------------------------------------

impl LightObjectInfo {
    // Constant-time type check.
    #[inline]
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        if self.is_dir {
            ObjectType::Dir
        } else if self.is_symlink {
            ObjectType::Symlink
        } else {
            ObjectType::File
        }
    }

    // Synchronous constructor (fast path).
    pub fn from_path_sync(path: &Path) -> Result<Self, CoreError> {
        let meta: Metadata = fs::symlink_metadata(path)?;
        let ftype: FileType = meta.file_type();

        let name = CompactString::new(path.file_name().and_then(OsStr::to_str).unwrap_or(""));

        let ext = if ftype.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|s: &str| -> CompactString { CompactString::new(s.to_lowercase()) })
        } else {
            None
        };

        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            name,
            extension: ext,
            is_dir: ftype.is_dir(),
            is_symlink: ftype.is_symlink(),
        })
    }

    // Synchronous constructor that returns both LightObjectInfo and Metadata
    // Critical for directory scanner optimization - avoids duplicate stat() calls
    pub fn from_path_sync_with_meta(path: &Path) -> Result<(Self, Metadata), CoreError> {
        let meta: Metadata = fs::symlink_metadata(path)?;
        let ftype: FileType = meta.file_type();

        let name = CompactString::new(path.file_name().and_then(OsStr::to_str).unwrap_or(""));

        let ext = if ftype.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|s: &str| -> CompactString { CompactString::new(s.to_lowercase()) })
        } else {
            None
        };

        let light = Self {
            path: Arc::new(path.to_path_buf()),
            name,
            extension: ext,
            is_dir: ftype.is_dir(),
            is_symlink: ftype.is_symlink(),
        };

        Ok((light, meta))
    }

    // Async constructor for background tasks.
    pub async fn from_path_async(path: &Path) -> Result<Self, CoreError> {
        let meta: Metadata = tokio_fs::symlink_metadata(path).await?;
        let ftype: FileType = meta.file_type();

        let name: CompactString =
            CompactString::new(path.file_name().and_then(OsStr::to_str).unwrap_or(""));

        let ext: Option<CompactString> = if ftype.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|s: &str| -> CompactString { CompactString::new(s.to_lowercase()) })
        } else {
            None
        };

        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            name,
            extension: ext,
            is_dir: ftype.is_dir(),
            is_symlink: ftype.is_symlink(),
        })
    }

    // Promote to full ObjectInfo synchronously.
    pub fn into_full_sync(self) -> Result<ObjectInfo, CoreError> {
        ObjectInfo::from_light_sync(self)
    }

    // Promote to full ObjectInfo asynchronously.
    pub async fn into_full_async(self) -> Result<ObjectInfo, CoreError> {
        ObjectInfo::from_light_async(self).await
    }
}

// ------------------------------------------------------------
// Helper methods for ObjectInfo.
// ------------------------------------------------------------

impl ObjectInfo {
    // Quick object-type check.
    #[inline]
    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        if self.is_dir {
            ObjectType::Dir
        } else if self.is_symlink {
            ObjectType::Symlink
        } else {
            ObjectType::File
        }
    }

    // Human-readable size string.
    #[inline]
    #[must_use]
    pub fn size_human(&self) -> String {
        ByteSize::b(self.size).to_string()
    }

    // Format the modification date.
    #[expect(clippy::cast_possible_wrap, reason = "Expected")]
    #[must_use]
    pub fn format_date(&self, fmt: &str) -> String {
        let dur: Duration = self
            .modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| -> Duration { Duration::from_secs(0) });

        let dt: DateTime<Local> = Local
            .timestamp_opt(dur.as_secs() as i64, dur.subsec_nanos())
            .single()
            .unwrap_or_else(|| -> DateTime<Local> { Local::now() });

        dt.format(fmt).to_string()
    }

    // Synchronous cold-path constructor.
    pub fn from_path_sync(path: &Path) -> Result<Self, CoreError> {
        let meta: Metadata = fs::symlink_metadata(path)?;

        Self::from_meta(path, &meta)
    }

    // Asynchronous cold-path constructor.
    pub async fn from_path_async(path: &Path) -> Result<Self, CoreError> {
        let meta: Metadata = tokio_fs::symlink_metadata(path).await?;

        Self::from_meta(path, &meta)
    }

    // Internal builder shared by both entry points.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "Actually necessary for '?' propagation"
    )]
    fn from_meta(path: &Path, meta: &Metadata) -> Result<Self, CoreError> {
        let ftype: FileType = meta.file_type();
        let is_dir: bool = ftype.is_dir();
        let is_link: bool = ftype.is_symlink();

        let name: CompactString =
            CompactString::new(path.file_name().and_then(OsStr::to_str).unwrap_or(""));

        let ext: Option<CompactString> = if ftype.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|s: &str| -> CompactString { CompactString::new(s.to_lowercase()) })
        } else {
            None
        };

        let size: u64 = if is_dir { 0 } else { meta.len() };

        // Lazy item counting optimization - avoid extra read_dir() syscall
        // Item count loaded on-demand when needed for display
        let items: u64 = 0;

        let mod_time: SystemTime = meta.modified().unwrap_or(UNIX_EPOCH);

        Ok(Self {
            path: Arc::new(path.to_path_buf()),
            modified: mod_time,
            name,
            extension: ext,
            size,
            items_count: items,
            is_dir,
            is_symlink: is_link,
            metadata_loaded: true,
        })
    }

    // Promote from LightObjectInfo synchronously.
    fn from_light_sync(light: LightObjectInfo) -> Result<Self, CoreError> {
        let meta: Metadata = fs::symlink_metadata(&*light.path)?;

        Self::from_light_common(light, &meta)
    }

    // Promote from LightObjectInfo asynchronously.
    async fn from_light_async(light: LightObjectInfo) -> Result<Self, CoreError> {
        let meta: Metadata = tokio_fs::symlink_metadata(&*light.path).await?;

        Self::from_light_common(light, &meta)
    }

    // Shared helper for sync/async promotions.
    pub fn from_light_common(light: LightObjectInfo, meta: &Metadata) -> Result<Self, CoreError> {
        let size: u64 = if light.is_dir { 0 } else { meta.len() };

        // Lazy item counting optimization - avoid extra read_dir() syscall
        // Item count loaded on-demand when needed for display
        let items: u64 = 0;

        let mod_time: SystemTime = meta.modified().unwrap_or(UNIX_EPOCH);

        Ok(Self {
            path: light.path,
            modified: mod_time,
            name: light.name,
            extension: light.extension,
            size,
            items_count: items,
            is_dir: light.is_dir,
            is_symlink: light.is_symlink,
            metadata_loaded: true,
        })
    }

    /// Lazy load directory item count on-demand (avoids syscall during construction)
    pub fn load_items_count(&mut self) -> Result<u64, CoreError> {
        if self.is_dir && self.items_count == 0 {
            self.items_count =
                fs::read_dir(&*self.path).map_or(0, |r: ReadDir| -> u64 { r.count() as u64 });
        }
        Ok(self.items_count)
    }

    /// Get item count, loading if needed
    pub fn get_items_count(&mut self) -> Result<u64, CoreError> {
        self.load_items_count()
    }
}

// ------------------------------------------------------------
// Default placeholder (used in tests).
// ------------------------------------------------------------

impl Default for ObjectInfo {
    fn default() -> Self {
        Self {
            path: Arc::new(PathBuf::new()),
            modified: UNIX_EPOCH,
            name: CompactString::new(""),
            extension: None,
            size: 0,
            items_count: 0,
            is_dir: false,
            is_symlink: false,
            metadata_loaded: false,
        }
    }
}
