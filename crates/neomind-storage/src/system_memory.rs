//! System Memory - Markdown-based memory storage
//!
//! This module provides a simple Markdown file-based memory system for AI agents.
//! Based on 2026 research (Voxos.ai, Letta), simple file storage (74% accuracy)
//! outperforms complex graph/RAG systems (68.5%).
//!
//! ## Architecture (2-file persistent + session temp files)
//!
//! ```text
//! data/memory/
//! ├── USER.md                          # Persistent: user profile (max 2000 chars)
//! ├── KNOWLEDGE.md                     # Persistent: system knowledge (max 3000 chars)
//! │   ## System Resources             #   Internal structure:
//! │   ## Domain Knowledge             #   - System Resources (auto-generated)
//! │   ## Agent Experiences            #   - Domain Knowledge (AI-managed)
//! │                                   #   - Agent Experiences (auto-generated)
//! ├── agents/{agent_id}.md            # Agent summaries (created by bridge)
//! ├── custom/{name}.md                # Custom domain-specific files (max agent_char_limit chars/file)
//! └── sessions/{session_id}/          # Session temp files (no char limit, 7-day TTL)
//!     └── notes.md
//! ```
//!
//! ## Migration from Legacy
//!
//! The old 4-category system (UserProfile, DomainKnowledge, TaskPatterns, SystemEvolution)
//! has been simplified to 2 persistent files + session temp files. Legacy APIs are
//! deprecated but still available for backward compatibility.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::atomic_write;
use crate::error::{Error, Result};
use crate::memory_config::MemoryConfig;

/// Maximum recommended memory entries per file (legacy, for backward compat)
pub const MAX_MEMORY_ENTRIES: usize = 30;

/// Default importance threshold for pruning (legacy, for backward compat)
pub const DEFAULT_MIN_IMPORTANCE: u8 = 30;

// ============================================================================
// Legacy Types (deprecated, kept for backward compatibility)
// ============================================================================

/// Memory category - four types for organized storage (DEPRECATED)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    /// 用户画像 - User preferences, habits, focus areas
    #[default]
    UserProfile,
    /// 领域知识 - Domain knowledge, devices, environment rules
    DomainKnowledge,
    /// 任务模式 - Task patterns, agent execution experience
    TaskPatterns,
    /// 系统进化 - System evolution, self-learning records
    SystemEvolution,
}

impl MemoryCategory {
    /// Get the markdown filename for this category (legacy mapping)
    pub fn filename(&self) -> &'static str {
        match self {
            Self::UserProfile => "user_profile.md",
            Self::DomainKnowledge => "domain_knowledge.md",
            Self::TaskPatterns => "task_patterns.md",
            Self::SystemEvolution => "system_evolution.md",
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::UserProfile => "User Profile",
            Self::DomainKnowledge => "Domain Knowledge",
            Self::TaskPatterns => "Task Patterns",
            Self::SystemEvolution => "System Evolution",
        }
    }

    /// Get max entries for this category (legacy)
    pub fn max_entries(&self) -> usize {
        match self {
            Self::UserProfile => 50,
            Self::DomainKnowledge => 100,
            Self::TaskPatterns => 80,
            Self::SystemEvolution => 30,
        }
    }

    /// Get the markdown section name (for backward compatibility)
    pub fn section_name(&self) -> &'static str {
        self.display_name()
    }

    /// Parse from string
    pub fn parse_category(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            // English names
            "user_profile" | "user profile" | "userprofile" => Some(Self::UserProfile),
            "domain_knowledge" | "domain knowledge" | "domainknowledge" => {
                Some(Self::DomainKnowledge)
            }
            "task_patterns" | "task patterns" | "taskpatterns" => Some(Self::TaskPatterns),
            "system_evolution" | "system evolution" | "systemevolution" => {
                Some(Self::SystemEvolution)
            }
            // Chinese names (backward compatibility)
            "用户画像" => Some(Self::UserProfile),
            "领域知识" => Some(Self::DomainKnowledge),
            "任务模式" => Some(Self::TaskPatterns),
            "系统进化" => Some(Self::SystemEvolution),
            // Legacy aliases for backward compatibility
            "pattern" | "patterns" => Some(Self::TaskPatterns),
            "entity" | "entities" => Some(Self::DomainKnowledge),
            "preference" | "preferences" => Some(Self::UserProfile),
            "fact" | "facts" => Some(Self::DomainKnowledge),
            _ => None,
        }
    }

    /// All categories
    pub fn all() -> &'static [Self] {
        &[
            Self::UserProfile,
            Self::DomainKnowledge,
            Self::TaskPatterns,
            Self::SystemEvolution,
        ]
    }
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self)
                .unwrap_or_default()
                .trim_matches('"')
        )
    }
}

/// Category statistics (legacy)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CategoryStats {
    /// Number of entries in the category
    pub entry_count: usize,
    /// File size in bytes
    pub file_size: u64,
    /// Last modified timestamp (Unix seconds)
    pub modified_at: i64,
}

/// Memory source - where the memory came from (DEPRECATED)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MemorySource {
    /// Memory from an AI Agent
    Agent { id: String, name: String },
    /// Memory from a chat session
    Chat { session_id: String },
    /// System-level global memory
    System,
}

impl MemorySource {
    /// Get the file path for this source (legacy mapping)
    pub fn file_path(&self, base_path: &Path) -> PathBuf {
        match self {
            MemorySource::Agent { id, .. } => base_path.join("agents").join(format!("{}.md", id)),
            MemorySource::Chat { session_id } => {
                base_path.join("chat").join(format!("{}.md", session_id))
            }
            MemorySource::System => base_path.join("system.md"),
        }
    }

    /// Get a display name for this source
    pub fn display_name(&self) -> String {
        match self {
            MemorySource::Agent { name, .. } => name.clone(),
            MemorySource::Chat { session_id } => {
                format!("Chat {}", &session_id[..8.min(session_id.len())])
            }
            MemorySource::System => "System".to_string(),
        }
    }
}

/// A single memory entry (DEPRECATED - use direct markdown writes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// The memory content
    pub content: String,
    /// Category of this memory
    pub category: MemoryCategory,
    /// Importance score (0-100)
    pub importance: u8,
    /// When this memory was created (Unix timestamp)
    pub created_at: i64,
    /// Source of this memory
    pub source: MemorySource,
}

impl MemoryEntry {
    /// Create a new memory entry
    pub fn new(content: impl Into<String>, category: MemoryCategory, source: MemorySource) -> Self {
        Self {
            content: content.into(),
            category,
            importance: 50,
            created_at: Utc::now().timestamp(),
            source,
        }
    }

    /// Set the importance
    pub fn with_importance(mut self, importance: u8) -> Self {
        self.importance = importance.min(100);
        self
    }

    /// Set the creation timestamp
    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.created_at = timestamp;
        self
    }

    /// Format as markdown list item
    ///
    /// Format: `- [date] content [importance: N]`
    pub fn to_markdown(&self) -> String {
        let date = DateTime::from_timestamp(self.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        format!(
            "- [{}] {} [importance: {}]",
            date, self.content, self.importance
        )
    }

    /// Parse from markdown line
    ///
    /// Supports two formats:
    /// - New: `- [2026-04-01] Content here [importance: 80]`
    /// - Legacy: `- 2026-04-01: Content here [importance: 80]`
    pub fn from_markdown(
        line: &str,
        category: MemoryCategory,
        source: MemorySource,
    ) -> Option<Self> {
        let line = line.trim();
        if !line.starts_with('-') {
            return None;
        }

        let line = line[1..].trim();

        // Extract importance from trailing [importance: N]
        let (content, importance) = if let Some(idx) = line.rfind("[importance:") {
            let content_part = line[..idx].trim();
            let importance_part = &line[idx..];
            let importance = importance_part
                .strip_prefix("[importance:")
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| s.trim().parse::<u8>().ok())
                .unwrap_or(50);
            (content_part, importance)
        } else {
            (line, 50)
        };

        // Extract content, handling both new `[date] content` and legacy `date: content`
        let content = if let Some(rest) = content.strip_prefix('[') {
            // New format: [2026-04-01] content
            if let Some(bracket_end) = rest.find(']') {
                rest[bracket_end + 1..].trim().to_string()
            } else {
                content.to_string()
            }
        } else if let Some(colon_idx) = content.find(':') {
            // Legacy format: 2026-04-01: content
            content[colon_idx + 1..].trim().to_string()
        } else {
            content.to_string()
        };

        if content.is_empty() {
            return None;
        }

        Some(Self {
            content,
            category,
            importance,
            created_at: Utc::now().timestamp(),
            source,
        })
    }
}

/// Aggregated memory result (DEPRECATED)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AggregatedMemory {
    /// All memory entries
    pub entries: Vec<MemoryEntry>,
    /// Total count
    pub total: usize,
    /// Count by category
    pub by_category: HashMap<String, usize>,
    /// Count by source
    pub by_source: HashMap<String, usize>,
}

/// Metadata for a memory file (for UI display, DEPRECATED)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFileInfo {
    /// File identifier (agent_id, session_id, or "system")
    pub id: String,
    /// Display name
    pub name: String,
    /// Source type: "agent", "chat", or "system"
    pub source_type: String,
    /// File size in bytes
    pub size: u64,
    /// Last modified timestamp (Unix seconds)
    pub modified_at: i64,
    /// Number of entries in the file
    pub entry_count: usize,
}

// ============================================================================
// New Types (2-file layout)
// ============================================================================

/// Memory statistics for the persistent-file memory layout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// User file stats
    pub user: FileStats,
    /// Knowledge file stats
    pub knowledge: FileStats,
    /// Procedures file stats
    pub procedures: FileStats,
    /// Agent file stats
    pub agents: Vec<AgentFileStats>,
    /// Session stats
    pub sessions: SessionStats,
    /// Custom files stats
    pub custom_files: Vec<CustomFileStats>,
}

/// File statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStats {
    /// Current character count
    pub chars: usize,
    /// Character limit
    pub limit: usize,
}

/// Agent file statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFileStats {
    /// Agent ID
    pub id: String,
    /// Agent name
    pub name: String,
    /// Character count
    pub chars: usize,
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Number of active session directories
    pub active_count: usize,
    /// Total temp files across all sessions
    pub total_temp_files: usize,
}

/// Custom file statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomFileStats {
    /// File name (without extension)
    pub name: String,
    /// Character count
    pub chars: usize,
}

// ============================================================================
// Main Store Implementation
// ============================================================================

/// Markdown-based memory store (new 2-file layout)
///
/// ## Architecture
///
/// **Persistent files (3):**
/// - `USER.md` - User profile (max `user_char_limit` chars, default 2000)
/// - `KNOWLEDGE.md` - System knowledge (max `knowledge_char_limit` chars, default 3000)
/// - `PROCEDURES.md` - Procedural memory: SOPs, playbooks, how-tos (max `procedures_char_limit` chars, default 3000)
///
/// **Session temp files (unlimited):**
/// - `sessions/{session_id}/notes.md` - Session notes (7-day TTL)
///
/// **Custom files (LLM auto-created):**
/// - `custom/{name}.md` - Domain-specific knowledge (max `agent_char_limit` chars/file)
///
/// **Agent files (managed by bridge):**
/// - `agents/{agent_id}.md` - Agent summaries (max `agent_char_limit` chars, default 20000)
#[derive(Debug)]
pub struct MarkdownMemoryStore {
    /// Base path for memory files
    base_path: PathBuf,
    /// Configuration (char limits, etc.)
    config: MemoryConfig,
    /// In-memory cache (legacy, for backward compatibility)
    cache: Arc<RwLock<HashMap<String, Vec<MemoryEntry>>>>,
    /// Serializes writers to shared files (USER.md / KNOWLEDGE.md / PROCEDURES.md).
    ///
    /// Multiple call sites concurrently read-modify-write these files:
    /// the system-context background task, the agent-summary background
    /// task, agent `memory` tool calls, and user-initiated API edits.
    /// Without this lock, a read-modify-write in `replace_marker_section`
    /// can lose the update written by a concurrent caller. The lock is held
    /// only across a single file operation (microseconds for typical
    /// memory-file sizes), so it has no measurable impact on throughput.
    write_lock: Arc<tokio::sync::Mutex<()>>,
    /// Serializes sync writers to custom memory files (`custom/{name}.md`,
    /// `agents/{id}/custom/{name}.md`). The custom-file write path is sync
    /// (called from sync tool/init paths), so it cannot share the async
    /// `write_lock`. Without this, concurrent writes to the same custom
    /// file from multiple chat sessions would lose updates — `atomic_write`
    /// prevents corruption but not lost-write races.
    custom_write_lock: Arc<tokio::sync::Mutex<()>>,
}

/// Manual clone: the two lock Arcs are SHARED, not duplicated.
///
/// The server keeps one canonical store for the background jobs and the
/// API, while the chat toolchain and the agent toolchain each wrap their
/// own handle built by CLONING the canonical store. With a derived Clone
/// every clone got fresh locks, so a tool's read-modify-write could
/// clobber a concurrent background `replace_marker_section` (and vice
/// versa) even though each side held "its" lock: several instances,
/// several locks, same three files. Sharing the lock Arcs restores the
/// single-writer discipline the field docs promise, across every clone.
impl Clone for MarkdownMemoryStore {
    fn clone(&self) -> Self {
        Self {
            base_path: self.base_path.clone(),
            config: self.config.clone(),
            cache: self.cache.clone(),
            write_lock: self.write_lock.clone(),
            custom_write_lock: self.custom_write_lock.clone(),
        }
    }
}

/// Per-path write-lock registry. The store is instantiated from several
/// places (canonical server state, per-request API handlers, agent-side
/// extraction), and PER-INSTANCE locks let read-modify-writes against the
/// SAME files interleave: extraction's unlocked read + late write silently
/// rolled back concurrent marker updates (and vice versa — a second
/// lost-facts channel, independent of the merge-fit bug). Keying both
/// locks by the canonical directory makes every instance over the same
/// memory dir share one mutex — single-writer discipline without
/// cross-crate plumbing. Manual `Clone` keeps working (it re-shares the
/// same Arcs, now also shared with every other instance).
fn shared_locks_for(
    base_path: &std::path::Path,
) -> (Arc<tokio::sync::Mutex<()>>, Arc<tokio::sync::Mutex<()>>) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static REGISTRY: OnceLock<
        Mutex<HashMap<String, (Arc<tokio::sync::Mutex<()>>, Arc<tokio::sync::Mutex<()>>)>>,
    > = OnceLock::new();
    // Deterministic key: ABSOLUTE, not canonical. canonicalize() resolves
    // differently before vs after the directory exists (and through
    // symlinks like macOS /var → /private/var), so an instance created
    // pre-init and one created post-init could land on DIFFERENT keys —
    // silently splitting the lock again. `absolute()` depends only on the
    // input path, which every call site derives from the same config.
    let key = std::path::absolute(base_path)
        .unwrap_or_else(|_| base_path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let registry = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(key)
        .or_insert_with(|| {
            (
                Arc::new(tokio::sync::Mutex::new(())),
                Arc::new(tokio::sync::Mutex::new(())),
            )
        })
        .clone()
}

impl MarkdownMemoryStore {
    /// Create a new memory store with default config
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self::with_config(base_path, MemoryConfig::default())
    }

    /// Create a new memory store with custom config
    pub fn with_config(base_path: impl Into<PathBuf>, config: MemoryConfig) -> Self {
        let base_path = base_path.into();
        let (write_lock, custom_write_lock) = shared_locks_for(&base_path);
        Self {
            base_path,
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            write_lock,
            custom_write_lock,
        }
    }

    /// Initialize the directory structure
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.base_path)?;
        fs::create_dir_all(self.base_path.join("agents"))?;
        fs::create_dir_all(self.base_path.join("sessions"))?;
        fs::create_dir_all(self.base_path.join("custom"))?;

        // Sweep stale `.{name}.tmp` files left by aborted atomic writes
        // (process killed between write-temp and rename). Best-effort,
        // non-fatal. Covers the top level + each subdir so a long-running
        // server doesn't accumulate hidden temp files across crashes.
        let swept = atomic_write::cleanup_stale_temps(&self.base_path)
            + atomic_write::cleanup_stale_temps(&self.base_path.join("agents"))
            + atomic_write::cleanup_stale_temps(&self.base_path.join("sessions"))
            + atomic_write::cleanup_stale_temps(&self.base_path.join("custom"));
        if swept > 0 {
            info!(
                swept,
                "Cleaned up stale atomic-write temp files in memory store"
            );
        }

        // Create USER.md if it doesn't exist
        let user_path = self.base_path.join("USER.md");
        if !user_path.exists() {
            let content = "# User Profile\n\n> Last updated: \n\n";
            atomic_write::write(&user_path, content)?;
            info!(path = %user_path.display(), "Created USER.md");
        }

        // Create KNOWLEDGE.md if it doesn't exist
        let knowledge_path = self.base_path.join("KNOWLEDGE.md");
        if !knowledge_path.exists() {
            let content = "# System Knowledge\n\n\
                ## System Resources\n\n<!-- auto-generated -->\n\n\
                ## Domain Knowledge\n\n<!-- AI-managed -->\n\n\
                ## Agent Experiences\n\n<!-- auto-generated -->\n\n";
            atomic_write::write(&knowledge_path, content)?;
            info!(path = %knowledge_path.display(), "Created KNOWLEDGE.md");
        }

        // Create PROCEDURES.md if it doesn't exist
        let procedures_path = self.base_path.join("PROCEDURES.md");
        if !procedures_path.exists() {
            let content = "# Procedures\n\n\
                > SOPs, playbooks, and how-tos learned across sessions.\n\
                > Add step-by-step procedures that future conversations should follow.\n\n";
            atomic_write::write(&procedures_path, content)?;
            info!(path = %procedures_path.display(), "Created PROCEDURES.md");
        }

        // Create legacy directories for backward compatibility
        fs::create_dir_all(self.base_path.join("chat"))?;

        // Create system.md if it doesn't exist (legacy)
        let system_path = self.base_path.join("system.md");
        if !system_path.exists() {
            let content = "# System Memory\n\n## User Profile\n\n## Domain Knowledge\n\n## Task Patterns\n\n## System Evolution\n";
            atomic_write::write(&system_path, content)?;
            info!(path = %system_path.display(), "Created legacy system.md");
        }

        Ok(())
    }

    // ========================================================================
    // New API: Persistent file operations (2-file layout)
    // ========================================================================

    /// Write to a persistent file (user, knowledge, or procedures). Enforces char limits.
    ///
    /// # Arguments
    /// * `target` - One of "user", "knowledge", or "procedures"
    /// * `content` - Markdown content to write
    ///
    /// # Errors
    /// - Returns error if content exceeds char limit
    /// - Returns error if target is not recognized
    pub async fn write_file(&self, target: &str, content: &str) -> Result<()> {
        let limit = match target {
            "user" => self.config.user_char_limit,
            "knowledge" => self.config.knowledge_char_limit,
            "procedures" => self.config.procedures_char_limit,
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                    target
                )))
            }
        };

        if content.chars().count() > limit {
            return Err(Error::InvalidInput(format!(
                "Content exceeds {} char limit: {} > {}",
                target,
                content.chars().count(),
                limit
            )));
        }

        let path = match target {
            "user" => self.base_path.join("USER.md"),
            "knowledge" => self.base_path.join("KNOWLEDGE.md"),
            "procedures" => self.base_path.join("PROCEDURES.md"),
            _ => unreachable!(), // Already checked above
        };

        // Hold the write lock across the file write so concurrent
        // read-modify-write callers (replace_marker_section, replace_section)
        // cannot lose this update. See `write_lock` doc on the struct.
        let _lock = self.write_lock.lock().await;

        // [fake-async fix] the atomic write (tmp file + rename) is blocking
        // filesystem work — run it on the blocking pool.
        let write_path = path.clone();
        let target_owned = target.to_string();
        let content_owned = content.to_string();
        let write_result = tokio::task::spawn_blocking(move || {
            atomic_write::write(&write_path, &content_owned)
                .map_err(|e| Error::Storage(format!("Failed to write {}.md: {}", target_owned, e)))
        })
        .await
        .map_err(|e| Error::Storage(format!("write_file join error: {}", e)))?;
        write_result?;

        info!(target = %target, chars = content.chars().count(), "Wrote persistent file");
        Ok(())
    }

    /// Atomic read-modify-write of a persistent file under `write_lock`.
    ///
    /// The `memory` tool's add/replace/remove actions used to compose
    /// `read_file` + `write_file` themselves; only the write half took the
    /// lock, so a background `replace_marker_section` (or another clone's
    /// RMW) landing between the read and the write was silently reverted.
    /// Routing those callers through this method makes the whole
    /// read→transform→write sequence one serialized step.
    ///
    /// The closure receives the current content and returns the new
    /// content. An unchanged result skips the write entirely (callers
    /// detect "no change" from the returned `false`, same semantics as
    /// `replace_marker_section`). The char limit is enforced on the
    /// closure's result — a caller that grows the file past the limit
    /// gets the same actionable error `write_file` produces.
    pub async fn modify_file<F>(&self, target: &str, mut transform: F) -> Result<bool>
    where
        F: FnMut(&str) -> String,
    {
        let (limit, path) = match target {
            "user" => (self.config.user_char_limit, self.base_path.join("USER.md")),
            "knowledge" => (
                self.config.knowledge_char_limit,
                self.base_path.join("KNOWLEDGE.md"),
            ),
            "procedures" => (
                self.config.procedures_char_limit,
                self.base_path.join("PROCEDURES.md"),
            ),
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                    target
                )))
            }
        };

        // [fake-async fix] the read + fsync'd write are BLOCKING IO — doing
        // them inline while holding the async lock parked a tokio worker for
        // the whole fsync. The lock stays held ACROSS the spawn_blocking (the
        // read-modify-write must stay atomic); only the thread stops blocking.
        let _lock = self.write_lock.lock().await;
        let target_owned = target.to_string();

        // [fake-async fix] the blocking read and the fsync'd write each run
        // on the blocking pool; the caller-supplied transform (arbitrary
        // F, not Send) runs on the async side between them. The lock is
        // held across all three, preserving read-modify-write atomicity.
        let read_path = path.clone();
        let current = tokio::task::spawn_blocking(move || {
            if read_path.exists() {
                fs::read_to_string(&read_path).map_err(|e| {
                    Error::Storage(format!("Failed to read {}.md: {}", target_owned, e))
                })
            } else {
                Ok(String::new())
            }
        })
        .await
        .map_err(|e| Error::Storage(format!("modify_file join error: {}", e)))??;

        let new_content = transform(&current);
        if new_content == current {
            return Ok(false);
        }
        if new_content.chars().count() > limit {
            return Err(Error::InvalidInput(format!(
                "Content exceeds {} char limit: {} > {}",
                target,
                new_content.chars().count(),
                limit
            )));
        }

        let target_owned = target.to_string();
        let write_path = path.clone();
        let chars = new_content.chars().count();
        tokio::task::spawn_blocking(move || {
            atomic_write::write(&write_path, &new_content)
                .map_err(|e| Error::Storage(format!("Failed to write {}.md: {}", target_owned, e)))
        })
        .await
        .map_err(|e| Error::Storage(format!("modify_file join error: {}", e)))??;

        info!(target = %target, chars = chars, "Modified persistent file");
        Ok(true)
    }

    /// Read a persistent file. Returns empty string if not found.
    ///
    /// # Arguments
    /// * `target` - One of "user", "knowledge", or "procedures"
    pub async fn read_file(&self, target: &str) -> Result<String> {
        let path = match target {
            "user" => self.base_path.join("USER.md"),
            "knowledge" => self.base_path.join("KNOWLEDGE.md"),
            "procedures" => self.base_path.join("PROCEDURES.md"),
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                    target
                )))
            }
        };

        // [fake-async fix] file reads run on the blocking pool — the memory
        // snapshot path loads these on session access.
        let read_path = path.clone();
        let target_owned = target.to_string();
        match tokio::task::spawn_blocking(move || {
            if !read_path.exists() {
                return Ok(String::new());
            }
            std::fs::read_to_string(&read_path)
                .map_err(|e| Error::Storage(format!("Failed to read {}.md: {}", target_owned, e)))
        })
        .await
        {
            Ok(result) => result,
            Err(e) => Err(Error::Storage(format!("read_file join error: {}", e))),
        }
    }

    /// Read a persistent file synchronously. Returns empty string if not found.
    /// Used by MemorySnapshot::load which runs from sync context and cannot
    /// use block_in_place on single-threaded (current_thread) tokio runtimes.
    pub fn read_file_sync(&self, target: &str) -> String {
        let path = match target {
            "user" => self.base_path.join("USER.md"),
            "knowledge" => self.base_path.join("KNOWLEDGE.md"),
            "procedures" => self.base_path.join("PROCEDURES.md"),
            _ => return String::new(),
        };
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    /// Replace a section within KNOWLEDGE.md by heading name.
    ///
    /// Finds "## {heading}" and replaces content until next "## " heading.
    /// If heading not found, prepends the section.
    ///
    /// # Arguments
    /// * `target` - Must be "knowledge"
    /// * `heading` - Section heading (e.g., "System Resources")
    /// * `new_body` - New content for the section (without the heading)
    pub async fn replace_section(&self, target: &str, heading: &str, new_body: &str) -> Result<()> {
        if target != "knowledge" {
            return Err(Error::InvalidInput(
                "Section replacement only works for 'knowledge' target".to_string(),
            ));
        }

        // Serialize against concurrent writers to KNOWLEDGE.md.
        let _lock = self.write_lock.lock().await;

        let path = self.base_path.join("KNOWLEDGE.md");
        let current = if path.exists() {
            fs::read_to_string(&path)
                .map_err(|e| Error::Storage(format!("Failed to read KNOWLEDGE.md: {}", e)))?
        } else {
            String::new()
        };

        let new_content = replace_section_in_content(&current, heading, new_body);

        // Check char limit
        if new_content.chars().count() > self.config.knowledge_char_limit {
            return Err(Error::InvalidInput(format!(
                "Content after section replacement exceeds knowledge char limit: {} > {}",
                new_content.chars().count(),
                self.config.knowledge_char_limit
            )));
        }

        atomic_write::write(&path, new_content)
            .map_err(|e| Error::Storage(format!("Failed to write KNOWLEDGE.md: {}", e)))?;

        info!(heading = %heading, "Replaced section in KNOWLEDGE.md");
        Ok(())
    }

    /// Replace content between `<!-- {marker} --> ... <!-- /{marker} -->` in a target file.
    ///
    /// If the marker is not found, the section is appended before the closing marker
    /// or at the end of the file. Each update fully replaces the previous content —
    /// zero accumulation.
    ///
    /// # Arguments
    /// * `target` - Either "user" or "knowledge"
    /// * `marker` - Marker name (e.g., "system-context", "chat-summary")
    /// * `new_content` - Content to place between the markers
    ///
    /// # Returns
    /// `Ok(true)` if file was modified, `Ok(false)` if content unchanged.
    /// Append fresh memory facts to a persistent file, marker-aware (see
    /// [`append_facts_transform`]). Built on [`modify_file`]: one locked
    /// read-modify-write, the char limit enforced on the result. Returns
    /// (wrote, fresh_count) — a dedup-only call reports (false, 0) and
    /// never touches the disk.
    pub async fn append_facts(&self, target: &str, facts: &[String]) -> Result<(bool, usize)> {
        let limit = match target {
            "user" => self.config.user_char_limit,
            "knowledge" => self.config.knowledge_char_limit,
            "procedures" => self.config.procedures_char_limit,
            _ => {
                return Err(Error::InvalidInput(format!(
                    "Invalid target: {}. Must be 'user', 'knowledge', or 'procedures'",
                    target
                )))
            }
        };
        let facts = facts.to_vec();
        let mut fresh_count = 0usize;
        // NOT `move`: modify_file runs the closure inline (no Send bound),
        // so a plain borrow lets the fresh-count write through to the outer
        // variable — the moved closure updated its own copy and the caller
        // always read 0 (caught by the integration test).
        let wrote = self
            .modify_file(target, |current| {
                let (out, n) = append_facts_transform(current, &facts, limit);
                fresh_count = n;
                out
            })
            .await?;
        Ok((wrote, fresh_count))
    }

    pub async fn replace_marker_section(
        &self,
        target: &str,
        marker: &str,
        new_content: &str,
    ) -> Result<bool> {
        let path = match target {
            "user" => self.base_path.join("USER.md"),
            "knowledge" => self.base_path.join("KNOWLEDGE.md"),
            "procedures" => self.base_path.join("PROCEDURES.md"),
            _ => return Err(Error::InvalidInput(format!("Invalid target: {}", target))),
        };

        // Serialize the read-modify-write so a concurrent writer cannot
        // read our pre-write state and clobber our update (and vice versa).
        // [fake-async fix] blocking IO (read + fsync'd write) runs on the
        // blocking pool — the lock is still held across it for atomicity.
        let _lock = self.write_lock.lock().await;
        let limit = match target {
            "user" => self.config.user_char_limit,
            "knowledge" => self.config.knowledge_char_limit,
            "procedures" => self.config.procedures_char_limit,
            _ => unreachable!(), // checked above
        };
        let target_owned = target.to_string();
        let marker_owned = marker.to_string();
        let new_content = new_content.to_string();
        // [fake-async fix] the read + fsync'd write are blocking IO — run on
        // the blocking pool; the async lock stays held across it (the
        // read-modify-write must remain atomic vs. concurrent writers).
        let inner_path = path.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            replace_marker_section_locked(
                &inner_path,
                &target_owned,
                &marker_owned,
                &new_content,
                limit,
            )
        })
        .await
        .map_err(|e| Error::Storage(format!("replace_marker_section join error: {}", e)))?
    }

    /// Get stats for all memory targets.
    pub async fn stats(&self) -> Result<MemoryStats> {
        // Read USER.md
        let user_path = self.base_path.join("USER.md");
        let user_chars = if user_path.exists() {
            fs::read_to_string(&user_path)?.chars().count()
        } else {
            0
        };

        // Read KNOWLEDGE.md
        let knowledge_path = self.base_path.join("KNOWLEDGE.md");
        let knowledge_chars = if knowledge_path.exists() {
            fs::read_to_string(&knowledge_path)?.chars().count()
        } else {
            0
        };

        // Read PROCEDURES.md
        let procedures_path = self.base_path.join("PROCEDURES.md");
        let procedures_chars = if procedures_path.exists() {
            fs::read_to_string(&procedures_path)?.chars().count()
        } else {
            0
        };

        // Scan agent files
        let mut agents = Vec::new();
        let agents_path = self.base_path.join("agents");
        if agents_path.exists() {
            for entry in fs::read_dir(&agents_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(agent_id) = path.file_stem().and_then(|s| s.to_str()) {
                        let chars = fs::read_to_string(&path)?.chars().count();
                        agents.push(AgentFileStats {
                            id: agent_id.to_string(),
                            name: agent_id.to_string(), // Will be resolved by bridge
                            chars,
                        });
                    }
                }
            }
        }

        // Scan session directories
        let sessions_path = self.base_path.join("sessions");
        let (active_count, total_temp_files) = if sessions_path.exists() {
            let mut session_count = 0;
            let mut temp_file_count = 0;
            for entry in fs::read_dir(&sessions_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    session_count += 1;
                    // Count temp files in this session
                    if let Ok(entries) = fs::read_dir(&path) {
                        for e in entries.flatten() {
                            if e.path().extension().map(|ex| ex == "md").unwrap_or(false) {
                                temp_file_count += 1;
                            }
                        }
                    }
                }
            }
            (session_count, temp_file_count)
        } else {
            (0, 0)
        };

        // Scan custom files
        let custom_files = self
            .list_custom_files()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, chars)| CustomFileStats { name, chars })
            .collect();

        Ok(MemoryStats {
            user: FileStats {
                chars: user_chars,
                limit: self.config.user_char_limit,
            },
            knowledge: FileStats {
                chars: knowledge_chars,
                limit: self.config.knowledge_char_limit,
            },
            procedures: FileStats {
                chars: procedures_chars,
                limit: self.config.procedures_char_limit,
            },
            agents,
            sessions: SessionStats {
                active_count,
                total_temp_files,
            },
            custom_files,
        })
    }

    // ========================================================================
    // New API: Session temp file operations
    // ========================================================================

    /// Write a session temp file (scratch/notes/todo). Creates directory on demand.
    ///
    /// # Arguments
    /// * `session_id` - Unique session identifier
    /// * `target` - One of: "scratch", "notes", "todo"
    /// * `content` - Content to write (no char limit)
    pub async fn write_session_file(
        &self,
        session_id: &str,
        target: &str,
        content: &str,
    ) -> Result<()> {
        Self::validate_session_id(session_id)?;
        // Validate target
        if !matches!(target, "scratch" | "notes" | "todo") {
            return Err(Error::InvalidInput(format!(
                "Invalid session target: {}. Must be 'scratch', 'notes', or 'todo'",
                target
            )));
        }

        let filename = format!("{}.md", target);
        let session_dir = self.base_path.join("sessions").join(session_id);
        fs::create_dir_all(&session_dir)
            .map_err(|e| Error::Storage(format!("Failed to create session directory: {}", e)))?;

        let path = session_dir.join(&filename);
        atomic_write::write(&path, content).map_err(|e| {
            Error::Storage(format!("Failed to write session file {}: {}", filename, e))
        })?;

        debug!(session_id = %session_id, target = %target, chars = content.chars().count(), "Wrote session temp file");
        Ok(())
    }

    /// Read a session temp file. Returns empty string if not found.
    ///
    /// # Arguments
    /// * `session_id` - Unique session identifier
    /// * `target` - One of: "scratch", "notes", "todo"
    pub async fn read_session_file(&self, session_id: &str, target: &str) -> Result<String> {
        Self::validate_session_id(session_id)?;
        let filename = format!("{}.md", target);
        let path = self
            .base_path
            .join("sessions")
            .join(session_id)
            .join(&filename);

        if !path.exists() {
            return Ok(String::new());
        }

        fs::read_to_string(&path)
            .map_err(|e| Error::Storage(format!("Failed to read session file {}: {}", filename, e)))
    }

    /// Delete session directories older than TTL days.
    ///
    /// # Arguments
    /// * `ttl_days` - Time-to-live in days (default: 7 from config)
    ///
    /// # Returns
    /// Number of session directories deleted
    pub async fn cleanup_old_sessions(&self, ttl_days: u64) -> Result<usize> {
        let sessions_path = self.base_path.join("sessions");
        if !sessions_path.exists() {
            return Ok(0);
        }

        let ttl_duration = Duration::days(ttl_days as i64);
        let now = Utc::now();
        let mut deleted_count = 0;

        for entry in fs::read_dir(&sessions_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let metadata = fs::metadata(&path)?;
                if let Ok(modified) = metadata.modified() {
                    let modified_dt = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0));

                    if let Some(dt) = modified_dt {
                        if now.signed_duration_since(dt) > ttl_duration {
                            fs::remove_dir_all(&path).map_err(|e| {
                                Error::Storage(format!("Failed to delete session directory: {}", e))
                            })?;
                            deleted_count += 1;
                            debug!(path = %path.display(), "Deleted old session directory");
                        }
                    }
                }
            }
        }

        if deleted_count > 0 {
            info!(
                deleted = deleted_count,
                ttl_days = ttl_days,
                "Cleaned up old session directories"
            );
        }

        Ok(deleted_count)
    }

    // ========================================================================
    // Custom files API (domain-specific memory files)
    // ========================================================================

    /// Validate a session ID (prevent path traversal).
    fn validate_session_id(session_id: &str) -> Result<()> {
        if session_id.is_empty() || session_id.len() > 128 {
            return Err(Error::InvalidInput(
                "session_id must be 1-128 characters".to_string(),
            ));
        }
        if session_id.contains('\0')
            || session_id.contains("..")
            || session_id.contains('/')
            || session_id.contains('\\')
        {
            return Err(Error::InvalidInput(
                "session_id contains invalid path characters".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate a custom file name.
    /// Must be 1-32 chars, only lowercase alphanumeric, hyphens, underscores.
    fn validate_custom_name(name: &str) -> Result<()> {
        if name.is_empty() || name.len() > 32 {
            return Err(Error::InvalidInput(
                "Custom file name must be 1-32 characters".to_string(),
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(Error::InvalidInput(
                "Custom file name must only contain lowercase letters, digits, hyphens, and underscores".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate an agent ID used as a path segment. Agent IDs are generated
    /// server-side (UUIDs), but `clean_agent_dir` and the agent-scoped
    /// custom-file methods also receive IDs that have traveled through API
    /// path parameters — hold every path join to the same traversal-safe
    /// standard as session IDs and custom names.
    fn validate_agent_id(agent_id: &str) -> Result<()> {
        if agent_id.is_empty() || agent_id.len() > 128 {
            return Err(Error::InvalidInput(
                "agent_id must be 1-128 characters".to_string(),
            ));
        }
        if agent_id.contains('\0')
            || agent_id.contains("..")
            || agent_id.contains('/')
            || agent_id.contains('\\')
        {
            return Err(Error::InvalidInput(
                "agent_id contains invalid path characters".to_string(),
            ));
        }
        Ok(())
    }

    /// Read a custom memory file. Returns empty string if not found.
    pub fn read_custom_file(&self, name: &str) -> Result<String> {
        Self::validate_custom_name(name)?;
        let path = self.base_path.join("custom").join(format!("{}.md", name));
        if !path.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(&path)
            .map_err(|e| Error::Storage(format!("Failed to read custom file {}: {}", name, e)))
    }

    /// Write a custom memory file. Enforces per-file char limit.
    pub async fn write_custom_file(&self, name: &str, content: &str) -> Result<()> {
        Self::validate_custom_name(name)?;
        let limit = self.config.agent_char_limit; // reuse agent_char_limit as per-file limit
        let char_count = content.chars().count();
        if char_count > limit {
            return Err(Error::InvalidInput(format!(
                "Custom file content exceeds {} char limit: {} > {}",
                name, char_count, limit
            )));
        }
        let dir = self.base_path.join("custom");
        let path = dir.join(format!("{}.md", name));
        // Serialize against concurrent writers to the same custom file.
        // [fake-async fix] the create-dir + fsync'd write are blocking IO —
        // run on the blocking pool, lock held across (same treatment
        // write_file got).
        let _lock = self.custom_write_lock.lock().await;
        let content_owned = content.to_string();
        let name_owned = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            fs::create_dir_all(&dir)?;
            atomic_write::write(&path, &content_owned).map_err(|e| {
                Error::Storage(format!("Failed to write custom file {}: {}", name_owned, e))
            })?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Storage(format!("write_custom_file join error: {}", e)))??;
        info!(name = %name, chars = char_count, "Wrote custom memory file");
        Ok(())
    }

    /// Atomic read-modify-write of a custom file (agent-scoped or global)
    /// under `custom_write_lock`. Same rationale as [`modify_file`]: the
    /// tool's read→merge→write on custom files becomes one serialized step
    /// across every clone of the store, so a concurrent writer's update
    /// cannot be silently reverted.
    ///
    /// Returns `false` when the closure left the content unchanged (no
    /// write happened). Enforces the per-file char limit on the result.
    pub async fn modify_custom_file<F>(
        &self,
        agent_id: Option<&str>,
        name: &str,
        mut transform: F,
    ) -> Result<bool>
    where
        F: FnMut(&str) -> String,
    {
        Self::validate_custom_name(name)?;
        if let Some(aid) = agent_id {
            Self::validate_agent_id(aid)?;
        }

        let path = match agent_id {
            Some(aid) => self
                .base_path
                .join("agents")
                .join(aid)
                .join("custom")
                .join(format!("{}.md", name)),
            None => self.base_path.join("custom").join(format!("{}.md", name)),
        };

        // [fake-async fix] same treatment as modify_file: the read and the
        // fsync'd write are blocking IO — they run on the blocking pool while
        // the async lock stays held ACROSS them (the read-modify-write must
        // stay atomic vs concurrent writers); the caller-supplied transform
        // (arbitrary F, not Send) runs on the async side between them.
        let _lock = self.custom_write_lock.lock().await;

        let read_path = path.clone();
        let name_owned = name.to_string();
        let current = tokio::task::spawn_blocking(move || {
            if read_path.exists() {
                fs::read_to_string(&read_path).map_err(|e| {
                    Error::Storage(format!("Failed to read custom file {}: {}", name_owned, e))
                })
            } else {
                Ok(String::new())
            }
        })
        .await
        .map_err(|e| Error::Storage(format!("modify_custom_file join error: {}", e)))??;

        let new_content = transform(&current);
        if new_content == current {
            return Ok(false);
        }

        let limit = self.config.agent_char_limit;
        if new_content.chars().count() > limit {
            return Err(Error::InvalidInput(format!(
                "Custom file content exceeds {} char limit: {} > {}",
                name,
                new_content.chars().count(),
                limit
            )));
        }

        let write_path = path;
        let name_owned = name.to_string();
        let chars = new_content.chars().count();
        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(parent) = write_path.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write::write(&write_path, &new_content).map_err(|e| {
                Error::Storage(format!("Failed to write custom file {}: {}", name_owned, e))
            })?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Storage(format!("modify_custom_file join error: {}", e)))??;

        info!(name = %name, chars = chars, "Modified custom memory file");
        Ok(true)
    }

    /// List all custom memory files. Returns (name, char_count) pairs.
    pub fn list_custom_files(&self) -> Result<Vec<(String, usize)>> {
        let custom_dir = self.base_path.join("custom");
        if !custom_dir.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(&custom_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    files.push((name.to_string(), content.chars().count()));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Delete a custom memory file.
    pub fn delete_custom_file(&self, name: &str) -> Result<()> {
        Self::validate_custom_name(name)?;
        let path = self.base_path.join("custom").join(format!("{}.md", name));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                Error::Storage(format!("Failed to delete custom file {}: {}", name, e))
            })?;
            info!(name = %name, "Deleted custom memory file");
        }
        Ok(())
    }

    /// Get the custom directory path.
    pub fn custom_dir(&self) -> PathBuf {
        self.base_path.join("custom")
    }

    // ========== Agent-scoped custom file methods ==========

    /// Read an agent-scoped custom file.
    /// Path: `agents/{agent_id}/custom/{name}.md`
    pub fn read_agent_custom_file(&self, agent_id: &str, name: &str) -> Result<String> {
        Self::validate_agent_id(agent_id)?;
        Self::validate_custom_name(name)?;
        let path = self
            .base_path
            .join("agents")
            .join(agent_id)
            .join("custom")
            .join(format!("{}.md", name));
        if !path.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(&path).map_err(|e| {
            Error::Storage(format!(
                "Failed to read agent custom file {}/{}: {}",
                agent_id, name, e
            ))
        })
    }

    /// Write an agent-scoped custom file. Enforces per-file char limit.
    /// Path: `agents/{agent_id}/custom/{name}.md`
    pub async fn write_agent_custom_file(
        &self,
        agent_id: &str,
        name: &str,
        content: &str,
    ) -> Result<()> {
        Self::validate_agent_id(agent_id)?;
        Self::validate_custom_name(name)?;
        let limit = self.config.agent_char_limit;
        let char_count = content.chars().count();
        if char_count > limit {
            return Err(Error::InvalidInput(format!(
                "Custom file content exceeds {} char limit: {} > {}",
                name, char_count, limit
            )));
        }
        let dir = self.base_path.join("agents").join(agent_id).join("custom");
        let path = dir.join(format!("{}.md", name));
        // Serialize against concurrent writers to the same agent custom file.
        // [fake-async fix] the create-dir + fsync'd write are blocking IO —
        // run on the blocking pool, lock held across.
        let _lock = self.custom_write_lock.lock().await;
        let content_owned = content.to_string();
        let (agent_owned, name_owned) = (agent_id.to_string(), name.to_string());
        tokio::task::spawn_blocking(move || -> Result<()> {
            fs::create_dir_all(&dir)?;
            atomic_write::write(&path, &content_owned).map_err(|e| {
                Error::Storage(format!(
                    "Failed to write agent custom file {}/{}: {}",
                    agent_owned, name_owned, e
                ))
            })?;
            Ok(())
        })
        .await
        .map_err(|e| Error::Storage(format!("write_agent_custom_file join error: {}", e)))??;
        info!(agent_id = %agent_id, name = %name, chars = char_count, "Wrote agent custom file");
        Ok(())
    }

    /// List agent-scoped custom files. Returns (name, char_count) pairs.
    pub fn list_agent_custom_files(&self, agent_id: &str) -> Result<Vec<(String, usize)>> {
        Self::validate_agent_id(agent_id)?;
        let custom_dir = self.base_path.join("agents").join(agent_id).join("custom");
        if !custom_dir.exists() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(&custom_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    files.push((name.to_string(), content.chars().count()));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(files)
    }

    /// Delete all memory files for an agent (the entire `agents/{agent_id}/` directory).
    /// Called when an agent is deleted to clean up associated knowledge files.
    pub fn clean_agent_dir(&self, agent_id: &str) -> Result<()> {
        Self::validate_agent_id(agent_id)?;
        let agent_dir = self.base_path.join("agents").join(agent_id);
        if agent_dir.exists() {
            fs::remove_dir_all(&agent_dir).map_err(|e| {
                Error::Storage(format!(
                    "Failed to clean agent memory dir {}: {}",
                    agent_id, e
                ))
            })?;
            info!(agent_id = %agent_id, "Cleaned agent memory directory");
        }
        Ok(())
    }

    // ========================================================================
    // Legacy API (deprecated - kept for backward compatibility)
    // ========================================================================

    /// Get the file path for a category (DEPRECATED - maps to legacy files)
    #[deprecated(note = "Use write_file/read_file with 'user' or 'knowledge' instead")]
    pub fn category_path(&self, category: &MemoryCategory) -> PathBuf {
        self.base_path.join(category.filename())
    }

    /// Read markdown content for a category (DEPRECATED)
    #[deprecated(note = "Use read_file instead")]
    #[allow(deprecated)]
    pub fn read_category(&self, category: &MemoryCategory) -> Result<String> {
        warn!(?category, "read_category called (deprecated)");
        let path = self.category_path(category);
        if !path.exists() {
            return Ok(self.default_category_content(category));
        }
        fs::read_to_string(&path)
            .map_err(|e| Error::Storage(format!("Failed to read {:?}: {}", category, e)))
    }

    /// Write markdown content for a category (DEPRECATED)
    #[deprecated(note = "Use write_file instead")]
    #[allow(deprecated)]
    pub fn write_category(&self, category: &MemoryCategory, content: &str) -> Result<()> {
        warn!(?category, "write_category called (deprecated)");
        let path = self.category_path(category);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        atomic_write::write(&path, content)
            .map_err(|e| Error::Storage(format!("Failed to write {:?}: {}", category, e)))?;

        info!(category = ?category, size = content.len(), "Wrote category memory file (deprecated)");
        Ok(())
    }

    /// Get statistics for a category (DEPRECATED)
    #[deprecated(note = "Use stats instead")]
    #[allow(deprecated)]
    pub fn category_stats(&self, category: &MemoryCategory) -> Result<CategoryStats> {
        warn!(?category, "category_stats called (deprecated)");
        let path = self.category_path(category);

        let content = self.read_category(category)?;
        let entry_count = content
            .lines()
            .filter(|l| l.trim().starts_with('-'))
            .count();

        let (file_size, modified_at) = if path.exists() {
            let metadata = fs::metadata(&path)?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (metadata.len(), modified)
        } else {
            (content.len() as u64, 0)
        };

        Ok(CategoryStats {
            entry_count,
            file_size,
            modified_at,
        })
    }

    /// Get statistics for all categories (DEPRECATED)
    #[deprecated(note = "Use stats instead")]
    #[allow(deprecated)]
    pub fn all_stats(&self) -> Result<HashMap<String, CategoryStats>> {
        warn!("all_stats called (deprecated)");
        let mut stats = HashMap::new();
        for category in MemoryCategory::all() {
            let key = category.to_string();
            stats.insert(key, self.category_stats(category)?);
        }
        Ok(stats)
    }

    /// Export all categories as a single markdown string (DEPRECATED)
    #[deprecated(note = "Use stats instead")]
    #[allow(deprecated)]
    pub fn export_all(&self) -> Result<String> {
        warn!("export_all called (deprecated)");
        let mut output = String::new();
        output.push_str("# NeoMind Memory Export\n\n");
        output.push_str(&format!(
            "Generated: {}\n\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        for category in MemoryCategory::all() {
            let content = self.read_category(category)?;
            output.push_str(&format!("---\n\n# {}\n\n", category.display_name()));
            output.push_str(&content);
            output.push_str("\n\n");
        }

        Ok(output)
    }

    /// Generate default content for a category file (DEPRECATED)
    fn default_category_content(&self, category: &MemoryCategory) -> String {
        format!(
            "# {}\n\n> Last updated: {}\n> Total entries: 0\n\n",
            category.display_name(),
            Utc::now().format("%Y-%m-%d %H:%M")
        )
    }

    /// Get the base path
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Read memory entries from a source (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn read(&self, source: &MemorySource) -> Result<Vec<MemoryEntry>> {
        warn!(source = %source.display_name(), "read called (deprecated)");
        // Check cache first
        let cache_key = self.cache_key(source);
        {
            let cache = self.cache.read();
            if let Some(entries) = cache.get(&cache_key) {
                return Ok(entries.clone());
            }
        }

        let file_path = source.file_path(&self.base_path);
        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&file_path)
            .map_err(|e| Error::Storage(format!("Failed to read memory file: {}", e)))?;

        let entries = self.parse_markdown(&content, source);

        // Update cache
        {
            let mut cache = self.cache.write();
            cache.insert(cache_key, entries.clone());
        }

        Ok(entries)
    }

    /// Append a memory entry (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn append(&self, source: &MemorySource, entry: &MemoryEntry) -> Result<()> {
        warn!(source = %source.display_name(), "append called (deprecated)");
        let file_path = source.file_path(&self.base_path);

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Read existing content or create new
        let mut content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .map_err(|e| Error::Storage(format!("Failed to read memory file: {}", e)))?
        } else {
            self.create_empty_markdown(source)
        };

        // Find the section and append
        let section_name = entry.category.section_name();
        let section_header = format!("## {}", section_name);

        // Find the section and add entry
        if let Some(section_start) = content.find(&section_header) {
            // Find next section or end of file
            let search_start = section_start + section_header.len();
            let next_section = content[search_start..]
                .find("\n## ")
                .map(|i| search_start + i)
                .unwrap_or(content.len());

            // Insert the new entry before the next section
            let insert_pos = next_section;
            let entry_text = format!("\n{}\n", entry.to_markdown());
            content.insert_str(insert_pos, &entry_text);
        } else {
            // Section doesn't exist, append it
            content.push_str(&format!(
                "\n## {}\n\n{}\n",
                section_name,
                entry.to_markdown()
            ));
        }

        // Write back
        atomic_write::write(&file_path, content)?;

        // Invalidate cache
        {
            let mut cache = self.cache.write();
            cache.remove(&self.cache_key(source));
        }

        debug!(
            source = %source.display_name(),
            category = ?entry.category,
            content = %entry.content,
            "Appended memory entry (deprecated)"
        );

        Ok(())
    }

    /// Append multiple entries (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    #[allow(deprecated)]
    pub fn append_batch(&self, source: &MemorySource, entries: &[MemoryEntry]) -> Result<()> {
        warn!("append_batch called (deprecated)");
        for entry in entries {
            self.append(source, entry)?;
        }
        Ok(())
    }

    /// Write complete memory file (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn write(&self, source: &MemorySource, entries: &[MemoryEntry]) -> Result<()> {
        warn!(source = %source.display_name(), "write called (deprecated)");
        let file_path = source.file_path(&self.base_path);

        // Group by category
        let mut by_category: HashMap<MemoryCategory, Vec<&MemoryEntry>> = HashMap::new();
        for entry in entries {
            by_category.entry(entry.category).or_default().push(entry);
        }

        // Build markdown content
        let mut content = match source {
            MemorySource::Agent { name, .. } => format!("# {} Memory\n\n", name),
            MemorySource::Chat { session_id } => format!("# Chat {} Memory\n\n", session_id),
            MemorySource::System => "# System Memory\n\n".to_string(),
        };

        for category in MemoryCategory::all() {
            content.push_str(&format!("## {}\n\n", category.section_name()));
            if let Some(cat_entries) = by_category.get(category) {
                for entry in cat_entries {
                    content.push_str(&entry.to_markdown());
                    content.push('\n');
                }
            }
            content.push('\n');
        }

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        atomic_write::write(&file_path, content)?;

        // Invalidate cache
        {
            let mut cache = self.cache.write();
            cache.remove(&self.cache_key(source));
        }

        info!(
            source = %source.display_name(),
            count = entries.len(),
            "Wrote memory file (deprecated)"
        );

        Ok(())
    }

    /// Aggregate all memory from all sources (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    #[allow(deprecated)]
    pub fn aggregate_all(&self) -> Result<AggregatedMemory> {
        warn!("aggregate_all called (deprecated)");
        let mut result = AggregatedMemory::default();

        // Read system memory
        let system_entries = self.read(&MemorySource::System)?;
        result.entries.extend(system_entries.clone());

        // Read agent memories
        let agents_path = self.base_path.join("agents");
        if agents_path.exists() {
            for entry in fs::read_dir(&agents_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(agent_id) = path.file_stem().and_then(|s| s.to_str()) {
                        let source = MemorySource::Agent {
                            id: agent_id.to_string(),
                            name: agent_id.to_string(),
                        };
                        if let Ok(entries) = self.read(&source) {
                            result.entries.extend(entries);
                        }
                    }
                }
            }
        }

        // Read chat memories
        let chat_path = self.base_path.join("chat");
        if chat_path.exists() {
            for entry in fs::read_dir(&chat_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) {
                        let source = MemorySource::Chat {
                            session_id: session_id.to_string(),
                        };
                        if let Ok(entries) = self.read(&source) {
                            result.entries.extend(entries);
                        }
                    }
                }
            }
        }

        // Calculate stats
        result.total = result.entries.len();
        for entry in &result.entries {
            *result
                .by_category
                .entry(entry.category.to_string().to_lowercase())
                .or_default() += 1;
            *result
                .by_source
                .entry(entry.source.display_name())
                .or_default() += 1;
        }

        Ok(result)
    }

    /// Search memory entries (simple text matching, DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    #[allow(deprecated)]
    pub fn search(&self, query: &str) -> Result<Vec<MemoryEntry>> {
        warn!("search called (deprecated)");
        let all = self.aggregate_all()?;
        let query_lower = query.to_lowercase();

        let mut matches: Vec<MemoryEntry> = all
            .entries
            .into_iter()
            .filter(|e| e.content.to_lowercase().contains(&query_lower))
            .collect();

        // Sort by importance (descending)
        matches.sort_by_key(|m| std::cmp::Reverse(m.importance));

        Ok(matches)
    }

    /// Prune memory to max entries, keeping highest importance (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    #[allow(deprecated)]
    pub fn prune(&self, source: &MemorySource, max_items: usize) -> Result<usize> {
        warn!(source = %source.display_name(), "prune called (deprecated)");
        let mut entries = self.read(source)?;

        if entries.len() <= max_items {
            return Ok(0);
        }

        // Sort by importance (descending)
        entries.sort_by_key(|e| std::cmp::Reverse(e.importance));

        let removed = entries.len() - max_items;
        entries.truncate(max_items);

        // Rewrite file
        self.write(source, &entries)?;

        info!(
            source = %source.display_name(),
            removed = removed,
            remaining = entries.len(),
            "Pruned memory entries (deprecated)"
        );

        Ok(removed)
    }

    /// Clear all memory for a source (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn clear(&self, source: &MemorySource) -> Result<()> {
        warn!(source = %source.display_name(), "clear called (deprecated)");
        let file_path = source.file_path(&self.base_path);

        if file_path.exists() {
            // Write empty file with headers
            let content = self.create_empty_markdown(source);
            atomic_write::write(&file_path, content)?;
        }

        // Invalidate cache
        {
            let mut cache = self.cache.write();
            cache.remove(&self.cache_key(source));
        }

        Ok(())
    }

    /// Export all memory as a single markdown string (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    #[allow(deprecated)]
    pub fn export_markdown(&self) -> Result<String> {
        warn!("export_markdown called (deprecated)");
        let all = self.aggregate_all()?;

        let mut output = String::new();
        output.push_str("# NeoMind Memory Export\n\n");
        output.push_str(&format!(
            "Generated: {}\n\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        output.push_str(&format!("Total entries: {}\n\n", all.total));

        // Group by category
        for category in MemoryCategory::all() {
            let cat_entries: Vec<_> = all
                .entries
                .iter()
                .filter(|e| e.category == *category)
                .collect();

            if !cat_entries.is_empty() {
                output.push_str(&format!("## {}\n\n", category.section_name()));
                for entry in cat_entries {
                    output.push_str(&format!(
                        "- **{}**: {} `[importance: {}]`\n",
                        entry.source.display_name(),
                        entry.content,
                        entry.importance
                    ));
                }
                output.push('\n');
            }
        }

        Ok(output)
    }

    // ========================================================================
    // Legacy File-based API (for UI display, DEPRECATED)
    // ========================================================================

    /// List all memory files (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn list_files(&self) -> Result<Vec<MemoryFileInfo>> {
        warn!("list_files called (deprecated)");
        let mut files = Vec::new();

        // System memory
        let system_path = self.base_path.join("system.md");
        if system_path.exists() {
            if let Some(info) = self.get_file_info(&system_path, "system", "System", "system")? {
                files.push(info);
            }
        }

        // Agent memories
        let agents_path = self.base_path.join("agents");
        if agents_path.exists() {
            for entry in fs::read_dir(&agents_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(agent_id) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Some(info) = self.get_file_info(
                            &path, agent_id,
                            agent_id, // Will be replaced with actual agent name if available
                            "agent",
                        )? {
                            files.push(info);
                        }
                    }
                }
            }
        }

        // Chat memories
        let chat_path = self.base_path.join("chat");
        if chat_path.exists() {
            for entry in fs::read_dir(&chat_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) {
                        let display_name =
                            format!("Chat {}", &session_id[..8.min(session_id.len())]);
                        if let Some(info) =
                            self.get_file_info(&path, session_id, &display_name, "chat")?
                        {
                            files.push(info);
                        }
                    }
                }
            }
        }

        // Sort by modified time, newest first
        files.sort_by_key(|f| std::cmp::Reverse(f.modified_at));

        Ok(files)
    }

    /// Get file info helper (DEPRECATED)
    fn get_file_info(
        &self,
        path: &Path,
        id: &str,
        name: &str,
        source_type: &str,
    ) -> Result<Option<MemoryFileInfo>> {
        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let content = fs::read_to_string(path).unwrap_or_default();
        let entry_count = content.lines().filter(|l| l.starts_with('-')).count();

        Ok(Some(MemoryFileInfo {
            id: id.to_string(),
            name: name.to_string(),
            source_type: source_type.to_string(),
            size: metadata.len(),
            modified_at,
            entry_count,
        }))
    }

    /// Read raw markdown content from a memory file (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn read_raw_markdown(&self, source_type: &str, id: &str) -> Result<String> {
        warn!(source_type, id, "read_raw_markdown called (deprecated)");
        let path = match source_type {
            "agent" => self.base_path.join("agents").join(format!("{}.md", id)),
            "chat" => self.base_path.join("chat").join(format!("{}.md", id)),
            _ => self.base_path.join("system.md"),
        };

        if !path.exists() {
            return Err(Error::Storage(format!("Memory file not found: {:?}", path)));
        }

        fs::read_to_string(&path)
            .map_err(|e| Error::Storage(format!("Failed to read memory file: {}", e)))
    }

    /// Update raw markdown content for a memory file (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn write_raw_markdown(&self, source_type: &str, id: &str, content: &str) -> Result<()> {
        warn!(source_type, id, "write_raw_markdown called (deprecated)");
        let path = match source_type {
            "agent" => self.base_path.join("agents").join(format!("{}.md", id)),
            "chat" => self.base_path.join("chat").join(format!("{}.md", id)),
            _ => self.base_path.join("system.md"),
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        atomic_write::write(&path, content)?;

        // Invalidate cache
        let source = match source_type {
            "agent" => MemorySource::Agent {
                id: id.to_string(),
                name: id.to_string(),
            },
            "chat" => MemorySource::Chat {
                session_id: id.to_string(),
            },
            _ => MemorySource::System,
        };
        {
            let mut cache = self.cache.write();
            cache.remove(&self.cache_key(&source));
        }

        Ok(())
    }

    /// Delete a memory file (DEPRECATED)
    #[deprecated(note = "Legacy API - not recommended for new code")]
    pub fn delete_file(&self, source_type: &str, id: &str) -> Result<()> {
        warn!(source_type, id, "delete_file called (deprecated)");
        let path = match source_type {
            "agent" => self.base_path.join("agents").join(format!("{}.md", id)),
            "chat" => self.base_path.join("chat").join(format!("{}.md", id)),
            _ => return Err(Error::Storage("Cannot delete system memory".to_string())),
        };

        if !path.exists() {
            return Err(Error::Storage(format!("Memory file not found: {:?}", path)));
        }

        fs::remove_file(&path)?;

        // Invalidate cache
        let source = match source_type {
            "agent" => MemorySource::Agent {
                id: id.to_string(),
                name: id.to_string(),
            },
            "chat" => MemorySource::Chat {
                session_id: id.to_string(),
            },
            _ => MemorySource::System,
        };
        {
            let mut cache = self.cache.write();
            cache.remove(&self.cache_key(&source));
        }

        Ok(())
    }

    // ========================================================================
    // Legacy Helper methods
    // ========================================================================

    fn cache_key(&self, source: &MemorySource) -> String {
        match source {
            MemorySource::Agent { id, .. } => format!("agent:{}", id),
            MemorySource::Chat { session_id } => format!("chat:{}", session_id),
            MemorySource::System => "system".to_string(),
        }
    }

    fn create_empty_markdown(&self, source: &MemorySource) -> String {
        match source {
            MemorySource::Agent { name, .. } => format!(
                "# {} Memory\n\n## User Profile\n\n## Domain Knowledge\n\n## Task Patterns\n\n## System Evolution\n",
                name
            ),
            MemorySource::Chat { session_id } => format!(
                "# Chat {} Memory\n\n## User Profile\n\n## Domain Knowledge\n\n## Task Patterns\n\n## System Evolution\n",
                session_id
            ),
            MemorySource::System => {
                "# System Memory\n\n## User Profile\n\n## Domain Knowledge\n\n## Task Patterns\n\n## System Evolution\n"
                    .to_string()
            }
        }
    }

    fn parse_markdown(&self, content: &str, source: &MemorySource) -> Vec<MemoryEntry> {
        let mut entries = Vec::new();
        let mut current_category: Option<MemoryCategory> = None;

        for line in content.lines() {
            let line = line.trim();

            // Check for section headers
            if let Some(section) = line.strip_prefix("## ") {
                let section = section.trim();
                current_category = MemoryCategory::parse_category(section);
                continue;
            }

            // Parse list items
            if line.starts_with('-') {
                if let Some(category) = current_category {
                    if let Some(entry) = MemoryEntry::from_markdown(line, category, source.clone())
                    {
                        entries.push(entry);
                    }
                }
            }
        }

        entries
    }
}

impl Default for MarkdownMemoryStore {
    fn default() -> Self {
        Self::new(neomind_core::paths::data_dir().join("memory"))
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Replace a section in markdown content by heading name.
///
/// Finds "## {heading}" and replaces content until next "## " heading.
/// If heading not found, prepends the section.
fn replace_section_in_content(content: &str, heading: &str, new_body: &str) -> String {
    let section_start = format!("## {}", heading);
    let lines: Vec<&str> = content.lines().collect();

    // Find start line (## {heading})
    let start_idx = lines.iter().position(|l| *l == section_start);

    if let Some(start) = start_idx {
        // Find end line (next ## heading or EOF)
        let end = lines[start + 1..]
            .iter()
            .position(|l| l.starts_with("## "))
            .map(|i| start + 1 + i)
            .unwrap_or(lines.len());

        // Build new content
        let mut result = lines[..start].to_vec();
        result.push(section_start.as_str());
        result.push("");
        for body_line in new_body.lines() {
            result.push(body_line);
        }
        result.extend_from_slice(&lines[end..]);

        result.join("\n")
    } else {
        // Heading not found, prepend
        let mut result = vec![section_start.as_str(), ""];
        for body_line in new_body.lines() {
            result.push(body_line);
        }
        result.push("");
        result.extend_from_slice(&lines);
        result.join("\n")
    }
}

/// Evict content to `max_chars`, treating `<!-- ... -->` marker blocks as
/// atomic, untouchable units (see `replace_marker_section` for why those
/// blocks must survive: they are the background jobs' self-replacing
/// sections, and dropping half of one corrupts the next regeneration).
///
/// Ordinary lines are evicted from the BOTTOM — the same end-of-file
/// One parsed marker block (`<!-- name -->` … `<!-- /name -->`, complete
/// pairs only — an unpaired open tag stays an ordinary line).
struct MarkerSegment {
    name: String,
    /// open tag, body lines, close tag — in order.
    lines: Vec<String>,
}

fn marker_open_name(line: &str) -> Option<String> {
    let t = line.trim();
    t.strip_prefix("<!-- ")
        .and_then(|r| r.strip_suffix(" -->"))
        .filter(|n| !n.is_empty() && !n.starts_with('/'))
        .map(str::to_string)
}

/// Marker-aware fact append — the extraction path's merge.
///
/// Placement: fresh facts go after the ordinary lines and BEFORE the marker
/// blocks (markers stay at the tail where `replace_marker_section` expects
/// them). Over budget, in order: (1) marker block BODIES shrink from the
/// last block backwards — tags stay balanced and the background jobs refill
/// the bodies next cycle, so regenerable content gives way first; (2) old
/// ordinary lines evict bottom-first, timestamped log lines ahead of
/// curated ones — never the fresh facts; (3) a line-boundary truncation as
/// the last-resort guard (callers cap per-fact length).
///
/// Exact-normalized duplicates against existing bullets are dropped and
/// not counted fresh. Returns (content, fresh_count); with nothing fresh
/// the content comes back UNCHANGED so the caller skips the write.
fn append_facts_transform(current: &str, facts: &[String], limit: usize) -> (String, usize) {
    let norm = |s: &str| -> String { s.to_lowercase().split_whitespace().collect::<String>() };
    fn bullet_text(l: &str) -> Option<&str> {
        let t = l.trim().trim_start_matches("- ").trim_start_matches("• ");
        (!t.is_empty()).then_some(t)
    }
    let existing_norms: std::collections::HashSet<String> = current
        .lines()
        .filter_map(|l| bullet_text(l).map(&norm))
        .collect();
    let fresh: Vec<&String> = facts
        .iter()
        .filter(|f| !existing_norms.contains(&norm(f)))
        .collect();
    if fresh.is_empty() {
        return (current.to_string(), 0);
    }

    // Parse: ordinary lines (+fresh flags) and complete marker blocks.
    let mut lines: Vec<(String, bool)> = Vec::new(); // (line, is_fresh)
    let mut markers: Vec<MarkerSegment> = Vec::new();
    let mut pending: Option<MarkerSegment> = None;
    for line in current.lines() {
        if let Some(name) = marker_open_name(line) {
            pending = Some(MarkerSegment {
                name,
                lines: vec![line.to_string()],
            });
            continue;
        }
        if let Some(seg) = pending.as_mut() {
            seg.lines.push(line.to_string());
            if line.trim() == format!("<!-- /{} -->", seg.name) {
                if let Some(done) = pending.take() {
                    markers.push(done);
                }
            }
            continue;
        }
        lines.push((line.to_string(), false));
    }
    // Unpaired open tag at EOF: keep as ordinary lines (corrupt input —
    // the sanitizer owns repairing it; we never invent a close tag).
    if let Some(seg) = pending.take() {
        for l in seg.lines {
            lines.push((l, false));
        }
    }

    let fresh_block: Vec<(String, bool)> = fresh.iter().map(|f| (format!("- {f}"), true)).collect();
    lines.extend(fresh_block);

    fn compose(lines: &[(String, bool)], markers: &mut [MarkerSegment]) -> String {
        let mut text = String::new();
        let mut first = true;
        for (l, _) in lines {
            if !first {
                text.push('\n');
            }
            first = false;
            text.push_str(l);
        }
        for seg in markers.iter_mut() {
            if !first {
                text.push('\n');
            }
            first = false;
            for l in &seg.lines {
                text.push_str(l);
            }
        }
        text
    }

    let mut text = compose(&lines, &mut markers);

    // Phase 1: shrink marker BODIES, last block first (tags survive, the
    // background job refills the body next cycle).
    while text.chars().count() > limit {
        let mut shrunk = false;
        for seg in markers.iter_mut().rev() {
            if seg.lines.len() > 2 {
                seg.lines.remove(seg.lines.len() - 2);
                shrunk = true;
                break;
            }
        }
        if !shrunk {
            break;
        }
        text = compose(&lines, &mut markers);
    }

    // Phase 2: evict old ordinary lines bottom-first — timestamped log
    // lines ahead of curated ones — never the fresh facts.
    let is_log = |l: &str| l.trim_start().starts_with("- [");
    while text.chars().count() > limit {
        let idx = (0..lines.len())
            .rev()
            .find(|&i| !lines[i].1 && is_log(&lines[i].0))
            .or_else(|| (0..lines.len()).rev().find(|&i| !lines[i].1));
        let Some(i) = idx else { break };
        lines.remove(i);
        text = compose(&lines, &mut markers);
    }

    // Phase 3 guard: line-boundary truncation (keeps marker tags balanced).
    if text.chars().count() > limit {
        let mut acc = 0usize;
        let mut cut = 1usize;
        for (i, (l, _)) in lines.iter().enumerate() {
            let add = l.chars().count() + usize::from(i > 0);
            if acc + add > limit {
                break;
            }
            acc += add;
            cut = i + 1;
        }
        lines.truncate(cut);
        text = compose(&lines, &mut markers);
        if text.chars().count() > limit {
            text = text.chars().take(limit).collect();
        }
    }

    if text.chars().count() < limit && !text.ends_with('\n') {
        text.push('\n');
    }
    (text, fresh.len())
}

/// policy the compressor's `evict_to_limit` uses — so the oldest tail
/// content goes first. If the file is over budget with markers alone,
/// hard-truncate on a char boundary and warn: the snapshot layer caps
/// the prompt independently, but the file must not grow without bound.
fn evict_to_limit_preserving_markers(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        // Same trailing-newline normalization the eviction path applies.
        if content.is_empty() {
            return String::new();
        }
        if content.ends_with('\n') {
            return content.to_string();
        }
        // At exactly the limit there is no room for the newline — appending
        // it returned limit+1 chars, and replace_marker_section wrote that
        // to disk verbatim, breaking the "file never exceeds its limit"
        // invariant (the write_file mirror then rejected the same length).
        if content.chars().count() < max_chars {
            return format!("{}\n", content);
        }
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();

    // Mark protected line ranges. A marker block runs from `<!-- name -->`
    // to the matching `<!-- /name -->` — both tags are SINGLE-LINE comments
    // (they contain `-->`), so the body between them is only recognizable
    // via the pair, not by comment-state scanning.
    let mut protected = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(name) = trimmed
            .strip_prefix("<!-- ")
            .and_then(|r| r.strip_suffix(" -->"))
        {
            if !name.is_empty() && !name.starts_with('/') {
                let close_tag = format!("<!-- /{} -->", name);
                if let Some(j) = lines[i + 1..]
                    .iter()
                    .position(|l| l.trim() == close_tag)
                    .map(|k| i + 1 + k)
                {
                    for p in &mut protected[i..=j] {
                        *p = true;
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    let char_count = |alive: &[bool]| -> usize {
        lines
            .iter()
            .zip(alive)
            .filter(|(_, a)| **a)
            .map(|(l, _)| l.chars().count() + 1)
            .sum::<usize>()
            .saturating_sub(1)
    };

    // Eviction priority: timestamped log-style lines ("- [MM-DD HH:MM] …",
    // what the memory tool's timestamped writes produce) go FIRST — they
    // are auto-generated observations, not curated content; among them,
    // bottom-first preserves append order (older entries sit lower in each
    // batch). Only when no timestamped line remains do curated non-marker
    // lines start going, still bottom-first so the hand-maintained top of
    // the file survives longest.
    let is_log_line = |l: &str| l.trim_start().starts_with("- [");
    let mut alive: Vec<bool> = vec![true; lines.len()];

    // Markers that can NEVER fit (their own chars alone ≥ the budget)
    // must go BEFORE any plain line burns: evicting user content first and
    // dropping markers afterwards destroyed facts for nothing (markers are
    // regenerable; facts are not).
    let protected_only = |alive: &[bool]| -> usize {
        lines
            .iter()
            .zip(alive)
            .zip(&protected)
            .filter(|((_, a), p)| **a && **p)
            .map(|((l, _), _)| l.chars().count() + 1)
            .sum::<usize>()
            .saturating_sub(1)
    };
    while char_count(&alive) > max_chars && protected_only(&alive) >= max_chars {
        let last_protected = (0..lines.len()).rev().find(|&k| alive[k] && protected[k]);
        let Some(end) = last_protected else { break };
        let mut start = end;
        while start > 0 && alive[start - 1] && protected[start - 1] {
            start -= 1;
        }
        for k in start..=end {
            alive[k] = false;
        }
    }

    while char_count(&alive) > max_chars {
        let next = (0..lines.len())
            .rev()
            .find(|&k| alive[k] && !protected[k] && is_log_line(lines[k]))
            .or_else(|| (0..lines.len()).rev().find(|&k| alive[k] && !protected[k]));
        let Some(pos) = next else { break };
        alive[pos] = false;
    }

    // Still over budget with protected markers alive: drop WHOLE trailing
    // marker blocks before ever hard-truncating. The old take(max_chars)
    // could slice through an open/close tag — the orphan tag then sent the
    // NEXT replace_marker_section down its "recover by deleting everything
    // after the open tag" path, destroying user content wholesale.
    // Marker blocks are regenerable (background jobs re-append them), so
    // losing one beats corrupting the structure.
    while char_count(&alive) > max_chars {
        let last_protected = (0..lines.len()).rev().find(|&k| alive[k] && protected[k]);
        let Some(end) = last_protected else { break };
        // Walk back to the start of this contiguous protected run.
        let mut start = end;
        while start > 0 && alive[start - 1] && protected[start - 1] {
            start -= 1;
        }
        for k in start..=end {
            alive[k] = false;
        }
    }

    let mut out = lines
        .iter()
        .zip(&alive)
        .filter(|(_, a)| **a)
        .map(|(l, _)| *l)
        .collect::<Vec<_>>()
        .join("\n");

    // Trailing newline only when it fits: the old unconditional push made
    // an exactly-at-limit result limit+1, and the guard's take() then ate
    // the newline while warning about "markers exceeding the budget" —
    // misleading on both counts.
    if out.chars().count() < max_chars {
        out.push('\n');
    } else if out.chars().count() > max_chars {
        tracing::warn!(
            chars = out.chars().count(),
            max_chars,
            "Memory file over limit even after eviction — hard-truncating"
        );
        out = out.chars().take(max_chars).collect();
    }
    out
}

// ============================================================================
// New API tests
// ============================================================================

#[cfg(test)]
mod tests_new_layout {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (TempDir, MarkdownMemoryStore) {
        let temp_dir = TempDir::new().unwrap();
        let config = MemoryConfig {
            enabled: true,
            storage_path: temp_dir.path().to_str().unwrap().to_string(),
            user_char_limit: 2000,
            knowledge_char_limit: 3000,
            procedures_char_limit: 3000,
            agent_char_limit: 5000,
            temp_file_ttl_days: 7,
            system_context_interval_secs: 600,
            summary_interval_secs: 7200,
            summary_backend_id: None,
        };
        let store = MarkdownMemoryStore::with_config(temp_dir.path(), config);
        store.init().unwrap();
        (temp_dir, store)
    }

    /// Boundary: a transform result EXACTLY at the limit is accepted;
    /// one char over is rejected with the file untouched.
    #[tokio::test]
    async fn test_modify_file_exact_limit_boundary() {
        let (_temp, store) = create_test_store().await;
        let limit = 2000; // user_char_limit from the harness config
        let exact: String = "a".repeat(limit);
        store.write_file("user", &exact).await.unwrap();
        let changed = store
            .modify_file("user", |cur| {
                assert_eq!(cur.chars().count(), limit);
                cur.to_string() // unchanged at exactly the limit
            })
            .await
            .unwrap();
        assert!(!changed);
        let over = "a".repeat(limit + 1);
        assert!(
            store.modify_file("user", |_| over.clone()).await.is_err(),
            "one char over the limit must be rejected"
        );
        // file untouched by the rejected attempt
        assert_eq!(
            store.read_file("user").await.unwrap().chars().count(),
            limit
        );
    }

    #[tokio::test]
    async fn test_read_write_user() {
        let (_temp, store) = create_test_store().await;

        // Write user content
        let content = "# User Profile\n\n- Name: Test User\n- Language: en\n";
        store.write_file("user", content).await.unwrap();

        // Read it back
        let read = store.read_file("user").await.unwrap();
        assert_eq!(read, content);
    }

    #[tokio::test]
    async fn test_char_limit_enforced() {
        let (_temp, store) = create_test_store().await;

        // Try to write content exceeding user limit
        let long_content = "x".repeat(2001);
        let result = store.write_file("user", &long_content).await;
        assert!(result.is_err());

        // Write within limit should succeed
        let ok_content = "x".repeat(2000);
        store.write_file("user", &ok_content).await.unwrap();
    }

    /// `modify_file` is the atomic read-modify-write the memory tool's
    /// add/replace/remove actions need: the transform sees the CURRENT
    /// content under the write lock, an unchanged result writes nothing,
    /// and the char limit rejects growth the same way `write_file` does.
    #[tokio::test]
    async fn test_modify_file_semantics() {
        let (_temp, store) = create_test_store().await;

        store.write_file("user", "# User\n\nline-a").await.unwrap();

        // Transform sees current content and appends atomically.
        let changed = store
            .modify_file("user", |cur| {
                assert!(cur.contains("line-a"), "transform must see current content");
                format!("{}\nline-b", cur)
            })
            .await
            .unwrap();
        assert!(changed);
        let after = store.read_file("user").await.unwrap();
        assert!(after.contains("line-b") && after.contains("line-a"));

        // Unchanged result → no write, Ok(false).
        let unchanged = store
            .modify_file("user", |cur| cur.to_string())
            .await
            .unwrap();
        assert!(!unchanged);

        // Limit enforced on the transformed result.
        let err = store
            .modify_file("user", |cur| format!("{}\n{}", cur, "y".repeat(2100)))
            .await;
        assert!(err.is_err(), "over-limit transform must be rejected");
        // and the file is untouched by the failed attempt
        assert_eq!(store.read_file("user").await.unwrap(), after);
    }

    /// Clones share the write lock: an RMW on a clone is serialized against
    /// an RMW on the canonical store (the chat toolchain and the agent
    /// toolchain each hold a clone). The observable contract here is that
    /// both clones always see each other's committed state — with derived
    /// Clone (fresh locks per instance) the interleaving below could lose
    /// an update.
    #[tokio::test]
    async fn test_clones_share_write_lock_state() {
        let (_temp, store) = create_test_store().await;
        let clone = store.clone();

        store.write_file("user", "base").await.unwrap();

        // Interleaved appends through both handles; with per-instance locks
        // the read in one modify could miss the other's in-flight write.
        for i in 0..10 {
            let via_clone = i % 2 == 0;
            let handle = if via_clone { &clone } else { &store };
            handle
                .modify_file("user", |cur| format!("{}\n{}", cur, i))
                .await
                .unwrap();
        }

        let final_content = store.read_file("user").await.unwrap();
        for i in 0..10 {
            assert!(
                final_content.contains(&format!("\n{}", i))
                    || final_content.ends_with(&i.to_string()),
                "line {} lost — clones are not serialized: {}",
                i,
                final_content
            );
        }
        // clone sees the same file
        assert_eq!(clone.read_file("user").await.unwrap(), final_content);
    }

    /// `modify_custom_file`: atomic RMW semantics + agent-scope + limit.
    #[tokio::test]
    async fn test_modify_custom_file() {
        let (_temp, store) = create_test_store().await;

        // Global scope: creates on demand.
        let changed = store
            .modify_custom_file(None, "patterns", |cur| {
                assert!(cur.is_empty());
                "## Patterns\n- temp normal".to_string()
            })
            .await
            .unwrap();
        assert!(changed);
        assert_eq!(
            store.read_custom_file("patterns").unwrap(),
            "## Patterns\n- temp normal"
        );

        // Agent scope lands in agents/{id}/custom/.
        store
            .modify_custom_file(Some("agent-1"), "notes", |_| "first".to_string())
            .await
            .unwrap();
        let merged = store
            .modify_custom_file(Some("agent-1"), "notes", |cur| format!("{}\nsecond", cur))
            .await
            .unwrap();
        assert!(merged);
        assert_eq!(
            store.read_agent_custom_file("agent-1", "notes").unwrap(),
            "first\nsecond"
        );

        // Unchanged → Ok(false). Over-limit → error, file untouched.
        assert!(!store
            .modify_custom_file(Some("agent-1"), "notes", |cur| cur.to_string())
            .await
            .unwrap());
        let before = store.read_agent_custom_file("agent-1", "notes").unwrap();
        assert!(store
            .modify_custom_file(Some("agent-1"), "notes", |cur| format!(
                "{}{}",
                cur,
                "z".repeat(21_000)
            ))
            .await
            .is_err());
        assert_eq!(
            store.read_agent_custom_file("agent-1", "notes").unwrap(),
            before
        );
    }

    /// Agent IDs are path segments in `agents/{id}/...` joins — same
    /// traversal standard as session IDs and custom names.
    #[tokio::test]
    async fn test_agent_id_validation_rejects_traversal() {
        let (_temp, store) = create_test_store().await;
        let long = "x".repeat(129);
        for bad in ["../escape", "a/b", "a\\b", "", long.as_str()] {
            assert!(
                store.read_agent_custom_file(bad, "name").is_err(),
                "accepted {bad:?}"
            );
            assert!(
                store.clean_agent_dir(bad).is_err(),
                "clean accepted {bad:?}"
            );
        }
        assert!(store.read_agent_custom_file("normal-id", "name").is_ok());
    }

    /// Background marker writes self-heal past the char limit by evicting
    /// ordinary bottom lines — never marker blocks, which the jobs
    /// regenerate and which would corrupt if half-dropped.
    #[tokio::test]
    async fn test_marker_replace_self_heals_over_limit() {
        let temp_dir = TempDir::new().unwrap();
        // Small knowledge budget so the test content overflows it easily.
        let config = MemoryConfig {
            knowledge_char_limit: 220,
            ..Default::default()
        };
        let store = MarkdownMemoryStore::with_config(temp_dir.path().to_path_buf(), config);
        store.init().unwrap();

        // Fill KNOWLEDGE.md near the limit with ordinary content.
        let base = "# System Knowledge\n\n".to_string() + &"k".repeat(180);
        store.write_file("knowledge", &base).await.unwrap();

        // A marker section pushing the total past the limit.
        let marker_body = "m".repeat(80);
        store
            .replace_marker_section("knowledge", "agent-summary", &marker_body)
            .await
            .unwrap();

        let final_content = store.read_file("knowledge").await.unwrap();
        assert!(
            final_content.chars().count() <= 220,
            "self-heal must land within the limit, got {}",
            final_content.chars().count()
        );
        assert!(
            final_content.contains("<!-- agent-summary -->"),
            "marker block must survive eviction"
        );
        assert!(
            final_content.contains(&marker_body),
            "the fresh marker content must survive"
        );
        assert!(
            final_content.chars().count() < base.chars().count() + marker_body.chars().count(),
            "some ordinary content must have been evicted"
        );
    }

    /// Log-style timestamped lines are evicted before curated prose at the
    /// same position class — auto-observations are cheaper to lose than
    /// hand-maintained content.
    #[test]
    fn test_evict_prefers_timestamped_lines_over_curated() {
        // 3 curated lines + 6 log lines + a marker block; budget forces
        // dropping several entries.
        let mut content = String::from("curated-top\ncurated-mid\ncurated-bottom\n");
        for i in 0..6 {
            content.push_str(&format!("- [09-30 0{}:00] observation number {}\n", i, i));
        }
        content.push_str("\n<!-- keep -->\nblock body\n<!-- /keep -->\n");
        // Budget fits the marker block + all curated lines + ~3 log lines.
        let out = evict_to_limit_preserving_markers(&content, 120);
        assert!(out.chars().count() <= 120);
        assert!(
            out.contains("curated-top") && out.contains("curated-bottom"),
            "curated lines must outlive log lines: {out}"
        );
        assert!(
            out.matches("- [09-30").count() < 6,
            "log lines must be evicted first: {out}"
        );
        assert!(out.contains("<!-- keep -->"), "markers stay atomic");
    }

    /// Boundary: content EXACTLY at the limit is untouched; one char over
    /// evicts the cheapest line (not the marker, not curated prose).
    #[test]
    fn evict_boundary_exact_limit_untouched() {
        let content = "line\n"; // 5 chars
        assert_eq!(
            evict_to_limit_preserving_markers(content, 5)
                .chars()
                .count(),
            5
        );
        let over = "line\nlong tail here\n";
        let out = evict_to_limit_preserving_markers(over, 10);
        assert!(out.chars().count() <= 10);
        assert!(
            out.contains("line"),
            "first line survives when cheaper lines exist"
        );
    }

    /// Unit level: pure eviction semantics with and without markers.
    #[test]
    fn test_evict_preserving_markers_unit() {
        let content = "head\n- old1\n- old2\n\n<!-- keep -->\nblock body\n<!-- /keep -->\n";
        // Budget must fit the marker block alone (~40 chars incl. tags and
        // body) plus the surviving head — otherwise the fallback hard-truncates.
        let out = evict_to_limit_preserving_markers(content, 45);
        assert!(out.chars().count() <= 45);
        assert!(out.contains("head"), "top content survives");
        assert!(out.contains("<!-- keep -->") && out.contains("<!-- /keep -->"));
        assert!(out.contains("block body"), "marker body is atomic");
        assert!(!out.contains("old2"), "bottom ordinary lines evicted first");

        // Markers alone over budget → hard truncate on a char boundary, no panic.
        let huge = format!("<!-- m -->\n{}\n<!-- /m -->", "q".repeat(500));
        let out = evict_to_limit_preserving_markers(&huge, 50);
        assert!(out.chars().count() <= 50);

        // Under limit → unchanged (single trailing newline normalized).
        let small = "a\nb";
        let out = evict_to_limit_preserving_markers(small, 100);
        assert_eq!(out, "a\nb\n");
    }

    #[tokio::test]
    async fn test_session_temp_file() {
        let (_temp, store) = create_test_store().await;

        // Write session files
        let session_id = "test-session-123";
        store
            .write_session_file(session_id, "notes", "Test notes")
            .await
            .unwrap();
        store
            .write_session_file(session_id, "scratch", "Scratch content")
            .await
            .unwrap();

        // Read them back
        let notes = store.read_session_file(session_id, "notes").await.unwrap();
        assert_eq!(notes, "Test notes");

        let scratch = store
            .read_session_file(session_id, "scratch")
            .await
            .unwrap();
        assert_eq!(scratch, "Scratch content");

        // Non-existent file should return empty string
        let todo = store.read_session_file(session_id, "todo").await.unwrap();
        assert_eq!(todo, "");
    }

    #[tokio::test]
    async fn test_section_replace() {
        let (_temp, store) = create_test_store().await;

        // Initial KNOWLEDGE.md should have default structure
        let initial = store.read_file("knowledge").await.unwrap();
        assert!(initial.contains("## System Resources"));

        // Replace Domain Knowledge section
        let new_body = "- Device: TestDevice\n- Location: TestRoom\n";
        store
            .replace_section("knowledge", "Domain Knowledge", new_body)
            .await
            .unwrap();

        // Verify replacement
        let updated = store.read_file("knowledge").await.unwrap();
        assert!(updated.contains("## Domain Knowledge"));
        assert!(updated.contains("TestDevice"));

        // Replace should preserve other sections
        assert!(updated.contains("## System Resources"));
        assert!(updated.contains("## Agent Experiences"));
    }

    #[tokio::test]
    async fn test_section_replace_inserts_if_missing() {
        let (_temp, store) = create_test_store().await;

        // Write KNOWLEDGE.md without Custom Section
        let minimal = "# System Knowledge\n\n## System Resources\n\nContent\n";
        store.write_file("knowledge", minimal).await.unwrap();

        // Replace non-existent section should prepend it
        store
            .replace_section("knowledge", "Custom Section", "Custom content")
            .await
            .unwrap();

        let updated = store.read_file("knowledge").await.unwrap();
        assert!(updated.contains("## Custom Section"));
        assert!(updated.contains("Custom content"));
        // Check that it's prepended (before System Resources)
        let custom_idx = updated.find("## Custom Section").unwrap();
        let resources_idx = updated.find("## System Resources").unwrap();
        assert!(custom_idx < resources_idx);
    }

    #[tokio::test]
    async fn test_stats() {
        let (_temp, store) = create_test_store().await;

        // Write some content
        store.write_file("user", "# User\n\nTest").await.unwrap();
        store
            .write_file("knowledge", "# Knowledge\n\nContent")
            .await
            .unwrap();

        // Create a session directory
        store
            .write_session_file("session-1", "notes", "Notes")
            .await
            .unwrap();

        // Get stats
        let stats = store.stats().await.unwrap();

        // The write_file method creates "# User Profile\n\n> Last updated: \n\n" initially
        // then our content overwrites it, but we need to account for what was actually written
        assert!(stats.user.chars >= 12); // At least "# User\n\nTest"
        assert_eq!(stats.user.limit, 2000);
        assert!(stats.knowledge.chars >= 20); // At least "# Knowledge\n\nContent"
        assert_eq!(stats.knowledge.limit, 3000);
        assert_eq!(stats.sessions.active_count, 1);
        assert_eq!(stats.sessions.total_temp_files, 1);
    }

    #[tokio::test]
    async fn test_cleanup_old_sessions() {
        let (_temp, store) = create_test_store().await;

        // Create multiple sessions
        store
            .write_session_file("old-session-1", "notes", "Old notes 1")
            .await
            .unwrap();
        store
            .write_session_file("old-session-2", "scratch", "Scratch 2")
            .await
            .unwrap();
        store
            .write_session_file("old-session-3", "todo", "Todo 3")
            .await
            .unwrap();

        // Create a new session (should not be deleted)
        store
            .write_session_file("new-session", "notes", "New notes")
            .await
            .unwrap();

        // Manually set modification times for old sessions to 10 days ago
        let sessions_path = store.base_path.join("sessions");
        let ten_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(864000);

        for name in &["old-session-1", "old-session-2", "old-session-3"] {
            let session_path = sessions_path.join(name);
            if session_path.exists() {
                // Set the directory modification time
                let file = std::fs::File::open(&session_path).unwrap();
                file.set_modified(ten_days_ago).ok();

                // Also set modification time for files inside
                for entry in std::fs::read_dir(&session_path).unwrap() {
                    let entry = entry.unwrap();
                    let file = std::fs::File::open(entry.path()).unwrap();
                    file.set_modified(ten_days_ago).ok();
                }
            }
        }

        // Cleanup with TTL 7 days
        let deleted = store.cleanup_old_sessions(7).await.unwrap();

        // Should delete 3 old sessions
        assert_eq!(deleted, 3);

        // Old session directories should be gone
        assert!(!sessions_path.join("old-session-1").exists());
        assert!(!sessions_path.join("old-session-2").exists());
        assert!(!sessions_path.join("old-session-3").exists());

        // New session should still exist
        assert!(sessions_path.join("new-session").exists());
    }
}

// ============================================================================
// Legacy API tests (kept for backward compatibility)
// ============================================================================

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    // These tests intentionally exercise the legacy MarkdownMemoryStore API
    // (append/read/write_category/aggregate_all/prune/list_files/
    // read_raw_markdown/write_raw_markdown/delete_file/export_all) to verify
    // backwards compatibility. The new API (write_file/stats) is covered by
    // tests_new_layout above.
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry::new(
            "User prefers dark mode",
            MemoryCategory::UserProfile,
            MemorySource::System,
        )
        .with_importance(80);

        assert_eq!(entry.content, "User prefers dark mode");
        assert_eq!(entry.category, MemoryCategory::UserProfile);
        assert_eq!(entry.importance, 80);
    }

    #[test]
    fn test_memory_entry_markdown() {
        let entry = MemoryEntry::new(
            "Test content",
            MemoryCategory::TaskPatterns,
            MemorySource::System,
        )
        .with_importance(75);

        let md = entry.to_markdown();
        assert!(md.contains("Test content"));
        assert!(md.contains("75"));
    }

    #[test]
    fn test_memory_category_parsing() {
        // New names
        assert_eq!(
            MemoryCategory::parse_category("user_profile"),
            Some(MemoryCategory::UserProfile)
        );
        assert_eq!(
            MemoryCategory::parse_category("domain_knowledge"),
            Some(MemoryCategory::DomainKnowledge)
        );
        // Legacy aliases
        assert_eq!(
            MemoryCategory::parse_category("pattern"),
            Some(MemoryCategory::TaskPatterns)
        );
        assert_eq!(
            MemoryCategory::parse_category("PREFERENCES"),
            Some(MemoryCategory::UserProfile)
        );
        assert_eq!(MemoryCategory::parse_category("invalid"), None);
    }

    #[test]
    fn test_new_memory_categories() {
        assert_eq!(MemoryCategory::UserProfile.filename(), "user_profile.md");
        assert_eq!(
            MemoryCategory::DomainKnowledge.filename(),
            "domain_knowledge.md"
        );
        assert_eq!(MemoryCategory::TaskPatterns.filename(), "task_patterns.md");
        assert_eq!(
            MemoryCategory::SystemEvolution.filename(),
            "system_evolution.md"
        );

        assert_eq!(MemoryCategory::UserProfile.max_entries(), 50);
        assert_eq!(MemoryCategory::DomainKnowledge.max_entries(), 100);
        assert_eq!(MemoryCategory::TaskPatterns.max_entries(), 80);
        assert_eq!(MemoryCategory::SystemEvolution.max_entries(), 30);

        assert_eq!(MemoryCategory::all().len(), 4);
    }

    #[test]
    fn test_memory_store_basic() {
        let temp_dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(temp_dir.path());
        store.init().unwrap();

        let entry = MemoryEntry::new(
            "Test pattern",
            MemoryCategory::TaskPatterns,
            MemorySource::System,
        );

        store.append(&MemorySource::System, &entry).unwrap();

        let entries = store.read(&MemorySource::System).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Test pattern");
    }

    #[test]
    fn test_memory_store_aggregation() {
        let temp_dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(temp_dir.path());
        store.init().unwrap();

        // Add system memory
        store
            .append(
                &MemorySource::System,
                &MemoryEntry::new(
                    "Global fact",
                    MemoryCategory::DomainKnowledge,
                    MemorySource::System,
                ),
            )
            .unwrap();

        // Add agent memory
        let agent_source = MemorySource::Agent {
            id: "agent_1".to_string(),
            name: "TestAgent".to_string(),
        };
        store
            .append(
                &agent_source,
                &MemoryEntry::new(
                    "Agent pattern",
                    MemoryCategory::TaskPatterns,
                    agent_source.clone(),
                ),
            )
            .unwrap();

        // Aggregate
        let all = store.aggregate_all().unwrap();
        assert_eq!(all.total, 2);
        assert_eq!(all.by_source.len(), 2);
    }

    #[test]
    fn test_memory_pruning() {
        let temp_dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(temp_dir.path());
        store.init().unwrap();

        let source = MemorySource::System;

        // Add more entries than max
        for i in 0..20 {
            let entry = MemoryEntry::new(
                format!("Entry {}", i),
                MemoryCategory::TaskPatterns,
                source.clone(),
            )
            .with_importance(50 + i); // Higher importance for higher numbers
            store.append(&source, &entry).unwrap();
        }

        // Prune to 10 entries
        let removed = store.prune(&source, 10).unwrap();
        assert_eq!(removed, 10);

        // Verify only high-importance entries remain
        let entries = store.read(&source).unwrap();
        assert_eq!(entries.len(), 10);
        // All remaining entries should have importance >= 60
        assert!(entries.iter().all(|e| e.importance >= 60));
    }

    #[test]
    fn test_export_import() {
        let temp_dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(temp_dir.path());
        store.init().unwrap();

        // Add some entries to category files (new API location)
        let content = "# Domain Knowledge\n\n- Test fact\n";
        store
            .write_category(&MemoryCategory::DomainKnowledge, content)
            .unwrap();

        // Export
        let exported = store.export_all().unwrap();
        assert!(exported.contains("Test fact"));
        assert!(exported.contains("NeoMind Memory Export"));
    }

    #[test]
    fn test_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let store = MarkdownMemoryStore::new(temp_dir.path());
        store.init().unwrap();

        // List files
        let files = store.list_files().unwrap();
        assert!(!files.is_empty());

        // Read raw markdown
        let content = store.read_raw_markdown("system", "system").unwrap();
        assert!(content.contains("System Memory"));

        // Write raw markdown
        let new_content = "# Test\n\nContent";
        store
            .write_raw_markdown("system", "system", new_content)
            .unwrap();
        let read_back = store.read_raw_markdown("system", "system").unwrap();
        assert_eq!(read_back, new_content);

        // Delete agent file
        store
            .write_raw_markdown("agent", "test_agent", "Test")
            .unwrap();
        let result = store.delete_file("agent", "test_agent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_replace_section_helper() {
        let content = "## Section1\n\nOld1\n\n## Section2\n\nOld2\n";
        let result = replace_section_in_content(content, "Section1", "New1\nLine2");
        assert!(result.contains("New1"));
        assert!(result.contains("Line2"));
        assert!(!result.contains("Old1"));

        // Replace non-existent section should prepend
        let result = replace_section_in_content(content, "NewSection", "NewContent");
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "## NewSection");
        assert!(lines.contains(&"NewContent"));
    }
}

/// Blocking core of [`MarkdownMemoryStore::replace_marker_section`] — runs
/// on the blocking pool while the caller holds the async write lock.
fn replace_marker_section_locked(
    path: &std::path::Path,
    target: &str,
    marker: &str,
    new_content: &str,
    limit: usize,
) -> Result<bool> {
    let current = if path.exists() {
        fs::read_to_string(path)
            .map_err(|e| Error::Storage(format!("Failed to read {}.md: {}", target, e)))?
    } else {
        String::new()
    };

    let original = current.clone();
    let open_tag = format!("<!-- {} -->", marker);
    let close_tag = format!("<!-- /{} -->", marker);

    let new_text = if let Some(start) = current.find(&open_tag) {
        // Marker exists — replace content between markers
        let after_open = start + open_tag.len();
        if let Some(end) = current[after_open..].find(&close_tag) {
            let close_pos = after_open + end;
            let mut result =
                String::with_capacity(start + new_content.len() + (current.len() - close_pos));
            result.push_str(&current[..after_open]);
            result.push('\n');
            result.push_str(new_content);
            result.push('\n');
            result.push_str(&current[close_pos..]);
            result
        } else {
            // Open tag found but no close tag — recover by replacing everything after open tag
            let mut result =
                String::with_capacity(after_open + new_content.len() + close_tag.len() + 4);
            result.push_str(&current[..after_open]);
            result.push('\n');
            result.push_str(new_content);
            result.push('\n');
            result.push_str(&close_tag);
            result.push('\n');
            result
        }
    } else {
        // No marker at all — append the full section at the end
        let mut result = String::with_capacity(
            current.len() + open_tag.len() + close_tag.len() + new_content.len() + 4,
        );
        result.push_str(&current);
        if !current.ends_with('\n') && !current.is_empty() {
            result.push('\n');
        }
        result.push_str(&open_tag);
        result.push('\n');
        result.push_str(new_content);
        result.push('\n');
        result.push_str(&close_tag);
        result.push('\n');
        result
    };

    if new_text == original {
        return Ok(false);
    }

    // Self-heal past the char limit (see the caller's doc for why eviction,
    // not rejection).
    let new_text = evict_to_limit_preserving_markers(&new_text, limit);

    atomic_write::write(path, &new_text)
        .map_err(|e| Error::Storage(format!("Failed to write {}.md: {}", target, e)))?;

    info!(target = %target, marker = %marker, "Replaced marker section");
    Ok(true)
}

/// Regression (limit+1 write): content exactly at the limit without a
/// trailing newline used to come back as limit+1 — and was written to
/// disk, breaking the file-never-exceeds-limit invariant.
#[test]
fn evict_at_exact_limit_keeps_invariant() {
    let content = "a".repeat(50);
    let out = evict_to_limit_preserving_markers(&content, 50);
    assert!(out.chars().count() <= 50, "got {}", out.chars().count());
    // Under the limit the newline is appended only when it fits.
    let out2 = evict_to_limit_preserving_markers("abc", 50);
    assert!(out2.chars().count() <= 50);
}

/// Every instance over the same memory dir must share one write lock —
/// per-instance locks let extraction (agent side), per-request API
/// handlers, and background marker jobs interleave RMWs on the same
/// files, silently rolling each other back.
#[test]
fn instances_over_same_path_share_locks() {
    let dir = tempfile::tempdir().unwrap();
    let a = MarkdownMemoryStore::new(dir.path());
    let b = MarkdownMemoryStore::new(dir.path());
    assert!(Arc::ptr_eq(&a.write_lock, &b.write_lock));
    assert!(Arc::ptr_eq(&a.custom_write_lock, &b.custom_write_lock));
    let dir2 = tempfile::tempdir().unwrap();
    let c = MarkdownMemoryStore::new(dir2.path());
    assert!(!Arc::ptr_eq(&a.write_lock, &c.write_lock));
}

/// A raised configured limit must govern writes — `new()` used the
/// DEFAULT limit, so a user who raised limits had every extraction
/// write rejected (fit passed at the configured limit, store rejected
/// at the default).
#[tokio::test]
async fn write_honors_configured_limit_not_default() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = MemoryConfig {
        storage_path: dir.path().to_str().unwrap().to_string(),
        user_char_limit: 5000,
        ..Default::default()
    };
    let store = MarkdownMemoryStore::with_config(dir.path(), cfg);
    store.init().unwrap();
    let big = "x".repeat(4500); // way over the default 2000
    store
        .write_file("user", &big)
        .await
        .expect("the CONFIGURED limit must govern writes");
}

// ---- append_facts_transform: marker-aware merge ----

#[test]
fn append_facts_shrinks_marker_bodies_before_facts() {
    let mut current = String::from("# Profile\n- old fact one\n- old fact two");
    current.push_str("\n<!-- chat-summary -->\n");
    for i in 0..20 {
        current.push_str(&format!("summary line {i} that is reasonably long\n"));
    }
    current.push_str("<!-- /chat-summary -->");
    let facts = vec!["brand new fact".to_string()];
    let (out, fresh) = append_facts_transform(&current, &facts, 220);
    assert_eq!(fresh, 1);
    assert!(out.chars().count() <= 220, "limit invariant");
    assert!(out.contains("brand new fact"), "fresh fact must land");
    assert!(out.contains("old fact one"), "facts outrank marker bodies");
    assert!(out.contains("old fact two"));
    // Tags balanced even after body shrink.
    assert!(out.contains("<!-- chat-summary -->"));
    assert!(out.contains("<!-- /chat-summary -->"));
}

#[test]
fn append_facts_go_before_markers() {
    let current = "# Profile\n<!-- chat-summary -->\nbody\n<!-- /chat-summary -->";
    let facts = vec!["likes tea".to_string()];
    let (out, fresh) = append_facts_transform(current, &facts, 500);
    assert_eq!(fresh, 1);
    let fact_pos = out.find("- likes tea").expect("fact present");
    let marker_pos = out.find("<!-- chat-summary -->").expect("marker present");
    assert!(fact_pos < marker_pos, "facts sit before the marker tail");
}

#[test]
fn append_facts_exact_duplicates_are_not_fresh() {
    let current = "- Likes Tea";
    let facts = vec!["likes tea".to_string()];
    let (out, fresh) = append_facts_transform(current, &facts, 100);
    assert_eq!(fresh, 0);
    assert_eq!(out, current, "unchanged content → caller skips the write");
}

#[test]
fn sanitize_drops_whole_marker_blocks_never_half() {
    // Markers alone exceed the budget: whole blocks must go, tags never
    // split (an orphan open tag used to trigger the replace-recovery
    // path that deleted everything after it).
    let mut content = String::from("# Profile\n- fact");
    for name in ["a", "b"] {
        content.push_str(&format!("\n<!-- {name} -->\n"));
        for i in 0..15 {
            content.push_str(&format!("body-{name}-{i} padding padding padding\n"));
        }
        content.push_str(&format!("<!-- /{name} -->"));
    }
    let out = evict_to_limit_preserving_markers(&content, 120);
    assert!(out.chars().count() <= 120, "limit invariant");
    let opens = out.matches("<!-- ").count();
    let closes = out.matches("<!-- /").count();
    assert_eq!(opens, closes, "no orphan tags: {out}");
    assert!(out.contains("- fact"), "facts outlive regenerable markers");
}

// ---- append_facts: on-disk integration (transform ↔ modify_file seam) ----

#[tokio::test]
async fn append_facts_integration_lands_on_disk() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = MemoryConfig {
        storage_path: temp.path().to_str().unwrap().to_string(),
        user_char_limit: 4000,
        ..Default::default()
    };
    let store = MarkdownMemoryStore::with_config(temp.path(), cfg);
    store.init().unwrap();

    // Seed: facts + a marker block (the shape extraction coexists with).
    store
        .write_file(
            "user",
            "# Profile\n- old fact\n<!-- chat-summary -->\nsummary body\n<!-- /chat-summary -->",
        )
        .await
        .unwrap();

    let (wrote, fresh) = store
        .append_facts("user", &["brand new fact".into()])
        .await
        .unwrap();
    assert!(wrote);
    assert_eq!(fresh, 1);
    let on_disk = store.read_file_sync("user");
    assert!(on_disk.contains("- brand new fact"), "fact persisted");
    let fact_at = on_disk.find("- brand new fact").unwrap();
    let marker_at = on_disk.find("<!-- chat-summary -->").unwrap();
    assert!(fact_at < marker_at, "facts before the marker tail");
    assert!(on_disk.contains("<!-- /chat-summary -->"), "tags balanced");

    // Dedup round: exact-normalized repeat writes NOTHING.
    let (wrote2, fresh2) = store
        .append_facts("user", &["Brand   New FACT".into()])
        .await
        .unwrap();
    assert!(!wrote2, "dedup round must not touch the disk");
    assert_eq!(fresh2, 0);
}

// ---- Gap 2: cross-instance concurrency — the lost-update regression.
// Two SEPARATE store instances over the same directory (extraction +
// canonical-server shape) interleave fact appends with marker writes.
// With per-instance locks the read-modify-writes interleaved and facts
// silently vanished; with the per-path registry every append survives.

#[tokio::test]
async fn concurrent_append_and_marker_writes_lose_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let mk = || {
        MarkdownMemoryStore::new(temp.path()) // separate instance = separate struct, shared lock
    };
    mk().init().unwrap();
    mk().write_file("user", "# Profile").await.unwrap();

    const TASKS: usize = 8;
    const FACTS_PER: usize = 5;
    let mut handles = Vec::new();
    for t in 0..TASKS {
        let store = mk();
        handles.push(tokio::spawn(async move {
            for i in 0..FACTS_PER {
                let fact = format!("fact-t{t}-i{i}");
                let (wrote, _) = store.append_facts("user", &[fact]).await.unwrap();
                assert!(wrote, "a fresh fact must always be written");
            }
        }));
    }
    // Marker churn from the "background job" instance, concurrent.
    let marker_store = mk();
    let marker_task = tokio::spawn(async move {
        for round in 0..10 {
            marker_store
                .replace_marker_section("user", "chat-summary", &format!("summary round {round}"))
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    for h in handles {
        h.await.unwrap();
    }
    marker_task.await.unwrap();

    let final_content = mk().read_file_sync("user");
    for t in 0..TASKS {
        for i in 0..FACTS_PER {
            assert!(
                final_content.contains(&format!("fact-t{t}-i{i}")),
                "lost update: fact-t{t}-i{i} missing — the per-path lock regressed"
            );
        }
    }
    let opens = final_content.matches("<!-- chat-summary -->").count();
    let closes = final_content.matches("<!-- /chat-summary -->").count();
    assert_eq!(opens, closes, "marker structure must stay balanced");
}

// ---- Gap 3 regression: instances created BEFORE and AFTER the
// directory exists must still share locks (the canonicalize variant
// split them).

#[test]
fn lock_key_stable_across_directory_creation() {
    let temp = tempfile::tempdir().unwrap();
    let not_yet = temp.path().join("deep/memory");
    let pre = MarkdownMemoryStore::new(&not_yet);
    std::fs::create_dir_all(&not_yet).unwrap();
    let post = MarkdownMemoryStore::new(&not_yet);
    assert!(
        Arc::ptr_eq(&pre.write_lock, &post.write_lock),
        "pre-init and post-init instances must share one lock"
    );
}
