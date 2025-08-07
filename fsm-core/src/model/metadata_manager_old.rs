//! `metadata_manager.rs`
//!
//! Extreme-performance metadata layer for the FSM project.
//!
//! Key design points
//! -----------------
//! • **Sync only** – avoids async scheduler overhead.
//! • **DashMap-backed registry** – lock-free reads, shard-level writes.
//! • **`moka::sync::Cache`** – O(1) hits, single-flight misses.
//! • **Arc<PathBuf> / Arc<ObjectInfo>** – zero-clone sharing.
//! • **No cache-registry divergence** – every path flows through here.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use ahash::{RandomState};                
use compact_str::CompactString;

use dashmap::DashMap;                    // Sharded, lock-free map
use moka::sync::Cache;                   // High-perf in-memory cache

use crate::{
    error_core::CoreError, 
    fs::object_info::{LightObjectInfo, ObjectInfo}, 
    model::object_registry::{ObjectId, SortableEntry}
};

/// ------------------------------------------------------------------
/// Type aliases – shorten signatures and emphasise zero-clone intent
/// ------------------------------------------------------------------
type SharedPath = Arc<PathBuf>;          // Key: path wrapped in Arc
type InfoArc   = Arc<ObjectInfo>;        // Value: metadata wrapped in Arc

/// ------------------------------------------------------------------
/// `MetadataManager` – the single entry point for all metadata access
/// ------------------------------------------------------------------
pub struct MetadataManager {
    /// Canonical store: `ObjectId` → Arc<ObjectInfo>
    registry: Arc<DashMap<ObjectId, InfoArc, RandomState>>,

    /// Fast lookup cache: Arc<PathBuf> → Arc<ObjectInfo>
    cache: Cache<SharedPath, InfoArc>,
}

impl MetadataManager {
    // ---------------------------------------------------------------
    // Construction
    // ---------------------------------------------------------------

    /// Create a new manager with a fixed cache capacity (in entries).
    #[must_use] 
    pub fn new(cache_capacity: u64) -> Self {
        // Initialise shard-based registry with ahash
        let registry: Arc<DashMap<u64, Arc<ObjectInfo>, RandomState>> = Arc::new(
            DashMap::with_hasher(
                RandomState::default(),
            )
        );

        // Build the high-capacity cache (LRU by default)
        let cache: Cache<Arc<PathBuf>, Arc<ObjectInfo>> = Cache::builder()
            .max_capacity(cache_capacity)
            .build();

        Self { registry, cache }
    }

    // ---------------------------------------------------------------
    // Utility – deterministic ObjectId from PathBuf
    // ---------------------------------------------------------------

    /// Hash the path into a u64 using the same hasher everywhere.
    #[inline]
    fn make_id(path: &PathBuf) -> ObjectId {
        RandomState::default().hash_one(path)
    }

    // ---------------------------------------------------------------
    // Public API – hot-path metadata lookup
    // ---------------------------------------------------------------

    /// Synchronously obtain metadata for a given path.
    ///
    /// Fast cases (cache hit / registry hit) never touch the disk.
    /// A cold miss performs one blocking `stat` and populates both
    /// data structures in a single-flight manner.
    pub fn get<P>(&self, path: P) -> Result<(InfoArc, SortableEntry), CoreError>
    where
        P: AsRef<Path>,
    {
        // ----------------------------------------------------------
        // 1. Normalize the path into an Arc<PathBuf> key
        // ----------------------------------------------------------
        let key: SharedPath = Arc::new(path.as_ref().to_path_buf());
        let id: ObjectId = Self::make_id(&key);

        // ----------------------------------------------------------
        // 2. Atomically look up or compute via cache::get_with
        // ----------------------------------------------------------
        let info_arc: Arc<ObjectInfo> = self.cache.try_get_with(key.clone(), {
            // The closure runs only on a cache miss.
            let registry: Arc<DashMap<u64, Arc<ObjectInfo>, RandomState>> = Arc::clone(&self.registry);
            let key_clone: Arc<PathBuf> = key;
            
            move || -> Result<Arc<ObjectInfo>, CoreError> 
            {
                // 2a. Try registry to avoid disk if another thread
                //     already inserted the same path.
                if let Some(existing) = registry.get(&id) {
                    return Ok(existing.clone());
                }

                // 2b. Cold path – fetch from filesystem (blocking).
                let info: ObjectInfo = ObjectInfo::from_path_sync(&key_clone)?;

                // 2c. Wrap in Arc and publish to registry.
                let arc: Arc<ObjectInfo> = Arc::new(info);
                registry.insert(id, arc.clone());
                
                Ok(arc)
            }
        })?;

        // ----------------------------------------------------------
        // 3. Generate lightweight SortableEntry for callers
        // ----------------------------------------------------------
        let sortable: SortableEntry = SortableEntry::from_object_info(&info_arc, id);

        Ok((info_arc, sortable))
    }

    pub async fn get_async<P>(
        self: Arc<Self>,
        path: P
    ) -> Result<(InfoArc, SortableEntry), CoreError>
    where
        P: AsRef<Path> + Send + 'static
    {
        let path_buf: PathBuf = path.as_ref().to_path_buf();

        tokio::task::spawn_blocking(
            move || -> Result<(Arc<ObjectInfo>, SortableEntry), CoreError> 
            {
                self.get(path_buf)
            }
        )
        .await
        .map_err(|_| 
            CoreError::Other(
                CompactString::const_new("spawn_blocking failed.")
            )
        )?
    }

    // ---------------------------------------------------------------
    // Public API – explicit insert (rarely used; mostly for tests)
    // ---------------------------------------------------------------
    ///
    /// Insert pre-computed metadata (e.g. during unit tests).
    #[must_use] 
    pub fn insert(&self, info: ObjectInfo) -> (ObjectId, SortableEntry)
    {
        // Promote into shared arcs
        let path: Arc<PathBuf> = info.path.clone();
        let id: u64 = Self::make_id(&path);
        let arc: Arc<ObjectInfo> = Arc::new(info);

        // Registry write – shard-local lock
        self.registry.insert(id, arc.clone());
        
        // Cache write – O(1) atomic
        self.cache.insert(path, arc.clone());

        // SortableEntry for UI tier
        let sortable: SortableEntry = SortableEntry::from_object_info(&arc, id);

        (id, sortable)
    }

    // ---------------------------------------------------------------
    // Public API – cache invalidation
    // ---------------------------------------------------------------

    /// Remove a path from the cache (registry keeps canonical copy).
    pub fn invalidate<P>(&self, path: P)
    where
        P: AsRef<Path>,
    {
        let key: SharedPath =
            Arc::new(path.as_ref().to_path_buf());
        self.cache.invalidate(&key);
    }

    /// Optimized method accepting `Arc<PathBuf>` directly (~15ns/hit savings)
    pub fn get_arc(&self, path: Arc<PathBuf>) -> Result<(InfoArc, SortableEntry), CoreError>
    {
        let id: u64 = Self::make_id(&path);

        let info_arc: Arc<ObjectInfo> = self.cache.try_get_with(path.clone(), {
            let registry: Arc<DashMap<u64, Arc<ObjectInfo>, RandomState>> = Arc::clone(&self.registry);

            move || -> Result<Arc<ObjectInfo>, CoreError>
            {
                if let Some(existing) = registry.get(&id) {
                    return Ok(existing.clone());
                }

                let info: ObjectInfo = ObjectInfo::from_path_sync(&path)?;
                let arc: Arc<ObjectInfo> = Arc::new(info);

                registry.insert(id, arc.clone());

                Ok(arc)
            }
        })?;

        let sortable: SortableEntry = SortableEntry::from_object_info(&info_arc, id);

        Ok((info_arc, sortable))
    }

    /// Get ObjectInfo by ObjectId directly from registry (lock-free)
    pub fn get_by_id(&self, id: ObjectId) -> Option<InfoArc> {
        self.registry.get(&id).map(|entry| entry.value().clone())
    }

    /// Insert ObjectInfo and return ObjectId + SortableEntry
    pub fn insert(&self, info: ObjectInfo) -> (ObjectId, SortableEntry) {
        let id = Self::make_id(&info.path);
        let info_arc = Arc::new(info);
        
        // Insert into both registry and cache
        self.registry.insert(id, info_arc.clone());
        self.cache.insert(info_arc.path.clone(), info_arc.clone());
        
        let sortable = SortableEntry::from_object_info(&info_arc, id);
        (id, sortable)
    }

    pub async fn get_arc_async(
        self: Arc<Self>,
        path: Arc<PathBuf>
    ) -> Result<(InfoArc, SortableEntry), CoreError>
    {
        tokio::task::spawn_blocking(move || self.get_arc(path))
            .await
            .map_err(
                |_|
                CoreError::Other(CompactString::const_new("spawn_blocking failed"))
            )?
    }

    /// Insert when caller already has Metadata (zero duplicate syscalls)
    /// 
    /// This method is critical for directory scanning performance - it allows
    /// reusing the `Metadata` already obtained during `LightObjectInfo` creation,
    /// avoiding duplicate `stat()` syscalls that cause 25-30% performance loss.
    #[must_use]
    #[expect(clippy::missing_panics_doc, reason = "Theoretically Improbabble")]
    pub fn insert_light_meta(
        &self,
        light: LightObjectInfo,
        meta: &std::fs::Metadata,
    ) -> (ObjectId, SortableEntry) {        
        // Use ObjectInfo::from_light_common() to avoid second stat()
        let info = ObjectInfo::from_light_common(light, meta)
            .expect("metadata conversion should not fail - only alloc errors possible");
        
        self.insert(info)
    }
}
