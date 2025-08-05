//! ``src/fs/object_info.rs``
//! ============================================================================
//! # `ObjectInfo`: Rich Filesystem Entry Metadata
//!
//! Cross-platform, async-friendly abstraction for a file or directory entry.
//! Integrates with `ObjectTable` (for TUI), moka cache, and async tasks.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, fs::{FileType, Metadata}, time::Duration};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs::{self as TokioFs};

/// Enum for object type, matching the table logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectType {
    Dir,
    File,
    Symlink,
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir => write!(f, "Dir"),
            Self::File => write!(f, "File"),
            Self::Symlink => write!(f, "Symlink"),
        }
    }
}

  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
  pub struct ObjectInfo {
      /// LARGEST FIRST (CLAUDE.md Rule 14)
      /// ~24 bytes (largest)
      pub path: PathBuf,

      /// 16 bytes (no timezone overhead)
      pub modified: SystemTime,

      /// ~24 bytes (stack-optimized) 
      pub name: CompactString,

      /// ~24 bytes when Some
      pub extension: Option<CompactString>,

      /// 8-BYTE ALIGNED PRIMITIVES
      pub size: u64,                          // 8 bytes
      pub items_count: u64,                   // 8 bytes

      // FLAGS GROUPED (cache-friendly)
      pub is_dir: bool,                       // 1 byte
      pub is_symlink: bool,                   // 1 byte
      pub metadata_loaded: bool,              // 1 byte
      // + 5 bytes padding = 8-byte boundary
  }
/// Lightweight version optimized for immediate display and minimal allocation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightObjectInfo {
    // LARGEST FIRST (CLAUDE.md Rule 14)
    pub path: PathBuf,                      // ~24 bytes (largest)
    pub name: CompactString,                // ~24 bytes (stack-optimized)
    pub extension: Option<CompactString>,   // ~24 bytes when Some

    // FLAGS GROUPED (cache-friendly)
    pub is_dir: bool,                       // 1 byte
    pub is_symlink: bool,                   // 1 byte
    // + 6 bytes padding = 8-byte boundary
}

impl LightObjectInfo {
    // COMPUTED PROPERTY - No redundant storage
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

    // OPTIMIZED CONSTRUCTION
    pub async fn from_path(path: &Path) -> std::io::Result<Self> {
        let metadata = TokioFs::metadata(path).await?;
        let file_type = metadata.file_type();

        let name = CompactString::new(
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("")
        );

        let extension = if file_type.is_file() {
            path.extension()
                .and_then(OsStr::to_str)
                .map(|s| CompactString::new(s.to_lowercase()))
        } else {
            None
        };

        Ok(Self {
            path: path.to_path_buf(),
            name,
            extension,
            is_dir: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        })
    }

    // CONVERSION TO FULL OBJECTINFO
    pub async fn into_full_info(self) -> std::io::Result<ObjectInfo> {
        let metadata = TokioFs::symlink_metadata(&self.path).await?;

        let size = if self.is_dir { 0 } else { metadata.len() };
        let items_count = if self.is_dir {
            // Optimized directory counting
            match TokioFs::read_dir(&self.path).await {
                Ok(mut entries) => {
                    let mut count = 0u64;
                    while entries.next_entry().await?.is_some() {
                        count += 1;
                    }
                    count
                }
                Err(_) => 0,
            }
        } else {
            0
        };

        let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(ObjectInfo {
            path: self.path,
            modified,
            name: self.name,
            extension: self.extension,
            size,
            items_count,
            is_dir: self.is_dir,
            is_symlink: self.is_symlink,
            metadata_loaded: true,
        })
    }
}

impl ObjectInfo {
      // COMPUTED PROPERTIES - No redundant storage
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

      #[inline]
      #[must_use]
      pub fn size_human(&self) -> String {
          bytesize::ByteSize::b(self.size).to_string()
      }

      // OPTIMIZED CONSTRUCTION - Zero-allocation where possible
      pub async fn from_path(path: &Path) -> std::io::Result<Self> {
          let metadata: Metadata = TokioFs::symlink_metadata(path).await?;
          let file_type: FileType = metadata.file_type();

          let is_dir = file_type.is_dir();
          let is_symlink: bool = file_type.is_symlink();

          // CompactString optimization for filenames
          let name: CompactString = CompactString::new(
              path.file_name()
                  .and_then(OsStr::to_str)
                  .unwrap_or("")
          );

          let extension = path
              .extension()
              .and_then(OsStr::to_str)
              .map(CompactString::new);

          let size = if is_dir { 0 } else { metadata.len() };
          let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);

          Ok(Self {
              path: path.to_path_buf(),
              modified,
              name,
              extension,
              size,
              items_count: 0, // Calculated separately if needed
              is_dir,
              is_symlink,
              metadata_loaded: true,
          })
      }

      #[expect(clippy::cast_possible_wrap, reason = "Expected accuracy")]
      #[must_use]
      pub fn format_date(&self, format: &str) -> String {
          use chrono::{Local, TimeZone};

          let duration: Duration = self.modified
              .duration_since(UNIX_EPOCH)
              .unwrap_or_default();

          let datetime: chrono::DateTime<Local> = Local
              .timestamp_opt(duration.as_secs() as i64, duration.subsec_nanos())
              .single()
              .unwrap_or_default();

          datetime.format(format).to_string()
      }
  }

  // Default implementation
  impl Default for ObjectInfo {
    fn default() -> Self 
    {
        Self 
        {
            path: PathBuf::new(),
            modified: SystemTime::UNIX_EPOCH,
            name: CompactString::const_new(""),
            extension: None,
            size: 0,
            items_count: 0,
            is_dir: false,
            is_symlink: false,
            metadata_loaded: false,
        }
    }
}