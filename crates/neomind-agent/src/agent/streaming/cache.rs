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

/// Check whether a tool call is safe to cache.
///
/// A call is cacheable when:
///   * the resolved tool name is not a known mutating tool, AND
///   * for shell calls, the underlying `neomind` CLI command is read-only
///     (list/get/history/etc.) — mutating actions (create/delete/control/...)
///     bypass the cache so their effects are always re-fetched.
pub(crate) fn is_tool_cacheable(name: &str, arguments: &Value) -> bool {
    // Resolve through the mapper so CLI domains (device, rule, ...) report as "shell".
    let resolved = super::resolve::resolve_tool_name(name);

    if resolved == "shell" {
        // Inspect the command string for mutation actions.
        let cmd = arguments
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return is_read_only_cli_command(cmd);
    }

    // The `memory` tool is NEVER cacheable. Its files change via `add`/`replace`/
    // `remove`/`create` actions within the same session, plus background summarization
    // and user edits. Caching `read`/`list` returns stale templates after writes —
    // observed in eval as read returning the 34-char template forever after the first
    // pre-write read, while `list` (different cache key) saw fresh content.
    if resolved == "memory" {
        return false;
    }

    // Other tools (skill, web_fetch, vision, ...) are read-only by default.
    true
}

/// Read-only `neomind` CLI action verbs. Anything else mutates state.
const READ_ONLY_ACTIONS: &[&str] = &[
    "list",
    "get",
    "history",
    "latest",
    "types",
    "models",
    "status",
    "logs",
    "memory",
    "executions",
    "latest-execution",
    "conversation",
    "data-sources",
    "metrics",
    "webhook-url",
    "channel-list",
    "channel-get",
    "channel-types",
    "test-code",
    "market-list",
];

/// Determine whether a `neomind <domain> <action> ...` command is read-only.
/// Returns `false` (not cacheable) for any unrecognized shape so that
/// unknown mutation commands are never cached.
fn is_read_only_cli_command(command: &str) -> bool {
    let trimmed = command.trim();
    if !trimmed.starts_with("neomind ") {
        // Non-neomind shell commands: never cache (could be `rm`, `mv`, etc.).
        return false;
    }
    // `neomind <domain> <action> ...`
    let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
    let action = parts
        .get(2)
        .map(|s| s.split_whitespace().next().unwrap_or(""));
    match action {
        Some(act) => READ_ONLY_ACTIONS.contains(&act),
        None => true, // `neomind <domain>` with no action defaults to list
    }
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
        // Read-only shell commands are cacheable
        let list_args = serde_json::json!({"command": "neomind device list"});
        assert!(is_tool_cacheable("shell", &list_args));
        assert!(is_tool_cacheable("device", &list_args));

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

        // Non-shell tools are cacheable
        assert!(is_tool_cacheable("skill", &serde_json::json!({})));

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
