//! Memory management tool for persistent and session-scoped storage.

use async_trait::async_trait;
use neomind_storage::{KnowledgeFileRef, MarkdownMemoryStore};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::memory::dedup::DedupProcessor;

use super::error::{Result, ToolError};
use super::tool::{MemoryToolHandles, Tool, ToolCategory};
use super::ToolOutput;

/// Type alias for the per-execution agent_id handle.
type AgentIdHandle = Arc<RwLock<Option<String>>>;
/// Type alias for the per-execution knowledge_files handle.
type KnowledgeFilesHandle = Arc<RwLock<Vec<KnowledgeFileRef>>>;

/// Tool for managing persistent memory across sessions.
///
/// Supports:
/// - Persistent: USER.md (user profile), KNOWLEDGE.md (domain facts), PROCEDURES.md (SOPs/how-tos)
/// - Agent-scoped custom files: `custom:{name}` → `agents/{agent_id}/custom/{name}.md`
/// - Session: `sessions/{id}/notes.md` (multi-step task tracking, 7-day TTL)
///
/// ## Concurrency safety
/// `agent_id` and `knowledge_files` use **per-execution Arc handles** stored in a
/// `parking_lot::RwLock`. Each execution swaps in its own fresh Arc before the tool
/// loop and reads it back after. Concurrent agent executions never interfere because
/// each swaps its own independent Arc — no shared mutable state between executions.
pub struct MemoryTool {
    store: Arc<RwLock<MarkdownMemoryStore>>,
    session_id: Arc<RwLock<Option<String>>>,
    /// Per-execution agent ID handle — swapped atomically via parking_lot::RwLock.
    agent_id: parking_lot::RwLock<AgentIdHandle>,
    /// Per-execution knowledge file index handle — swapped atomically via parking_lot::RwLock.
    knowledge_files: parking_lot::RwLock<KnowledgeFilesHandle>,
}

impl MemoryTool {
    /// Create a new memory tool (no agent context — global scope).
    pub fn new(store: Arc<RwLock<MarkdownMemoryStore>>) -> Self {
        Self {
            store,
            session_id: Arc::new(RwLock::new(None)),
            agent_id: parking_lot::RwLock::new(Arc::new(RwLock::new(None))),
            knowledge_files: parking_lot::RwLock::new(Arc::new(RwLock::new(Vec::new()))),
        }
    }

    /// Create a new memory tool with a shared session ID handle.
    pub fn with_session_handle(
        store: Arc<RwLock<MarkdownMemoryStore>>,
        session_handle: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            store,
            session_id: session_handle,
            agent_id: parking_lot::RwLock::new(Arc::new(RwLock::new(None))),
            knowledge_files: parking_lot::RwLock::new(Arc::new(RwLock::new(Vec::new()))),
        }
    }

    /// Create a new memory tool with shared handles for all dynamic state.
    /// The agent_id and knowledge_files handles are shared with the executor
    /// so it can inject agent_id per-execution and sync knowledge_files back.
    pub fn with_shared_handles(
        store: Arc<RwLock<MarkdownMemoryStore>>,
        session_handle: Arc<RwLock<Option<String>>>,
        agent_id_handle: AgentIdHandle,
        knowledge_files_handle: KnowledgeFilesHandle,
    ) -> Self {
        Self {
            store,
            session_id: session_handle,
            agent_id: parking_lot::RwLock::new(agent_id_handle),
            knowledge_files: parking_lot::RwLock::new(knowledge_files_handle),
        }
    }

    /// Get a handle to set the agent ID per-execution.
    pub fn agent_id_handle(&self) -> AgentIdHandle {
        self.agent_id.read().clone()
    }

    /// Get a handle to read/modify the knowledge file index.
    /// The executor reads this after tool loop to sync back to AgentMemory.
    pub fn knowledge_files_handle(&self) -> KnowledgeFilesHandle {
        self.knowledge_files.read().clone()
    }

    /// Set the session ID for session-scoped memory operations (chat path).
    /// Called by the Agent at the start of each process cycle to ensure the
    /// correct session ID is active, avoiding the global-handle race where
    /// a concurrent session could overwrite the ID between handler-set and tool-read.
    pub async fn set_session_id(&self, id: String) {
        *self.session_id.write().await = Some(id);
    }

    /// Swap in a fresh per-execution agent_id handle.
    /// Returns the new handle for the executor to use.
    /// This is concurrency-safe: each execution creates its own Arc.
    pub fn swap_agent_id_handle(&self, new_handle: AgentIdHandle) -> AgentIdHandle {
        let mut guard = self.agent_id.write();
        *guard = new_handle;
        guard.clone()
    }

    /// Swap in a fresh per-execution knowledge_files handle.
    /// Returns the new handle for the executor to use.
    pub fn swap_knowledge_files_handle(
        &self,
        new_handle: KnowledgeFilesHandle,
    ) -> KnowledgeFilesHandle {
        let mut guard = self.knowledge_files.write();
        *guard = new_handle;
        guard.clone()
    }

    /// Get preview text (first 50 chars).
    fn get_preview(content: &str) -> String {
        content.chars().take(50).collect::<String>()
    }

    /// Append content to existing content.
    fn append_content(existing: &str, new_content: &str) -> String {
        if existing.is_empty() {
            new_content.to_string()
        } else if existing.ends_with('\n') {
            format!("{}{}", existing, new_content)
        } else {
            format!("{}\n{}", existing, new_content)
        }
    }

    /// Parse markdown content into a leading preamble and ordered `(header_line, body_lines)` sections.
    ///
    /// `body_lines` excludes the header line itself. Lines before the first `##`/`###`
    /// header accumulate into the preamble.
    fn parse_sections(content: &str) -> (String, Vec<(String, Vec<String>)>) {
        let mut preamble = String::new();
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        let mut current: Option<(String, Vec<String>)> = None;
        for line in content.lines() {
            if line.starts_with("## ") || line.starts_with("### ") {
                if let Some((h, b)) = current.take() {
                    sections.push((h, b));
                }
                current = Some((line.to_string(), Vec::new()));
            } else if let Some((_, body)) = current.as_mut() {
                body.push(line.to_string());
            } else {
                preamble.push_str(line);
                preamble.push('\n');
            }
        }
        if let Some((h, b)) = current.take() {
            sections.push((h, b));
        }
        (preamble, sections)
    }

    /// Strip leading `#` and whitespace from a header line for equality comparison.
    fn clean_header(header_line: &str) -> String {
        header_line.trim_start_matches('#').trim().to_string()
    }

    /// Merge `new_content` into `existing`, returning the full resulting file.
    ///
    /// Sections split on `##`/`###` headers. For each section in `new_content`:
    /// - **Header matches an existing section** → only the body lines NOT already
    ///   present are appended in place to that section. If every line is already
    ///   there, the section is unchanged (exact duplicate dropped).
    /// - **Header is new** → appended as a new section, unless it's a near-identical
    ///   duplicate (similarity ≥ 0.9) of any existing block.
    /// - **Header-less leading text** → novel non-blank lines appended to the file.
    ///
    /// This stops the quadratic blowup where an agent re-sends an entire growing
    /// section (e.g. "Pattern Tracking") on every analysis: only the new timestamp
    /// line lands inside its section, instead of the full history being appended again.
    fn merge_custom_content(existing: &str, new_content: &str, dedup: &DedupProcessor) -> String {
        let (existing_preamble, existing_sections) = Self::parse_sections(existing);
        let (new_preamble, new_sections) = Self::parse_sections(new_content);

        // Working copy: (header_line, body_lines)
        let mut sections: Vec<(String, Vec<String>)> = existing_sections.clone();
        let mut appended_loose: Vec<String> = Vec::new();

        // Collect every existing non-blank line for novel-line checks on loose text.
        let existing_all_lines: Vec<String> = {
            let mut v: Vec<String> = existing_preamble
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            for (_, b) in &sections {
                v.extend(
                    b.iter()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty()),
                );
            }
            v
        };

        // Header-less leading text in the new content: append each novel line.
        for line in new_preamble.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let already = existing_all_lines.iter().any(|l| *l == t)
                || appended_loose.iter().any(|l| l.trim() == t);
            if !already {
                appended_loose.push(line.to_string());
            }
        }

        for (nheader, nbody) in &new_sections {
            let nhead_clean = Self::clean_header(nheader);
            if nhead_clean.is_empty() {
                continue;
            }
            let idx = sections
                .iter()
                .position(|(h, _)| Self::clean_header(h) == nhead_clean);
            match idx {
                Some(i) => {
                    // Same section: append only novel body lines in place.
                    for line in nbody {
                        let t = line.trim();
                        if t.is_empty() {
                            continue;
                        }
                        let already = sections[i].1.iter().any(|l| l.trim() == t);
                        if !already {
                            sections[i].1.push(line.clone());
                        }
                    }
                }
                None => {
                    // New section — guard against near-duplicate of an existing block.
                    let nb_full = format!("{}\n{}", nheader, nbody.join("\n"));
                    let is_near_dup = sections.iter().any(|(h, b)| {
                        let eb_full = format!("{}\n{}", h, b.join("\n"));
                        dedup.similarity(&nb_full, &eb_full) >= 0.9
                    });
                    if !is_near_dup {
                        sections.push((nheader.clone(), nbody.clone()));
                    }
                }
            }
        }

        // Reassemble.
        let mut out = String::new();
        if !existing_preamble.trim().is_empty() {
            out.push_str(&existing_preamble);
        }
        for (h, b) in &sections {
            out.push_str(h);
            out.push('\n');
            for line in b {
                out.push_str(line);
                out.push('\n');
            }
        }
        for line in &appended_loose {
            out.push_str(line);
            out.push('\n');
        }
        // Normalize trailing newlines to exactly one.
        let trimmed = out.trim_end_matches('\n');
        format!("{}\n", trimmed)
    }

    /// Find and replace text in content (first occurrence only).
    fn replace_in_content(content: &str, old_text: &str, new_text: &str) -> Result<String> {
        if !content.contains(old_text) {
            return Err(ToolError::InvalidArguments(format!(
                "Text '{}' not found in content",
                old_text
            )));
        }
        Ok(content.replacen(old_text, new_text, 1))
    }

    /// Find and remove text from content (first occurrence only).
    fn remove_from_content(content: &str, old_text: &str) -> Result<String> {
        if !content.contains(old_text) {
            return Err(ToolError::InvalidArguments(format!(
                "Text '{}' not found in content",
                old_text
            )));
        }
        Ok(content.replacen(old_text, "", 1))
    }

    /// Validate that session_id is set for session-scoped operations.
    async fn require_session_id(&self) -> Result<String> {
        self.session_id.read().await.clone().ok_or_else(|| {
            ToolError::Execution("Session ID required for session-scoped operations.".into())
        })
    }

    /// Parse a target string. Returns Some(name) for custom:{name}, None for built-in targets.
    fn parse_custom_target(target: &str) -> Option<&str> {
        target.strip_prefix("custom:")
    }

    /// Get the current agent_id from the per-execution handle.
    async fn get_agent_id(&self) -> Option<String> {
        let handle: AgentIdHandle = self.current_agent_id_handle();
        let guard = handle.read().await;
        guard.clone()
    }

    /// Read the current agent_id Arc handle (synchronous).
    fn current_agent_id_handle(&self) -> AgentIdHandle {
        self.agent_id.read().clone()
    }

    /// Read the current knowledge_files Arc handle (synchronous).
    fn current_knowledge_files_handle(&self) -> KnowledgeFilesHandle {
        self.knowledge_files.read().clone()
    }

    /// Read a custom file, respecting agent scope.
    fn read_custom(
        &self,
        store: &MarkdownMemoryStore,
        agent_id: Option<&str>,
        name: &str,
    ) -> Result<String> {
        if let Some(aid) = agent_id {
            store
                .read_agent_custom_file(aid, name)
                .map_err(|e| ToolError::Execution(e.to_string()))
        } else {
            store
                .read_custom_file(name)
                .map_err(|e| ToolError::Execution(e.to_string()))
        }
    }

    /// Write a custom file, respecting agent scope.
    fn write_custom(
        &self,
        store: &MarkdownMemoryStore,
        agent_id: Option<&str>,
        name: &str,
        content: &str,
    ) -> Result<()> {
        if let Some(aid) = agent_id {
            store
                .write_agent_custom_file(aid, name, content)
                .map_err(|e| ToolError::Execution(e.to_string()))
        } else {
            store
                .write_custom_file(name, content)
                .map_err(|e| ToolError::Execution(e.to_string()))
        }
    }

    /// Update the knowledge file index when creating a new custom file.
    async fn register_knowledge_file(&self, name: &str, description: &str) {
        let now = chrono::Utc::now().timestamp();
        let handle = self.current_knowledge_files_handle();
        let mut files = handle.write().await;
        if let Some(existing) = files.iter_mut().find(|f| f.name == name) {
            existing.updated_at = now;
        } else {
            files.push(KnowledgeFileRef {
                name: name.to_string(),
                description: truncate_to(description, 100),
                created_at: now,
                updated_at: now,
            });
        }
    }
}

fn truncate_to(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        truncated + "..."
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        r##"Manage persistent memory across sessions. Use this to store and retrieve information that should persist between conversations. This is the agent's own cross-run memory (the targets below); to create or edit arbitrary files (skill files, configs, bundles) use `file_write` or `file_edit` instead.

Actions:
- add: Append content to a memory target (requires content)
- replace: Find and replace text in a memory target (requires BOTH old_text AND content)
- remove: Find and remove text from a memory target (requires old_text)
- read: Read the full content of a memory target
- list: Show overview of all memory targets (chars used, preview)
- create: Create a new custom memory file (requires content; target must be custom:{name})

Targets:
- user: Persistent user profile and preferences (USER.md, ~2000 chars)
- knowledge: System knowledge and domain facts (KNOWLEDGE.md, ~3000 chars)
- procedures: Procedural memory — SOPs, playbooks, how-tos (PROCEDURES.md, ~3000 chars)
- session: Session-scoped notes for multi-step task tracking (cleared after 7 days)
- custom:{name}: Domain-specific custom file (escape hatch — see below)

PREFER the 3 standard targets (user / knowledge / procedures) wherever the content fits.
Global `custom:{name}` is an ADVANCED escape hatch — only when content is genuinely scoped to a
specific topic AND does not fit any of the 3 standard targets. Global custom files persist across
ALL future conversations, so writing one is a high-bar decision. When in doubt, use the standard targets.

Limits: every target enforces a per-file char limit (custom files ~20000 chars). `content` is REQUIRED for add/replace/create. If a write is rejected for exceeding the limit, the error reports the exact limit — TRUNCATE your content and retry. Keep entries concise (bullet points).

Examples:
- Add user preference: action='add', target='user', content='Prefers dark mode'
- Replace in knowledge: action='replace', target='knowledge', old_text='old info', content='new info'
- Add a procedure: action='add', target='procedures', content='## Reset Camera\n1. Power off\n2. Hold reset 10s'
- Read session notes: action='read', target='session'
- Create custom file: action='create', target='custom:device-patterns', content='- temp normal: 22-28°C\n- alert threshold: 40°C'
- List all targets: action='list'"##
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "replace", "remove", "read", "list", "create"],
                    "description": "The memory action to perform"
                },
                "target": {
                    "type": "string",
                    "description": "Which memory target to operate on: 'user', 'knowledge', 'procedures', 'session', or 'custom:{name}'"
                },
                "content": {
                    "type": "string",
                    "description": "Content to add or replace with. REQUIRED for add/replace/create."
                },
                "old_text": {
                    "type": "string",
                    "description": "Text to find (REQUIRED for replace and remove)."
                }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn swap_agent_context(
        &self,
        agent_id: String,
        knowledge_files: Vec<neomind_storage::KnowledgeFileRef>,
    ) -> MemoryToolHandles {
        let id_handle = std::sync::Arc::new(tokio::sync::RwLock::new(Some(agent_id)));
        let kf_handle = std::sync::Arc::new(tokio::sync::RwLock::new(knowledge_files));

        self.swap_agent_id_handle(id_handle.clone());
        self.swap_knowledge_files_handle(kf_handle.clone());

        Some((id_handle, kf_handle))
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("action is required".into()))?;

        let target = args["target"].as_str().unwrap_or("");

        match action {
            "list" => {
                // target is optional for list
            }
            _ => {
                if target.is_empty() {
                    return Err(ToolError::InvalidArguments(
                        "target is required for this action".into(),
                    ));
                }
            }
        }

        // Read agent_id once for this execution
        let agent_id = self.get_agent_id().await;

        match action {
            "create" => {
                let content = args["content"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("content is required for create".into())
                })?;

                let custom_name = Self::parse_custom_target(target).ok_or_else(|| {
                    ToolError::InvalidArguments(format!(
                        "Create action requires target in format 'custom:{{name}}'. Got: '{}'",
                        target
                    ))
                })?;

                let store = self.store.write().await;
                // Stricter bar for GLOBAL custom files (chat scope, agent_id=None):
                // these leak across all agents and accumulate noise. Log a warn so
                // operators can audit; agent-scoped custom writes are unrestricted.
                if agent_id.is_none() {
                    tracing::warn!(
                        custom = %custom_name,
                        chars = content.chars().count(),
                        "Global custom memory file write — chat scope. Prefer user/knowledge/procedures targets \
                         unless content is genuinely scoped and reusable across all future conversations."
                    );
                }
                self.write_custom(&store, agent_id.as_deref(), custom_name, content)?;

                // Extract description from first line of content
                let description = content
                    .lines()
                    .next()
                    .unwrap_or("Knowledge file")
                    .trim_start_matches("# ")
                    .to_string();

                drop(store);
                self.register_knowledge_file(custom_name, &description)
                    .await;

                Ok(ToolOutput::success(serde_json::json!({
                    "message": format!("Created custom file '{}' ({} chars)", custom_name, content.chars().count())
                })))
            }
            "add" => {
                let content = args["content"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("content is required for add".into())
                })?;

                let store = self.store.write().await;
                let dedup = DedupProcessor::with_defaults();

                let result = if let Some(custom_name) = Self::parse_custom_target(target) {
                    let existing = self.read_custom(&store, agent_id.as_deref(), custom_name)?;
                    // Merge new content into existing, deduplicating at the section level.
                    // Without this, custom files grow quadratically (agent re-sends the
                    // full pattern history every analysis) — user/knowledge targets are
                    // protected by line-level dedup below, but custom files weren't.
                    let merged = Self::merge_custom_content(&existing, content, &dedup);
                    if merged.trim() == existing.trim() {
                        return Ok(ToolOutput::success(serde_json::json!({
                            "message": format!(
                                "Skipped: content duplicates existing sections in custom:{}",
                                custom_name
                            )
                        })));
                    }
                    self.write_custom(&store, agent_id.as_deref(), custom_name, &merged)?;
                    format!(
                        "Added to custom:{} ({} chars)",
                        custom_name,
                        merged.chars().count()
                    )
                } else {
                    match target {
                        "user" | "knowledge" | "procedures" => {
                            let existing = store.read_file(target).await?;
                            let existing_lines: Vec<String> = existing
                                .lines()
                                .filter(|l| l.trim().starts_with("- ["))
                                .filter_map(|l| {
                                    let trimmed = l.trim();
                                    let after_date = trimmed.strip_prefix("- [")?;
                                    let close_bracket = after_date.find(']')?;
                                    let content_part = &after_date[close_bracket + 1..];
                                    let cleaned = if let Some(idx) = content_part.rfind(" [importance:") {
                                        &content_part[..idx]
                                    } else {
                                        content_part
                                    };
                                    Some(cleaned.trim().to_string())
                                })
                                .collect();
                            if let Some((_, sim)) = dedup.find_similar(content, &existing_lines) {
                                return Ok(ToolOutput::success(serde_json::json!({
                                    "message": format!("Skipped: similar content already exists (similarity: {:.0}%)", sim * 100.0)
                                })));
                            }
                            let new_content = Self::append_content(&existing, content);
                            store.write_file(target, &new_content).await?;
                            format!("Added to {} ({} chars)", target, new_content.chars().count())
                        }
                        "session" => {
                            let session_id = self.require_session_id().await?;
                            let existing = store.read_session_file(&session_id, "notes").await?;
                            let new_content = Self::append_content(&existing, content);
                            store
                                .write_session_file(&session_id, "notes", &new_content)
                                .await?;
                            format!(
                                "Added to session notes ({} chars)",
                                new_content.chars().count()
                            )
                        }
                        _ => {
                            return Err(ToolError::InvalidArguments(format!(
                                "Invalid target '{}'. Must be one of: user, knowledge, procedures, session, custom:{{name}}",
                                target
                            )))
                        }
                    }
                };

                Ok(ToolOutput::success(serde_json::json!({
                    "message": result
                })))
            }
            "replace" => {
                let content = args["content"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("content is required for replace".into())
                })?;
                let old_text = args["old_text"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("old_text is required for replace".into())
                })?;

                let store = self.store.write().await;

                let result = if let Some(custom_name) = Self::parse_custom_target(target) {
                    let existing = self.read_custom(&store, agent_id.as_deref(), custom_name)?;
                    let new_content = Self::replace_in_content(&existing, old_text, content)?;
                    self.write_custom(&store, agent_id.as_deref(), custom_name, &new_content)?;
                    format!(
                        "Replaced in custom:{} ({} chars)",
                        custom_name,
                        new_content.chars().count()
                    )
                } else {
                    match target {
                        "user" | "knowledge" | "procedures" => {
                            let existing = store.read_file(target).await?;
                            let new_content = Self::replace_in_content(&existing, old_text, content)?;
                            store.write_file(target, &new_content).await?;
                            format!("Replaced in {} ({} chars)", target, new_content.chars().count())
                        }
                        "session" => {
                            let session_id = self.require_session_id().await?;
                            let existing = store.read_session_file(&session_id, "notes").await?;
                            let new_content = Self::replace_in_content(&existing, old_text, content)?;
                            store
                                .write_session_file(&session_id, "notes", &new_content)
                                .await?;
                            format!(
                                "Replaced in session notes ({} chars)",
                                new_content.chars().count()
                            )
                        }
                        _ => {
                            return Err(ToolError::InvalidArguments(format!(
                                "Invalid target '{}'. Must be one of: user, knowledge, procedures, session, custom:{{name}}",
                                target
                            )))
                        }
                    }
                };

                Ok(ToolOutput::success(serde_json::json!({
                    "message": result
                })))
            }
            "remove" => {
                let old_text = args["old_text"].as_str().ok_or_else(|| {
                    ToolError::InvalidArguments("old_text is required for remove".into())
                })?;

                let store = self.store.write().await;

                let result = if let Some(custom_name) = Self::parse_custom_target(target) {
                    let existing = self.read_custom(&store, agent_id.as_deref(), custom_name)?;
                    let new_content = Self::remove_from_content(&existing, old_text)?;
                    self.write_custom(&store, agent_id.as_deref(), custom_name, &new_content)?;
                    format!(
                        "Removed from custom:{} ({} chars)",
                        custom_name,
                        new_content.chars().count()
                    )
                } else {
                    match target {
                        "user" | "knowledge" | "procedures" => {
                            let existing = store.read_file(target).await?;
                            let new_content = Self::remove_from_content(&existing, old_text)?;
                            store.write_file(target, &new_content).await?;
                            format!("Removed from {} ({} chars)", target, new_content.chars().count())
                        }
                        "session" => {
                            let session_id = self.require_session_id().await?;
                            let existing = store.read_session_file(&session_id, "notes").await?;
                            let new_content = Self::remove_from_content(&existing, old_text)?;
                            store
                                .write_session_file(&session_id, "notes", &new_content)
                                .await?;
                            format!(
                                "Removed from session notes ({} chars)",
                                new_content.chars().count()
                            )
                        }
                        _ => {
                            return Err(ToolError::InvalidArguments(format!(
                                "Invalid target '{}'. Must be one of: user, knowledge, procedures, session, custom:{{name}}",
                                target
                            )))
                        }
                    }
                };

                Ok(ToolOutput::success(serde_json::json!({
                    "message": result
                })))
            }
            "read" => {
                let store = self.store.read().await;

                let result = if let Some(custom_name) = Self::parse_custom_target(target) {
                    let content = self.read_custom(&store, agent_id.as_deref(), custom_name)?;
                    serde_json::json!({
                        "target": target,
                        "content": content,
                        "chars": content.chars().count()
                    })
                } else {
                    match target {
                        "user" | "knowledge" | "procedures" => {
                            let content = store.read_file(target).await?;
                            serde_json::json!({
                                "target": target,
                                "content": content,
                                "chars": content.chars().count()
                            })
                        }
                        "session" => {
                            let session_id = self.require_session_id().await?;
                            let content = store.read_session_file(&session_id, "notes").await?;
                            serde_json::json!({
                                "target": "session",
                                "content": content,
                                "chars": content.chars().count()
                            })
                        }
                        _ => {
                            return Err(ToolError::InvalidArguments(format!(
                                "Invalid target '{}'. Must be one of: user, knowledge, procedures, session, custom:{{name}}",
                                target
                            )))
                        }
                    }
                };

                Ok(ToolOutput::success(result))
            }
            "list" => {
                let store = self.store.read().await;

                let user_content = store.read_file("user").await?;
                let knowledge_content = store.read_file("knowledge").await?;
                let procedures_content = store.read_file("procedures").await?;

                let mut result = serde_json::json!({
                    "user": {
                        "chars": user_content.chars().count(),
                        "preview": Self::get_preview(&user_content)
                    },
                    "knowledge": {
                        "chars": knowledge_content.chars().count(),
                        "preview": Self::get_preview(&knowledge_content)
                    },
                    "procedures": {
                        "chars": procedures_content.chars().count(),
                        "preview": Self::get_preview(&procedures_content)
                    }
                });

                if let Some(session_id) = self.session_id.read().await.clone() {
                    let notes_content = store.read_session_file(&session_id, "notes").await?;
                    result["session"] = serde_json::json!({
                        "chars": notes_content.chars().count(),
                        "preview": Self::get_preview(&notes_content)
                    });
                }

                // Read custom files (agent-scoped or global)
                let custom_files = if let Some(ref aid) = agent_id {
                    store
                        .list_agent_custom_files(aid)
                        .map_err(|e| ToolError::Execution(e.to_string()))?
                } else {
                    store
                        .list_custom_files()
                        .map_err(|e| ToolError::Execution(e.to_string()))?
                };

                if !custom_files.is_empty() {
                    let mut customs = serde_json::Map::new();
                    for (name, chars) in &custom_files {
                        let content = self.read_custom(&store, agent_id.as_deref(), name)?;
                        customs.insert(
                            format!("custom:{}", name),
                            serde_json::json!({
                                "chars": chars,
                                "preview": Self::get_preview(&content)
                            }),
                        );
                    }
                    result["custom_files"] = Value::Object(customs);
                }

                Ok(ToolOutput::success(result))
            }
            _ => Err(ToolError::InvalidArguments(format!(
                "Unknown action '{}'. Available actions: add, replace, remove, read, list, create",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::dedup::DedupProcessor;

    #[test]
    fn test_merge_exact_duplicate_section_is_noop() {
        let existing = "# Task\n\n## Role\nYou are an agent.\n\n## 2026-06-11 22:00 Analysis\n- temp 25C\n- ok\n";
        let dedup = DedupProcessor::with_defaults();
        let merged = MemoryTool::merge_custom_content(existing, existing, &dedup);
        assert_eq!(
            merged.trim(),
            existing.trim(),
            "re-merging identical content must be a no-op"
        );
    }

    #[test]
    fn test_merge_pattern_tracking_superset_grows_by_one_line() {
        // The real-world quadratic blowup: agent re-sends the full Pattern Tracking
        // history plus one new timestamp line on every analysis.
        let existing = "## Pattern Tracking\n- 22:00: temp 25C\n- 21:00: temp 24C\n";
        let resent =
            "## Pattern Tracking\n- 22:00: temp 25C\n- 21:00: temp 24C\n- 23:00: temp 26C\n";
        let dedup = DedupProcessor::with_defaults();
        let merged = MemoryTool::merge_custom_content(existing, resent, &dedup);
        // Only the genuinely-new line should land — NOT the full resent block.
        assert!(
            merged.contains("- 23:00: temp 26C"),
            "new line must be present"
        );
        // The two old lines must appear exactly ONCE (not duplicated).
        assert_eq!(
            merged.matches("- 22:00: temp 25C").count(),
            1,
            "old lines must not be duplicated"
        );
        assert_eq!(
            merged.matches("- 21:00: temp 24C").count(),
            1,
            "old lines must not be duplicated"
        );
    }

    #[test]
    fn test_merge_genuinely_new_section_appended() {
        let existing = "## Role\nYou are an agent.\n";
        let new = "## Thresholds\n- CPU alert: >85%\n- Temp alert: >40C\n";
        let dedup = DedupProcessor::with_defaults();
        let merged = MemoryTool::merge_custom_content(existing, new, &dedup);
        assert!(merged.contains("## Thresholds"));
        assert!(merged.contains("- CPU alert: >85%"));
        assert!(merged.contains("## Role"), "existing section preserved");
    }

    #[test]
    fn test_merge_near_duplicate_whole_block_dropped() {
        // Two near-identical timestamped analyses with different headers are kept
        // (distinct events), but a near-identical block under a NEW header is dropped.
        let existing = "## 2026-06-11 22:00 Analysis\nThe temperature sensor reported 25C and everything is within normal range.\n";
        // Same body, different (new) header — near-duplicate of the existing block.
        let dup = "## 2026-06-11 22:00 Analysis 2\nThe temperature sensor reported 25C and everything is within normal range.\n";
        let dedup = DedupProcessor::with_defaults();
        let merged = MemoryTool::merge_custom_content(existing, dup, &dedup);
        assert_eq!(
            merged.trim(),
            existing.trim(),
            "near-duplicate block must be dropped"
        );
    }

    #[tokio::test]
    async fn test_add_then_read_user_roundtrip() {
        // Reproduce eval case tools-memory-read: add to user, then read must
        // observe the just-written content. Previously read returned the empty
        // template even though add reported a successful cumulative length.
        let temp = tempfile::TempDir::new().unwrap();
        let store = neomind_storage::MarkdownMemoryStore::new(temp.path());
        store.init().unwrap();
        let store = std::sync::Arc::new(tokio::sync::RwLock::new(store));
        let tool = MemoryTool::new(store.clone());

        // 1. Baseline read: returns init() template (34 chars)
        let r = tool
            .execute(serde_json::json!({"action":"read","target":"user"}))
            .await
            .unwrap();
        let body0 = &r.data;
        let chars0 = body0["chars"].as_u64().unwrap();
        assert!(
            chars0 < 60,
            "baseline USER.md should be the template, got {} chars: {:?}",
            chars0,
            body0["content"]
        );

        // 2. Add new content
        let a = tool
            .execute(serde_json::json!({
                "action": "add",
                "target": "user",
                "content": "## 传感器告警阈值偏好\n\n- 仓库 B 区 3 号货架：摄氏 35 度告警（非默认 40°C）",
            }))
            .await
            .unwrap();
        let msg = a.data["message"].as_str().unwrap_or("");
        assert!(
            msg.starts_with("Added to user"),
            "add must succeed: {}",
            msg
        );

        // 3. Read MUST observe the new content
        let r2 = tool
            .execute(serde_json::json!({"action":"read","target":"user"}))
            .await
            .unwrap();
        let body2 = &r2.data;
        let content2 = body2["content"].as_str().unwrap_or("");
        let chars2 = body2["chars"].as_u64().unwrap_or(0);
        assert!(
            content2.contains("35") && content2.contains("仓库"),
            "read after add must contain the new content; got {} chars: {:?}",
            chars2,
            content2
        );
    }
}
