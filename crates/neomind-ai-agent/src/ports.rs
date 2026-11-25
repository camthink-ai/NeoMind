//! Host-injected ports for LLM, tools, vision, and image fetch.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AgentError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    ReadOnly,
    SideEffect,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationCapability {
    pub class: CapabilityClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub idempotent: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmBackendCapabilities {
    pub multimodal: bool,
    pub tools: bool,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmRequest {
    pub backend_id: Option<String>,
    pub system: String,
    pub user: String,
    pub tools_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    pub text: String,
    #[serde(default)]
    pub alert_criteria_met: bool,
}

#[async_trait]
pub trait LlmPort: Send + Sync {
    async fn resolve_backend(&self, backend_id: Option<&str>) -> Result<LlmBackendCapabilities>;

    async fn complete(&self, request: LlmRequest, cancel: CancellationToken)
        -> Result<LlmResponse>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolFilter {
    pub allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub capability: OperationCapability,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolExecContext {
    pub dry_run: bool,
    pub execution_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub ok: bool,
    pub value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[async_trait]
pub trait ToolRegistryPort: Send + Sync {
    fn list_definitions(&self, filter: &ToolFilter) -> Vec<ToolSpec>;

    fn inspect_capability(&self, call: &ToolInvocation) -> OperationCapability;

    async fn execute(&self, call: ToolInvocation, ctx: &ToolExecContext) -> Result<ToolOutput>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisionAnalyzeRequest {
    pub image_bytes: Vec<u8>,
    pub prompt: String,
    pub backend_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisionAnalysis {
    pub text: String,
}

#[async_trait]
pub trait VisionPort: Send + Sync {
    async fn has_vision_backend(&self, backend_id: Option<&str>) -> bool;
    async fn analyze_image(
        &self,
        req: VisionAnalyzeRequest,
        cancel: CancellationToken,
    ) -> Result<VisionAnalysis>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub source: String,
}

#[async_trait]
pub trait ImageFetchPort: Send + Sync {
    async fn fetch_image(&self, image_ref: &str) -> Result<FetchedImage>;
}

/// Host adapter that reads authorized extension/device inputs for InputPipeline.
#[async_trait]
pub trait SourceReadPort: Send + Sync {
    async fn read_extension(
        &self,
        extension_id: &str,
        operation: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> Result<SourceReadResult>;

    async fn read_device_metric(
        &self,
        device_id: &str,
        field: &str,
        params: &serde_json::Value,
        timeout_secs: u32,
    ) -> Result<SourceReadResult>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceReadResult {
    pub source_id: String,
    pub binding_proof: String,
    pub observed_at: Option<i64>,
    pub valid_window: Option<(i64, i64)>,
    pub value: serde_json::Value,
}

/// Notification delivery port (host wires real channels; tests use FakeReceiver).
#[async_trait]
pub trait NotifyPort: Send + Sync {
    async fn deliver(
        &self,
        channel: &str,
        subject: &str,
        body: &str,
        execution_id: &str,
    ) -> Result<()>;
}

/// Device/extension write port for controlled actions.
#[async_trait]
pub trait SideEffectPort: Send + Sync {
    async fn invoke(
        &self,
        destination: &str,
        action: &str,
        params: &serde_json::Value,
        execution_id: &str,
    ) -> Result<()>;
}

/// Clock port for deterministic tests.
pub trait ClockPort: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

pub struct FixedClock {
    pub now_ms: std::sync::atomic::AtomicI64,
}

impl FixedClock {
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: std::sync::atomic::AtomicI64::new(now_ms),
        }
    }

    pub fn set(&self, now_ms: i64) {
        self.now_ms
            .store(now_ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ClockPort for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// No-op LLM used when ToolPolicy::Disabled and single-pass summarises locally.
pub struct LocalBriefLlm;

#[async_trait]
impl LlmPort for LocalBriefLlm {
    async fn resolve_backend(&self, _backend_id: Option<&str>) -> Result<LlmBackendCapabilities> {
        Ok(LlmBackendCapabilities {
            multimodal: false,
            tools: false,
            model: "local-brief".into(),
        })
    }

    async fn complete(
        &self,
        request: LlmRequest,
        _cancel: CancellationToken,
    ) -> Result<LlmResponse> {
        // Deterministic single-pass: echo structured evidence into a brief.
        // Never treat a bare "OVERLOAD" substring as positive (negations).
        let alert = crate::runner::alert_from_model_text(&request.user);
        Ok(LlmResponse {
            text: request.user,
            alert_criteria_met: alert,
        })
    }
}

/// Reject-all tool registry (ToolPolicy::Disabled path).
pub struct DisabledTools;

#[async_trait]
impl ToolRegistryPort for DisabledTools {
    fn list_definitions(&self, _filter: &ToolFilter) -> Vec<ToolSpec> {
        vec![]
    }

    fn inspect_capability(&self, _call: &ToolInvocation) -> OperationCapability {
        OperationCapability {
            class: CapabilityClass::Unknown,
            scope: None,
            idempotent: false,
        }
    }

    async fn execute(&self, call: ToolInvocation, _ctx: &ToolExecContext) -> Result<ToolOutput> {
        Err(AgentError::ToolRejected(format!(
            "tools disabled; refused {}",
            call.name
        )))
    }
}

pub struct NoVision;

#[async_trait]
impl VisionPort for NoVision {
    async fn has_vision_backend(&self, _backend_id: Option<&str>) -> bool {
        false
    }

    async fn analyze_image(
        &self,
        _req: VisionAnalyzeRequest,
        _cancel: CancellationToken,
    ) -> Result<VisionAnalysis> {
        Err(AgentError::Other("vision unavailable".into()))
    }
}

pub struct NoImageFetch;

#[async_trait]
impl ImageFetchPort for NoImageFetch {
    async fn fetch_image(&self, _image_ref: &str) -> Result<FetchedImage> {
        Err(AgentError::Other("image fetch unavailable".into()))
    }
}
