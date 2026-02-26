//! Response caching layer for API endpoints.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Cache entry with expiration.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Cached response body as JSON string
    body: String,
    /// Content type
    content_type: String,
    /// Expiration timestamp (Unix timestamp)
    expires_at: i64,
}

impl CacheEntry {
    /// Check if the entry has expired.
    fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() > self.expires_at
    }
}

/// In-memory response cache with LRU eviction.
pub struct ResponseCache {
    /// Cache entries keyed by request key - using DashMap for concurrent access
    entries: Arc<DashMap<String, CacheEntry>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Maximum number of entries before LRU eviction
    max_entries: usize,
}

impl ResponseCache {
    /// Create a new response cache with default max entries.
    pub fn new(default_ttl: Duration) -> Self {
        Self::with_max_entries(default_ttl, 10000)
    }

    /// Create a cache with max entries limit for LRU eviction.
    pub fn with_max_entries(default_ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: Arc::new(DashMap::with_capacity(max_entries.min(1000))),
            default_ttl,
            max_entries,
        }
    }

    /// Create a cache with 60 second default TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(60))
    }

    /// Get a cached response if available and not expired.
    pub fn get(&self, key: &str) -> Option<CachedResponse> {
        self.entries.get(key).and_then(|entry| {
            if entry.is_expired() {
                drop(entry); // Release read lock before removing
                self.entries.remove(key);
                None
            } else {
                Some(CachedResponse {
                    body: entry.body.clone(),
                    content_type: entry.content_type.clone(),
                })
            }
        })
    }

    /// Store a response in the cache with LRU eviction.
    pub fn put(&self, key: String, response: CachedResponse, ttl: Option<Duration>) {
        // Evict if at capacity
        if self.entries.len() >= self.max_entries {
            self.evict_expired_or_oldest();
        }

        let expires_at =
            chrono::Utc::now().timestamp() + ttl.unwrap_or(self.default_ttl).as_secs() as i64;

        let entry = CacheEntry {
            body: response.body,
            content_type: response.content_type,
            expires_at,
        };

        self.entries.insert(key, entry);
    }

    /// Evict expired entries or the oldest entry if none expired.
    fn evict_expired_or_oldest(&self) {
        let mut oldest_time = i64::MAX;
        let mut oldest_key = None;

        // Find expired or oldest entry
        for ref_item in self.entries.iter() {
            let (key, entry) = ref_item.pair();
            if entry.is_expired() {
                self.entries.remove(key);
                return;
            }
            if entry.expires_at < oldest_time {
                oldest_time = entry.expires_at;
                oldest_key = Some(key.clone());
            }
        }

        // Remove oldest if no expired entries
        if let Some(key) = oldest_key {
            self.entries.remove(&key);
        }
    }

    /// Invalidate a cache entry.
    pub fn invalidate(&self, key: &str) {
        self.entries.remove(key);
    }

    /// Clear all cache entries.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Clean up expired entries.
    pub fn cleanup(&self) {
        let expired_keys: Vec<String> = self
            .entries
            .iter()
            .filter(|item| item.value().is_expired())
            .map(|item| item.key().clone())
            .collect();

        for key in expired_keys {
            self.entries.remove(&key);
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let now = chrono::Utc::now().timestamp();
        let mut active = 0;
        let mut expired = 0;

        for ref_item in self.entries.iter() {
            if ref_item.value().expires_at > now {
                active += 1;
            } else {
                expired += 1;
            }
        }

        CacheStats {
            total_entries: self.entries.len(),
            active_entries: active,
            expired_entries: expired,
        }
    }
}

impl Clone for ResponseCache {
    fn clone(&self) -> Self {
        Self {
            entries: Arc::clone(&self.entries),
            default_ttl: self.default_ttl,
            max_entries: self.max_entries,
        }
    }
}

/// Cached response data.
#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub body: String,
    pub content_type: String,
}

/// Cache statistics.
#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub active_entries: usize,
    pub expired_entries: usize,
}

/// Cache configuration for different endpoint types.
#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    /// Enable/disable caching
    #[serde(default)]
    pub enabled: bool,

    /// TTL for device list endpoints (seconds)
    #[serde(default = "default_devices_ttl")]
    pub devices_ttl: u64,

    /// TTL for stats endpoints (seconds)
    #[serde(default = "default_stats_ttl")]
    pub stats_ttl: u64,

    /// TTL for settings endpoints (seconds)
    #[serde(default = "default_settings_ttl")]
    pub settings_ttl: u64,

    /// TTL for search results (seconds)
    #[serde(default = "default_search_ttl")]
    pub search_ttl: u64,
}

fn default_devices_ttl() -> u64 {
    30
}
fn default_stats_ttl() -> u64 {
    10
}
fn default_settings_ttl() -> u64 {
    60
}
fn default_search_ttl() -> u64 {
    30
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default
            devices_ttl: default_devices_ttl(),
            stats_ttl: default_stats_ttl(),
            settings_ttl: default_settings_ttl(),
            search_ttl: default_search_ttl(),
        }
    }
}

impl CacheConfig {
    /// Get TTL for a given endpoint path.
    pub fn ttl_for_path(&self, path: &str) -> Option<Duration> {
        if !self.enabled {
            return None;
        }

        let ttl = if path.contains("/devices") && !path.contains("/devices/") {
            Some(self.devices_ttl)
        } else if path.contains("/stats/") {
            Some(self.stats_ttl)
        } else if path.contains("/settings/") {
            Some(self.settings_ttl)
        } else if path.contains("/search") {
            Some(self.search_ttl)
        } else {
            None
        };

        ttl.map(Duration::from_secs)
    }
}

/// Generate a cache key from a request path and query.
pub fn cache_key(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{}?{}", path, q),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_put_get() {
        let cache = ResponseCache::with_default_ttl();

        let response = CachedResponse {
            body: "{\"test\": true}".to_string(),
            content_type: "application/json".to_string(),
        };

        cache
            .put(
                "test_key".to_string(),
                response.clone(),
                Some(Duration::from_secs(10)),
            )
            .await;

        let retrieved = cache.get("test_key").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().body, "{\"test\": true}");
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = ResponseCache::with_default_ttl();

        let response = CachedResponse {
            body: "{\"test\": true}".to_string(),
            content_type: "application/json".to_string(),
        };

        // Put with very short TTL (1 second)
        cache
            .put(
                "test_key".to_string(),
                response,
                Some(Duration::from_secs(1)),
            )
            .await;

        // Verify it exists initially
        let retrieved = cache.get("test_key").await;
        assert!(retrieved.is_some());

        // Wait for expiration (plus a buffer)
        tokio::time::sleep(Duration::from_secs(2)).await;

        let retrieved = cache.get("test_key").await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = ResponseCache::with_default_ttl();

        let response = CachedResponse {
            body: "{\"test\": true}".to_string(),
            content_type: "application/json".to_string(),
        };

        cache
            .put(
                "test_key".to_string(),
                response,
                Some(Duration::from_secs(10)),
            )
            .await;
        cache.invalidate("test_key").await;

        let retrieved = cache.get("test_key").await;
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_cache_key_generation() {
        assert_eq!(cache_key("/api/devices", None), "/api/devices");
        assert_eq!(
            cache_key("/api/devices", Some("limit=10")),
            "/api/devices?limit=10"
        );
    }

    #[test]
    fn test_cache_config_ttl_for_path() {
        let config = CacheConfig {
            enabled: true,
            devices_ttl: 30,
            stats_ttl: 10,
            settings_ttl: 60,
            search_ttl: 30,
        };

        assert_eq!(
            config.ttl_for_path("/api/devices"),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            config.ttl_for_path("/api/stats/system"),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            config.ttl_for_path("/api/settings/llm"),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            config.ttl_for_path("/api/search?q=test"),
            Some(Duration::from_secs(30))
        );
        assert_eq!(config.ttl_for_path("/api/sessions/123"), None); // Not cached
    }

    #[test]
    fn test_cache_config_disabled() {
        let config = CacheConfig {
            enabled: false,
            ..Default::default()
        };

        assert_eq!(config.ttl_for_path("/api/devices"), None);
    }
}
