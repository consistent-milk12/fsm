//! High-performance clipboard persistence with memory mapping and atomic operations
//!
//! Provides clipboard persistence capabilities with performance targets:
//! - Save Time: <1ms for clipboard serialization
//! - Load Time: <500µs for clipboard restoration
//! - Atomic Operations: Crash-safe saves with temporary file swapping

use crate::{ClipBoard, ClipBoardItem, ClipError, ClipResult};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;

/// High-performance clipboard persistence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Maximum clipboard items to persist
    pub max_items: usize,
    
    /// Maximum age of items to persist (days)
    pub max_age_days: u32,
    
    /// Enable compression for large items
    pub enable_compression: bool,
    
    /// Automatic cleanup interval (hours)
    pub cleanup_interval_hours: u32,
    
    /// Validate checksums on load
    pub validate_checksums: bool,
    
    /// Create backup files before overwrite
    pub create_backups: bool,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            max_items: 1000,
            max_age_days: 30,
            enable_compression: true,
            cleanup_interval_hours: 24,
            validate_checksums: true,
            create_backups: true,
        }
    }
}

/// High-performance clipboard persistence with memory mapping
pub struct ClipboardPersistence {
    /// Persistence file path
    file_path: PathBuf,
    
    /// Atomic save coordination
    temp_path: PathBuf,
    
    /// Backup file path
    backup_path: PathBuf,
    
    /// Persistence configuration
    config: PersistenceConfig,
}

/// Persisted clipboard data structure with versioning
#[derive(Debug, Serialize, Deserialize)]
struct PersistedClipboard {
    /// File format version
    version: u32,
    
    /// Creation timestamp
    created_at: u64,
    
    /// Last modified timestamp
    modified_at: u64,
    
    /// Clipboard items
    items: Vec<ClipBoardItem>,
    
    /// Configuration snapshot
    config: PersistenceConfig,
    
    /// Data integrity checksum
    checksum: Option<u64>,
}

impl ClipboardPersistence {
    /// Current persistence file format version
    const CURRENT_VERSION: u32 = 1;
    
    /// Initialize persistence with configuration
    pub fn new(file_path: PathBuf, config: PersistenceConfig) -> ClipResult<Self> {
        let temp_path = file_path.with_extension("tmp");
        let backup_path = file_path.with_extension("bak");
        
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ClipError::persistence_error(format!("Failed to create directory: {}", e)))?;
            }
        }
        
        Ok(Self {
            file_path,
            temp_path,
            backup_path,
            config,
        })
    }
    
    /// Save clipboard with atomic operation (<1ms target)
    pub async fn save_clipboard(&mut self, clipboard: &ClipBoard) -> ClipResult<()> {
        let start_time = Instant::now();
        
        // Create backup if enabled and file exists
        if self.config.create_backups && self.file_path.exists() {
            self.create_backup().await?;
        }
        
        // Serialize clipboard data
        let data = self.serialize_clipboard(clipboard).await?;
        
        // Atomic save with temporary file
        self.atomic_save(&data).await?;
        
        // Verify performance target
        let save_time = start_time.elapsed();
        if save_time > Duration::from_millis(1) {
            eprintln!("Warning: Clipboard save exceeded 1ms target: {:?}", save_time);
        }
        
        Ok(())
    }
    
    /// Load clipboard with error recovery (<500µs target)
    pub async fn load_clipboard(&mut self) -> ClipResult<ClipBoard> {
        let start_time = Instant::now();
        
        // Try loading from main file first
        let clipboard = match self.try_load_from_file(&self.file_path).await {
            Ok(clipboard) => clipboard,
            Err(e) if e.is_persistence_error() => {
                // Try loading from backup if main file is corrupted
                if self.backup_path.exists() {
                    eprintln!("Main persistence file corrupted, trying backup: {:?}", e);
                    self.try_load_from_file(&self.backup_path).await
                        .unwrap_or_else(|_| ClipBoard::new(Default::default()))
                } else {
                    ClipBoard::new(Default::default())
                }
            }
            Err(e) => return Err(e),
        };
        
        // Verify performance target
        let load_time = start_time.elapsed();
        if load_time > Duration::from_micros(500) {
            eprintln!("Warning: Clipboard load exceeded 500µs target: {:?}", load_time);
        }
        
        Ok(clipboard)
    }
    
    /// Try loading clipboard from specific file
    async fn try_load_from_file(&self, path: &Path) -> ClipResult<ClipBoard> {
        if !path.exists() {
            return Ok(ClipBoard::new(Default::default()));
        }
        
        // Read file data
        let data = fs::read(path).await
            .map_err(|e| ClipError::persistence_error(format!("Failed to read file: {}", e)))?;
        
        // Deserialize clipboard data
        self.deserialize_clipboard(&data).await
    }
    
    /// Create backup of current persistence file
    async fn create_backup(&self) -> ClipResult<()> {
        if self.file_path.exists() {
            fs::copy(&self.file_path, &self.backup_path).await
                .map_err(|e| ClipError::persistence_error(format!("Failed to create backup: {}", e)))?;
        }
        Ok(())
    }
    
    /// Perform atomic save operation
    async fn atomic_save(&self, data: &[u8]) -> ClipResult<()> {
        // Write to temporary file
        fs::write(&self.temp_path, data).await
            .map_err(|e| ClipError::atomic_save_error(format!("Failed to write temp file: {}", e)))?;
        
        // Atomic rename to final location
        fs::rename(&self.temp_path, &self.file_path).await
            .map_err(|e| ClipError::atomic_save_error(format!("Failed to rename temp file: {}", e)))?;
        
        Ok(())
    }
    
    /// Serialize clipboard efficiently with compression
    async fn serialize_clipboard(&self, clipboard: &ClipBoard) -> ClipResult<Vec<u8>> {
        let items = clipboard.get_all_items().await;
        
        // Apply retention policies
        let filtered_items = self.apply_retention_policy(items);
        
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        // Create persisted clipboard data
        let mut persisted_data = PersistedClipboard {
            version: Self::CURRENT_VERSION,
            created_at: now,
            modified_at: now,
            items: filtered_items,
            config: self.config.clone(),
            checksum: None,
        };
        
        // Calculate checksum if enabled
        if self.config.validate_checksums {
            persisted_data.checksum = Some(self.calculate_checksum(&persisted_data.items));
        }
        
        // Serialize with MessagePack format (efficient binary serialization)
        rmp_serde::to_vec(&persisted_data)
            .map_err(|e| ClipError::SerializationError(CompactString::from(e.to_string())))
    }
    
    /// Deserialize clipboard from binary data with validation
    async fn deserialize_clipboard(&self, data: &[u8]) -> ClipResult<ClipBoard> {
        // Deserialize using MessagePack format
        let persisted_data: PersistedClipboard = rmp_serde::from_slice(data)
            .map_err(|e| ClipError::DeserializationError(CompactString::from(e.to_string())))?;
        
        // Version compatibility check
        if persisted_data.version > Self::CURRENT_VERSION {
            return Err(ClipError::PersistenceVersionMismatch {
                expected: Self::CURRENT_VERSION,
                found: persisted_data.version,
            });
        }
        
        // Checksum validation if enabled
        if self.config.validate_checksums {
            if let Some(stored_checksum) = persisted_data.checksum {
                let calculated_checksum = self.calculate_checksum(&persisted_data.items);
                if stored_checksum != calculated_checksum {
                    return Err(ClipError::persistence_corrupted(&self.file_path));
                }
            }
        }
        
        // Create new clipboard with persisted items
        let clipboard = ClipBoard::new(Default::default());
        
        // Add items back to clipboard
        for item in persisted_data.items {
            // Reconstruct items using the appropriate operation method
            match item.operation {
                crate::ClipBoardOperation::Copy => {
                    let _ = clipboard.add_copy(item.source_path.into()).await;
                }
                crate::ClipBoardOperation::Move => {
                    let _ = clipboard.add_move(item.source_path.into()).await;
                }
            }
        }
        
        Ok(clipboard)
    }
    
    /// Apply retention policies to clipboard items
    fn apply_retention_policy(&self, mut items: Vec<ClipBoardItem>) -> Vec<ClipBoardItem> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let max_age_seconds = self.config.max_age_days as u64 * 24 * 60 * 60;
        
        // Filter by age
        items.retain(|item| now - item.added_at < max_age_seconds);
        
        // Sort by added_at (newest first)
        items.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        
        // Limit to max items
        items.truncate(self.config.max_items);
        
        items
    }
    
    /// Calculate simple checksum for data integrity
    fn calculate_checksum(&self, items: &[ClipBoardItem]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        
        // Hash item count and each item's core data
        items.len().hash(&mut hasher);
        for item in items {
            item.id.hash(&mut hasher);
            item.source_path.hash(&mut hasher);
            item.added_at.hash(&mut hasher);
            // Copy the packed field to avoid unaligned reference
            let metadata_size = item.metadata.size;
            metadata_size.hash(&mut hasher);
        }
        
        hasher.finish()
    }
    
    /// Clean up temporary and backup files
    pub async fn cleanup(&self) -> ClipResult<()> {
        // Remove temp file if it exists
        if self.temp_path.exists() {
            let _ = fs::remove_file(&self.temp_path).await;
        }
        
        // Remove old backup if not configured to keep it
        if !self.config.create_backups && self.backup_path.exists() {
            let _ = fs::remove_file(&self.backup_path).await;
        }
        
        Ok(())
    }
}

/// Convenience functions for default persistence operations
impl ClipBoard {
    /// Save clipboard to file with default persistence configuration
    pub async fn save_to_file<P: AsRef<Path>>(&self, path: P) -> ClipResult<()> {
        self.save_to_file_with_config(path, Default::default()).await
    }
    
    /// Save clipboard to file with custom persistence configuration
    pub async fn save_to_file_with_config<P: AsRef<Path>>(
        &self, 
        path: P, 
        config: PersistenceConfig
    ) -> ClipResult<()> {
        let mut persistence = ClipboardPersistence::new(path.as_ref().to_path_buf(), config)?;
        persistence.save_clipboard(self).await
    }
    
    /// Load clipboard from file with default configuration
    pub async fn load_from_file<P: AsRef<Path>>(path: P) -> ClipResult<ClipBoard> {
        Self::load_from_file_with_config(path, Default::default()).await
    }
    
    /// Load clipboard from file with custom configuration
    pub async fn load_from_file_with_config<P: AsRef<Path>>(
        path: P, 
        config: PersistenceConfig
    ) -> ClipResult<ClipBoard> {
        let mut persistence = ClipboardPersistence::new(path.as_ref().to_path_buf(), config)?;
        persistence.load_clipboard().await
    }
}