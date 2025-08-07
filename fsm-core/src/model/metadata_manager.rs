//! `metadata_manager.rs`
//!
//! Simplified metadata layer using only `ObjectRegistry` (no cache redundancy).
//!
//! Key design points
//! -----------------
//! • **`ObjectRegistry` only** – eliminates cache redundancy and synchronization complexity
//! • **DashMap-backed registry** – lock-free reads, shard-level writes
//! • **Arc<PathBuf> / Arc<ObjectInfo>** – zero-clone sharing
//! • **Single source of truth** – no cache-registry divergence possible

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use ahash::RandomState;

use dashmap::DashMap; // Sharded, lock-free map

use crate::{
    error_core::CoreError,
    fs::object_info::{LightObjectInfo, ObjectInfo},
    model::object_registry::{ObjectId, SortableEntry},
};

/// ------------------------------------------------------------------
/// Type aliases – shorten signatures and emphasise zero-clone intent
/// ------------------------------------------------------------------
type InfoArc = Arc<ObjectInfo>; // Value: metadata wrapped in Arc

/// ------------------------------------------------------------------
/// `MetadataManager` – simplified single source of truth for metadata
/// ------------------------------------------------------------------
pub struct MetadataManager {
    /// Canonical store: `ObjectId` → Arc<ObjectInfo>
    registry: Arc<DashMap<ObjectId, InfoArc, RandomState>>,
}

impl MetadataManager {
    // ---------------------------------------------------------------
    // Construction
    // ---------------------------------------------------------------

    /// Create a new manager (no cache capacity needed)
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Arc::new(DashMap::with_hasher(RandomState::new())),
        }
    }

    /// Create from existing registry (for compatibility)
    #[must_use]
    pub fn from_registry(registry: Arc<DashMap<ObjectId, InfoArc, RandomState>>) -> Self {
        Self { registry }
    }

    // ---------------------------------------------------------------
    // Internal utilities
    // ---------------------------------------------------------------

    fn make_id(path: &Arc<PathBuf>) -> ObjectId {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        path.hash(&mut hasher);
        hasher.finish()
    }

    // ---------------------------------------------------------------
    // Public API – registry-only metadata lookup
    // ---------------------------------------------------------------

    /// Get metadata for a given path
    /// If not in registry, performs filesystem lookup and caches result
    pub fn get<P>(&self, path: P) -> Result<(InfoArc, SortableEntry), CoreError>
    where
        P: AsRef<Path>,
    {
        let path_buf = Arc::new(path.as_ref().to_path_buf());
        let id = Self::make_id(&path_buf);

        // Try registry first
        if let Some(info_arc) = self.registry.get(&id) {
            let sortable = SortableEntry::from_object_info(&info_arc, id);
            return Ok((info_arc.clone(), sortable));
        }

        // Cold path – fetch from filesystem
        let info = ObjectInfo::from_path_sync(&path_buf)?;
        let info_arc = Arc::new(info);

        // Store in registry
        self.registry.insert(id, info_arc.clone());

        let sortable = SortableEntry::from_object_info(&info_arc, id);
        Ok((info_arc, sortable))
    }

    /// Get metadata using pre-wrapped Arc<PathBuf>
    pub fn get_arc(&self, path: Arc<PathBuf>) -> Result<(InfoArc, SortableEntry), CoreError> {
        let id = Self::make_id(&path);

        // Try registry first
        if let Some(info_arc) = self.registry.get(&id) {
            let sortable = SortableEntry::from_object_info(&info_arc, id);
            return Ok((info_arc.clone(), sortable));
        }

        // Cold path – fetch from filesystem
        let info = ObjectInfo::from_path_sync(&path)?;
        let info_arc = Arc::new(info);

        // Store in registry
        self.registry.insert(id, info_arc.clone());

        let sortable = SortableEntry::from_object_info(&info_arc, id);
        Ok((info_arc, sortable))
    }

    /// Get ObjectInfo by ObjectId directly from registry (lock-free)
    pub fn get_by_id(&self, id: ObjectId) -> Option<InfoArc> {
        self.registry.get(&id).map(|entry| entry.value().clone())
    }

    /// Insert `ObjectInfo` and return `ObjectId` + `SortableEntry`
    pub fn insert(&self, info: ObjectInfo) -> (ObjectId, SortableEntry) {
        let id = Self::make_id(&info.path);
        let info_arc = Arc::new(info);

        // Insert into registry
        self.registry.insert(id, info_arc.clone());

        let sortable = SortableEntry::from_object_info(&info_arc, id);
        (id, sortable)
    }

    /// Invalidate entry by path
    pub fn invalidate<P>(&self, path: P)
    where
        P: AsRef<Path>,
    {
        let path_buf = Arc::new(path.as_ref().to_path_buf());
        let id = Self::make_id(&path_buf);
        self.registry.remove(&id);
    }

    /// Insert when caller already has Metadata (zero duplicate syscalls)
    #[must_use]
    #[expect(clippy::missing_panics_doc, reason = "Theoretically Improbable")]
    pub fn insert_light_meta(
        &self,
        light: LightObjectInfo,
        meta: &std::fs::Metadata,
    ) -> (ObjectId, SortableEntry) {
        let info = ObjectInfo::from_light_common(light, meta)
            .expect("metadata conversion should not fail - only alloc errors possible");

        self.insert(info)
    }

    /// Get registry size (for debugging)
    pub fn size(&self) -> usize {
        self.registry.len()
    }
}

impl Default for MetadataManager {
    fn default() -> Self {
        Self::new()
    }
}
