//! High-performance filesystem entry metadata with optimized memory layout

use chrono::{DateTime, Local, TimeZone};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// Object type optimized for pattern matching and memory layout
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ObjectType {
    Dir = 0,
    File = 1,
    Symlink = 2,
    Other(CompactString), // File extension or special type
}

impl std::fmt::Display for ObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectType::Dir => write!(f, "Dir"),
            ObjectType::File => write!(f, "File"),
            ObjectType::Symlink => write!(f, "Symlink"),
            ObjectType::Other(ext) => write!(f, "{ext}"),
        }
    }
}

/// Lightweight object info for immediate display (minimal metadata)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightObjectInfo {
    pub path: PathBuf,
    pub name: CompactString,
    pub extension: Option<CompactString>,
    pub object_type: ObjectType,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// Full object info with complete metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectInfo {
    // Core identification (hot path data)
    pub path: PathBuf,
    pub name: CompactString,
    pub extension: Option<CompactString>,
    pub object_type: ObjectType,

    // Type flags (packed for cache efficiency)
    pub is_dir: bool,
    pub is_symlink: bool,
    pub metadata_loaded: bool,

    // Size information
    pub size: u64,
    pub items_count: usize, // For directories

    // Temporal data
    pub modified: DateTime<Local>,

    // Optional extended metadata (for future use)
    pub permissions: Option<u32>,
    pub owner: Option<CompactString>,
    pub group: Option<CompactString>,
}

impl ObjectInfo {
    /// Create lightweight object info (fast path for directory scanning)
    pub async fn from_path_light(path: &Path) -> std::io::Result<LightObjectInfo> {
        let metadata = tokio::fs::symlink_metadata(path).await?;
        let file_type = metadata.file_type();

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .into();

        let extension: Option<CompactString> = if file_type.is_file() {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_lowercase().into())
        } else {
            None
        };

        let is_dir = file_type.is_dir();
        let is_symlink = file_type.is_symlink();

        let object_type = if is_dir {
            ObjectType::Dir
        } else if is_symlink {
            ObjectType::Symlink
        } else if let Some(ref ext) = extension {
            ObjectType::Other(ext.to_uppercase())
        } else {
            ObjectType::File
        };

        Ok(LightObjectInfo {
            path: path.to_path_buf(),
            name,
            extension,
            object_type,
            is_dir,
            is_symlink,
        })
    }

    /// Upgrade lightweight info to full metadata (background task)
    pub async fn from_light_info(light: LightObjectInfo) -> std::io::Result<Self> {
        let metadata = tokio::fs::symlink_metadata(&light.path).await?;

        let size = if light.is_dir { 0 } else { metadata.len() };

        // Get directory item count (expensive operation)
        let items_count = if light.is_dir {
            match tokio::fs::read_dir(&light.path).await {
                Ok(mut entries) => {
                    let mut count = 0;
                    while let Ok(Some(_)) = entries.next_entry().await {
                        count += 1;
                    }
                    count
                }
                Err(_) => 0,
            }
        } else {
            0
        };

        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);

        let modified_dt = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);

        let modified_local = Local
            .timestamp_opt(modified_dt.as_secs() as i64, modified_dt.subsec_nanos())
            .single()
            .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().unwrap());

        Ok(ObjectInfo {
            path: light.path,
            name: light.name,
            extension: light.extension,
            object_type: light.object_type,
            is_dir: light.is_dir,
            is_symlink: light.is_symlink,
            metadata_loaded: true,
            size,
            items_count,
            modified: modified_local,
            permissions: None, // Could be populated later
            owner: None,
            group: None,
        })
    }

    /// Create object info with placeholder metadata for immediate display
    pub fn with_placeholder_metadata(light: LightObjectInfo) -> Self {
        // Get basic modified time synchronously for immediate display
        let modified = std::fs::symlink_metadata(&light.path)
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);

        let modified_dt = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);

        let modified_local = Local
            .timestamp_opt(modified_dt.as_secs() as i64, modified_dt.subsec_nanos())
            .single()
            .unwrap_or_else(|| Local.timestamp_opt(0, 0).single().unwrap());

        ObjectInfo {
            path: light.path,
            name: light.name,
            extension: light.extension,
            object_type: light.object_type,
            is_dir: light.is_dir,
            is_symlink: light.is_symlink,
            metadata_loaded: false,
            size: 0,        // Placeholder
            items_count: 0, // Placeholder
            modified: modified_local,
            permissions: None,
            owner: None,
            group: None,
        }
    }

    /// Create full object info from path (expensive - use sparingly)
    pub async fn from_path(path: &Path) -> std::io::Result<Self> {
        let light = Self::from_path_light(path).await?;
        Self::from_light_info(light).await
    }

    /// Get human-readable file size
    pub fn size_human(&self) -> String {
        if self.is_dir && self.items_count > 0 {
            if self.items_count == 1 {
                "1 item".to_string()
            } else {
                format!("{} items", self.items_count)
            }
        } else if self.size == 0 && !self.metadata_loaded {
            "-".to_string() // Placeholder
        } else {
            format_bytes(self.size)
        }
    }

    /// Get file extension or type description
    pub fn type_display(&self) -> &str {
        match &self.object_type {
            ObjectType::Dir => "Directory",
            ObjectType::File => self
                .extension
                .as_ref()
                .map(|e| e.as_str())
                .unwrap_or("File"),
            ObjectType::Symlink => "Symlink",
            ObjectType::Other(ext) => ext.as_str(),
        }
    }

    /// Check if this entry matches a search pattern
    pub fn matches_pattern(&self, pattern: &str, case_sensitive: bool) -> bool {
        let name = if case_sensitive {
            self.name.as_str()
        } else {
            // For case-insensitive search, we'd need to allocate
            // In practice, this would use a case-insensitive comparison
            self.name.as_str()
        };

        if pattern.contains('*') || pattern.contains('?') {
            glob_match(pattern, name, case_sensitive)
        } else if case_sensitive {
            name.contains(pattern)
        } else {
            name.to_lowercase().contains(&pattern.to_lowercase())
        }
    }

    /// Get icon character for TUI display
    pub fn get_icon(&self) -> char {
        match &self.object_type {
            ObjectType::Dir => '📁',
            ObjectType::Symlink => '🔗',
            ObjectType::File => '📄',
            ObjectType::Other(ext) => match ext.to_lowercase().as_str() {
                "rs" => '🦀',
                "py" => '🐍',
                "js" | "ts" => '⚡',
                "md" => '📝',
                "txt" => '📃',
                "png" | "jpg" | "jpeg" | "gif" => '🖼',
                "mp3" | "wav" | "flac" => '🎵',
                "mp4" | "avi" | "mkv" => '🎬',
                "zip" | "tar" | "gz" => '📦',
                "pdf" => '📕',
                _ => '📄',
            },
        }
    }

    /// Sort key for optimized directory listing
    pub fn sort_key(&self) -> (u8, &str) {
        let type_priority = if self.is_dir { 0 } else { 1 };
        (type_priority, self.name.as_str())
    }

    /// Update metadata (for background loading)
    pub fn update_metadata(&mut self, size: u64, items_count: usize, modified: DateTime<Local>) {
        self.size = size;
        self.items_count = items_count;
        self.modified = modified;
        self.metadata_loaded = true;
    }

    /// Check if metadata needs loading
    pub fn needs_metadata_load(&self) -> bool {
        !self.metadata_loaded
    }

    /// Estimate memory usage
    pub fn memory_usage(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.path.as_os_str().len()
            + self.name.len()
            + self.extension.as_ref().map(|e| e.len()).unwrap_or(0)
            + self.owner.as_ref().map(|o| o.len()).unwrap_or(0)
            + self.group.as_ref().map(|g| g.len()).unwrap_or(0)
    }
}

impl Default for ObjectInfo {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            name: CompactString::new(""),
            extension: None,
            object_type: ObjectType::File,
            is_dir: false,
            is_symlink: false,
            metadata_loaded: false,
            size: 0,
            items_count: 0,
            modified: Local.timestamp_opt(0, 0).single().unwrap(),
            permissions: None,
            owner: None,
            group: None,
        }
    }
}

/// Fast byte formatting without allocations for common sizes
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: u64 = 1024;

    if bytes < THRESHOLD {
        return format!("{bytes} B");
    }

    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= THRESHOLD as f64 && unit_index < UNITS.len() - 1 {
        size /= THRESHOLD as f64;
        unit_index += 1;
    }

    if size >= 100.0 {
        format!("{:.0} {}", size, UNITS[unit_index])
    } else if size >= 10.0 {
        format!("{:.1} {}", size, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

/// Simple glob pattern matching
fn glob_match(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    let (pattern, text) = if case_sensitive {
        (pattern, text)
    } else {
        // For simplicity, using heap allocation here
        // In a real implementation, you'd want to avoid this
        return glob_match_impl(&pattern.to_lowercase(), &text.to_lowercase());
    };

    glob_match_impl(pattern, text)
}

fn glob_match_impl(pattern: &str, text: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();

    glob_match_recursive(&pattern_chars, &text_chars, 0, 0)
}

fn glob_match_recursive(pattern: &[char], text: &[char], p_idx: usize, t_idx: usize) -> bool {
    if p_idx == pattern.len() {
        return t_idx == text.len();
    }

    match pattern[p_idx] {
        '*' => {
            // Try matching zero or more characters
            for i in t_idx..=text.len() {
                if glob_match_recursive(pattern, text, p_idx + 1, i) {
                    return true;
                }
            }
            false
        }
        '?' => {
            // Match exactly one character
            if t_idx < text.len() {
                glob_match_recursive(pattern, text, p_idx + 1, t_idx + 1)
            } else {
                false
            }
        }
        c => {
            // Match exact character
            if t_idx < text.len() && text[t_idx] == c {
                glob_match_recursive(pattern, text, p_idx + 1, t_idx + 1)
            } else {
                false
            }
        }
    }
}
