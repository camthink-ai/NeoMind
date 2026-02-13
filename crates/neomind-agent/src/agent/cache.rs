//! Tool Result Caching (P2.2)
//!
//! This module implements an in-memory cache for tool results to avoid
//! redundant tool calls with identical arguments.
//!
//! ## Features
//!
//! - TTL-based expiration (60s for device queries, 300s for static data)
//! - Configurable cache size limits
//! - Automatic stale entry eviction
//! - Hash-based cache key generation

use crate::agent::ToolCall;
use neomind_tools::ToolOutput;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Default TTL for device query results (60 seconds)
const DEFAULT_DEVICE_TTL: Duration = Duration::from_secs(60);

/// Default TTL for static data (5 minutes)
const DEFAULT_STATIC_TTL: Duration = Duration::from_secs(300);

/// Maximum number of cached entries
const MAX_CACHE_SIZE: usize = 100;

/// Cached tool result with metadata.
#[derive(Clone)]
struct CachedResult {
    /// The cached output
    output: ToolOutput,
    /// When this entry was cached
    cached_at: Instant,
    /// Time-to-live for this entry
    ttl: Duration,
}

impl CachedResult {
    /// Check if this cached result is still valid.
    fn is_valid(&self) -> bool {
        self.cached_at.elapsed() < self.ttl
    }
}

/// Tool result cache.
///
/// Caches tool results to avoid redundant calls with identical arguments.
pub struct ToolResultCache {
    /// Cache entries keyed by (tool_name, serialized_args)
    cache: HashMap<CacheKey, CachedResult>,
    /// Maximum number of entries
    max_size: usize,
}

/// Cache key for tool results.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct CacheKey {
    /// Tool name
    tool_name: String,
    /// Hash of arguments
    args_hash: u64,
}

impl CacheKey {
    /// Create a new cache key from a tool call.
    fn from_call(call: &ToolCall) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        call.name.hash(&mut hasher);

        // Hash the arguments JSON
        let args_str = call.arguments.to_string();
        args_str.hash(&mut hasher);

        CacheKey {
            tool_name: call.name.clone(),
            args_hash: hasher.finish(),
        }
    }
}

impl ToolResultCache {
    /// Create a new tool result cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            max_size: MAX_CACHE_SIZE,
        }
    }

    /// Create a new tool result cache with custom size limit.
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_size,
        }
    }

    /// Get a cached result if available and still valid.
    pub fn get(&self, call: &ToolCall) -> Option<ToolOutput> {
        let key = CacheKey::from_call(call);
        self.cache.get(&key).and_then(|cached| {
            if cached.is_valid() {
                Some(cached.output.clone())
            } else {
                None
            }
        })
    }

    /// Insert a tool result into the cache.
    ///
    /// Uses default TTL based on tool category:
    /// - Device queries: 60 seconds
    /// - Static data: 300 seconds
    pub fn insert(&mut self, call: &ToolCall, output: ToolOutput) {
        self.insert_with_ttl(call, output, self.default_ttl(&call.name));
    }

    /// Insert a tool result with a specific TTL.
    pub fn insert_with_ttl(&mut self, call: &ToolCall, output: ToolOutput, ttl: Duration) {
        // Evict stale entries if cache is full
        if self.cache.len() >= self.max_size {
            self.evict_stale();
        }

        let key = CacheKey::from_call(call);
        self.cache.insert(
            key,
            CachedResult {
                output,
                cached_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Invalidate cache entries for a specific tool.
    ///
    /// Useful when device state changes or data is updated.
    pub fn invalidate(&mut self, tool_name: &str) {
        self.cache.retain(|key, _| key.tool_name != tool_name);
    }

    /// Invalidate all cache entries.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let valid_count = self.cache.values().filter(|entry| entry.is_valid()).count();
        let stale_count = self.cache.len() - valid_count;

        CacheStats {
            total_entries: self.cache.len(),
            valid_entries: valid_count,
            stale_entries: stale_count,
            max_size: self.max_size,
        }
    }

    /// Evict stale entries from the cache.
    fn evict_stale(&mut self) {
        self.cache.retain(|_, entry| entry.is_valid());

        // If still full, remove oldest entries
        if self.cache.len() >= self.max_size {
            // Simple FIFO eviction (remove half the entries)
            let keys_to_remove: Vec<_> =
                self.cache.keys().take(self.max_size / 2).cloned().collect();
            for key in keys_to_remove {
                self.cache.remove(&key);
            }
        }
    }

    /// Determine default TTL for a tool based on its name.
    fn default_ttl(&self, tool_name: &str) -> Duration {
        // Device queries have shorter TTL (state can change)
        if tool_name.contains("device")
            || tool_name.contains("query")
            || tool_name.contains("control")
        {
            return DEFAULT_DEVICE_TTL;
        }

        // Static data has longer TTL
        if tool_name.contains("list") || tool_name.contains("get") || tool_name.contains("agent") {
            return DEFAULT_STATIC_TTL;
        }

        // Default TTL
        DEFAULT_DEVICE_TTL
    }
}

impl Default for ToolResultCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total entries in cache
    pub total_entries: usize,
    /// Valid (not expired) entries
    pub valid_entries: usize,
    /// Expired entries
    pub stale_entries: usize,
    /// Maximum cache size
    pub max_size: usize,
}

impl CacheStats {
    /// Calculate cache hit ratio if you track hits.
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            return 0.0;
        }
        self.total_entries as f64 / self.max_size as f64
    }

    /// Percentage of valid entries.
    pub fn validity_rate(&self) -> f64 {
        if self.total_entries == 0 {
            return 1.0;
        }
        self.valid_entries as f64 / self.total_entries as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn make_test_call(tool_name: &str, args: Value) -> ToolCall {
        ToolCall {
            name: tool_name.to_string(),
            id: "".to_string(),
            arguments: args,
            result: None,
        }
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = ToolResultCache::new();
        let call = make_test_call("test_tool", json!({"arg": "value"}));
        let output = ToolOutput::success("result");

        cache.insert(&call, output);

        let result = cache.get(&call);
        assert!(result.is_some());
        assert!(result.unwrap().success);
    }

    #[test]
    fn test_cache_miss() {
        let cache = ToolResultCache::new();
        let call = make_test_call("test_tool", json!({"arg": "value"}));

        assert!(cache.get(&call).is_none());
    }

    #[test]
    fn test_cache_key_different_args() {
        let mut cache = ToolResultCache::new();
        let call1 = make_test_call("test_tool", json!({"arg": "value1"}));
        let call2 = make_test_call("test_tool", json!({"arg": "value2"}));
        let output1 = ToolOutput::success("result1");

        cache.insert(&call1, output1);

        assert!(cache.get(&call1).is_some());
        assert!(cache.get(&call2).is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let mut cache = ToolResultCache::new();
        let call1 = make_test_call("device_query", json!({}));
        let call2 = make_test_call("list_agents", json!({}));

        cache.insert(&call1, ToolOutput::success("device_data"));
        cache.insert(&call2, ToolOutput::success("agent_list"));

        cache.invalidate("device_query");

        assert!(cache.get(&call1).is_none());
        assert!(cache.get(&call2).is_some());
    }

    #[test]
    fn test_cache_stats() {
        let mut cache = ToolResultCache::new();
        let call = make_test_call("test_tool", json!({}));

        cache.insert(&call, ToolOutput::success("result"));
        let stats = cache.stats();

        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.valid_entries, 1);
        assert_eq!(stats.stale_entries, 0);
    }
}
