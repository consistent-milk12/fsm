//! src/cache/cache_manager.rs
//! ============================================================================
//! # ObjectInfoCache — Futures‑aware metadata cache
//!
//! This cache wraps [`moka::future::Cache`](https://docs.rs/moka/latest/moka/future/struct.Cache.html)
//! to store and asynchronously populate [`ObjectInfo`] records.  
//!
//! **Critical point from the official docs**  
//! `Cache::get_with` expects the *second* argument to be **a `Future`**,  
//! **not** a closure returning a future.  
//!
//! The correct call pattern is:
//! ```rust,ignore
//! cache.get_with(key, async move { /* compute & return V */ }).await;
//! ```
//! Therefore our `get_or_load`:
//! 1.  Accepts a **zero‑arg loader closure** producing `Future<Output = Result<ObjectInfo, AppError>>`.
//! 2.  Immediately wraps (`loader()`) inside an `async move { … }` **future** and
//!     passes *that future value* to `get_with`.
//!
//! This removes the “closure is not a future / trait `Future` not implemented”
//! compiler error.

use moka::future::Cache;
use std::{sync::Arc, time::Duration};

use crate::{error::AppError, fs::object_info::ObjectInfo};

/// String key = canonical, absolute path.
pub type ObjectKey = String;

/// Thread‑safe async cache for [`ObjectInfo`].
#[derive(Clone)]
pub struct ObjectInfoCache {
    inner: Arc<Cache<ObjectKey, ObjectInfo>>,
}

impl ObjectInfoCache {
    // --------------------------------------------------------------------- //
    // Constructors
    // --------------------------------------------------------------------- //

    /// Create a cache with `max_entries` and a global TTL.
    pub fn new(max_entries: u64, ttl: Duration) -> Self {
        let inner: Cache<String, ObjectInfo> = Cache::builder()
            .max_capacity(max_entries)
            .time_to_live(ttl)
            .build();
        Self {
            inner: Arc::new(inner),
        }
    }

    // --------------------------------------------------------------------- //
    // Public API
    // --------------------------------------------------------------------- //

    /// Retrieve an [`ObjectInfo`] or compute it if absent.
    ///
    /// *`loader`* **takes no parameters** (it captures `key` via move) and
    /// returns `Future<Output = Result<ObjectInfo, AppError>>`.
    ///
    /// ### Example
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
        // Build ONE future value and hand it to moka::Cache::get_with.
        let tmp_key: String = key.clone();
        let future_value = async move {
            match loader().await {
                Ok(obj) => obj,
                Err(e) => {
                    tracing::error!("ObjectInfo loader failed for '{}': {}", tmp_key, e);
                    ObjectInfo::default()
                }
            }
        };

        let entry: ObjectInfo = self.inner.get_with(key, future_value).await;

        if entry.path.as_os_str().is_empty() && entry.name.is_empty() {
            Err(AppError::Cache("loader failed; default cached".into()))
        } else {
            Ok(entry)
        }
    }

    /// Insert or replace a value.
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

    /// Non‑blocking check (no load).
    pub async fn get_if_present(&self, key: &ObjectKey) -> Option<ObjectInfo> {
        self.inner.get(key).await
    }
}

// ------------------------------------------------------------------------- //
// Tests
// ------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn insert_present_roundtrip() {
        let cache: ObjectInfoCache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key: String = "/tmp/demo".to_string();
        let obj_expected: ObjectInfo = ObjectInfo::default();
        let tmp_obj: ObjectInfo = obj_expected.clone();
        cache.insert(key.clone(), tmp_obj).await;

        let obj: ObjectInfo = cache.get_if_present(&key).await.expect("Failed to unwrap.");

        assert_eq!(obj, obj_expected);
    }

    #[tokio::test]
    async fn loader_success() {
        let cache: ObjectInfoCache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key: String = "Cargo.toml".to_string();
        let res: Result<ObjectInfo, AppError> = cache
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
        let cache: ObjectInfoCache = ObjectInfoCache::new(8, Duration::from_secs(30));
        let key: String = "/non/existent/path".to_string();
        let res: Result<ObjectInfo, AppError> = cache
            .get_or_load(key.clone(), || async move {
                ObjectInfo::from_path(&PathBuf::from(&key))
                    .await
                    .map_err(AppError::from)
            })
            .await;
        assert!(res.is_err());
    }
}
