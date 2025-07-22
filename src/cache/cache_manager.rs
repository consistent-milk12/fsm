//! src/cache/cache_manager.rs
//! ============================================================================
//! # ObjectInfoCache — Futures-aware metadata cache for ObjectInfo
//!
//! - Wraps [`moka::future::Cache`](https://docs.rs/moka/latest/moka/future/struct.Cache.html)
//! - Asynchronous and thread-safe for fast filesystem TUI use
//! - Robust loader pattern, returning `AppError` on failures
//! - Designed for integration with AppState, async tasks, and TUI overlays

use moka::future::Cache;
use std::{sync::Arc, time::Duration};

use crate::error::AppError;
use crate::fs::object_info::ObjectInfo;

/// String key = canonical, absolute path
pub type ObjectKey = String;

/// Thread-safe async cache for [`ObjectInfo`].
#[derive(Clone)]
pub struct ObjectInfoCache {
    inner: Arc<Cache<ObjectKey, ObjectInfo>>,
    capacity: u64,
    ttl: Duration,
}

impl ObjectInfoCache {
    // --------------------------------------------------------------------- //
    // Constructors
    // --------------------------------------------------------------------- //

    /// Create a cache with `max_entries` and a global TTL.
    pub fn new(max_entries: u64, ttl: Duration) -> Self {
        let inner = Cache::builder()
            .max_capacity(max_entries)
            .time_to_live(ttl)
            .build();
        Self {
            inner: Arc::new(inner),
            capacity: max_entries,
            ttl,
        }
    }
}

impl Default for ObjectInfoCache {
    fn default() -> Self {
        // 16K entries, 30min TTL—sane defaults for most TUIs
        Self::new(16_384, Duration::from_secs(1800))
    }
}

impl ObjectInfoCache {
    // --------------------------------------------------------------------- //
    // Public API
    // --------------------------------------------------------------------- //

    /// Retrieve an [`ObjectInfo`] or compute it if absent.
    ///
    /// *`loader`* takes **no parameters** (captures key via move) and
    /// returns `Future<Output = Result<ObjectInfo, AppError>>`.
    ///
    /// Example usage:
    /// ```rust,ignore
    /// let key = path_str.clone();
    /// let info = cache.get_or_load(key.clone(), || async move {
    ///     ObjectInfo::from_path(PathBuf::from(&key))
    ///         .await
    ///         .map_err(AppError::from)
    /// }).await?;
    /// ```
    pub async fn get_or_load<F, Fut>(
        &self,
        key: ObjectKey,
        loader: F,
    ) -> Result<ObjectInfo, AppError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<ObjectInfo, AppError>> + Send + 'static,
    {
        let tmp_key = key.clone();
        let loader_future = async move {
            match loader().await {
                Ok(obj) => obj,
                Err(e) => {
                    tracing::error!("ObjectInfo loader failed for '{}': {}", tmp_key, e);
                    ObjectInfo::default()
                }
            }
        };

        let entry = self.inner.get_with(key, loader_future).await;

        if entry.path.as_os_str().is_empty() && entry.name.is_empty() {
            Err(AppError::Cache("loader failed; default cached".into()))
        } else {
            Ok(entry)
        }
    }

    /// Insert or replace a value in the cache.
    pub async fn insert(&self, key: ObjectKey, info: ObjectInfo) {
        self.inner.insert(key, info).await;
    }

    /// Remove a single entry.
    pub async fn invalidate(&self, key: ObjectKey) {
        self.inner.invalidate(&key).await;
    }

    /// Flush the entire cache.
    pub async fn clear(&self) {
        self.inner.invalidate_all();
    }

    /// Non-blocking check (no loader or load).
    pub async fn get_if_present(&self, key: &ObjectKey) -> Option<ObjectInfo> {
        self.inner.get(key).await
    }

    /// Access current capacity and TTL for stats/config
    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn insert_present_roundtrip() {
        let cache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key = "/tmp/demo".to_string();
        let obj_expected = ObjectInfo::default();
        cache.insert(key.clone(), obj_expected.clone()).await;

        let obj = cache.get_if_present(&key).await.expect("Failed to unwrap.");
        assert_eq!(obj, obj_expected);
    }

    #[tokio::test]
    async fn loader_success() {
        let cache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key = "Cargo.toml".to_string();
        let res = cache
            .get_or_load(key.clone(), || async move {
                ObjectInfo::from_path(&PathBuf::from(&key))
                    .await
                    .map_err(AppError::from)
            })
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn loader_failure_returns_error() {
        let cache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key = "/non/existent/path".to_string();
        let res = cache
            .get_or_load(key.clone(), || async move {
                ObjectInfo::from_path(&PathBuf::from(&key))
                    .await
                    .map_err(AppError::from)
            })
            .await;
        assert!(res.is_err());
    }
}
