<!-- src/cache/cache_manager.rs -->

# High-Performance ObjectInfo Cache

A thread-safe, async cache for filesystem metadata with the following features:
- LRU eviction with TTL support
- Batch operations for improved performance
- Cache statistics and monitoring
- Configurable cache policies
- Memory-efficient key management
- Proper error handling without poisoning cache

## `ObjectKey`

A type alias for `Arc<str>`, representing a compact string key for cache entries. Using `Arc` makes cloning cheap.

## `CacheError` (Enum)

Defines cache-specific errors:
- `LoaderFailed(String)`: Indicates that the cache loader failed to retrieve an entry.
- `InvalidKey(String)`: The provided key is invalid.
- `Timeout`: A cache operation timed out.

## `CacheStats` (Struct)

Provides atomic counters for monitoring and debugging cache performance. It tracks hits, misses, loads, evictions, and exceptions.

### Methods
- `record_hit()`: Increments the hit count.
- `record_miss()`: Increments the miss count.
- `record_load(duration: Duration, success: bool)`: Records a load operation, its duration, and whether it was successful.
- `record_eviction()`: Increments the eviction count.
- `snapshot()`: Returns a `CacheStatsSnapshot` of the current statistics.

## `CacheStatsSnapshot` (Struct)

A read-only snapshot of the cache statistics at a point in time. It provides methods to calculate metrics like hit rate and exception rate.

## `ObjectInfoCache` (Struct)

The main struct for the high-performance, async cache for `ObjectInfo`.

### Key Methods

- `with_config(config: CacheConfig) -> Self`: Creates a new cache with a custom configuration.
- `new() -> Self`: Creates a new cache with the default configuration.
- `path_to_key<P: AsRef<Path>>(path: P) -> ObjectKey`: Efficiently converts a filesystem path to a normalized cache key.
- `get(&self, key: &ObjectKey) -> Option<ObjectInfo>`: Retrieves an entry from the cache if it's present (non-blocking).
- `get_or_load<F, Fut>(&self, key: ObjectKey, loader: F) -> Result<ObjectInfo, AppError>`: Retrieves an entry from the cache, or if it's missing, uses the provided async `loader` function to fetch it and insert it into the cache.
- `insert(&self, key: ObjectKey, info: ObjectInfo)`: Inserts an entry into the cache.
- `insert_batch(&self, entries: impl IntoIterator<Item = (ObjectKey, ObjectInfo)>)`: Inserts multiple entries into the cache efficiently.
- `remove(&self, key: &ObjectKey)`: Removes an entry from the cache.
- `invalidate_entries_if<F>(&self, predicate: F)`: Removes entries from the cache that match a given predicate.
- `clear(&self)`: Clears all entries from the cache.
- `stats(&self) -> CacheStatsSnapshot`: Returns a snapshot of the current cache statistics.
- `health_check(&self)`: Performs a health check on the cache and logs warnings for low hit rates, high memory usage, or high exception rates.
