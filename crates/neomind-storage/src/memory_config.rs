//! Memory system configuration
//!
//! Simplified configuration using character limits for memory files.

use serde::{Deserialize, Serialize};

/// Simplified memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Whether the memory system is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Storage path for memory files
    #[serde(default = "default_storage_path")]
    pub storage_path: String,
    /// Max chars for USER.md
    #[serde(default = "default_user_limit")]
    pub user_char_limit: usize,
    /// Max chars for KNOWLEDGE.md
    #[serde(default = "default_knowledge_limit")]
    pub knowledge_char_limit: usize,
    /// Max chars for PROCEDURES.md (SOPs / playbooks / how-tos)
    #[serde(default = "default_procedures_limit")]
    pub procedures_char_limit: usize,
    /// Max chars per agent/custom memory file
    #[serde(default = "default_agent_limit")]
    pub agent_char_limit: usize,
    /// TTL in days for session temp files
    #[serde(default = "default_ttl")]
    pub temp_file_ttl_days: u64,
    /// Interval in seconds for system context resource inventory refresh
    #[serde(default = "default_context_interval")]
    pub system_context_interval_secs: u64,
    /// Interval in seconds for LLM-based chat/agent summarization
    #[serde(default = "default_summary_interval")]
    pub summary_interval_secs: u64,
    /// LLM backend ID for summarization. None = use active backend.
    #[serde(default)]
    pub summary_backend_id: Option<String>,
}

fn default_enabled() -> bool {
    true
}
fn default_storage_path() -> String {
    neomind_core::paths::data_dir()
        .join("memory")
        .to_string_lossy()
        .to_string()
}
fn default_user_limit() -> usize {
    2000
}
fn default_knowledge_limit() -> usize {
    3000
}
fn default_procedures_limit() -> usize {
    3000
}
fn default_agent_limit() -> usize {
    20000
}
fn default_ttl() -> u64 {
    7
}
fn default_context_interval() -> u64 {
    600
}
fn default_summary_interval() -> u64 {
    7200
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            storage_path: default_storage_path(),
            user_char_limit: default_user_limit(),
            knowledge_char_limit: default_knowledge_limit(),
            procedures_char_limit: default_procedures_limit(),
            agent_char_limit: default_agent_limit(),
            temp_file_ttl_days: default_ttl(),
            system_context_interval_secs: default_context_interval(),
            summary_interval_secs: default_summary_interval(),
            summary_backend_id: None,
        }
    }
}

impl MemoryConfig {
    /// Configuration file path
    pub fn config_file() -> std::path::PathBuf {
        neomind_core::paths::store_path("memory_config.json")
    }

    /// Load configuration from file
    pub fn load() -> Self {
        let path = Self::config_file();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save configuration to file
    pub fn save(&self) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        crate::atomic_write::write(&Self::config_file(), content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MemoryConfig::default();
        assert!(config.enabled);
        assert_eq!(config.user_char_limit, 2000);
        assert_eq!(config.knowledge_char_limit, 3000);
        assert_eq!(config.procedures_char_limit, 3000);
        assert_eq!(config.agent_char_limit, 20000);
        assert_eq!(config.temp_file_ttl_days, 7);
        assert_eq!(config.system_context_interval_secs, 600);
        assert_eq!(config.summary_interval_secs, 7200);
        assert!(config.summary_backend_id.is_none());
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = MemoryConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: MemoryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.user_char_limit, parsed.user_char_limit);
    }

    #[test]
    fn test_old_config_fields_ignored() {
        // Old config JSON with removed fields (max_agents, schedule_interval_secs) should still parse
        let old_json = r#"{
            "enabled": true,
            "storage_path": "data/memory",
            "user_char_limit": 2000,
            "knowledge_char_limit": 3000,
            "max_agents": 5,
            "schedule_interval_secs": 3600,
            "extraction": {"similarity_threshold": 0.85}
        }"#;
        let config: MemoryConfig = serde_json::from_str(old_json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.system_context_interval_secs, 600); // uses default
    }
}
