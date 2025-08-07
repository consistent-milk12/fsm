//! `src/cache/cache_manager.rs`
//! ============================================================================
//! # High-Performance `ObjectInfo` Cache
//!
//! A thread-safe, async cache for filesystem metadata with the following features:
//! - LRU eviction with TTL support
//! - Batch operations for improved performance
//! - Cache statistics and monitoring
//! - Configurable cache policies
//! - Memory-efficient key management
//! - Proper error handling without poisoning cache

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use compact_str::ToCompactString;
use moka::future::Cache;
// Serde traits now imported via config module
use thiserror::Error;
use tracing::{debug, error, info, instrument, warn};

use crate::{config::CacheConfig, error_core::CoreError, fs::object_info::ObjectInfo};

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

impl From<CacheError> for CoreError {
    fn from(e: CacheError) -> Self {
        Self::Cache(e.to_compact_string())
    }
}

// Cache configuration is now centralized in src/config.rs
// This module re-exports it for backward compatibility

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

    #[expect(clippy::cast_possible_truncation, reason = "Expected accurrcy")]
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
    #[expect(clippy::cast_precision_loss, reason = "Expected precision loss")]
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    #[must_use]
    pub const fn load_count(&self) -> u64 {
        self.loads
    }

    #[expect(clippy::cast_precision_loss, reason = "Expected precision loss")]
    #[must_use]
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

/// High-performance, async cache for `ObjectInfo` with comprehensive features
#[derive(Clone)]
pub struct ObjectInfoCache {
    inner: Cache<ObjectKey, CacheEntry>,
    config: CacheConfig,
    stats: Arc<CacheStats>,
    startup_time: Instant,
}

impl ObjectInfoCache {
    /// Create a new cache with custom configuration
    #[must_use]
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
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Get cache configuration
    #[must_use]
    pub const fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// Convert path to cache key efficiently
    pub fn path_to_key<P: AsRef<Path>>(path: P) -> ObjectKey {
        // Normalize path for consistent keys
        let path: &Path = path.as_ref();

        // Use to_string_lossy for better Unicode handling
        let key_str: String = if cfg!(windows) {
            // Windows: normalize separators
            path.to_string_lossy().replace('\\', "/")
        } else {
            path.to_string_lossy().to_string()
        };

        Arc::from(key_str)
    }

    /// Get entry if present in cache (non-blocking) with operational tracing
    #[instrument(skip(self), fields(cache_key = %key))]
    pub async fn get(&self, key: &ObjectKey) -> Option<ObjectInfo> {
        let lookup_start = Instant::now();
        let result: Option<CacheEntry> = self.inner.get(key).await;
        let lookup_duration = lookup_start.elapsed();

        if self.config.enable_stats {
            match &result {
                Some(CacheEntry::Success(_)) => {
                    self.stats.record_hit();
                    info!(
                        marker = "CACHE_OPERATION",
                        operation_type = "cache_hit",
                        cache_key = %key,
                        lookup_time_us = lookup_duration.as_micros(),
                        "Cache hit - fast retrieval"
                    );
                }
                Some(CacheEntry::Failed) => {
                    self.stats.record_miss();
                    warn!(
                        marker = "CACHE_OPERATION",
                        operation_type = "cache_failed_entry",
                        cache_key = %key,
                        "Cache contained failed entry"
                    );
                }
                None => {
                    self.stats.record_miss();
                    debug!(
                        marker = "CACHE_OPERATION",
                        operation_type = "cache_miss",
                        cache_key = %key,
                        lookup_time_us = lookup_duration.as_micros(),
                        "Cache miss - key not found"
                    );
                }
            }
        }

        match result {
            Some(CacheEntry::Success(info)) => Some(info),
            _ => None,
        }
    }

    /// Get entry by path (convenience method) with path normalization tracing
    #[instrument(skip(self), fields(path = %path.as_ref().display()))]
    pub async fn get_by_path<P: AsRef<Path>>(&self, path: P) -> Option<ObjectInfo> {
        let key = Self::path_to_key(&path);
        debug!(
            marker = "CACHE_OPERATION",
            operation_type = "path_to_key_conversion",
            original_path = %path.as_ref().display(),
            cache_key = %key,
            "Converted path to cache key"
        );
        self.get(&key).await
    }

    /// Get entry or load if missing with comprehensive tracing
    #[expect(clippy::too_many_lines, reason = "Expected due to logging")]
    #[instrument(skip(self, loader), fields(cache_key = %key, operation_type = "cache_get_or_load"))]
    pub async fn get_or_load<F, Fut>(
        &self,
        key: ObjectKey,
        loader: F,
    ) -> Result<ObjectInfo, CoreError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<ObjectInfo, CoreError>> + Send,
    {
        // Fast path: check if already cached
        let fast_check_start = Instant::now();
        if let Some(entry) = self.inner.get(&key).await {
            let fast_check_duration = fast_check_start.elapsed();
            match entry {
                CacheEntry::Success(info) => {
                    info!(
                        marker = "CACHE_OPERATION",
                        operation_type = "fast_path_hit",
                        cache_key = %key,
                        fast_check_time_us = fast_check_duration.as_micros(),
                        "Fast path cache hit in get_or_load"
                    );
                    return Ok(info);
                }
                CacheEntry::Failed => {
                    warn!(
                        marker = "CACHE_OPERATION",
                        operation_type = "fast_path_failed_entry",
                        cache_key = %key,
                        "Fast path found failed entry"
                    );
                    return Err(CacheError::LoaderFailed(format!(
                        "Previous load failed for key: {key}"
                    ))
                    .into());
                }
            }
        }

        info!(
            marker = "CACHE_OPERATION",
            operation_type = "slow_path_loading",
            cache_key = %key,
            fast_check_time_us = fast_check_start.elapsed().as_micros(),
            "Taking slow path - loading with try_get_with"
        );

        // Slow path: load with proper stats tracking
        let load_start: Instant = Instant::now();
        let key_clone: Arc<str> = key.clone();
        let stats: Arc<CacheStats> = self.stats.clone();
        let enable_stats: bool = self.config.enable_stats;

        let result: Result<CacheEntry, Arc<CoreError>> = self
            .inner
            .try_get_with(key.clone(), async move {
                info!(
                    marker = "CACHE_OPERATION",
                    operation_type = "loader_function_start",
                    cache_key = %key_clone,
                    "Starting loader function execution"
                );

                let load_result: Result<ObjectInfo, CoreError> = loader().await;
                let load_duration: Duration = load_start.elapsed();

                if enable_stats {
                    stats.record_load(load_duration, load_result.is_ok());
                }

                match load_result {
                    Ok(info) => {
                        info!(
                            marker = "CACHE_OPERATION",
                            operation_type = "loader_success",
                            cache_key = %key_clone,
                            load_duration_ms = load_duration.as_millis(),
                            "Loader function succeeded, caching result"
                        );
                        Ok(CacheEntry::Success(info))
                    }
                    Err(e) => {
                        error!(
                            marker = "CACHE_OPERATION",
                            operation_type = "loader_failure",
                            cache_key = %key_clone,
                            load_duration_ms = load_duration.as_millis(),
                            error = %e,
                            "Loader function failed, not caching failure"
                        );
                        // Don't cache the failure - let it be retried
                        Err(e)
                    }
                }
            })
            .await;

        match result {
            Ok(CacheEntry::Success(info)) => {
                info!(
                    marker = "CACHE_OPERATION",
                    operation_type = "get_or_load_success",
                    cache_key = %key,
                    total_duration_ms = load_start.elapsed().as_millis(),
                    "get_or_load completed successfully"
                );
                Ok(info)
            }
            Ok(CacheEntry::Failed) => {
                error!(
                    marker = "CACHE_OPERATION",
                    operation_type = "get_or_load_failed_entry",
                    cache_key = %key,
                    "get_or_load returned failed entry"
                );
                Err(CacheError::LoaderFailed(format!("Load failed for key: {key}")).into())
            }
            Err(e) => {
                error!(
                    marker = "CACHE_OPERATION",
                    operation_type = "get_or_load_error",
                    cache_key = %key,
                    error = %e,
                    "get_or_load encountered error"
                );

                Err(CoreError::Other(e.to_compact_string()))
            }
        }
    }

    /// Convenience method to get or load by path
    pub async fn get_or_load_path<P, F, Fut>(
        &self,
        path: P,
        loader: F,
    ) -> Result<ObjectInfo, CoreError>
    where
        P: AsRef<Path>,
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<ObjectInfo, CoreError>> + Send,
    {
        let key: Arc<str> = Self::path_to_key(path);
        self.get_or_load(key, loader).await
    }

    /// Insert entry into cache with operational tracing
    #[instrument(skip(self, info), fields(cache_key = %key))]
    pub async fn insert(&self, key: ObjectKey, info: ObjectInfo) {
        let insert_start: Instant = Instant::now();
        self.inner
            .insert(key.clone(), CacheEntry::Success(info.clone()))
            .await;
        let insert_duration: Duration = insert_start.elapsed();

        info!(
            marker = "CACHE_OPERATION",
            operation_type = "cache_insert",
            cache_key = %key,
            insert_duration_us = insert_duration.as_micros(),
            path = %info.path.display(),
            "Inserted entry into cache"
        );
    }

    /// Insert entry by path with path conversion tracing
    #[instrument(skip(self, info), fields(path = %path.as_ref().display()))]
    pub async fn insert_path<P: AsRef<Path>>(&self, path: P, info: ObjectInfo) {
        let key = Self::path_to_key(&path);
        debug!(
            marker = "CACHE_OPERATION",
            operation_type = "insert_path_conversion",
            original_path = %path.as_ref().display(),
            cache_key = %key,
            "Converting path to key for cache insertion"
        );
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
    pub fn invalidate_entries_if<F>(&self, predicate: F)
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
    pub fn clear(&self) {
        self.inner.invalidate_all();

        if self.config.enable_stats {
            // Reset stats (create new Arc since we can't modify the existing one)
            // Note: This won't reset stats for existing references to the Arc
        }
        info!("Cache cleared");
    }

    /// Get current cache entry count
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Get weighted size (approximate memory usage)
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.inner.weighted_size()
    }

    /// Get cache statistics snapshot
    #[must_use]
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

        let stats: CacheStatsSnapshot = self.stats();
        let uptime: Duration = self.startup_time.elapsed();
        let entry_count: u64 = self.entry_count();
        let memory_usage: u64 = self.weighted_size();

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
        let stats: CacheStatsSnapshot = self.stats();
        let entry_count: u64 = self.entry_count();
        let memory_usage_mb: u64 = self.weighted_size() / (1024 * 1024);

        // Warn if hit rate is low
        if stats.hit_rate() < 0.5 && stats.hits + stats.misses > 1000 {
            info!(
                "Low cache hit rate: {:.2}% (consider increasing cache size or TTL)",
                stats.hit_rate() * 100.0
            );
        }

        // Warn if memory usage is high
        if memory_usage_mb > self.config.max_memory_mb {
            info!(
                "Cache memory usage ({} MB) exceeds configured limit ({} MB)",
                memory_usage_mb, self.config.max_memory_mb
            );
        }

        // Warn if exception rate is high
        if stats.exception_rate() > 0.1 && stats.loads > 100 {
            info!(
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

    #[must_use]
    #[expect(clippy::cast_possible_truncation, reason = "Expected accuracy loss")]
    /// Get approximate memory usage per entry
    pub fn avg_entry_size(&self) -> usize {
        let entry_count: u64 = self.entry_count();

        if entry_count == 0 {
            0
        } else {
            (self.weighted_size() / entry_count) as usize
        }
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "Expected accuracy loss")]
    /// Check if cache is near capacity
    pub fn is_near_capacity(&self) -> bool {
        self.entry_count() as f64 > self.config.max_capacity as f64 * 0.9
    }
}

impl ObjectInfoCache {
    /// Pre-warm cache with directory relationships using iterative queue-based approach
    #[instrument(skip(self), fields(base_path = %base_path.as_ref().display()))]
    pub async fn warm_directory_relationships<P: AsRef<Path>>(
        &self,
        base_path: P,
        depth: usize,
    ) -> Result<usize, CoreError> {
        let base_path: &Path = base_path.as_ref();
        let mut warmed_count: usize = 0;
        let mut work_queue: VecDeque<(PathBuf, usize)> = VecDeque::new();

        // Warm parent directory first
        if let Some(parent) = base_path.parent() {
            let parent_key: Arc<str> = Self::path_to_key(parent);
            if self.get(&parent_key).await.is_none() {
                match ObjectInfo::from_path_sync(parent) {
                    Ok(info) => {
                        self.insert(parent_key, info).await;

                        warmed_count += 1;
                    }

                    Err(e) => warn!(
                        "Failed to warm parent directory {}: {}",
                        parent.display(),
                        e
                    ),
                }
            }
        }

        // Start with base path
        work_queue.push_back((base_path.to_path_buf(), depth));

        // Process queue iteratively - no recursion
        while let Some((current_path, remaining_depth)) = work_queue.pop_front() {
            if remaining_depth == 0 {
                continue;
            }

            match tokio::fs::read_dir(&current_path).await {
                Ok(mut entries) => {
                    while let Some(entry) = entries.next_entry().await? {
                        let entry_path: PathBuf = entry.path();
                        let entry_key: Arc<str> = Self::path_to_key(&entry_path);

                        if self.get(&entry_key).await.is_none() {
                            match ObjectInfo::from_path_sync(&entry_path) {
                                Ok(info) => {
                                    self.insert(entry_key, info.clone()).await;
                                    warmed_count += 1;

                                    // Add subdirectories to queue for next level
                                    if info.is_dir && remaining_depth > 1 {
                                        work_queue.push_back((entry_path, remaining_depth - 1));
                                    }
                                }

                                Err(e) => {
                                    warn!("Failed to warm entry {}: {}", entry_path.display(), e);
                                }
                            }
                        }
                    }
                }

                Err(e) => warn!(
                    "Failed to read directory for warming {}: {}",
                    current_path.display(),
                    e
                ),
            }
        }

        info!(
            marker = "CACHE_OPERATION",
            operation_type = "cache_warm_complete",
            warmed_count = warmed_count,
            base_path = %base_path.display(),
            "Iterative cache warming completed"
        );

        Ok(warmed_count)
    }

    /// Smart warming based on navigation patterns - optimized for UI responsiveness
    #[instrument(skip(self), fields(current_path = %current_path.as_ref().display()))]
    pub async fn warm_for_navigation<P: AsRef<Path>>(
        &self,
        current_path: P,
    ) -> Result<usize, CoreError> {
        let current_path = current_path.as_ref();
        let mut total_warmed = 0;

        // Warm parent for "go up" navigation (depth 1 - immediate parent only)
        if let Some(parent) = current_path.parent() {
            total_warmed += self.warm_directory_relationships(parent, 1).await?;
        }

        // Warm current directory children for "enter" navigation (depth 1 - immediate children)
        total_warmed += self.warm_directory_relationships(current_path, 1).await?;

        info!(
            marker = "CACHE_OPERATION",
            operation_type = "navigation_warming_complete",
            total_warmed = total_warmed,
            current_path = %current_path.display(),
            "Navigation-optimized cache warming completed"
        );

        Ok(total_warmed)
    }
}

impl Default for ObjectInfoCache {
    fn default() -> Self {
        Self::new()
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "Intended loss of debug data"
)]
// Implement Debug manually to avoid exposing internal cache state
impl std::fmt::Debug for ObjectInfoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectInfoCache")
            .field("config", &self.config)
            .field("entry_count", &self.entry_count())
            .finish()
    }
}
