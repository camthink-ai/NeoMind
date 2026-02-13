//! AI Agent storage for persistent autonomous agents.
//!
//! This module provides storage for AI Agents that:
//! - Execute periodically or based on events
//! - Maintain persistent memory across executions
//! - Record decision processes for verification
//! - Handle errors gracefully for long-running stability

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::Error;

// Tables for agent storage
const AGENTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agents");
const AGENT_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_executions");
const AGENT_MEMORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_memory");

/// AI Agent store for persisting autonomous agents.
pub struct AgentStore {
    /// redb database
    db: Arc<Database>,
}

/// An AI Agent definition.
///
/// Represents a user-created autonomous agent that monitors devices,
/// analyzes data, and takes actions based on natural language requirements.
/// The agent maintains a persistent conversation history across executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAgent {
    /// Unique agent ID
    pub id: String,
    /// Agent name
    pub name: String,
    /// User-provided description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User's natural language description of requirements
    pub user_prompt: String,
    /// Optional LLM backend ID for this agent (uses default if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    /// AI-generated understanding of the requirements
    pub parsed_intent: Option<ParsedIntent>,
    /// Selected resources (devices, metrics, commands)
    pub resources: Vec<AgentResource>,
    /// Schedule configuration
    pub schedule: AgentSchedule,
    /// Agent status
    pub status: AgentStatus,
    /// Priority for execution (0-255, higher = more priority)
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Creation timestamp
    pub created_at: i64,
    /// Last update timestamp
    pub updated_at: i64,
    /// Last execution timestamp
    pub last_execution_at: Option<i64>,
    /// Execution statistics
    pub stats: AgentStats,
    /// Persistent memory across executions
    pub memory: AgentMemory,
    /// Conversation history - recent executions for context
    #[serde(default)]
    pub conversation_history: Vec<ConversationTurn>,
    /// User messages sent between executions
    #[serde(default)]
    pub user_messages: Vec<UserMessage>,
    /// Compressed summary of old conversation turns
    #[serde(default)]
    pub conversation_summary: Option<String>,
    /// How many recent turns to include in LLM context
    #[serde(default = "default_context_window")]
    pub context_window_size: usize,
    /// Enable tool chaining - allows tool outputs to be used as inputs for subsequent tools
    #[serde(default)]
    pub enable_tool_chaining: bool,
    /// Maximum chain depth (prevents infinite loops)
    #[serde(default = "default_max_chain_depth")]
    pub max_chain_depth: usize,
    /// Error message (if status is error)
    pub error_message: Option<String>,
}

/// Default value for context window size.
fn default_context_window() -> usize {
    10
}

/// Default value for max chain depth.
fn default_max_chain_depth() -> usize {
    3 // Allow up to 3 chain steps by default
}

/// Default value for agent priority.
fn default_priority() -> u8 {
    128 // Middle priority (0-255 range)
}

/// Parsed intent from user's natural language description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedIntent {
    /// Intent type
    pub intent_type: IntentType,
    /// Target metrics to monitor
    pub target_metrics: Vec<String>,
    /// Conditions to evaluate
    pub conditions: Vec<String>,
    /// Actions to take
    pub actions: Vec<String>,
    /// Confidence in parsing (0-1)
    pub confidence: f32,
}

/// Type of intent extracted from user prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentType {
    /// Monitor and alert on conditions
    Monitoring,
    /// Generate periodic reports
    ReportGeneration,
    /// Analyze data for anomalies
    AnomalyDetection,
    /// Execute control commands
    Control,
    /// Complex multi-step automation
    Automation,
}

/// A resource selected by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResource {
    /// Resource type
    pub resource_type: ResourceType,
    /// Resource ID (device_id, metric_name, etc.)
    pub resource_id: String,
    /// Display name
    pub name: String,
    /// Additional configuration
    pub config: serde_json::Value,
}

/// Type of resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Device,
    Metric,
    Command,
    DataStream,
    ExtensionTool,
    ExtensionMetric,
}

/// Agent schedule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSchedule {
    /// Schedule type
    pub schedule_type: ScheduleType,
    /// Cron expression (for Cron type)
    pub cron_expression: Option<String>,
    /// Interval in seconds (for Interval type)
    pub interval_seconds: Option<u64>,
    /// Event filter (for Event type)
    pub event_filter: Option<String>,
    /// Timezone for schedule
    pub timezone: Option<String>,
}

/// Schedule type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    /// Event-triggered execution
    Event,
    /// Cron-based schedule
    Cron,
    /// Fixed interval
    Interval,
}

/// Agent status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    /// Agent is active and running
    Active,
    /// Paused by user
    Paused,
    /// Stopped
    Stopped,
    /// In error state
    Error,
    /// Executing
    Executing,
}

/// Agent execution statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentStats {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: u64,
    /// Last execution duration in milliseconds
    pub last_duration_ms: Option<u64>,
}

/// Hierarchical memory for an agent across executions.
///
/// Based on MemGPT/Letta architecture with three tiers:
/// - Working Memory: Current execution context (ephemeral)
/// - Short-Term Memory: Recent compressed summaries (time-bounded)
/// - Long-Term Memory: Important patterns and knowledge (retrieval-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    /// Working memory - current execution context
    #[serde(default)]
    pub working: WorkingMemory,
    /// Short-term memory - recent compressed summaries
    #[serde(default)]
    pub short_term: ShortTermMemory,
    /// Long-term memory - important patterns and knowledge
    #[serde(default)]
    pub long_term: LongTermMemory,
    /// Legacy state variables (for backward compatibility)
    #[serde(default)]
    pub state_variables: HashMap<String, serde_json::Value>,
    /// Legacy learned patterns (migrated to long_term)
    #[serde(default)]
    pub learned_patterns: Vec<LearnedPattern>,
    /// Historical baselines
    #[serde(default)]
    pub baselines: HashMap<String, f64>,
    /// Trend data points (for analysis)
    #[serde(default)]
    pub trend_data: Vec<TrendPoint>,
    /// Last memory update
    pub updated_at: i64,
}

/// Working memory - current execution context.
///
/// Stores ephemeral data for the current execution only.
/// Automatically cleared after each execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingMemory {
    /// Current analysis results (this execution only)
    #[serde(default)]
    pub current_analysis: Option<String>,
    /// Current conclusion (this execution only)
    #[serde(default)]
    pub current_conclusion: Option<String>,
    /// Current decisions being considered
    #[serde(default)]
    pub pending_decisions: Vec<serde_json::Value>,
    /// Temporary data collection for this execution
    #[serde(default)]
    pub temp_data: HashMap<String, serde_json::Value>,
    /// Timestamp when this working memory was created
    #[serde(default)]
    pub created_at: i64,
}

/// Short-term memory - recent compressed summaries.
///
/// Stores the last N executions in compressed form.
/// Automatically archived to long-term memory when full.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShortTermMemory {
    /// Compressed summaries of recent executions
    #[serde(default)]
    pub summaries: Vec<MemorySummary>,
    /// Maximum number of summaries to keep
    #[serde(default = "default_short_term_limit")]
    pub max_summaries: usize,
    /// Timestamp of last archival
    #[serde(default)]
    pub last_archived_at: Option<i64>,
}

/// Long-term memory - important patterns and knowledge.
///
/// Stores high-value memories with importance scoring.
/// Retrieved using RAG-style semantic matching.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LongTermMemory {
    /// Important memories with importance scores
    #[serde(default)]
    pub memories: Vec<ImportantMemory>,
    /// Learned patterns (high-confidence, reusable)
    #[serde(default)]
    pub patterns: Vec<LearnedPattern>,
    /// Maximum number of memories to keep
    #[serde(default = "default_long_term_limit")]
    pub max_memories: usize,
    /// Minimum importance score for retention
    #[serde(default = "default_min_importance")]
    pub min_importance: f32,
}

/// A compressed memory summary for short-term storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySummary {
    /// Timestamp of this summary
    pub timestamp: i64,
    /// Execution ID this summarizes
    pub execution_id: String,
    /// Compressed situation analysis
    pub situation: String,
    /// Compressed conclusion
    pub conclusion: String,
    /// Key decisions made
    pub decisions: Vec<String>,
    /// Success flag
    pub success: bool,
}

/// An important memory for long-term storage with importance scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportantMemory {
    /// Unique memory ID
    pub id: String,
    /// Memory type (pattern, anomaly, decision, knowledge)
    pub memory_type: String,
    /// Memory content (compressed)
    pub content: String,
    /// Importance score (0-1), higher is more important
    pub importance: f32,
    /// Creation timestamp
    pub created_at: i64,
    /// Last access timestamp (for LRU eviction)
    pub last_accessed_at: i64,
    /// Access count (for importance boost)
    pub access_count: u64,
    /// Associated metadata
    pub metadata: HashMap<String, String>,
}

/// Default short-term memory limit (number of summaries).
fn default_short_term_limit() -> usize {
    10
}

/// Default long-term memory limit (number of important memories).
fn default_long_term_limit() -> usize {
    50
}

/// Default minimum importance score for long-term retention.
fn default_min_importance() -> f32 {
    0.3
}

/// Normalize agent memory limits for backward compatibility.
///
/// Agents created before proper memory limits were implemented may have
/// zero values for these limits. This function ensures they are set to
/// sensible defaults.
fn normalize_agent_memory_limits(agent: &mut AiAgent) {
    if agent.memory.short_term.max_summaries == 0 {
        agent.memory.short_term.max_summaries = default_short_term_limit();
    }
    if agent.memory.long_term.max_memories == 0 {
        agent.memory.long_term.max_memories = default_long_term_limit();
    }
    if agent.memory.long_term.min_importance == 0.0 {
        agent.memory.long_term.min_importance = default_min_importance();
    }
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self {
            working: WorkingMemory::default(),
            short_term: ShortTermMemory::default(),
            long_term: LongTermMemory::default(),
            state_variables: HashMap::new(),
            learned_patterns: Vec::new(),
            baselines: HashMap::new(),
            trend_data: Vec::new(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

// ========== Hierarchical Memory Methods ==========

impl AgentMemory {
    /// Add a memory summary to short-term memory.
    /// Automatically archives to long-term if capacity exceeded.
    pub fn add_to_short_term(
        &mut self,
        execution_id: String,
        situation: String,
        conclusion: String,
        decisions: Vec<String>,
        success: bool,
    ) {
        let summary = MemorySummary {
            timestamp: chrono::Utc::now().timestamp(),
            execution_id,
            situation,
            conclusion,
            decisions,
            success,
        };

        self.short_term.summaries.push(summary);

        // Check if we need to archive
        if self.short_term.summaries.len() > self.short_term.max_summaries {
            self.archive_to_long_term();
        }

        self.updated_at = chrono::Utc::now().timestamp();
    }

    /// Archive old short-term memories to long-term memory.
    /// Keeps only the most recent summaries in short-term.
    pub fn archive_to_long_term(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.short_term.last_archived_at = Some(now);

        // Keep only the most recent half in short-term
        let keep_count = self.short_term.max_summaries / 2;
        let drain_count = self.short_term.summaries.len().saturating_sub(keep_count);
        let to_archive: Vec<_> = self.short_term.summaries.drain(..drain_count).collect();

        // Convert archived summaries to important memories
        for summary in to_archive {
            // Calculate importance based on multiple factors
            let importance = self.calculate_importance(&summary);

            // Only keep memories above threshold
            if importance >= self.long_term.min_importance {
                let memory = ImportantMemory {
                    id: uuid::Uuid::new_v4().to_string(),
                    memory_type: if summary.success {
                        "successful_execution"
                    } else {
                        "failed_execution"
                    }
                    .to_string(),
                    content: format!("{} -> {}", summary.situation, summary.conclusion),
                    importance,
                    created_at: summary.timestamp,
                    last_accessed_at: now,
                    access_count: 0,
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("execution_id".to_string(), summary.execution_id.clone());
                        meta.insert("success".to_string(), summary.success.to_string());
                        if !summary.decisions.is_empty() {
                            meta.insert("decisions".to_string(), summary.decisions.join("; "));
                        }
                        meta
                    },
                };
                self.long_term.memories.push(memory);
            }
        }

        // Prune long-term memory if needed
        self.prune_long_term_memory();

        tracing::debug!(
            short_term_count = self.short_term.summaries.len(),
            long_term_count = self.long_term.memories.len(),
            "Archived short-term memories to long-term"
        );
    }

    /// Calculate importance score for a memory summary.
    /// Factors in: recency, success/failure, decision significance.
    fn calculate_importance(&self, summary: &MemorySummary) -> f32 {
        let mut importance = 0.5; // Base importance

        // Boost for failed executions (learning opportunities)
        if !summary.success {
            importance += 0.2;
        }

        // Boost for summaries with decisions (actionable)
        if !summary.decisions.is_empty() {
            importance += 0.15;
        }

        // Time decay (older memories are less important unless accessed)
        let age_hours = (chrono::Utc::now().timestamp() - summary.timestamp) as f32 / 3600.0;
        let decay_factor = 1.0 / (1.0 + age_hours / 24.0); // Half-life of 24 hours
        importance *= decay_factor;

        // Clamp to [0, 1]
        importance.clamp(0.0, 1.0)
    }

    /// Prune long-term memory to stay within capacity.
    /// Removes low-importance and stale memories.
    pub fn prune_long_term_memory(&mut self) {
        if self.long_term.memories.len() <= self.long_term.max_memories {
            return;
        }

        // Sort by: (1) low importance first, (2) old last_accessed_at
        self.long_term.memories.sort_by(|a, b| {
            // Prioritize keeping high-importance memories
            match a.importance.partial_cmp(&b.importance) {
                Some(std::cmp::Ordering::Equal) => {}
                Some(ord) => return ord,
                None => return std::cmp::Ordering::Equal,
            }
            // If equal importance, keep recently accessed ones
            b.last_accessed_at.cmp(&a.last_accessed_at)
        });

        // Keep only the top memories
        let keep_count = self.long_term.max_memories;
        let removed_count = self.long_term.memories.len() - keep_count;
        self.long_term.memories.truncate(keep_count);

        tracing::debug!(
            removed_count = removed_count,
            remaining_count = self.long_term.memories.len(),
            "Pruned long-term memory"
        );
    }

    /// Retrieve relevant long-term memories based on keyword matching.
    /// Simple RAG-like retrieval without vector embeddings.
    /// Returns a vector of (memory_id, content) tuples for easy access.
    pub fn retrieve_memories(&mut self, query: &str, limit: usize) -> Vec<(String, String)> {
        let now = chrono::Utc::now().timestamp();
        let query_lower = query.to_lowercase();

        // First pass: Update access stats and compute scores
        let mut scores: Vec<(usize, f32)> = self
            .long_term
            .memories
            .iter()
            .enumerate()
            .map(|(idx, mem)| {
                let mut score = mem.importance;
                if mem.content.to_lowercase().contains(&query_lower)
                    || mem.memory_type.to_lowercase().contains(&query_lower)
                {
                    // Relevance boost
                    score += 0.1;
                }
                (idx, score)
            })
            .collect();

        // Sort by score (descending)
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Second pass: Update access stats for relevant memories and collect results
        let results: Vec<(String, String)> = scores
            .into_iter()
            .take(limit)
            .filter_map(|(idx, _)| {
                let mem = &mut self.long_term.memories[idx];
                if mem.content.to_lowercase().contains(&query_lower)
                    || mem.memory_type.to_lowercase().contains(&query_lower)
                {
                    mem.last_accessed_at = now;
                    mem.access_count += 1;

                    // Boost importance based on access frequency
                    let access_boost = (mem.access_count as f32).log10() * 0.05;
                    mem.importance = (mem.importance + access_boost).min(1.0);

                    Some((mem.id.clone(), mem.content.clone()))
                } else {
                    None
                }
            })
            .collect();

        results
    }

    /// Clear working memory (called after each execution).
    pub fn clear_working(&mut self) {
        self.working = WorkingMemory::default();
    }

    /// Store current analysis in working memory.
    pub fn set_working_analysis(&mut self, analysis: String, conclusion: String) {
        self.working.current_analysis = Some(analysis);
        self.working.current_conclusion = Some(conclusion);
        self.working.created_at = chrono::Utc::now().timestamp();
    }

    /// Add a pattern directly to long-term memory.
    /// Also updates the legacy learned_patterns field for backward compatibility.
    pub fn add_pattern(&mut self, pattern: LearnedPattern) {
        // Check if similar pattern exists in both locations
        let exists_in_long_term = self.long_term.patterns.iter().any(|p| {
            p.pattern_type == pattern.pattern_type && p.description == pattern.description
        });
        let exists_in_legacy = self.learned_patterns.iter().any(|p| {
            p.pattern_type == pattern.pattern_type && p.description == pattern.description
        });

        if !exists_in_long_term && !exists_in_legacy && pattern.confidence >= 0.7 {
            // Add to both long_term.patterns and legacy learned_patterns
            // This maintains backward compatibility while using the new structure
            self.long_term.patterns.push(pattern.clone());
            self.learned_patterns.push(pattern);

            // Prune long_term.patterns if needed
            if self.long_term.patterns.len() > 15 {
                self.long_term.patterns.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                self.long_term.patterns.truncate(15);
            }

            // Also prune legacy learned_patterns
            if self.learned_patterns.len() > 15 {
                self.learned_patterns.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                self.learned_patterns.truncate(15);
            }
        }
    }

    /// Get a summary of memory usage for debugging.
    pub fn memory_usage_summary(&self) -> String {
        format!(
            "Working: {}, Short-term: {}/{}, Long-term: {}/{}, Patterns: {}, Trend points: {}, Baselines: {}",
            if self.working.current_analysis.is_some() {
                "active"
            } else {
                "empty"
            },
            self.short_term.summaries.len(),
            self.short_term.max_summaries,
            self.long_term.memories.len(),
            self.long_term.max_memories,
            self.long_term.patterns.len(),
            self.trend_data.len(),
            self.baselines.len()
        )
    }
}

/// A learned pattern from historical data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPattern {
    /// Pattern identifier
    pub id: String,
    /// Pattern type
    pub pattern_type: String,
    /// Pattern description
    pub description: String,
    /// Confidence (0-1)
    pub confidence: f32,
    /// When this pattern was learned
    pub learned_at: i64,
    /// Pattern data
    pub data: serde_json::Value,
}

/// A data point for trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    /// Timestamp
    pub timestamp: i64,
    /// Metric name
    pub metric: String,
    /// Value
    pub value: f64,
    /// Additional context
    pub context: Option<serde_json::Value>,
}

/// Input for a single conversation turn (execution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnInput {
    /// Data collected during this execution
    pub data_collected: Vec<DataCollected>,
    /// Event data if this was event-triggered
    pub event_data: Option<serde_json::Value>,
}

/// Output from a single conversation turn (execution).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutput {
    /// Situation analysis
    pub situation_analysis: String,
    /// Reasoning steps taken
    pub reasoning_steps: Vec<ReasoningStep>,
    /// Decisions made
    pub decisions: Vec<Decision>,
    /// Final conclusion
    pub conclusion: String,
}

/// A single conversation turn - one complete execution with context.
///
/// This represents one "turn" in the long-running conversation with an agent.
/// Each execution adds a new turn, and recent turns are included in the LLM context
/// to maintain conversational memory across executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    /// Execution ID (links to AgentExecutionRecord)
    pub execution_id: String,
    /// Timestamp when this turn occurred
    pub timestamp: i64,
    /// What triggered this execution (event, schedule, manual)
    pub trigger_type: String,
    /// Input to the agent
    pub input: TurnInput,
    /// Output from the agent
    pub output: TurnOutput,
    /// How long this turn took (milliseconds)
    pub duration_ms: u64,
    /// Whether this turn completed successfully
    pub success: bool,
}

/// User message sent to an agent between executions.
///
/// Users can send messages to agents during the gap between executions
/// to provide additional context, corrections, or updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// Unique message ID
    pub id: String,
    /// Timestamp when the message was sent
    pub timestamp: i64,
    /// The message content from the user
    pub content: String,
    /// Optional message type/tag for categorization
    pub message_type: Option<String>,
}

/// Agent execution record with full decision process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Agent ID
    pub agent_id: String,
    /// Execution timestamp
    pub timestamp: i64,
    /// Trigger type (schedule, event, manual)
    pub trigger_type: String,
    /// Execution status
    pub status: ExecutionStatus,
    /// AI decision process with reasoning steps
    pub decision_process: DecisionProcess,
    /// Execution result
    pub result: Option<ExecutionResult>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
}

/// Execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    /// Running
    Running,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Partially completed
    Partial,
}

/// AI decision process with full reasoning trace.
///
/// This provides transparency into how the AI made its decisions,
/// enabling verification and debugging of agent behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProcess {
    /// Initial understanding of the situation
    pub situation_analysis: String,
    /// Data collected for decision making
    pub data_collected: Vec<DataCollected>,
    /// Step-by-step reasoning
    pub reasoning_steps: Vec<ReasoningStep>,
    /// Decisions made
    pub decisions: Vec<Decision>,
    /// Final conclusion
    pub conclusion: String,
    /// Confidence level (0-1)
    pub confidence: f32,
}

/// Data collected for decision making.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollected {
    /// Data source
    pub source: String,
    /// Data type
    pub data_type: String,
    /// Collected values
    pub values: serde_json::Value,
    /// Timestamp
    pub timestamp: i64,
}

/// A single reasoning step in the decision process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    /// Step number
    pub step_number: u32,
    /// Step description
    pub description: String,
    /// Step type (analysis, comparison, inference, etc.)
    pub step_type: String,
    /// Input data for this step
    pub input: Option<String>,
    /// Output of this step
    pub output: String,
    /// Confidence in this step (0-1)
    pub confidence: f32,
}

/// A decision made during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Decision type
    pub decision_type: String,
    /// Decision description
    pub description: String,
    /// Chosen action
    pub action: String,
    /// Rationale
    pub rationale: String,
    /// Expected outcome
    pub expected_outcome: String,
}

/// Execution result with actions taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Actions executed
    pub actions_executed: Vec<ActionExecuted>,
    /// Generated report (if any)
    pub report: Option<GeneratedReport>,
    /// Notifications sent
    pub notifications_sent: Vec<NotificationSent>,
    /// Summary of execution
    pub summary: String,
    /// Success rate (0-1)
    pub success_rate: f32,
}

/// An action that was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecuted {
    /// Action type
    pub action_type: String,
    /// Action description
    pub description: String,
    /// Target of the action
    pub target: String,
    /// Parameters
    pub parameters: serde_json::Value,
    /// Success status
    pub success: bool,
    /// Result or error
    pub result: Option<String>,
}

/// A generated report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedReport {
    /// Report type
    pub report_type: String,
    /// Report content (markdown)
    pub content: String,
    /// Data included
    pub data_summary: Vec<DataSummary>,
    /// Generated at timestamp
    pub generated_at: i64,
}

/// Summary of data included in report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSummary {
    /// Data source
    pub source: String,
    /// Metric name
    pub metric: String,
    /// Data points count
    pub count: usize,
    /// Statistical summary
    pub statistics: serde_json::Value,
}

/// A notification that was sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSent {
    /// Notification channel
    pub channel: String,
    /// Recipient
    pub recipient: String,
    /// Message
    pub message: String,
    /// Sent timestamp
    pub sent_at: i64,
    /// Success status
    pub success: bool,
}

/// Query filter for agents.
#[derive(Debug, Clone, Default)]
pub struct AgentFilter {
    /// Filter by status
    pub status: Option<AgentStatus>,
    /// Filter by schedule type
    pub schedule_type: Option<ScheduleType>,
    /// Filter by creation time range (start)
    pub start_time: Option<i64>,
    /// Filter by creation time range (end)
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

/// Query filter for execution records.
#[derive(Debug, Clone, Default)]
pub struct ExecutionFilter {
    /// Filter by agent ID
    pub agent_id: Option<String>,
    /// Filter by execution status
    pub status: Option<ExecutionStatus>,
    /// Filter by start time
    pub start_time: Option<i64>,
    /// Filter by end time
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl AgentStore {
    /// Open or create an agent store at the given path.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>, Error> {
        let db = Database::create(path)?;
        let write_txn = db.begin_write()?;

        // Create tables if they don't exist
        write_txn.open_table(AGENTS_TABLE)?;
        write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
        write_txn.open_table(AGENT_MEMORY_TABLE)?;
        write_txn.commit()?;

        Ok(Arc::new(Self { db: Arc::new(db) }))
    }

    /// Create an in-memory agent store for testing.
    pub fn memory() -> Result<Arc<Self>, Error> {
        let temp_path =
            std::env::temp_dir().join(format!("agents_test_{}.redb", uuid::Uuid::new_v4()));
        Self::open(temp_path)
    }

    /// Save an agent to the store.
    pub async fn save_agent(&self, agent: &AiAgent) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(agent.id.as_str(), value.as_slice())?;

            // Also save memory separately for efficient updates
            let memory_value = serde_json::to_vec(&agent.memory)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.insert(agent.id.as_str(), memory_value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get an agent by ID.
    pub async fn get_agent(&self, id: &str) -> Result<Option<AiAgent>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        match table.get(id)? {
            Some(bytes) => {
                let mut agent: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                // Normalize memory limits for backward compatibility
                normalize_agent_memory_limits(&mut agent);

                Ok(Some(agent))
            }
            None => Ok(None),
        }
    }

    /// Query agents with filters.
    pub async fn query_agents(&self, filter: AgentFilter) -> Result<Vec<AiAgent>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let mut agents = Vec::new();

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let mut agent: AiAgent = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            // Normalize memory limits for backward compatibility
            normalize_agent_memory_limits(&mut agent);

            if self.matches_agent_filter(&agent, &filter) {
                agents.push(agent);
            }
        }

        // Sort by updated_at descending
        agents.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        // Apply pagination
        if let Some(offset) = filter.offset {
            if offset < agents.len() {
                agents = agents.into_iter().skip(offset).collect();
            } else {
                agents.clear();
            }
        }

        if let Some(limit) = filter.limit {
            agents.truncate(limit);
        }

        Ok(agents)
    }

    /// Update agent status.
    pub async fn update_agent_status(
        &self,
        id: &str,
        status: AgentStatus,
        error_message: Option<String>,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.status = status;
                ag.error_message = error_message;
                ag.updated_at = chrono::Utc::now().timestamp();
                ag
            }
            None => return Ok(()), // Agent doesn't exist
        };
        drop(table);
        drop(read_txn);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update agent parsed intent after initial parsing.
    pub async fn update_agent_parsed_intent(
        &self,
        id: &str,
        parsed_intent: Option<ParsedIntent>,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.parsed_intent = parsed_intent;
                ag.updated_at = chrono::Utc::now().timestamp();
                ag
            }
            None => return Ok(()), // Agent doesn't exist
        };
        drop(table);
        drop(read_txn);

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Update agent memory after execution.
    /// First reads the agent, then updates both tables in a single write transaction.
    pub async fn update_agent_memory(&self, id: &str, memory: AgentMemory) -> Result<(), Error> {
        // First, read the current agent data (before starting write transaction)
        let agent = {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(AGENTS_TABLE)?;
            match table.get(id)? {
                Some(bytes) => Some(
                    serde_json::from_slice::<AiAgent>(bytes.value())
                        .map_err(|e| Error::Serialization(e.to_string()))?,
                ),
                None => None,
            }
        };

        // Now start write transaction and update both tables
        let write_txn = self.db.begin_write()?;

        // Update memory in dedicated table
        {
            let memory_value =
                serde_json::to_vec(&memory).map_err(|e| Error::Serialization(e.to_string()))?;
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.insert(id, memory_value.as_slice())?;
        }

        // Also update the agent record
        if let Some(mut ag) = agent {
            ag.memory = memory;
            ag.updated_at = chrono::Utc::now().timestamp();

            let value = serde_json::to_vec(&ag).map_err(|e| Error::Serialization(e.to_string()))?;
            let mut agent_table = write_txn.open_table(AGENTS_TABLE)?;
            agent_table.insert(id, value.as_slice())?;
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Update agent stats after execution.
    /// Reads latest memory from AGENT_MEMORY_TABLE and only writes to AGENTS_TABLE.
    /// This avoids overwriting memory data that was just written by update_agent_memory.
    pub async fn update_agent_stats(
        &self,
        id: &str,
        success: bool,
        duration_ms: u64,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;

        // Read agent and latest memory in the same transaction
        let table = read_txn.open_table(AGENTS_TABLE)?;
        let memory_table = read_txn.open_table(AGENT_MEMORY_TABLE)?;

        let agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                // Try to get latest memory from memory table
                let latest_mem = match memory_table.get(id)? {
                    Some(mem_bytes) => Some(
                        serde_json::from_slice::<AgentMemory>(mem_bytes.value())
                            .map_err(|e| Error::Serialization(e.to_string()))?,
                    ),
                    None => None,
                };

                // Use latest memory if available (preserves short_term, long_term, etc.)
                if let Some(ref mem) = latest_mem {
                    ag.memory = mem.clone();
                }

                // Update stats
                ag.stats.total_executions += 1;
                if success {
                    ag.stats.successful_executions += 1;
                } else {
                    ag.stats.failed_executions += 1;
                }
                let total = ag.stats.total_executions as u64;
                ag.stats.avg_duration_ms =
                    (ag.stats.avg_duration_ms * (total - 1) + duration_ms) / total;
                ag.stats.last_duration_ms = Some(duration_ms);
                ag.last_execution_at = Some(chrono::Utc::now().timestamp());
                ag.updated_at = chrono::Utc::now().timestamp();

                ag
            }
            None => return Ok(()),
        };
        drop(table);
        drop(memory_table);
        drop(read_txn);

        // Only write to AGENTS_TABLE, not to AGENT_MEMORY_TABLE
        // (update_agent_memory handles writing to AGENT_MEMORY_TABLE)
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;

            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(id, value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Delete an agent by ID.
    pub async fn delete_agent(&self, id: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            table.remove(id)?;
        }
        {
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.remove(id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save an execution record.
    pub async fn save_execution(&self, execution: &AgentExecutionRecord) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

            let value =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(execution.id.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Save execution record and optionally append conversation turn in a single transaction.
    /// This is more efficient than calling save_execution and append_conversation_turn separately.
    pub async fn save_execution_with_conversation(
        &self,
        execution: &AgentExecutionRecord,
        agent_id: Option<&str>,
        conversation_turn: Option<&ConversationTurn>,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        // Save execution record
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
            let value =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution.id.as_str(), value.as_slice())?;
        }

        // Optionally update conversation history in the same transaction
        if let (Some(agent_id), Some(turn)) = (agent_id, conversation_turn) {
            tracing::debug!(
                agent_id = %agent_id,
                execution_id = %execution.id,
                turn_execution_id = %turn.execution_id,
                "Updating conversation history for agent"
            );
            // Get current agent state using the write transaction (it can also read)
            let mut agent = {
                let result = match write_txn.open_table(AGENTS_TABLE)?.get(agent_id)? {
                    Some(bytes) => {
                        let value = bytes.value().to_vec();
                        let a: AiAgent = serde_json::from_slice(&value)
                            .map_err(|e| Error::Serialization(e.to_string()))?;
                        Ok(a)
                    }
                    None => Err(Error::NotFound(format!("Agent {} not found", agent_id))),
                };
                result?
            };

            // Update conversation history
            let old_len = agent.conversation_history.len();
            agent.conversation_history.push(turn.clone());
            tracing::debug!(
                "[DEBUG] Conversation history: old_len={}, new_len={}",
                old_len,
                agent.conversation_history.len()
            );

            tracing::debug!(
                agent_id = %agent_id,
                old_history_len = old_len,
                new_history_len = agent.conversation_history.len(),
                "Conversation history updated"
            );

            // Trim history to prevent unbounded growth
            // Keep the most recent MAX_CONVERSATION_TURNS entries
            if agent.conversation_history.len() > Self::MAX_CONVERSATION_TURNS {
                let removed_count = agent.conversation_history.len() - Self::MAX_CONVERSATION_TURNS;
                // Remove the oldest entries (from the beginning) to keep the most recent ones
                agent.conversation_history.drain(0..removed_count);

                tracing::debug!(
                    agent_id = %agent_id,
                    removed_count = removed_count,
                    remaining_count = agent.conversation_history.len(),
                    "Trimmed conversation history to prevent unbounded growth"
                );
            }

            agent.updated_at = chrono::Utc::now().timestamp();

            // Save updated agent
            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value =
                    serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
        } else {
            tracing::warn!(
                execution_id = %execution.id,
                agent_id = ?agent_id,
                has_turn = conversation_turn.is_some(),
                "Skipping conversation history update - agent_id or turn is None"
            );
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Get an execution record by ID.
    pub async fn get_execution(&self, id: &str) -> Result<Option<AgentExecutionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        match table.get(id)? {
            Some(bytes) => {
                let execution: AgentExecutionRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(execution))
            }
            None => Ok(None),
        }
    }

    /// Query execution records with filters.
    pub async fn query_executions(
        &self,
        filter: ExecutionFilter,
    ) -> Result<Vec<AgentExecutionRecord>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        let mut executions = Vec::new();

        for item in table.iter()? {
            let (_id, bytes) = item?;
            let execution: AgentExecutionRecord = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            if self.matches_execution_filter(&execution, &filter) {
                executions.push(execution);
            }
        }

        // Sort by timestamp descending
        executions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply pagination
        if let Some(offset) = filter.offset {
            if offset < executions.len() {
                executions = executions.into_iter().skip(offset).collect();
            } else {
                executions.clear();
            }
        }

        if let Some(limit) = filter.limit {
            executions.truncate(limit);
        }

        Ok(executions)
    }

    /// Get recent executions for an agent.
    pub async fn get_agent_executions(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentExecutionRecord>, Error> {
        self.query_executions(ExecutionFilter {
            agent_id: Some(agent_id.to_string()),
            limit: Some(limit),
            ..Default::default()
        })
        .await
    }

    /// Delete old execution records.
    pub async fn cleanup_executions(&self, older_than: i64) -> Result<usize, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENT_EXECUTIONS_TABLE)?;

        let mut to_remove: Vec<String> = Vec::new();
        for item in table.iter()? {
            let (id, bytes) = item?;
            let execution: AgentExecutionRecord = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

            if execution.timestamp < older_than {
                to_remove.push(id.value().to_string());
            }
        }
        drop(table);
        drop(read_txn);

        if to_remove.is_empty() {
            return Ok(0);
        }

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENT_EXECUTIONS_TABLE)?;
            for key in &to_remove {
                table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;

        Ok(to_remove.len())
    }

    /// Check if an agent matches the given filter.
    fn matches_agent_filter(&self, agent: &AiAgent, filter: &AgentFilter) -> bool {
        if let Some(status) = filter.status {
            if agent.status != status {
                return false;
            }
        }

        if let Some(schedule_type) = &filter.schedule_type {
            if agent.schedule.schedule_type != *schedule_type {
                return false;
            }
        }

        if let Some(start_time) = filter.start_time {
            if agent.created_at < start_time {
                return false;
            }
        }

        if let Some(end_time) = filter.end_time {
            if agent.created_at > end_time {
                return false;
            }
        }

        true
    }

    /// Check if an execution matches the given filter.
    fn matches_execution_filter(
        &self,
        execution: &AgentExecutionRecord,
        filter: &ExecutionFilter,
    ) -> bool {
        if let Some(agent_id) = &filter.agent_id {
            if &execution.agent_id != agent_id {
                return false;
            }
        }

        if let Some(status) = filter.status {
            if execution.status != status {
                return false;
            }
        }

        if let Some(start_time) = filter.start_time {
            if execution.timestamp < start_time {
                return false;
            }
        }

        if let Some(end_time) = filter.end_time {
            if execution.timestamp > end_time {
                return false;
            }
        }

        true
    }

    // ========== Conversation History Methods ==========

    /// Maximum number of conversation turns to keep in history.
    /// Older turns are automatically removed to prevent unbounded growth.
    const MAX_CONVERSATION_TURNS: usize = 20;

    /// Append a new conversation turn to an agent's history.
    /// Automatically trims history to MAX_CONVERSATION_TURNS to prevent unbounded growth.
    pub async fn append_conversation_turn(
        &self,
        agent_id: &str,
        turn: &ConversationTurn,
    ) -> Result<(), Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        agent.conversation_history.push(turn.clone());

        // Trim history to prevent unbounded growth
        // Keep the most recent MAX_CONVERSATION_TURNS entries
        if agent.conversation_history.len() > Self::MAX_CONVERSATION_TURNS {
            let removed_count = agent.conversation_history.len() - Self::MAX_CONVERSATION_TURNS;
            // Remove the oldest entries (from the beginning) to keep the most recent ones
            agent.conversation_history.drain(0..removed_count);

            tracing::debug!(
                agent_id = %agent_id,
                removed_count = removed_count,
                remaining_count = agent.conversation_history.len(),
                "Trimmed conversation history to prevent unbounded growth"
            );
        }

        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    /// Get recent conversation turns for an agent.
    pub async fn get_conversation_history(
        &self,
        agent_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ConversationTurn>, Error> {
        let agent = self.get_agent(agent_id).await?;
        let agent =
            agent.ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let history = &agent.conversation_history;
        if let Some(limit) = limit {
            if history.len() > limit {
                Ok(history[history.len() - limit..].to_vec())
            } else {
                Ok(history.clone())
            }
        } else {
            Ok(history.clone())
        }
    }

    /// Compress conversation history by keeping recent turns and summarizing old ones.
    pub async fn compress_conversation(
        &self,
        agent_id: &str,
        keep_recent: usize,
        summary: String,
    ) -> Result<(), Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        if agent.conversation_history.len() > keep_recent {
            // Remove old turns, keeping only the most recent ones
            agent.conversation_history = agent
                .conversation_history
                .split_off(agent.conversation_history.len() - keep_recent);
            agent.conversation_summary = Some(summary);
            agent.updated_at = chrono::Utc::now().timestamp();

            // Save the updated agent
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value =
                    serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
            write_txn.commit()?;
        }

        Ok(())
    }

    /// Clear all conversation history for an agent.
    pub async fn clear_conversation_history(&self, agent_id: &str) -> Result<(), Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        agent.conversation_history.clear();
        agent.conversation_summary = None;
        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
    }

    // ========== User Message Methods ==========

    /// Maximum number of user messages to keep.
    const MAX_USER_MESSAGES: usize = 50;

    /// Add a user message to an agent.
    pub async fn add_user_message(
        &self,
        agent_id: &str,
        content: String,
        message_type: Option<String>,
    ) -> Result<UserMessage, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let message = UserMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            content,
            message_type,
        };

        agent.user_messages.push(message.clone());

        // Trim old messages if needed
        if agent.user_messages.len() > Self::MAX_USER_MESSAGES {
            let removed_count = agent.user_messages.len() - Self::MAX_USER_MESSAGES;
            agent.user_messages = agent.user_messages.split_off(Self::MAX_USER_MESSAGES);

            tracing::debug!(
                agent_id = %agent_id,
                removed_count = removed_count,
                remaining_count = agent.user_messages.len(),
                "Trimmed user messages to prevent unbounded growth"
            );
        }

        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(message)
    }

    /// Get user messages for an agent.
    pub async fn get_user_messages(
        &self,
        agent_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<UserMessage>, Error> {
        let agent = self.get_agent(agent_id).await?;
        let agent =
            agent.ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let messages = &agent.user_messages;
        if let Some(limit) = limit {
            if messages.len() > limit {
                Ok(messages[messages.len() - limit..].to_vec())
            } else {
                Ok(messages.clone())
            }
        } else {
            Ok(messages.clone())
        }
    }

    /// Delete a specific user message.
    pub async fn delete_user_message(
        &self,
        agent_id: &str,
        message_id: &str,
    ) -> Result<bool, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let original_len = agent.user_messages.len();
        agent.user_messages.retain(|m| m.id != message_id);

        if agent.user_messages.len() < original_len {
            agent.updated_at = chrono::Utc::now().timestamp();

            // Save the updated agent
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value =
                    serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
            write_txn.commit()?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clear all user messages for an agent.
    pub async fn clear_user_messages(&self, agent_id: &str) -> Result<usize, Error> {
        let mut agent = self
            .get_agent(agent_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        let count = agent.user_messages.len();
        agent.user_messages.clear();
        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value =
                serde_json::to_vec(&agent).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Arc<AgentStore> {
        AgentStore::memory().unwrap()
    }

    #[tokio::test]
    async fn test_save_and_get_agent() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Temperature Monitor".to_string(),
            description: None,
            user_prompt: "Monitor warehouse temperatures and alert if above 30°C".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            error_message: None,
        };

        store.save_agent(&agent).await.unwrap();
        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "agent-1");
        assert_eq!(retrieved.name, "Temperature Monitor");
    }

    #[tokio::test]
    async fn test_update_agent_status() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Test Agent".to_string(),
            description: None,
            user_prompt: "Test".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            error_message: None,
        };

        store.save_agent(&agent).await.unwrap();
        store
            .update_agent_status("agent-1", AgentStatus::Paused, None)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.status, AgentStatus::Paused);
    }

    #[tokio::test]
    async fn test_save_and_get_execution() {
        let store = test_store();

        let execution = AgentExecutionRecord {
            id: "exec-1".to_string(),
            agent_id: "agent-1".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            trigger_type: "schedule".to_string(),
            status: ExecutionStatus::Completed,
            decision_process: DecisionProcess {
                situation_analysis: "Temperature is normal".to_string(),
                data_collected: vec![],
                reasoning_steps: vec![],
                decisions: vec![],
                conclusion: "No action needed".to_string(),
                confidence: 0.95,
            },
            result: None,
            duration_ms: 150,
            error: None,
        };

        store.save_execution(&execution).await.unwrap();
        let retrieved = store.get_execution("exec-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "exec-1");
        assert_eq!(retrieved.agent_id, "agent-1");
    }

    #[tokio::test]
    async fn test_agent_memory_persistence() {
        let store = test_store();

        let mut agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Learning Agent".to_string(),
            description: None,
            user_prompt: "Learn patterns".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            error_message: None,
        };

        // Save initial agent
        store.save_agent(&agent).await.unwrap();

        // Update memory
        agent
            .memory
            .state_variables
            .insert("baseline_temp".to_string(), serde_json::json!(25.0));
        agent.memory.trend_data.push(TrendPoint {
            timestamp: chrono::Utc::now().timestamp(),
            metric: "temperature".to_string(),
            value: 25.5,
            context: None,
        });

        store
            .update_agent_memory("agent-1", agent.memory.clone())
            .await
            .unwrap();

        // Retrieve and verify
        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(
            retrieved.memory.state_variables.get("baseline_temp"),
            Some(&serde_json::json!(25.0))
        );
        assert_eq!(retrieved.memory.trend_data.len(), 1);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Stats Agent".to_string(),
            description: None,
            user_prompt: "Test stats".to_string(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Interval,
                cron_expression: None,
                interval_seconds: Some(300),
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 3,
            error_message: None,
        };

        store.save_agent(&agent).await.unwrap();

        // Record successful execution
        store
            .update_agent_stats("agent-1", true, 200)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.stats.total_executions, 1);
        assert_eq!(retrieved.stats.successful_executions, 1);
        assert_eq!(retrieved.stats.avg_duration_ms, 200);

        // Record failed execution
        store
            .update_agent_stats("agent-1", false, 100)
            .await
            .unwrap();

        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.stats.total_executions, 2);
        assert_eq!(retrieved.stats.failed_executions, 1);
        assert_eq!(retrieved.stats.avg_duration_ms, 150); // (200 + 100) / 2
    }
}
