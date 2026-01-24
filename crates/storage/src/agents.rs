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
const AGENT_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_executions");
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
    /// Agent role (Monitor, Executor, Analyst)
    #[serde(default)]
    pub role: AgentRole,
    /// User's natural language description of requirements
    pub user_prompt: String,
    /// AI-generated understanding of the requirements
    pub parsed_intent: Option<ParsedIntent>,
    /// Selected resources (devices, metrics, commands)
    pub resources: Vec<AgentResource>,
    /// Schedule configuration
    pub schedule: AgentSchedule,
    /// Agent status
    pub status: AgentStatus,
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
    /// Compressed summary of old conversation turns
    #[serde(default)]
    pub conversation_summary: Option<String>,
    /// How many recent turns to include in LLM context
    #[serde(default = "default_context_window")]
    pub context_window_size: usize,
    /// Error message (if status is error)
    pub error_message: Option<String>,
}

/// Default value for context window size.
fn default_context_window() -> usize {
    10
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

/// Agent role defining its core responsibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Data monitoring and alerting
    Monitor,
    /// Device control and automation
    Executor,
    /// Data analysis and reporting
    Analyst,
}

impl Default for AgentRole {
    fn default() -> Self {
        AgentRole::Monitor
    }
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
    /// One-time execution
    Once,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for AgentStats {
    fn default() -> Self {
        Self {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            avg_duration_ms: 0,
            last_duration_ms: None,
        }
    }
}

/// Persistent memory for an agent across executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    /// State variables
    pub state_variables: HashMap<String, serde_json::Value>,
    /// Learned patterns
    pub learned_patterns: Vec<LearnedPattern>,
    /// Historical baselines
    pub baselines: HashMap<String, f64>,
    /// Trend data points (for analysis)
    pub trend_data: Vec<TrendPoint>,
    /// Last memory update
    pub updated_at: i64,
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self {
            state_variables: HashMap::new(),
            learned_patterns: Vec::new(),
            baselines: HashMap::new(),
            trend_data: Vec::new(),
            updated_at: chrono::Utc::now().timestamp(),
        }
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
                let agent: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
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
            let agent: AiAgent = serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?;

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

        let mut agent = match table.get(id)? {
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

    /// Update agent memory after execution.
    pub async fn update_agent_memory(
        &self,
        id: &str,
        memory: AgentMemory,
    ) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;

        // Update memory in dedicated table
        {
            let memory_value = serde_json::to_vec(&memory)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            let mut memory_table = write_txn.open_table(AGENT_MEMORY_TABLE)?;
            memory_table.insert(id, memory_value.as_slice())?;
        }

        // Also update the agent record
        {
            let read_txn = self.db.begin_read()?;
            let table = read_txn.open_table(AGENTS_TABLE)?;

            if let Some(bytes) = table.get(id)? {
                let mut agent: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                agent.memory = memory.clone();
                agent.updated_at = chrono::Utc::now().timestamp();
                drop(table);
                drop(read_txn);

                let value = serde_json::to_vec(&agent)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                let mut agent_table = write_txn.open_table(AGENTS_TABLE)?;
                agent_table.insert(id, value.as_slice())?;
            }
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Update agent stats after execution.
    pub async fn update_agent_stats(
        &self,
        id: &str,
        success: bool,
        duration_ms: u64,
    ) -> Result<(), Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS_TABLE)?;

        let mut agent = match table.get(id)? {
            Some(bytes) => {
                let mut ag: AiAgent = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                ag.stats.total_executions += 1;
                if success {
                    ag.stats.successful_executions += 1;
                } else {
                    ag.stats.failed_executions += 1;
                }
                // Update average duration
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

            let value = serde_json::to_vec(execution)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            table.insert(execution.id.as_str(), value.as_slice())?;
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
        if let Some(status) = filter.status && agent.status != status {
            return false;
        }

        if let Some(schedule_type) = &filter.schedule_type
            && agent.schedule.schedule_type != *schedule_type {
            return false;
        }

        if let Some(start_time) = filter.start_time && agent.created_at < start_time {
            return false;
        }

        if let Some(end_time) = filter.end_time && agent.created_at > end_time {
            return false;
        }

        true
    }

    /// Check if an execution matches the given filter.
    fn matches_execution_filter(
        &self,
        execution: &AgentExecutionRecord,
        filter: &ExecutionFilter,
    ) -> bool {
        if let Some(agent_id) = &filter.agent_id && &execution.agent_id != agent_id {
            return false;
        }

        if let Some(status) = filter.status && execution.status != status {
            return false;
        }

        if let Some(start_time) = filter.start_time && execution.timestamp < start_time {
            return false;
        }

        if let Some(end_time) = filter.end_time && execution.timestamp > end_time {
            return false;
        }

        true
    }

    // ========== Conversation History Methods ==========

    /// Append a new conversation turn to an agent's history.
    pub async fn append_conversation_turn(
        &self,
        agent_id: &str,
        turn: &ConversationTurn,
    ) -> Result<(), Error> {
        let mut agent = self.get_agent(agent_id).await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        agent.conversation_history.push(turn.clone());
        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value = serde_json::to_vec(&agent)
                .map_err(|e| Error::Serialization(e.to_string()))?;
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
        let agent = agent.ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

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
        let mut agent = self.get_agent(agent_id).await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        if agent.conversation_history.len() > keep_recent {
            // Remove old turns, keeping only the most recent ones
            agent.conversation_history = agent.conversation_history
                .split_off(agent.conversation_history.len() - keep_recent);
            agent.conversation_summary = Some(summary);
            agent.updated_at = chrono::Utc::now().timestamp();

            // Save the updated agent
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(AGENTS_TABLE)?;
                let value = serde_json::to_vec(&agent)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, value.as_slice())?;
            }
            write_txn.commit()?;
        }

        Ok(())
    }

    /// Clear all conversation history for an agent.
    pub async fn clear_conversation_history(&self, agent_id: &str) -> Result<(), Error> {
        let mut agent = self.get_agent(agent_id).await?
            .ok_or_else(|| Error::NotFound(format!("Agent {} not found", agent_id)))?;

        agent.conversation_history.clear();
        agent.conversation_summary = None;
        agent.updated_at = chrono::Utc::now().timestamp();

        // Save the updated agent
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS_TABLE)?;
            let value = serde_json::to_vec(&agent)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, value.as_slice())?;
        }
        write_txn.commit()?;

        Ok(())
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
            role: AgentRole::Monitor,
            user_prompt: "Monitor warehouse temperatures and alert if above 30°C".to_string(),
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
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            conversation_summary: None,
            context_window_size: 10,
            error_message: None,
        };

        store.save_agent(&agent).await.unwrap();
        let retrieved = store.get_agent("agent-1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "agent-1");
        assert_eq!(retrieved.name, "Temperature Monitor");
        assert_eq!(retrieved.role, AgentRole::Monitor);
    }

    #[tokio::test]
    async fn test_update_agent_status() {
        let store = test_store();

        let agent = AiAgent {
            id: "agent-1".to_string(),
            name: "Test Agent".to_string(),
            role: AgentRole::Monitor,
            user_prompt: "Test".to_string(),
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
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            conversation_summary: None,
            context_window_size: 10,
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
            role: AgentRole::Analyst,
            user_prompt: "Learn patterns".to_string(),
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
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            conversation_summary: None,
            context_window_size: 10,
            error_message: None,
        };

        // Save initial agent
        store.save_agent(&agent).await.unwrap();

        // Update memory
        agent.memory.state_variables.insert(
            "baseline_temp".to_string(),
            serde_json::json!(25.0),
        );
        agent.memory.trend_data.push(TrendPoint {
            timestamp: chrono::Utc::now().timestamp(),
            metric: "temperature".to_string(),
            value: 25.5,
            context: None,
        });

        store.update_agent_memory("agent-1", agent.memory.clone()).await.unwrap();

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
            role: AgentRole::Monitor,
            user_prompt: "Test stats".to_string(),
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
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            conversation_summary: None,
            context_window_size: 10,
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
