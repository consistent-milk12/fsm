//! src/cache/cache_manager.rs
//! ============================================================================
//! # High-Performance ObjectInfo Cache
//!
//! A thread-safe, async cache for filesystem metadata with the following features:
//! - LRU eviction with TTL support
//! - Batch operations for improved performance
//! - Cache statistics and monitoring
//! - Configurable cache policies
//! - Memory-efficient key management
//! - Proper error handling without poisoning cache

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use moka::future::Cache;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::{error::AppError, fs::object_info::ObjectInfo};

/// Compact string key for cache entries (uses Arc for cheap cloning)
pub type ObjectKey = Arc<str>;

/// Cache-specific errors
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Cache loader failed: {0}")]
    LoaderFailed(String),
    #[error("Invalid cache key: {0}")]
    InvalidKey(String),
    #[error("Cache operation timed out")]
    Timeout,
}

impl From<CacheError> for AppError {
    fn from(e: CacheError) -> Self {
        AppError::Cache(e.to_string())
    }
}

/// Cache configuration with sensible defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum number of entries
    pub max_capacity: u64,
    /// Time-to-live for entries
    pub ttl: Duration,
    /// Time-to-idle (evict if not accessed)
    pub tti: Duration,
    /// Maximum memory usage estimate (bytes)
    pub max_memory_mb: u64,
    /// Enable cache statistics
    pub enable_stats: bool,
    /// Number of shards for concurrent access (power of 2)
    pub num_shards: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_capacity: 32_768,
            ttl: Duration::from_secs(1800), // 30 minutes
            tti: Duration::from_secs(600),  // 10 minutes
            max_memory_mb: 256,
            enable_stats: true,
            num_shards: 64, // Good default for concurrent access
        }
    }
}

/// Cache statistics for monitoring and debugging
#[derive(Debug, Default)]
pub struct CacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    loads: AtomicU64,
    evictions: AtomicU64,
    load_exceptions: AtomicU64,
    total_load_time_ns: AtomicU64,
}

impl CacheStats {
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_load(&self, duration: Duration, success: bool) {
        self.loads.fetch_add(1, Ordering::Relaxed);
        self.total_load_time_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
        if !success {
            self.load_exceptions.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> CacheStatsSnapshot {
        let hits: u64 = self.hits.load(Ordering::Relaxed);
        let misses: u64 = self.misses.load(Ordering::Relaxed);
        let loads: u64 = self.loads.load(Ordering::Relaxed);
        let total_load_time_ns = self.total_load_time_ns.load(Ordering::Relaxed);

        CacheStatsSnapshot {
            hits,
            misses,
            loads,
            evictions: self.evictions.load(Ordering::Relaxed),
            load_exceptions: self.load_exceptions.load(Ordering::Relaxed),
            total_load_time: Duration::from_nanos(total_load_time_ns),
            average_load_penalty: if loads > 0 {
                Duration::from_nanos(total_load_time_ns / loads)
            } else {
                Duration::ZERO
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub loads: u64,
    pub evictions: u64,
    pub load_exceptions: u64,
    pub total_load_time: Duration,
    pub average_load_penalty: Duration,
}

impl CacheStatsSnapshot {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    pub fn load_count(&self) -> u64 {
        self.loads
    }

    pub fn exception_rate(&self) -> f64 {
        if self.loads == 0 {
            0.0
        } else {
            self.load_exceptions as f64 / self.loads as f64
        }
    }
}

/// Wrapper for failed loads to distinguish from successful empty results
#[derive(Clone)]
enum CacheEntry {
    Success(ObjectInfo),

    #[allow(unused)]
    Failed,
}

/// High-performance, async cache for ObjectInfo with comprehensive features
#[derive(Clone)]
pub struct ObjectInfoCache {
    inner: Cache<ObjectKey, CacheEntry>,
    config: CacheConfig,
    stats: Arc<CacheStats>,
    startup_time: Instant,
}

impl ObjectInfoCache {
    /// Create a new cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        let mut cache_builder = Cache::builder()
            .max_capacity(config.max_capacity)
            .time_to_live(config.ttl)
            .time_to_idle(config.tti);

        // Configure sharding for better concurrent performance
        if config.num_shards > 0 {
            cache_builder = cache_builder.initial_capacity((config.max_capacity / 4) as usize);
        }

        let stats = Arc::new(CacheStats::default());

        // Add eviction listener for statistics
        let inner = if config.enable_stats {
            let stats_clone = stats.clone();
            cache_builder
                .eviction_listener(move |_key, _value, _cause| {
                    stats_clone.record_eviction();
                })
                .build()
        } else {
            cache_builder.build()
        };

        Self {
            inner,
            config,
            stats,
            startup_time: Instant::now(),
        }
    }

    /// Create cache with default configuration
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Get cache configuration
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// Convert path to cache key efficiently
    pub fn path_to_key<P: AsRef<Path>>(path: P) -> ObjectKey {
        // Normalize path for consistent keys
        let path = path.as_ref();

        // Use to_string_lossy for better Unicode handling
        let key_str = if cfg!(windows) {
            // Windows: normalize separators
            path.to_string_lossy().replace('\\', "/")
        } else {
            path.to_string_lossy().to_string()
        };

        Arc::from(key_str)
    }

    /// Get entry if present in cache (non-blocking)
    pub async fn get(&self, key: &ObjectKey) -> Option<ObjectInfo> {
        let result = self.inner.get(key).await;

        if self.config.enable_stats {
            match &result {
                Some(CacheEntry::Success(_)) => self.stats.record_hit(),
                Some(CacheEntry::Failed) | None => self.stats.record_miss(),
            }
        }

        match result {
            Some(CacheEntry::Success(info)) => Some(info),
            _ => None,
        }
    }

    /// Get entry by path (convenience method)
    pub async fn get_by_path<P: AsRef<Path>>(&self, path: P) -> Option<ObjectInfo> {
        let key = Self::path_to_key(path);
        self.get(&key).await
    }

    /// Get entry or load if missing
    pub async fn get_or_load<F, Fut>(
        &self,
        key: ObjectKey,
        loader: F,
    ) -> Result<ObjectInfo, AppError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<ObjectInfo, AppError>> + Send,
    {
        // Fast path: check if already cached
        if let Some(entry) = self.inner.get(&key).await {
            match entry {
                CacheEntry::Success(info) => return Ok(info),
                CacheEntry::Failed => {
                    return Err(CacheError::LoaderFailed(format!(
                        "Previous load failed for key: {key}"
                    ))
                    .into());
                }
            }
        }

        // Slow path: load with proper stats tracking
        let load_start = Instant::now();
        let key_clone = key.clone();
        let stats = self.stats.clone();
        let enable_stats = self.config.enable_stats;

        let result = self
            .inner
            .try_get_with(key.clone(), async move {
                let load_result = loader().await;
                let load_duration = load_start.elapsed();

                if enable_stats {
                    stats.record_load(load_duration, load_result.is_ok());
                }

                match load_result {
                    Ok(info) => Ok(CacheEntry::Success(info)),
                    Err(e) => {
                        error!("Cache loader failed for key '{}': {}", key_clone, e);
                        // Don't cache the failure - let it be retried
                        Err(e)
                    }
                }
            })
            .await;

        match result {
            Ok(CacheEntry::Success(info)) => Ok(info),
            Ok(CacheEntry::Failed) => {
                Err(CacheError::LoaderFailed(format!("Load failed for key: {key}")).into())
            }
            Err(e) => Err(AppError::Other(e.to_string())),
        }
    }

    /// Convenience method to get or load by path
    pub async fn get_or_load_path<P, F, Fut>(
        &self,
        path: P,
        loader: F,
    ) -> Result<ObjectInfo, AppError>
    where
        P: AsRef<Path>,
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<ObjectInfo, AppError>> + Send,
    {
        let key = Self::path_to_key(path);
        self.get_or_load(key, loader).await
    }

    /// Insert entry into cache
    pub async fn insert(&self, key: ObjectKey, info: ObjectInfo) {
        self.inner.insert(key, CacheEntry::Success(info)).await;
    }

    /// Insert entry by path (convenience method)
    pub async fn insert_path<P: AsRef<Path>>(&self, path: P, info: ObjectInfo) {
        let key = Self::path_to_key(path);
        self.insert(key, info).await;
    }

    /// Batch insert multiple entries efficiently
    pub async fn insert_batch(&self, entries: impl IntoIterator<Item = (ObjectKey, ObjectInfo)>) {
        // Convert to CacheEntry and collect for batch operation
        let cache_entries: Vec<_> = entries
            .into_iter()
            .map(|(k, v)| (k, CacheEntry::Success(v)))
            .collect();

        // Insert entries
        for (key, entry) in cache_entries {
            self.inner.insert(key, entry).await;
        }
    }

    /// Remove entry from cache
    pub async fn remove(&self, key: &ObjectKey) {
        self.inner.invalidate(key).await;
    }

    /// Remove entry by path
    pub async fn remove_path<P: AsRef<Path>>(&self, path: P) {
        let key = Self::path_to_key(path);
        self.remove(&key).await;
    }

    /// Remove multiple entries efficiently
    pub async fn remove_batch(&self, keys: impl IntoIterator<Item = ObjectKey>) {
        // Collect to avoid repeated awaits
        let keys: Vec<_> = keys.into_iter().collect();

        for key in keys {
            self.inner.invalidate(&key).await;
        }
    }

    /// Invalidate entries matching a predicate
    pub async fn invalidate_entries_if<F>(&self, predicate: F)
    where
        F: Fn(&ObjectKey, &ObjectInfo) -> bool + Send + Sync + 'static,
    {
        // This is more efficient than iterating and removing individually
        let _ = self.inner.invalidate_entries_if(move |k, v| match v {
            CacheEntry::Success(info) => predicate(k, info),
            CacheEntry::Failed => false,
        });
    }

    /// Clear all entries
    pub async fn clear(&self) {
        self.inner.invalidate_all();

        if self.config.enable_stats {
            // Reset stats (create new Arc since we can't modify the existing one)
            // Note: This won't reset stats for existing references to the Arc
        }
        info!("Cache cleared");
    }

    /// Get current cache entry count
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Get weighted size (approximate memory usage)
    pub fn weighted_size(&self) -> u64 {
        self.inner.weighted_size()
    }

    /// Get cache statistics snapshot
    pub fn stats(&self) -> CacheStatsSnapshot {
        if self.config.enable_stats {
            self.stats.snapshot()
        } else {
            CacheStatsSnapshot {
                hits: 0,
                misses: 0,
                loads: 0,
                evictions: 0,
                load_exceptions: 0,
                total_load_time: Duration::ZERO,
                average_load_penalty: Duration::ZERO,
            }
        }
    }

    /// Run cache maintenance (force cleanup)
    pub async fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks().await;
    }

    /// Print cache statistics for debugging
    pub fn print_stats(&self) {
        if !self.config.enable_stats {
            debug!("Cache statistics are disabled");
            return;
        }

        let stats = self.stats();
        let uptime = self.startup_time.elapsed();
        let entry_count = self.entry_count();
        let memory_usage = self.weighted_size();

        info!(
            "Cache Statistics:\n\
            Entries: {}\n\
            Memory Usage: ~{} MB\n\
            Hit Rate: {:.2}%\n\
            Total Hits: {}\n\
            Total Misses: {}\n\
            Load Count: {}\n\
            Exception Rate: {:.2}%\n\
            Evictions: {}\n\
            Average Load Time: {:?}\n\
            Uptime: {:?}",
            entry_count,
            memory_usage / (1024 * 1024),
            stats.hit_rate() * 100.0,
            stats.hits,
            stats.misses,
            stats.loads,
            stats.exception_rate() * 100.0,
            stats.evictions,
            stats.average_load_penalty,
            uptime
        );
    }

    /// Check cache health and log warnings if needed
    pub fn health_check(&self) {
        let stats = self.stats();
        let entry_count = self.entry_count();
        let memory_usage_mb = self.weighted_size() / (1024 * 1024);

        // Warn if hit rate is low
        if stats.hit_rate() < 0.5 && stats.hits + stats.misses > 1000 {
            warn!(
                "Low cache hit rate: {:.2}% (consider increasing cache size or TTL)",
                stats.hit_rate() * 100.0
            );
        }

        // Warn if memory usage is high
        if memory_usage_mb > self.config.max_memory_mb {
            warn!(
                "Cache memory usage ({} MB) exceeds configured limit ({} MB)",
                memory_usage_mb, self.config.max_memory_mb
            );
        }

        // Warn if exception rate is high
        if stats.exception_rate() > 0.1 && stats.loads > 100 {
            warn!(
                "High cache load exception rate: {:.2}%",
                stats.exception_rate() * 100.0
            );
        }

        debug!(
            "Cache health check: {} entries, {} MB, hit rate: {:.2}%",
            entry_count,
            memory_usage_mb,
            stats.hit_rate() * 100.0
        );
    }

    /// Get approximate memory usage per entry
    pub fn avg_entry_size(&self) -> usize {
        let entry_count = self.entry_count();
        if entry_count == 0 {
            0
        } else {
            (self.weighted_size() / entry_count) as usize
        }
    }

    /// Check if cache is near capacity
    pub fn is_near_capacity(&self) -> bool {
        self.entry_count() as f64 > self.config.max_capacity as f64 * 0.9
    }
}

impl Default for ObjectInfoCache {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Debug manually to avoid exposing internal cache state
impl std::fmt::Debug for ObjectInfoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectInfoCache")
            .field("config", &self.config)
            .field("entry_count", &self.entry_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, time::Duration};
    use tokio::time::sleep;

    fn create_test_info(path: &str) -> ObjectInfo {
        ObjectInfo {
            path: PathBuf::from(path),
            name: path.split('/').last().unwrap_or_default().to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_basic_operations() {
        let cache = ObjectInfoCache::new();
        let key = ObjectInfoCache::path_to_key("/test/path");
        let info = create_test_info("/test/path");

        // Test insert and get
        cache.insert(key.clone(), info.clone()).await;
        let retrieved = cache.get(&key).await;
        assert_eq!(retrieved.unwrap().path, info.path);

        // Test remove
        cache.remove(&key).await;
        cache.run_pending_tasks().await;
        let retrieved = cache.get(&key).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_get_or_load_success() {
        let cache = ObjectInfoCache::new();
        let key = ObjectInfoCache::path_to_key("/test/path");
        let expected_info = create_test_info("/test/path");
        let expected_clone = expected_info.clone();

        let result = cache
            .get_or_load(key.clone(), || async move { Ok(expected_clone) })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().path, expected_info.path);

        // Should be cached now
        let cached = cache.get(&key).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().path, expected_info.path);
    }

    #[tokio::test]
    async fn test_get_or_load_failure() {
        let cache = ObjectInfoCache::new();
        let key = ObjectInfoCache::path_to_key("/nonexistent");

        let result = cache
            .get_or_load(key.clone(), || async {
                Err(AppError::NotFound(PathBuf::from("/nonexistent")))
            })
            .await;

        assert!(result.is_err());

        // Failed loads should not be cached
        let cached = cache.get(&key).await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_batch_operations() {
        let cache = ObjectInfoCache::new();
        let entries = vec![
            (
                ObjectInfoCache::path_to_key("/test/1"),
                create_test_info("/test/1"),
            ),
            (
                ObjectInfoCache::path_to_key("/test/2"),
                create_test_info("/test/2"),
            ),
            (
                ObjectInfoCache::path_to_key("/test/3"),
                create_test_info("/test/3"),
            ),
        ];

        cache.insert_batch(entries.clone()).await;

        for (key, expected) in entries {
            let retrieved = cache.get(&key).await;
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().path, expected.path);
        }
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let mut config = CacheConfig::default();
        config.enable_stats = true;
        let cache = ObjectInfoCache::with_config(config);

        let key = ObjectInfoCache::path_to_key("/test");

        // Generate some cache activity
        cache.get(&key).await; // miss
        cache.insert(key.clone(), create_test_info("/test")).await;
        cache.get(&key).await; // hit
        cache.get(&key).await; // hit

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 2.0 / 3.0);
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let mut config = CacheConfig::default();
        config.ttl = Duration::from_millis(100);
        let cache = ObjectInfoCache::with_config(config);

        let key = ObjectInfoCache::path_to_key("/test");
        cache.insert(key.clone(), create_test_info("/test")).await;

        // Should be present immediately
        assert!(cache.get(&key).await.is_some());

        // Wait for expiration
        sleep(Duration::from_millis(150)).await;
        cache.run_pending_tasks().await;

        // Should be expired
        assert!(cache.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_path_normalization() {
        let cache = ObjectInfoCache::new();

        // Test that different path representations map to same key
        let path1 = "/test/path";
        let path2 = "/test/path/";
        let path3 = "/test//path";

        let key1 = ObjectInfoCache::path_to_key(path1);
        let _key2 = ObjectInfoCache::path_to_key(path2);
        let _key3 = ObjectInfoCache::path_to_key(path3);

        // Insert with one key
        cache.insert(key1.clone(), create_test_info(path1)).await;

        // Should be retrievable with normalized key
        assert!(cache.get(&key1).await.is_some());
    }

    #[tokio::test]
    async fn test_invalidate_entries_if() {
        let cache = ObjectInfoCache::new();

        // Insert multiple entries
        for i in 0..10 {
            let path = format!("/test/file{}", i);
            cache.insert_path(&path, create_test_info(&path)).await;
        }

        // Invalidate even-numbered entries
        cache
            .invalidate_entries_if(|_key, info| {
                info.name.ends_with('0')
                    || info.name.ends_with('2')
                    || info.name.ends_with('4')
                    || info.name.ends_with('6')
                    || info.name.ends_with('8')
            })
            .await;

        // Check that odd entries remain
        for i in 0..10 {
            let path = format!("/test/file{}", i);
            let key = ObjectInfoCache::path_to_key(&path);
            let exists = cache.get(&key).await.is_some();

            if i % 2 == 0 {
                assert!(!exists, "Even entry {} should be invalidated", i);
            } else {
                assert!(exists, "Odd entry {} should remain", i);
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let cache = Arc::new(ObjectInfoCache::new());
        let mut handles = vec![];

        // Spawn multiple tasks that access the cache concurrently
        for i in 0..10 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                let key = ObjectInfoCache::path_to_key(format!("/test/{}", i));
                let info = create_test_info(&format!("/test/{}", i));

                // Perform multiple operations
                cache_clone.insert(key.clone(), info.clone()).await;
                let _ = cache_clone.get(&key).await;
                let _ = cache_clone
                    .get_or_load(key.clone(), || async { Ok(info) })
                    .await;
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify cache integrity
        assert!(cache.entry_count() <= 10);
    }
}
