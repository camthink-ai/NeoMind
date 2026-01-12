//! Response caching layer for API endpoints.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
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

/// In-memory response cache.
pub struct ResponseCache {
    /// Cache entries keyed by request key
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Default TTL for cache entries
    default_ttl: Duration,
}

impl ResponseCache {
    /// Create a new response cache.
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
        }
    }

    /// Create a cache with 60 second default TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(60))
    }

    /// Get a cached response if available and not expired.
    pub async fn get(&self, key: &str) -> Option<CachedResponse> {
        let entries = self.entries.read().await;
        entries.get(key).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(CachedResponse {
                    body: entry.body.clone(),
                    content_type: entry.content_type.clone(),
                })
            }
        })
    }

    /// Store a response in the cache.
    pub async fn put(&self, key: String, response: CachedResponse, ttl: Option<Duration>) {
        let expires_at = chrono::Utc::now().timestamp()
            + ttl.unwrap_or(self.default_ttl).as_secs() as i64;

        let entry = CacheEntry {
            body: response.body,
            content_type: response.content_type,
            expires_at,
        };

        let mut entries = self.entries.write().await;
        entries.insert(key, entry);
    }

    /// Invalidate a cache entry.
    pub async fn invalidate(&self, key: &str) {
        let mut entries = self.entries.write().await;
        entries.remove(key);
    }

    /// Clear all cache entries.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }

    /// Clean up expired entries.
    pub async fn cleanup(&self) {
        let mut entries = self.entries.write().await;
        entries.retain(|_, entry| !entry.is_expired());
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let entries = self.entries.read().await;
        let now = chrono::Utc::now().timestamp();
        let active = entries.values().filter(|e| e.expires_at > now).count();
        let expired = entries.len() - active;

        CacheStats {
            total_entries: entries.len(),
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

fn default_devices_ttl() -> u64 { 30 }
fn default_stats_ttl() -> u64 { 10 }
fn default_settings_ttl() -> u64 { 60 }
fn default_search_ttl() -> u64 { 30 }

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

        cache.put("test_key".to_string(), response.clone(), Some(Duration::from_secs(10))).await;

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
        cache.put("test_key".to_string(), response, Some(Duration::from_secs(1))).await;

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

        cache.put("test_key".to_string(), response, Some(Duration::from_secs(10))).await;
        cache.invalidate("test_key").await;

        let retrieved = cache.get("test_key").await;
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_cache_key_generation() {
        assert_eq!(cache_key("/api/devices", None), "/api/devices");
        assert_eq!(cache_key("/api/devices", Some("limit=10")), "/api/devices?limit=10");
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

        assert_eq!(config.ttl_for_path("/api/devices"), Some(Duration::from_secs(30)));
        assert_eq!(config.ttl_for_path("/api/stats/system"), Some(Duration::from_secs(10)));
        assert_eq!(config.ttl_for_path("/api/settings/llm"), Some(Duration::from_secs(60)));
        assert_eq!(config.ttl_for_path("/api/search?q=test"), Some(Duration::from_secs(30)));
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
