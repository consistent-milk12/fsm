//! Extreme performance configuration with zero-allocation serialization and lock-free access

use crate::error::{ClipError, ClipResult};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Lock-free configuration with atomic fields for zero-contention access
#[derive(Debug, Serialize, Deserialize)]
pub struct ClipBoardConfig {
    /// Maximum number of items in clipboard (lock-free atomic access)
    pub max_items: AtomicUsize,

    /// Auto-expire items after nanoseconds (None = never expire)
    #[serde(with = "duration_ns_option")]
    pub item_expiry_ns: Option<AtomicU64>,

    /// Enable clipboard persistence across sessions (atomic flag)
    pub persist_clipboard: AtomicBool,

    /// Clipboard file location for persistence (CompactString for efficiency)
    #[serde(with = "compact_string_option")]
    pub clipboard_file: Option<CompactString>,

    /// Show visual indicators in main UI (atomic flag)
    pub show_clipboard_indicators: AtomicBool,

    /// Confirm destructive operations (atomic flag)
    pub confirm_destructive_ops: AtomicBool,

    /// Enable SIMD acceleration (atomic flag with CPU detection)
    pub enable_simd: AtomicBool,

    /// Enable memory mapping for large clipboards (atomic flag)
    pub enable_mmap: AtomicBool,

    /// Memory mapping threshold in bytes (atomic)
    pub mmap_threshold_bytes: AtomicU64,

    /// Enable parallel processing with Rayon (atomic flag)
    pub enable_parallel: AtomicBool,

    /// Maximum number of parallel threads (0 = auto-detect)
    pub max_parallel_threads: AtomicUsize,

    /// Enable lock-free data structures (atomic flag)
    pub enable_lockfree: AtomicBool,

    /// Clipboard cache size for frequent access patterns
    pub cache_size: AtomicUsize,

    /// Performance monitoring enabled (atomic flag)
    pub enable_performance_monitoring: AtomicBool,
}

impl Default for ClipBoardConfig {
    fn default() -> Self {
        Self {
            max_items: AtomicUsize::new(1000), // Increased for high-performance use
            item_expiry_ns: Some(AtomicU64::new(3_600_000_000_000)), // 1 hour in nanoseconds
            persist_clipboard: AtomicBool::new(true),
            clipboard_file: None, // Will use default location
            show_clipboard_indicators: AtomicBool::new(true),
            confirm_destructive_ops: AtomicBool::new(true),
            enable_simd: AtomicBool::new(true), // Enable SIMD by default
            enable_mmap: AtomicBool::new(true), // Enable memory mapping
            mmap_threshold_bytes: AtomicU64::new(1_048_576), // 1MB threshold
            enable_parallel: AtomicBool::new(true), // Enable parallel processing
            max_parallel_threads: AtomicUsize::new(0), // Auto-detect
            enable_lockfree: AtomicBool::new(true), // Enable lock-free structures
            cache_size: AtomicUsize::new(256),  // LRU cache for frequent items
            enable_performance_monitoring: AtomicBool::new(false), // Opt-in monitoring
        }
    }
}

impl ClipBoardConfig {
    /// Create optimized configuration for maximum performance
    pub fn high_performance() -> Self {
        Self {
            max_items: AtomicUsize::new(10_000), // High capacity
            item_expiry_ns: None,                // No expiration for maximum retention
            persist_clipboard: AtomicBool::new(true),
            clipboard_file: None,
            show_clipboard_indicators: AtomicBool::new(true),
            confirm_destructive_ops: AtomicBool::new(false), // Skip confirmations for speed
            enable_simd: AtomicBool::new(true),
            enable_mmap: AtomicBool::new(true),
            mmap_threshold_bytes: AtomicU64::new(262_144), // 256KB threshold (more aggressive)
            enable_parallel: AtomicBool::new(true),
            max_parallel_threads: AtomicUsize::new(0), // Use all cores
            enable_lockfree: AtomicBool::new(true),
            cache_size: AtomicUsize::new(1024), // Large cache
            enable_performance_monitoring: AtomicBool::new(true), // Enable for optimization
        }
    }

    /// Create conservative configuration for compatibility
    pub fn conservative() -> Self {
        Self {
            max_items: AtomicUsize::new(100), // Conservative limit
            item_expiry_ns: Some(AtomicU64::new(1_800_000_000_000)), // 30 minutes
            persist_clipboard: AtomicBool::new(false), // Disable persistence
            clipboard_file: None,
            show_clipboard_indicators: AtomicBool::new(true),
            confirm_destructive_ops: AtomicBool::new(true),
            enable_simd: AtomicBool::new(false), // Disable for compatibility
            enable_mmap: AtomicBool::new(false), // Disable memory mapping
            mmap_threshold_bytes: AtomicU64::new(u64::MAX), // Never use mmap
            enable_parallel: AtomicBool::new(false), // Single-threaded
            max_parallel_threads: AtomicUsize::new(1),
            enable_lockfree: AtomicBool::new(false), // Use standard locks
            cache_size: AtomicUsize::new(32),        // Small cache
            enable_performance_monitoring: AtomicBool::new(false),
        }
    }

    /// Load configuration from file with MessagePack for performance
    pub async fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> ClipResult<Self> {
        let content = tokio::fs::read(path.as_ref()).await.map_err(|e| {
            ClipError::ConfigError(CompactString::from(format!(
                "Failed to read config file: {e}"
            )))
        })?;

        // Use MessagePack for faster deserialization
        let config: Self = rmp_serde::from_slice(&content).map_err(|e| {
            ClipError::ConfigError(CompactString::from(format!("Failed to parse config: {e}")))
        })?;

        Ok(config)
    }

    /// Save configuration to file with MessagePack for performance
    pub async fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> ClipResult<()> {
        // Use MessagePack for faster serialization
        let content = rmp_serde::to_vec(self).map_err(|e| {
            ClipError::ConfigError(CompactString::from(format!(
                "Failed to serialize config: {e}"
            )))
        })?;

        tokio::fs::write(path.as_ref(), content)
            .await
            .map_err(|e| {
                ClipError::ConfigError(CompactString::from(format!(
                    "Failed to write config file: {e}"
                )))
            })?;

        Ok(())
    }

    /// Fast lock-free atomic getters for hot paths
    #[inline(always)]
    pub fn max_items(&self) -> usize {
        self.max_items.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn item_expiry_ns(&self) -> Option<u64> {
        self.item_expiry_ns
            .as_ref()
            .map(|atomic| atomic.load(Ordering::Relaxed))
    }

    #[inline(always)]
    pub fn persist_clipboard(&self) -> bool {
        self.persist_clipboard.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn show_clipboard_indicators(&self) -> bool {
        self.show_clipboard_indicators.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn confirm_destructive_ops(&self) -> bool {
        self.confirm_destructive_ops.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn enable_simd(&self) -> bool {
        self.enable_simd.load(Ordering::Relaxed) && self.cpu_supports_simd()
    }

    #[inline(always)]
    pub fn enable_mmap(&self) -> bool {
        self.enable_mmap.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn mmap_threshold_bytes(&self) -> u64 {
        self.mmap_threshold_bytes.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn enable_parallel(&self) -> bool {
        self.enable_parallel.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn max_parallel_threads(&self) -> usize {
        let threads = self.max_parallel_threads.load(Ordering::Relaxed);
        if threads == 0 {
            num_cpus::get()
        } else {
            threads
        }
    }

    #[inline(always)]
    pub fn enable_lockfree(&self) -> bool {
        self.enable_lockfree.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn cache_size(&self) -> usize {
        self.cache_size.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn enable_performance_monitoring(&self) -> bool {
        self.enable_performance_monitoring.load(Ordering::Relaxed)
    }

    /// Fast lock-free atomic setters
    #[inline]
    pub fn set_max_items(&self, value: usize) {
        self.max_items.store(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_item_expiry_ns(&self, value: Option<u64>) {
        if let Some(atomic) = &self.item_expiry_ns
            && let Some(ns) = value
        {
            atomic.store(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn set_enable_simd(&self, value: bool) {
        self.enable_simd.store(value, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_enable_parallel(&self, value: bool) {
        self.enable_parallel.store(value, Ordering::Relaxed);
    }

    /// CPU feature detection for SIMD optimization
    #[inline]
    fn cpu_supports_simd(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("avx2") || is_x86_feature_detected!("sse4.2")
        }
        #[cfg(target_arch = "aarch64")]
        {
            is_aarch64_feature_detected!("neon")
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            false // Conservative fallback for other architectures
        }
    }

    /// Get default clipboard file path with OS-specific optimization
    pub fn default_clipboard_file() -> ClipResult<CompactString> {
        use directories::ProjectDirs;

        let proj_dirs = ProjectDirs::from("com", "fsm", "clipr").ok_or_else(|| {
            ClipError::ConfigError(CompactString::from("Cannot determine config directory"))
        })?;

        let mut path = proj_dirs.data_dir().to_path_buf();
        path.push("clipboard.msgpack"); // Use MessagePack extension

        Ok(CompactString::from(path.to_string_lossy()))
    }

    /// Auto-tune configuration based on system resources
    pub fn auto_tune(&mut self) -> ClipResult<()> {
        // Auto-detect optimal settings based on system capabilities
        let cpu_count = num_cpus::get();
        let available_memory = Self::get_available_memory();

        // Adjust parallel thread count
        let optimal_threads = (cpu_count * 3 / 4).max(1); // Use 75% of cores
        self.max_parallel_threads
            .store(optimal_threads, Ordering::Relaxed);

        // Adjust cache size based on available memory
        let optimal_cache_size = if available_memory > 8 * 1024 * 1024 * 1024 {
            // > 8GB RAM
            2048
        } else if available_memory > 4 * 1024 * 1024 * 1024 {
            // > 4GB RAM
            1024
        } else {
            // <= 4GB RAM
            512
        };
        self.cache_size.store(optimal_cache_size, Ordering::Relaxed);

        // Adjust memory mapping threshold based on available memory
        let optimal_mmap_threshold = if available_memory > 16 * 1024 * 1024 * 1024 {
            // > 16GB RAM - aggressive memory mapping
            128 * 1024 // 128KB
        } else if available_memory > 8 * 1024 * 1024 * 1024 {
            // > 8GB RAM
            512 * 1024 // 512KB
        } else {
            // <= 8GB RAM - conservative
            2 * 1024 * 1024 // 2MB
        };
        self.mmap_threshold_bytes
            .store(optimal_mmap_threshold, Ordering::Relaxed);

        Ok(())
    }

    /// Get available system memory in bytes
    fn get_available_memory() -> u64 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(info) = sys_info::mem_info() {
                info.avail * 1024 // Convert KB to bytes
            } else {
                4 * 1024 * 1024 * 1024 // 4GB fallback
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            8 * 1024 * 1024 * 1024 // 8GB fallback for other platforms
        }
    }
}

impl Clone for ClipBoardConfig {
    fn clone(&self) -> Self {
        Self {
            max_items: AtomicUsize::new(self.max_items.load(Ordering::Relaxed)),
            item_expiry_ns: self
                .item_expiry_ns
                .as_ref()
                .map(|atomic| AtomicU64::new(atomic.load(Ordering::Relaxed))),
            persist_clipboard: AtomicBool::new(self.persist_clipboard.load(Ordering::Relaxed)),
            clipboard_file: self.clipboard_file.clone(),
            show_clipboard_indicators: AtomicBool::new(
                self.show_clipboard_indicators.load(Ordering::Relaxed),
            ),
            confirm_destructive_ops: AtomicBool::new(
                self.confirm_destructive_ops.load(Ordering::Relaxed),
            ),
            enable_simd: AtomicBool::new(self.enable_simd.load(Ordering::Relaxed)),
            enable_mmap: AtomicBool::new(self.enable_mmap.load(Ordering::Relaxed)),
            mmap_threshold_bytes: AtomicU64::new(self.mmap_threshold_bytes.load(Ordering::Relaxed)),
            enable_parallel: AtomicBool::new(self.enable_parallel.load(Ordering::Relaxed)),
            max_parallel_threads: AtomicUsize::new(
                self.max_parallel_threads.load(Ordering::Relaxed),
            ),
            enable_lockfree: AtomicBool::new(self.enable_lockfree.load(Ordering::Relaxed)),
            cache_size: AtomicUsize::new(self.cache_size.load(Ordering::Relaxed)),
            enable_performance_monitoring: AtomicBool::new(
                self.enable_performance_monitoring.load(Ordering::Relaxed),
            ),
        }
    }
}

/// Custom serde serialization for Duration as nanoseconds
mod duration_ns_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::atomic::AtomicU64;

    pub fn serialize<S>(value: &Option<AtomicU64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(atomic) => {
                let ns = atomic.load(std::sync::atomic::Ordering::Relaxed);
                Some(ns).serialize(serializer)
            }
            None => None::<u64>.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AtomicU64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(AtomicU64::new))
    }
}

/// Custom serde serialization for CompactString option
mod compact_string_option {
    use compact_str::CompactString;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<CompactString>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(s) => Some(s.as_str()).serialize(serializer),
            None => None::<&str>.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<CompactString>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        Ok(opt.map(CompactString::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_serialization() {
        let config = ClipBoardConfig::high_performance();
        let temp_path = "/tmp/test_config.msgpack";

        // Test save and load
        config.save_to_file(temp_path).await.unwrap();
        let loaded_config = ClipBoardConfig::load_from_file(temp_path).await.unwrap();

        // Verify atomic values are preserved
        assert_eq!(config.max_items(), loaded_config.max_items());
        assert_eq!(config.enable_simd(), loaded_config.enable_simd());
        assert_eq!(config.enable_parallel(), loaded_config.enable_parallel());

        // Cleanup
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_auto_tune() {
        let mut config = ClipBoardConfig::default();
        config.auto_tune().unwrap();

        // Verify auto-tuning set reasonable values
        assert!(config.max_parallel_threads() > 0);
        assert!(config.cache_size() > 0);
        assert!(config.mmap_threshold_bytes() > 0);
    }

    #[test]
    fn test_cpu_feature_detection() {
        let config = ClipBoardConfig::default();

        // Should not panic and return a boolean
        let _simd_supported = config.cpu_supports_simd();
    }
}
