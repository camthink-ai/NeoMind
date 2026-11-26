//! Five-table agent persistence layout and [`AgentRecordStore`].
//!
//! One-way startup migration from residual legacy `agents` tables lives in
//! [`crate::agent_migration`].
//!
//! Storage must not depend on `neomind-ai-agent` — DTO shapes here mirror that
//! crate's contracts so rows stay JSON-compatible across the boundary.

use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::agents::{AgentMemory, AgentStatus, UserMessage};
use crate::error::{Error, Result};

// ── Table names ─────────────────────────────────────────────────────────────

pub(crate) const LEGACY_AGENTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agents");
pub(crate) const AGENT_SPECS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_specs");
pub(crate) const AGENT_RUNTIME_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_runtime");
pub(crate) const AGENT_EXECUTIONS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_executions");
pub(crate) const AGENT_MEMORY_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_memory");
pub(crate) const AGENT_USER_MESSAGES_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("agent_user_messages");

// ── Storage DTO types (JSON-compatible with neomind-ai-agent) ───────────────

/// Tool access policy for a managed agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    Disabled,
    Scoped {
        allowed_tools: Vec<String>,
        #[serde(default)]
        allowed_commands: Vec<String>,
    },
    Unrestricted,
}

/// Three-way notify policy (complete / alert / failure) + migration flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NotifyPolicy {
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub on_complete: bool,
    #[serde(default)]
    pub on_alert: bool,
    #[serde(default)]
    pub alert_depends_on: Vec<String>,
    #[serde(default)]
    pub on_failure: bool,
    #[serde(default)]
    pub pending_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSpec {
    Cron {
        expr: String,
        timezone: String,
    },
    Interval {
        every_secs: u64,
    },
    Event {
        #[serde(default)]
        filter: serde_json::Value,
    },
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PacingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_merge_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interval_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs_per_day: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_failure_threshold() -> u32 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputContract {
    #[serde(default)]
    pub requirements: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputContract {
    #[serde(default)]
    pub fields: Vec<serde_json::Value>,
    #[serde(default)]
    pub publish: bool,
}

/// Frozen static configuration written to `agent_specs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpecRecord {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub user_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_backend_id: Option<String>,
    pub trigger: TriggerSpec,
    #[serde(default)]
    pub pacing: PacingPolicy,
    #[serde(default)]
    pub input: InputContract,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub output: OutputContract,
    #[serde(default)]
    pub notify: NotifyPolicy,
    #[serde(default)]
    pub controlled_actions: Vec<serde_json::Value>,
    pub config_version: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AgentRuntimeStats {
    pub total_runs: u64,
    pub successful_runs: u64,
    pub failed_runs: u64,
    #[serde(default)]
    pub partial_runs: u64,
    #[serde(default)]
    pub skipped_daily_limit: u64,
    #[serde(default)]
    pub skipped_permit_timeout: u64,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub runs_today: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_day: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_execution_id: Option<String>,
    #[serde(default)]
    pub avg_duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_duration_ms: Option<u64>,
}

/// High-frequency runtime row written to `agent_runtime`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRuntimeRecord {
    pub agent_id: String,
    pub status: AgentStatus,
    pub stats: AgentRuntimeStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_probe_at: Option<i64>,
    pub updated_at: i64,
}

/// Execution outcome written to `agent_executions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionStatus {
    Completed,
    Partial,
    Skipped,
    Failed,
}

/// Delivery intent / receipt state for a side-effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeliveryState {
    IntentPending,
    Rejected { reason: String },
    Accepted,
    Confirmed,
    OutcomeUnknown { reason: String },
}

/// Unique send-intent + receipt keyed by `(execution_id, destination, fingerprint)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDeliveryRecord {
    pub execution_id: String,
    pub destination: String,
    pub action_fingerprint: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub idempotent: bool,
    pub state: DeliveryState,
}

/// Execution row written to `agent_executions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentExecutionRow {
    pub id: String,
    pub agent_id: String,
    pub config_version: u64,
    pub status: AgentExecutionStatus,
    pub started_at: i64,
    pub finished_at: i64,
    pub is_probe: bool,
    pub dry_run: bool,
    /// How this run was submitted: `schedule` | `event` | `manual` | `rule` | `extension`.
    /// Absent on rows written before trigger provenance was persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_config: Option<bool>,
    #[serde(default)]
    pub evidence: serde_json::Value,
    /// Deliveries stored on the execution row (unique claim via transactional RMW).
    #[serde(default)]
    pub deliveries: Vec<ActionDeliveryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    /// Structured JSON fields (OutputContract projection); optional for older rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_verdict: Option<String>,
}

fn delivery_matches(rec: &ActionDeliveryRecord, destination: &str, fingerprint: &str) -> bool {
    rec.destination == destination && rec.action_fingerprint == fingerprint
}

// ── Store ───────────────────────────────────────────────────────────────────

/// Create the five agent tables if missing.
pub fn ensure_agent_tables(db: &Database) -> Result<()> {
    let write = db.begin_write()?;
    write.open_table(AGENT_SPECS_TABLE)?;
    write.open_table(AGENT_RUNTIME_TABLE)?;
    write.open_table(AGENT_EXECUTIONS_TABLE)?;
    write.open_table(AGENT_MEMORY_TABLE)?;
    write.open_table(AGENT_USER_MESSAGES_TABLE)?;
    write.commit()?;
    Ok(())
}

/// Persistent agent store over the five agent tables.
pub struct AgentRecordStore {
    db: Arc<Database>,
}

impl AgentRecordStore {
    /// Open or create a store at `path`, ensuring the five agent tables exist.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Arc<Self>> {
        Self::from_db(Database::create(path)?)
    }

    /// In-memory store (tests).
    pub fn memory() -> Result<Arc<Self>> {
        Self::from_db(
            Database::builder()
                .create_with_backend(redb::backends::InMemoryBackend::new())
                .map_err(|e| Error::Storage(e.to_string()))?,
        )
    }

    /// Take ownership of a [`Database`] and ensure tables.
    pub fn from_db(db: Database) -> Result<Arc<Self>> {
        Self::from_arc(Arc::new(db))
    }

    /// Share an existing [`Database`].
    pub fn from_arc(db: Arc<Database>) -> Result<Arc<Self>> {
        ensure_agent_tables(&db)?;
        Ok(Arc::new(Self { db }))
    }

    pub fn db(&self) -> &Arc<Database> {
        &self.db
    }

    // ── Specs ───────────────────────────────────────────────────────────────

    pub fn save_spec(&self, spec: &AgentSpecRecord) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_SPECS_TABLE)?;
            let bytes =
                serde_json::to_vec(spec).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(spec.id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_spec(&self, id: &str) -> Result<Option<AgentSpecRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_SPECS_TABLE)?;
        match table.get(id)? {
            Some(bytes) => {
                let spec: AgentSpecRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(spec))
            }
            None => Ok(None),
        }
    }

    pub fn list_specs(&self) -> Result<Vec<AgentSpecRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_SPECS_TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, bytes) = entry?;
            match serde_json::from_slice::<AgentSpecRecord>(bytes.value()) {
                Ok(spec) => out.push(spec),
                Err(e) => {
                    tracing::warn!("skip corrupt agent_specs row: {e}");
                }
            }
        }
        Ok(out)
    }

    pub fn delete_spec(&self, id: &str) -> Result<bool> {
        let write = self.db.begin_write()?;
        let removed = {
            let mut specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existed = specs.get(id)?.is_some();
            if existed {
                specs.remove(id)?;
            }
            existed
        };
        if removed {
            let mut runtime = write.open_table(AGENT_RUNTIME_TABLE)?;
            let _ = runtime.remove(id)?;
        }
        write.commit()?;
        Ok(removed)
    }

    /// Drop every row for `id` (spec, runtime, memory, user messages).
    pub fn delete_agent_rows(&self, id: &str) -> Result<bool> {
        let write = self.db.begin_write()?;
        let removed = {
            let mut specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existed = specs.get(id)?.is_some();
            if existed {
                specs.remove(id)?;
            }
            existed
        };
        {
            let mut runtime = write.open_table(AGENT_RUNTIME_TABLE)?;
            let _ = runtime.remove(id)?;
        }
        {
            let mut memory = write.open_table(AGENT_MEMORY_TABLE)?;
            let _ = memory.remove(id)?;
        }
        {
            let mut messages = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let _ = messages.remove(id)?;
        }
        write.commit()?;
        Ok(removed)
    }

    // ── Runtime ─────────────────────────────────────────────────────────────

    pub fn get_runtime(&self, agent_id: &str) -> Result<Option<AgentRuntimeRecord>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_RUNTIME_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let rt: AgentRuntimeRecord = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(rt))
            }
            None => Ok(None),
        }
    }

    pub fn set_runtime(&self, runtime: &AgentRuntimeRecord) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let bytes =
                serde_json::to_vec(runtime).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(runtime.agent_id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Single-write-txn stats tick: increments counters without rewriting status.
    ///
    /// Does not revive a paused/stopped agent — only mutates `stats` (+ `updated_at`).
    /// Concurrent callers serialize on the redb write lock so both increments stick.
    pub fn apply_runtime_stats_tick(
        &self,
        agent_id: &str,
        success: bool,
        duration_ms: u64,
        now: i64,
    ) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existing = table.get(agent_id)?.map(|g| g.value().to_vec());
            let mut rt = match existing {
                Some(bytes) => serde_json::from_slice::<AgentRuntimeRecord>(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None if specs.get(agent_id)?.is_some() => AgentRuntimeRecord {
                    agent_id: agent_id.to_string(),
                    status: AgentStatus::Active,
                    stats: AgentRuntimeStats::default(),
                    error_message: None,
                    next_probe_at: None,
                    updated_at: now,
                },
                None => {
                    return Ok(());
                }
            };
            rt.stats.total_runs = rt.stats.total_runs.saturating_add(1);
            if success {
                rt.stats.successful_runs = rt.stats.successful_runs.saturating_add(1);
                rt.stats.consecutive_failures = 0;
            } else {
                rt.stats.failed_runs = rt.stats.failed_runs.saturating_add(1);
            }
            let total = rt.stats.total_runs.max(1);
            rt.stats.avg_duration_ms = (rt
                .stats
                .avg_duration_ms
                .saturating_mul(total - 1)
                .saturating_add(duration_ms))
                / total;
            rt.stats.last_duration_ms = Some(duration_ms);
            rt.stats.last_execution_at = Some(now);
            rt.updated_at = now;
            let bytes = serde_json::to_vec(&rt).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Single-write-txn status update (does not invent a row after delete).
    pub fn apply_runtime_status(
        &self,
        agent_id: &str,
        status: AgentStatus,
        error_message: Option<String>,
        now: i64,
    ) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_RUNTIME_TABLE)?;
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            let existing = table.get(agent_id)?.map(|g| g.value().to_vec());
            let mut rt = match existing {
                Some(bytes) => serde_json::from_slice::<AgentRuntimeRecord>(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None if specs.get(agent_id)?.is_some() => AgentRuntimeRecord {
                    agent_id: agent_id.to_string(),
                    status,
                    stats: AgentRuntimeStats::default(),
                    error_message: error_message.clone(),
                    next_probe_at: None,
                    updated_at: now,
                },
                None => return Ok(()),
            };
            rt.status = status;
            rt.error_message = error_message;
            rt.updated_at = now;
            let bytes = serde_json::to_vec(&rt).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    // ── Executions ──────────────────────────────────────────────────────────

    pub fn save_execution(&self, execution: &AgentExecutionRow) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let bytes =
                serde_json::to_vec(execution).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution.id.as_str(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_execution(&self, id: &str) -> Result<Option<AgentExecutionRow>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_EXECUTIONS_TABLE)?;
        match table.get(id)? {
            Some(bytes) => {
                let exec: AgentExecutionRow = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(exec))
            }
            None => Ok(None),
        }
    }

    /// Formal (non-`dry_run`) executions for `agent_id`, newest `finished_at` first.
    pub fn list_formal_executions(&self, agent_id: &str) -> Result<Vec<AgentExecutionRow>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_EXECUTIONS_TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, bytes) = entry?;
            let exec: AgentExecutionRow = match serde_json::from_slice(bytes.value()) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("skip corrupt agent_executions row: {e}");
                    continue;
                }
            };
            if exec.agent_id == agent_id && !exec.dry_run {
                out.push(exec);
            }
        }
        out.sort_by(|a, b| b.finished_at.cmp(&a.finished_at));
        Ok(out)
    }

    /// Insert `IntentPending` uniquely on the execution row.
    ///
    /// Returns `Ok(true)` when this caller obtained send rights; `Ok(false)` if
    /// a delivery for the same `(execution_id, destination, fingerprint)` already
    /// exists (any state).
    pub fn try_claim_delivery(&self, record: ActionDeliveryRecord) -> Result<bool> {
        let write = self.db.begin_write()?;
        let claimed = {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let existing_bytes = match table.get(record.execution_id.as_str())? {
                Some(guard) => guard.value().to_vec(),
                None => {
                    return Err(Error::NotFound(format!(
                        "execution {}",
                        record.execution_id
                    )));
                }
            };
            let mut exec: AgentExecutionRow = serde_json::from_slice(&existing_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            if exec
                .deliveries
                .iter()
                .any(|d| delivery_matches(d, &record.destination, &record.action_fingerprint))
            {
                false
            } else {
                let exec_id = exec.id.clone();
                exec.deliveries.push(record);
                let bytes =
                    serde_json::to_vec(&exec).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(exec_id.as_str(), bytes.as_slice())?;
                true
            }
        };
        write.commit()?;
        Ok(claimed)
    }

    pub fn update_delivery_state(
        &self,
        execution_id: &str,
        destination: &str,
        fingerprint: &str,
        state: DeliveryState,
    ) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let existing_bytes = match table.get(execution_id)? {
                Some(guard) => guard.value().to_vec(),
                None => {
                    return Err(Error::NotFound(format!("execution {execution_id}")));
                }
            };
            let mut exec: AgentExecutionRow = serde_json::from_slice(&existing_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            let rec = exec
                .deliveries
                .iter_mut()
                .find(|d| delivery_matches(d, destination, fingerprint))
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "delivery {execution_id}/{destination}/{fingerprint}"
                    ))
                })?;
            rec.state = state;
            let bytes =
                serde_json::to_vec(&exec).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(execution_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Mark every `IntentPending` delivery as `OutcomeUnknown` (crash / restart).
    ///
    /// Does not resend — receiver count must stay at whatever already landed.
    pub fn recover_stale_intents(&self) -> Result<Vec<ActionDeliveryRecord>> {
        let write = self.db.begin_write()?;
        let mut out = Vec::new();
        {
            let mut table = write.open_table(AGENT_EXECUTIONS_TABLE)?;
            let keys: Vec<String> = table
                .iter()?
                .filter_map(|e| e.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            for key in keys {
                let existing = {
                    let Some(guard) = table.get(key.as_str())? else {
                        continue;
                    };
                    guard.value().to_vec()
                };
                let mut exec: AgentExecutionRow = match serde_json::from_slice(&existing) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("skip corrupt agent_executions row on recover: {e}");
                        continue;
                    }
                };
                let mut changed = false;
                for d in &mut exec.deliveries {
                    if matches!(d.state, DeliveryState::IntentPending) {
                        d.state = DeliveryState::OutcomeUnknown {
                            reason: "crashed_before_receipt".into(),
                        };
                        out.push(d.clone());
                        changed = true;
                    }
                }
                if changed {
                    let bytes = serde_json::to_vec(&exec)
                        .map_err(|e| Error::Serialization(e.to_string()))?;
                    table.insert(key.as_str(), bytes.as_slice())?;
                }
            }
        }
        write.commit()?;
        Ok(out)
    }

    // ── Memory / user messages ──────────────────────────────────────────────

    pub fn save_memory(&self, agent_id: &str, memory: &AgentMemory) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_MEMORY_TABLE)?;
            let bytes =
                serde_json::to_vec(memory).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Atomic single-write-txn memory update: writes only when the agent spec
    /// still exists, so a concurrent delete can never leave an orphan memory row.
    pub fn apply_memory_if_exists(&self, agent_id: &str, memory: &AgentMemory) -> Result<bool> {
        self.apply_memory_with_hook(agent_id, memory, || {})
    }

    pub(crate) fn apply_memory_with_hook<F: FnOnce()>(
        &self,
        agent_id: &str,
        memory: &AgentMemory,
        after_begin_write: F,
    ) -> Result<bool> {
        let bytes = serde_json::to_vec(memory).map_err(|e| Error::Serialization(e.to_string()))?;
        let write = self.db.begin_write()?;
        after_begin_write();
        let written = {
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            if specs.get(agent_id)?.is_some() {
                let mut table = write.open_table(AGENT_MEMORY_TABLE)?;
                table.insert(agent_id, bytes.as_slice())?;
                true
            } else {
                false
            }
        };
        write.commit()?;
        Ok(written)
    }

    /// Append one execution journal record atomically if the agent still exists.
    /// Enforces `memory.journal.max_records` FIFO cap.
    pub fn append_journal_entry(
        &self,
        agent_id: &str,
        record: crate::agents::ExecutionRecord,
    ) -> Result<bool> {
        let write = self.db.begin_write()?;
        let written = {
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            if specs.get(agent_id)?.is_some() {
                let mut table = write.open_table(AGENT_MEMORY_TABLE)?;
                let mut memory = match table.get(agent_id)?.map(|g| g.value().to_vec()) {
                    Some(bytes) => serde_json::from_slice::<AgentMemory>(&bytes)
                        .unwrap_or_else(|_| AgentMemory::default()),
                    None => AgentMemory::default(),
                };
                let cap = memory.journal.max_records.max(1);
                memory.journal.records.push(record);
                if memory.journal.records.len() > cap {
                    let excess = memory.journal.records.len() - cap;
                    memory.journal.records.drain(..excess);
                }
                memory.updated_at = chrono::Utc::now().timestamp();
                let bytes =
                    serde_json::to_vec(&memory).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, bytes.as_slice())?;
                true
            } else {
                false
            }
        };
        write.commit()?;
        Ok(written)
    }

    pub fn get_memory(&self, agent_id: &str) -> Result<Option<AgentMemory>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_MEMORY_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let mem: AgentMemory = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(mem))
            }
            None => Ok(None),
        }
    }

    pub fn save_user_messages(&self, agent_id: &str, messages: &[UserMessage]) -> Result<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let bytes =
                serde_json::to_vec(messages).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_user_messages(&self, agent_id: &str) -> Result<Option<Vec<UserMessage>>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(AGENT_USER_MESSAGES_TABLE)?;
        match table.get(agent_id)? {
            Some(bytes) => {
                let msgs: Vec<UserMessage> = serde_json::from_slice(bytes.value())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                Ok(Some(msgs))
            }
            None => Ok(None),
        }
    }

    /// Add a user message to an agent (atomic check-spec + append + FIFO trim).
    pub fn add_user_message(
        &self,
        agent_id: &str,
        content: String,
        message_type: Option<String>,
        max_messages: usize,
    ) -> Result<UserMessage> {
        let message = UserMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            content,
            message_type,
        };
        let write = self.db.begin_write()?;
        {
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            if specs.get(agent_id)?.is_none() {
                return Err(Error::NotFound(format!("Agent {} not found", agent_id)));
            }
            let mut table = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let mut msgs: Vec<UserMessage> = match table.get(agent_id)?.map(|g| g.value().to_vec())
            {
                Some(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None => Vec::new(),
            };
            msgs.push(message.clone());
            let cap = max_messages.max(1);
            if msgs.len() > cap {
                let excess = msgs.len() - cap;
                msgs.drain(..excess);
            }
            let bytes =
                serde_json::to_vec(&msgs).map_err(|e| Error::Serialization(e.to_string()))?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write.commit()?;
        Ok(message)
    }

    /// Read user messages for an agent, returning NotFound if the spec does not exist.
    pub fn get_user_messages_checked(
        &self,
        agent_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<UserMessage>> {
        let read = self.db.begin_read()?;
        let specs = read.open_table(AGENT_SPECS_TABLE)?;
        if specs.get(agent_id)?.is_none() {
            return Err(Error::NotFound(format!("Agent {} not found", agent_id)));
        }
        let table = read.open_table(AGENT_USER_MESSAGES_TABLE)?;
        let msgs: Vec<UserMessage> = match table.get(agent_id)? {
            Some(bytes) => serde_json::from_slice(bytes.value())
                .map_err(|e| Error::Serialization(e.to_string()))?,
            None => Vec::new(),
        };
        if let Some(limit) = limit {
            if msgs.len() > limit {
                return Ok(msgs[msgs.len() - limit..].to_vec());
            }
        }
        Ok(msgs)
    }

    /// Delete one user message by id for an agent.
    pub fn delete_user_message(&self, agent_id: &str, message_id: &str) -> Result<bool> {
        let write = self.db.begin_write()?;
        let removed = {
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            if specs.get(agent_id)?.is_none() {
                return Err(Error::NotFound(format!("Agent {} not found", agent_id)));
            }
            let mut table = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let mut msgs: Vec<UserMessage> = match table.get(agent_id)?.map(|g| g.value().to_vec())
            {
                Some(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None => Vec::new(),
            };
            let before = msgs.len();
            msgs.retain(|m| m.id != message_id);
            let changed = msgs.len() < before;
            if changed {
                let bytes =
                    serde_json::to_vec(&msgs).map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, bytes.as_slice())?;
            }
            changed
        };
        write.commit()?;
        Ok(removed)
    }

    /// Clear all user messages for an agent; returns number removed.
    pub fn clear_user_messages(&self, agent_id: &str) -> Result<usize> {
        let write = self.db.begin_write()?;
        let count = {
            let specs = write.open_table(AGENT_SPECS_TABLE)?;
            if specs.get(agent_id)?.is_none() {
                return Err(Error::NotFound(format!("Agent {} not found", agent_id)));
            }
            let mut table = write.open_table(AGENT_USER_MESSAGES_TABLE)?;
            let existing: Vec<UserMessage> = match table.get(agent_id)?.map(|g| g.value().to_vec())
            {
                Some(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
                None => Vec::new(),
            };
            let count = existing.len();
            if count > 0 {
                let bytes = serde_json::to_vec(&Vec::<UserMessage>::new())
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                table.insert(agent_id, bytes.as_slice())?;
            }
            count
        };
        write.commit()?;
        Ok(count)
    }
}

// ── Legacy table detection ──────────────────────────────────────────────────

fn table_exists(
    db: &Database,
    name: TableDefinition<&str, &[u8]>,
) -> std::result::Result<bool, String> {
    let read = db.begin_read().map_err(|e| e.to_string())?;
    Ok(read.open_table(name).is_ok())
}

/// Whether the legacy single-table `agents` layout is still present.
pub fn has_legacy_agents_table(db: &Database) -> std::result::Result<bool, String> {
    table_exists(db, LEGACY_AGENTS_TABLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_migration::{map_notify_policy, map_tool_policy, migrate_legacy_agents_table};
    use crate::agents::{
        AgentNotify, AgentResource, AgentSchedule, AgentStats, AgentStatus, AgentToolConfig,
        AiAgent, ExecutionMode, NotifyOn, ResourceType, ScheduleType,
    };

    fn bare_resource(resource_type: ResourceType, resource_id: &str) -> AgentResource {
        AgentResource {
            resource_type,
            resource_id: resource_id.into(),
            name: resource_id.into(),
            config: serde_json::Value::Null,
        }
    }

    fn fixture_agent() -> AiAgent {
        AiAgent {
            id: "a1".into(),
            name: "fixture".into(),
            description: None,
            user_prompt: "p".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                event_filter: None,
                timezone: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: 1,
            updated_at: 2,
            last_execution_at: None,
            stats: AgentStats::default(),
            memory: AgentMemory::default(),
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 10,
            enable_tool_chaining: false,
            max_chain_depth: 10,
            tool_config: None,
            execution_mode: ExecutionMode::Free,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: None,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn structured_maps_to_disabled_no_privilege_expansion() {
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec!["shell".into(), "memory".into()],
        });
        let resources = vec![bare_resource(ResourceType::Command, "relay.on")];
        let policy = map_tool_policy(ExecutionMode::Structured, &cfg, &resources);
        assert_eq!(
            policy,
            ToolPolicy::Disabled,
            "Structured must stay Disabled (no privilege expansion)"
        );
    }

    #[test]
    fn tool_config_disabled_maps_to_disabled() {
        let cfg = Some(AgentToolConfig {
            enabled: false,
            allowed_tools: vec!["shell".into()],
        });
        assert_eq!(
            map_tool_policy(ExecutionMode::Free, &cfg, &[]),
            ToolPolicy::Disabled
        );
    }

    #[test]
    fn non_empty_allowlist_maps_to_scoped_with_commands() {
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec!["shell".into()],
        });
        let resources = vec![
            bare_resource(ResourceType::Command, "cam.snapshot"),
            bare_resource(ResourceType::Device, "dev-1"),
        ];
        match map_tool_policy(ExecutionMode::Focused, &cfg, &resources) {
            ToolPolicy::Scoped {
                allowed_tools,
                allowed_commands,
            } => {
                assert_eq!(allowed_tools, vec!["shell".to_string()]);
                assert_eq!(allowed_commands, vec!["cam.snapshot".to_string()]);
            }
            other => panic!("expected Scoped, got {other:?}"),
        }
    }

    #[test]
    fn empty_or_absent_allowlist_maps_to_unrestricted() {
        assert_eq!(
            map_tool_policy(ExecutionMode::Free, &None, &[]),
            ToolPolicy::Unrestricted
        );
        let cfg = Some(AgentToolConfig {
            enabled: true,
            allowed_tools: vec![],
        });
        assert_eq!(
            map_tool_policy(ExecutionMode::Focused, &cfg, &[]),
            ToolPolicy::Unrestricted
        );
    }

    #[test]
    fn empty_channels_stay_silent() {
        let notify = Some(AgentNotify {
            channels: vec![],
            on: NotifyOn::Always,
        });
        let policy = map_notify_policy(&notify);
        assert!(
            policy.channels.is_empty(),
            "empty channels must stay empty (never invent IM)"
        );
        assert!(policy.on_complete);
        assert!(!policy.on_failure);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn failure_notify_maps_on_failure_and_keeps_channels() {
        let notify = Some(AgentNotify {
            channels: vec!["ops".into()],
            on: NotifyOn::Failure,
        });
        let policy = map_notify_policy(&notify);
        assert_eq!(policy.channels, vec!["ops".to_string()]);
        assert!(policy.on_failure);
        assert!(!policy.on_complete);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn unrouted_agent_keeps_failure_floor_without_inventing_im() {
        let policy = map_notify_policy(&None);
        assert!(policy.channels.is_empty());
        assert!(policy.on_failure);
        assert!(!policy.on_complete);
        assert!(!policy.pending_confirmation);
    }

    #[test]
    fn judgment_without_clear_dest_sets_pending_confirmation() {
        let notify = Some(AgentNotify {
            channels: vec![],
            on: NotifyOn::Judgment,
        });
        let policy = map_notify_policy(&notify);
        assert!(
            policy.pending_confirmation,
            "Judgment with no channels must park for operator confirmation"
        );
        assert!(
            policy.channels.is_empty(),
            "must not auto-route Judgment to IM"
        );
        assert!(!policy.on_complete);
        assert!(!policy.on_alert);
        assert!(!policy.on_failure);
    }

    #[test]
    fn judgment_with_channels_is_not_pending() {
        let notify = Some(AgentNotify {
            channels: vec!["ops".into()],
            on: NotifyOn::Judgment,
        });
        let policy = map_notify_policy(&notify);
        assert!(!policy.pending_confirmation);
        assert_eq!(policy.channels, vec!["ops".to_string()]);
    }

    #[test]
    fn migrate_creates_five_tables_and_is_idempotent() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();

        // Seed a legacy agents row alongside a corrupt row.
        {
            let write = db.begin_write().unwrap();
            {
                let mut agents = write.open_table(LEGACY_AGENTS_TABLE).unwrap();
                let mut execs = write.open_table(AGENT_EXECUTIONS_TABLE).unwrap();
                let mut mem = write.open_table(AGENT_MEMORY_TABLE).unwrap();
                let mut agent = fixture_agent();
                agent.execution_mode = ExecutionMode::Structured;
                agent.tool_config = Some(AgentToolConfig {
                    enabled: true,
                    allowed_tools: vec!["shell".into()],
                });
                agent.notify = Some(AgentNotify {
                    channels: vec![],
                    on: NotifyOn::Judgment,
                });
                agent.user_messages = vec![UserMessage {
                    id: "m1".into(),
                    timestamp: 10,
                    content: "hi".into(),
                    message_type: None,
                }];
                let bytes = serde_json::to_vec(&agent).unwrap();
                agents.insert(agent.id.as_str(), bytes.as_slice()).unwrap();
                agents
                    .insert("corrupt-row", b"{not valid json".as_slice())
                    .unwrap();
                let _ = &mut execs;
                let _ = &mut mem;
            }
            write.commit().unwrap();
        }

        migrate_legacy_agents_table(&db).unwrap();

        {
            let read = db.begin_read().unwrap();
            assert!(read.open_table(AGENT_SPECS_TABLE).is_ok());
            assert!(read.open_table(AGENT_RUNTIME_TABLE).is_ok());
            assert!(read.open_table(AGENT_EXECUTIONS_TABLE).is_ok());
            assert!(read.open_table(AGENT_MEMORY_TABLE).is_ok());
            assert!(read.open_table(AGENT_USER_MESSAGES_TABLE).is_ok());
            assert!(
                read.open_table(LEGACY_AGENTS_TABLE).is_err(),
                "legacy agents table must be removed"
            );

            let specs = read.open_table(AGENT_SPECS_TABLE).unwrap();
            let bytes = specs.get("a1").unwrap().unwrap().value().to_vec();
            let spec: AgentSpecRecord = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(spec.tool_policy, ToolPolicy::Disabled);
            assert!(spec.notify.pending_confirmation);
            assert!(spec.notify.channels.is_empty());
            assert!(specs.get("corrupt-row").unwrap().is_none());

            let msgs = read.open_table(AGENT_USER_MESSAGES_TABLE).unwrap();
            let msg_bytes = msgs.get("a1").unwrap().unwrap().value().to_vec();
            let messages: Vec<UserMessage> = serde_json::from_slice(&msg_bytes).unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].content, "hi");
        }

        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);

        // Idempotent second pass.
        migrate_legacy_agents_table(&db).unwrap();
        let read = db.begin_read().unwrap();
        let specs = read.open_table(AGENT_SPECS_TABLE).unwrap();
        assert!(specs.get("a1").unwrap().is_some());
    }

    fn sample_spec(id: &str) -> AgentSpecRecord {
        AgentSpecRecord {
            id: id.into(),
            name: "roundtrip".into(),
            description: Some("spec round-trip".into()),
            user_prompt: "do the thing".into(),
            llm_backend_id: None,
            trigger: TriggerSpec::Manual,
            pacing: PacingPolicy::default(),
            input: InputContract::default(),
            tool_policy: ToolPolicy::Disabled,
            output: OutputContract::default(),
            notify: NotifyPolicy::default(),
            controlled_actions: Vec::new(),
            config_version: 1,
            created_at: 100,
            updated_at: 200,
        }
    }

    #[test]
    fn ensure_agent_tables_creates_five_tables() {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();

        ensure_agent_tables(&db).unwrap();

        let read = db.begin_read().unwrap();
        assert!(read.open_table(LEGACY_AGENTS_TABLE).is_err());
        assert!(read.open_table(AGENT_SPECS_TABLE).is_ok());
        assert!(read.open_table(AGENT_RUNTIME_TABLE).is_ok());
        assert!(read.open_table(AGENT_EXECUTIONS_TABLE).is_ok());
        assert!(read.open_table(AGENT_MEMORY_TABLE).is_ok());
        assert!(read.open_table(AGENT_USER_MESSAGES_TABLE).is_ok());
        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);
    }

    #[test]
    fn agent_record_store_spec_roundtrip() {
        let store = AgentRecordStore::memory().unwrap();
        let spec = sample_spec("spec-1");
        store.save_spec(&spec).unwrap();

        let got = store.get_spec("spec-1").unwrap().expect("spec present");
        assert_eq!(got, spec);
        assert_eq!(store.list_specs().unwrap().len(), 1);

        assert!(store.delete_spec("spec-1").unwrap());
        assert!(store.get_spec("spec-1").unwrap().is_none());
        assert!(store.list_specs().unwrap().is_empty());
        assert_eq!(crate::schema::CURRENT_SCHEMA_VERSION, 2);
    }

    #[test]
    fn agent_record_store_runtime_and_execution_delivery() {
        let store = AgentRecordStore::memory().unwrap();
        store.save_spec(&sample_spec("a1")).unwrap();

        let runtime = AgentRuntimeRecord {
            agent_id: "a1".into(),
            status: AgentStatus::Active,
            stats: AgentRuntimeStats::default(),
            error_message: None,
            next_probe_at: None,
            updated_at: 50,
        };
        store.set_runtime(&runtime).unwrap();
        assert_eq!(store.get_runtime("a1").unwrap().unwrap(), runtime);

        let formal = AgentExecutionRow {
            id: "exec-formal".into(),
            agent_id: "a1".into(),
            config_version: 1,
            status: AgentExecutionStatus::Completed,
            started_at: 10,
            finished_at: 20,
            is_probe: false,
            dry_run: false,
            trigger_source: Some("schedule".into()),
            stale_config: None,
            evidence: serde_json::json!({}),
            deliveries: Vec::new(),
            brief_text: Some("ok".into()),
            conclusion: None,
            structured_result: None,
            error: None,
            alert_verdict: None,
        };
        let dry = AgentExecutionRow {
            id: "exec-dry".into(),
            dry_run: true,
            finished_at: 30,
            ..formal.clone()
        };
        store.save_execution(&formal).unwrap();
        store.save_execution(&dry).unwrap();

        let formal_list = store.list_formal_executions("a1").unwrap();
        assert_eq!(formal_list.len(), 1);
        assert_eq!(formal_list[0].id, "exec-formal");

        let claim = ActionDeliveryRecord {
            execution_id: "exec-formal".into(),
            destination: "dev-1".into(),
            action_fingerprint: "fp".into(),
            depends_on: vec!["src".into()],
            idempotent: false,
            state: DeliveryState::IntentPending,
        };
        assert!(store.try_claim_delivery(claim.clone()).unwrap());
        assert!(!store.try_claim_delivery(claim).unwrap());

        store
            .update_delivery_state("exec-formal", "dev-1", "fp", DeliveryState::Accepted)
            .unwrap();
        let exec = store.get_execution("exec-formal").unwrap().unwrap();
        assert_eq!(exec.deliveries.len(), 1);
        assert_eq!(exec.deliveries[0].state, DeliveryState::Accepted);
    }

    #[test]
    fn agent_record_store_user_messages_and_journal_roundtrip() {
        let store = AgentRecordStore::memory().unwrap();
        assert!(store
            .add_user_message("ghost", "hi".into(), None, 50)
            .is_err());
        assert!(store.get_user_messages_checked("ghost", None).is_err());

        store.save_spec(&sample_spec("u1")).unwrap();
        let m1 = store
            .add_user_message("u1", "first".into(), Some("note".into()), 2)
            .unwrap();
        let m2 = store
            .add_user_message("u1", "second".into(), None, 2)
            .unwrap();
        let m3 = store
            .add_user_message("u1", "third".into(), Some("instruction".into()), 2)
            .unwrap();

        let msgs = store.get_user_messages_checked("u1", None).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, m2.id);
        assert_eq!(msgs[1].id, m3.id);
        assert!(!msgs.iter().any(|m| m.id == m1.id));

        assert!(store.delete_user_message("u1", &m2.id).unwrap());
        assert!(!store.delete_user_message("u1", &m2.id).unwrap());
        assert_eq!(store.clear_user_messages("u1").unwrap(), 1);
        assert!(store
            .get_user_messages_checked("u1", None)
            .unwrap()
            .is_empty());

        let rec = crate::agents::ExecutionRecord {
            timestamp: 100,
            execution_id: "exec-1".into(),
            outcome: "ok".into(),
            action_taken: "no action".into(),
            success: true,
            stop_reason: "completed".into(),
        };
        assert!(store.append_journal_entry("u1", rec.clone()).unwrap());
        let mem = store.get_memory("u1").unwrap().expect("memory");
        assert_eq!(mem.journal.records.len(), 1);
        assert_eq!(mem.journal.records[0].execution_id, "exec-1");

        assert!(store.delete_agent_rows("u1").unwrap());
        assert!(!store.append_journal_entry("u1", rec).unwrap());
        assert!(store.get_memory("u1").unwrap().is_none());
    }
}
