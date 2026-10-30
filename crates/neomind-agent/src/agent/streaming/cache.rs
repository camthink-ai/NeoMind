use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Simple in-memory cache for tool results with TTL and size limit
#[derive(Debug)]
pub struct ToolResultCache {
    entries: HashMap<String, (crate::toolkit::ToolOutput, Instant)>,
    ttl: Duration,
    max_entries: usize,
}

impl ToolResultCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            max_entries: 1000, // Prevent unbounded memory growth
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<crate::toolkit::ToolOutput> {
        self.entries.get(key).and_then(|(result, timestamp)| {
            if timestamp.elapsed() < self.ttl {
                Some(result.clone())
            } else {
                None
            }
        })
    }

    pub(crate) fn insert(&mut self, key: String, value: crate::toolkit::ToolOutput) {
        // Enforce size limit - evict the entry with the oldest timestamp
        if self.entries.len() >= self.max_entries {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, ts))| *ts)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(key, (value, Instant::now()));
    }

    pub(crate) fn cleanup_expired(&mut self) {
        self.entries
            .retain(|_, (_, timestamp)| timestamp.elapsed() < self.ttl);

        // Also enforce size limit after cleanup — evict oldest by timestamp
        while self.entries.len() > self.max_entries {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, ts))| *ts)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    /// Generate cache key from tool name and arguments.
    /// Sorts object keys to ensure consistent keys regardless of parameter order.
    pub fn make_key(name: &str, arguments: &Value) -> String {
        // For objects, sort keys to ensure consistent cache keys
        if let Some(obj) = arguments.as_object() {
            let mut sorted_pairs: Vec<_> = obj.iter().collect();
            sorted_pairs.sort_by(|a, b| a.0.cmp(b.0));

            let sorted_obj: serde_json::Map<String, Value> = sorted_pairs
                .into_iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            format!(
                "{}:{}",
                name,
                serde_json::to_string(&sorted_obj).unwrap_or_default()
            )
        } else {
            // For non-objects (arrays, strings, numbers, etc.), use as-is
            format!("{}:{}", name, arguments)
        }
    }
}

/// Cache only immutable skill READS (search/load). The skill tool also has
/// create/update/delete actions — caching those made `create → delete →
/// create` return the FIRST call's stale success without executing (the
/// third create silently didn't happen). Read-only does not imply immutable
/// for anything else either: device status, file reads and web fetches must
/// observe fresh state, including after a write in the same turn.
/// Unknown/extension tools fail closed.
pub(crate) fn is_tool_cacheable(name: &str, arguments: &Value) -> bool {
    if super::resolve::resolve_tool_name(name) != "skill" {
        return false;
    }
    matches!(
        arguments.get("action").and_then(|a| a.as_str()),
        Some("search") | Some("load")
    )
}

/// Minimum size (bytes) for a result to be considered large enough to strip base64 from.
pub(crate) const BASE64_STRIP_THRESHOLD: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_consistency() {
        // Same arguments in different order should produce same key
        let key1 = ToolResultCache::make_key("shell", &serde_json::json!({"b": 2, "a": 1}));
        let key2 = ToolResultCache::make_key("shell", &serde_json::json!({"a": 1, "b": 2}));
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_different_tools() {
        let key1 = ToolResultCache::make_key("shell", &serde_json::json!({"cmd": "ls"}));
        let key2 = ToolResultCache::make_key("device", &serde_json::json!({"cmd": "ls"}));
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_is_tool_cacheable() {
        // Live reads bypass the cache even though they have no side effects.
        let list_args = serde_json::json!({"command": "neomind device list"});
        assert!(!is_tool_cacheable("shell", &list_args));
        assert!(!is_tool_cacheable("device", &list_args));

        // Mutating shell commands bypass cache
        let delete_args = serde_json::json!({"command": "neomind device delete abc123"});
        assert!(!is_tool_cacheable("shell", &delete_args));
        assert!(!is_tool_cacheable("device", &delete_args));

        let control_args = serde_json::json!({"command": "neomind device control abc123 on"});
        assert!(!is_tool_cacheable("device", &control_args));

        let send_args = serde_json::json!({"command": "neomind message send --title x"});
        assert!(!is_tool_cacheable("message", &send_args));

        // Non-neomind shell commands bypass cache
        let rm_args = serde_json::json!({"command": "rm -rf /tmp/foo"});
        assert!(!is_tool_cacheable("shell", &rm_args));

        // Only static skill READS are cacheable.
        assert!(is_tool_cacheable(
            "skill",
            &serde_json::json!({"action": "search", "query": "device"})
        ));
        assert!(is_tool_cacheable(
            "skill",
            &serde_json::json!({"action": "load", "id": "device-management"})
        ));
        // Skill WRITES must never come from cache: create → delete → create
        // returned the first call's stale success (review 2026-10-30).
        assert!(!is_tool_cacheable(
            "skill",
            &serde_json::json!({"action": "create", "name": "x"})
        ));
        assert!(!is_tool_cacheable(
            "skill",
            &serde_json::json!({"action": "update", "id": "x"})
        ));
        assert!(!is_tool_cacheable(
            "skill",
            &serde_json::json!({"action": "delete", "id": "x"})
        ));

        // Memory tool is NEVER cacheable — files mutate via add/replace/remove
        // and background summarization. Caching read returned stale templates
        // after writes (eval regression: tools-memory-read zh).
        assert!(!is_tool_cacheable(
            "memory",
            &serde_json::json!({"action": "read", "target": "user"})
        ));
        assert!(!is_tool_cacheable(
            "memory",
            &serde_json::json!({"action": "list"})
        ));
        assert!(!is_tool_cacheable(
            "memory",
            &serde_json::json!({"action": "add", "target": "user", "content": "x"})
        ));
    }

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = ToolResultCache::new(Duration::from_secs(60));
        let output = crate::toolkit::ToolOutput::success("test result");
        let key = "test_key".to_string();
        cache.insert(key.clone(), output);
        let result = cache.get(&key);
        assert!(result.is_some());
        assert_eq!(result.unwrap().data, serde_json::json!("test result"));
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let mut cache = ToolResultCache::new(Duration::from_millis(10));
        let output = crate::toolkit::ToolOutput::success("expires");
        cache.insert("key".to_string(), output);
        assert!(cache.get("key").is_some()); // not expired yet
        std::thread::sleep(Duration::from_millis(50));
        assert!(cache.get("key").is_none()); // expired
    }
}
