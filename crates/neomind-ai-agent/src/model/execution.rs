//! Execution records, evidence, and delivery receipts.

use serde::{Deserialize, Serialize};

use super::spec::InputModality;

pub const EXECUTION_ID_KEY: &str = "execution_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Completed,
    Partial,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceQuality {
    Fresh,
    Stale,
    Missing,
    Error { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceDetail {
    Vision {
        image_source_field: String,
        vision_path: String,
    },
    TimeSeries {
        points: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
    },
    Structured {
        value: serde_json::Value,
    },
    Text {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub key: String,
    pub source_id: String,
    pub result_type: InputModality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_window: Option<(i64, i64)>,
    pub fetched_at: i64,
    pub quality: EvidenceQuality,
    pub binding_proof: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<EvidenceDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EvidenceReport {
    pub items: Vec<EvidenceItem>,
    #[serde(default)]
    pub missing_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeliveryState {
    IntentPending,
    Rejected { reason: String },
    Accepted,
    Confirmed,
    OutcomeUnknown { reason: String },
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentExecutionRecord {
    pub id: String,
    pub agent_id: String,
    pub config_version: u64,
    pub status: ExecutionStatus,
    pub started_at: i64,
    pub finished_at: i64,
    pub is_probe: bool,
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_config: Option<bool>,
    pub evidence: EvidenceReport,
    #[serde(default)]
    pub deliveries: Vec<ActionDeliveryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    /// Structured JSON fields from the run (OutputContract projection).
    /// Readable like legacy Structured mode via API/CLI/Chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub alert_verdict: Option<AlertVerdict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertVerdict {
    CriteriaMet,
    CriteriaNotMet,
    InsufficientEvidence,
}

/// Response envelope for preview / dry-run / formal invoke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResponse {
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<AgentExecutionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_plan: Option<super::spec::EffectivePlan>,
    pub external_read_counts: super::spec::ExternalReadCounts,
    pub receiver_counts: ReceiverCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReceiverCounts {
    pub notify: u64,
    pub device_control: u64,
    pub extension_write: u64,
}
