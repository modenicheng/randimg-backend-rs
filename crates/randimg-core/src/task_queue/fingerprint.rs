//! Fingerprint-based task deduplication cache.
//!
//! Prevents duplicate task creation by hashing `(task_type, params)` and
//! storing the hash in a TTL-based cache. If a matching fingerprint is
//! found within the TTL window, the task is considered a duplicate.

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Fingerprint-based deduplication cache with TTL eviction.
///
/// Uses `DashMap` for concurrent access without explicit locks.
/// Each entry stores an `Instant` timestamp; entries older than `ttl`
/// are treated as expired and replaced on the next `check_and_insert()`.
pub struct FingerprintCache {
    cache: DashMap<u64, Instant>,
    ttl: Duration,
}

impl std::fmt::Debug for FingerprintCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FingerprintCache")
            .field("entries", &self.cache.len())
            .field("ttl_secs", &self.ttl.as_secs())
            .finish()
    }
}

impl FingerprintCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: DashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Check whether a task with the given type and params was seen recently.
    ///
    /// Returns `true` if the task is new (or the old entry expired), inserting
    /// a fresh timestamp. Returns `false` if a matching non-expired fingerprint
    /// exists — the caller should skip task creation.
    pub fn check_and_insert(&self, task_type: &str, params: &str) -> bool {
        let fingerprint = hash_task(task_type, params);

        match self.cache.entry(fingerprint) {
            Entry::Occupied(mut entry) => {
                if entry.get().elapsed() < self.ttl {
                    return false;
                }
                entry.insert(Instant::now());
                true
            }
            Entry::Vacant(entry) => {
                entry.insert(Instant::now());
                true
            }
        }
    }

    pub fn remove(&self, task_type: &str, params: &str) {
        let fingerprint = hash_task(task_type, params);
        self.cache.remove(&fingerprint);
    }

    /// Get the number of entries currently in the cache (including expired).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// Compute a stable hash for a `(task_type, params)` pair.
fn hash_task(task_type: &str, params: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    task_type.hash(&mut hasher);
    params.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn new_task_returns_true() {
        let cache = FingerprintCache::new(300);
        assert!(cache.check_and_insert("crawl", r#"{"url":"https://a.com"}"#));
    }

    #[test]
    fn duplicate_within_ttl_returns_false() {
        let cache = FingerprintCache::new(300);
        assert!(cache.check_and_insert("crawl", r#"{"url":"https://a.com"}"#));
        assert!(!cache.check_and_insert("crawl", r#"{"url":"https://a.com"}"#));
    }

    #[test]
    fn different_params_returns_true() {
        let cache = FingerprintCache::new(300);
        assert!(cache.check_and_insert("crawl", r#"{"url":"https://a.com"}"#));
        assert!(cache.check_and_insert("crawl", r#"{"url":"https://b.com"}"#));
    }

    #[test]
    fn different_type_same_params_returns_true() {
        let cache = FingerprintCache::new(300);
        assert!(cache.check_and_insert("crawl", "same"));
        assert!(cache.check_and_insert("download", "same"));
    }

    #[test]
    fn expired_entry_returns_true() {
        let cache = FingerprintCache::new(1); // 1-second TTL
        assert!(cache.check_and_insert("crawl", "x"));
        sleep(Duration::from_secs(2));
        assert!(cache.check_and_insert("crawl", "x"));
    }

    #[test]
    fn cache_grows_with_unique_entries() {
        let cache = FingerprintCache::new(300);
        assert_eq!(cache.len(), 0);
        cache.check_and_insert("a", "1");
        cache.check_and_insert("b", "2");
        assert_eq!(cache.len(), 2);
    }
}
