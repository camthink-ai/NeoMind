//! Auto-onboarding API handlers for zero-config device discovery
//!
//! This module provides REST endpoints for:
//! - Listing draft devices
//! - Getting draft device details
//! - Approving/rejecting draft devices
//! - Managing auto-onboarding configuration

use edge_ai_automation::{AutoOnboardManager, DraftDevice, GeneratedDeviceType, DiscoveredMetric, ProcessingSummary, RegistrationResult};
use edge_ai_automation::discovery::auto_onboard::AutoOnboardConfig;
use edge_ai_automation::discovery::types::DataType;
use edge_ai_automation::SemanticType;
use edge_ai_core::llm::backend::LlmRuntime;
use edge_ai_llm::backends::{OllamaConfig, OllamaRuntime, CloudConfig, CloudRuntime};
use edge_ai_devices::{DeviceService, DeviceTypeTemplate, MdlMetricDefinition as MetricDefinition, CommandDefinition, DeviceConfig, ConnectionConfig, DeviceTypeMode};

use crate::handlers::devices::models::*;
use crate::handlers::common::{HandlerResult, ok};
use crate::models::ErrorResponse;
use crate::server::types::ServerState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use futures::Stream;
use tokio::sync::RwLock;

/// Helper to create an LLM runtime from a configuration
/// This creates a simple default runtime for auto-onboarding when LLM is not fully configured
fn create_default_llm_runtime() -> Arc<dyn LlmRuntime> {
    use std::time::Duration;

    // Create a default Ollama runtime with standard settings
    let config = OllamaConfig::new("qwen2.5:3b")
        .with_endpoint("http://localhost:11434")
        .with_timeout_secs(120);

    match OllamaRuntime::new(config) {
        Ok(runtime) => Arc::new(runtime) as Arc<dyn LlmRuntime>,
        Err(_) => {
            // Fallback: create a mock runtime that returns empty results
            struct DummyRuntime;
            #[async_trait::async_trait]
            impl LlmRuntime for DummyRuntime {
                fn backend_id(&self) -> edge_ai_core::llm::backend::BackendId {
                    edge_ai_core::llm::backend::BackendId::new("dummy")
                }
                fn model_name(&self) -> &str {
                    "dummy"
                }
                fn capabilities(&self) -> edge_ai_core::llm::backend::BackendCapabilities {
                    edge_ai_core::llm::backend::BackendCapabilities::default()
                }
                async fn generate(&self, _input: edge_ai_core::llm::backend::LlmInput) -> Result<edge_ai_core::llm::backend::LlmOutput, edge_ai_core::llm::backend::LlmError> {
                    Ok(edge_ai_core::llm::backend::LlmOutput {
                        text: String::new(),
                        thinking: None,
                        finish_reason: edge_ai_core::llm::backend::FinishReason::Stop,
                        usage: Some(edge_ai_core::llm::backend::TokenUsage::new(0, 0)),
                    })
                }
                async fn generate_stream(
                    &self,
                    _input: edge_ai_core::llm::backend::LlmInput,
                ) -> Result<Pin<Box<dyn Stream<Item = Result<(String, bool), edge_ai_core::llm::backend::LlmError>> + Send>>, edge_ai_core::llm::backend::LlmError> {
                    Ok(Box::pin(futures::stream::empty()))
                }
                fn max_context_length(&self) -> usize {
                    4096
                }
            }
            Arc::new(DummyRuntime) as Arc<dyn LlmRuntime>
        }
    }
}

/// Helper to get or create the AutoOnboardManager from ServerState
/// Uses double-checked locking to ensure only one instance is created.
async fn get_auto_onboard_manager(state: &ServerState) -> Arc<AutoOnboardManager> {
    // First check: read lock (fast path)
    {
        let manager_guard = state.auto_onboard_manager.read().await;
        if let Some(manager) = manager_guard.as_ref() {
            return manager.clone();
        }
    }

    // Second check: write lock (create if needed)
    let mut manager_guard = state.auto_onboard_manager.write().await;
    if let Some(manager) = manager_guard.as_ref() {
        return manager.clone();
    }

    // Create the manager
    let llm = create_default_llm_runtime();
    let event_bus = state.event_bus.as_ref().unwrap().clone();
    let manager = Arc::new(AutoOnboardManager::new(llm, event_bus));

    // Store in state
    *manager_guard = Some(manager.clone());

    tracing::info!("AutoOnboardManager initialized and cached");
    manager
}

/// List all draft devices
pub async fn list_draft_devices(
    State(state): State<ServerState>,
) -> HandlerResult<DraftDevicesResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let drafts = manager.get_drafts().await;

    let items: Vec<DraftDeviceDto> = drafts.into_iter().map(DraftDeviceDto::from).collect();

    ok(DraftDevicesResponse {
        total: items.len(),
        items,
    })
}

/// Get a specific draft device by ID
pub async fn get_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<DraftDeviceDto> {
    let manager = get_auto_onboard_manager(&state).await;
    let draft = manager
        .get_draft(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(&format!("Draft device '{}' not found", device_id)))?;

    ok(DraftDeviceDto::from(draft))
}

/// Update a draft device (user edits)
pub async fn update_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<UpdateDraftDeviceRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    manager
        .update_draft(&device_id, request.name, request.description)
        .await
        .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

    ok(SuccessResponse {
        message: format!("Draft device '{}' updated", device_id),
    })
}

/// Approve and register a draft device
///
/// This function:
/// 1. Retrieves the draft device
/// 2. Converts the generated type to a DeviceTypeTemplate
/// 3. Registers the device type with DeviceService
/// 4. Creates and registers a DeviceConfig
/// 5. Updates the draft status to Registered
pub async fn approve_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<Option<ApproveDraftDeviceRequest>>,
) -> HandlerResult<ApproveDraftResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let device_service = state.device_service.clone();

    // Get the draft
    let draft = manager
        .get_draft(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(&format!("Draft device '{}' not found", device_id)))?;

    let draft_id = draft.id.clone();

    // Get device name from request (user-provided name for this device instance)
    let device_instance_name = request.as_ref()
        .and_then(|r| r.device_name.as_ref())
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("Device {}", device_id));

    // Check if user wants to reuse an existing type
    let use_existing_type = request.as_ref().and_then(|r| r.existing_type.as_ref());

    // Check if user provided new type details
    let new_type_info = request.as_ref().and_then(|r| r.new_type.as_ref());

    let (device_type, system_device_id, recommended_topic, message) = if let Some(existing_type) = use_existing_type {
        // User wants to reuse an existing type - no need to create a new one
        tracing::info!(
            "Device {} being assigned to existing type {}",
            device_id,
            existing_type
        );

        // Use the original MQTT topic as the telemetry topic
        // The device publishes to this topic (e.g., "ashuau" - the topic where it was discovered)
        // We cannot change the device's topic, so we subscribe to what it's using
        // Prefer original_topic if available, otherwise fall back to source
        let topic = draft.original_topic.clone().unwrap_or_else(|| draft.source.clone());

        // Create device config with the existing type
        let device_config = DeviceConfig {
            device_id: device_id.clone(),
            name: device_instance_name.clone(),
            device_type: existing_type.clone(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some(topic.clone()),
                json_path: Some("$.value".to_string()),
                ..Default::default()
            },
            adapter_id: draft.adapter_id.clone().or_else(|| Some("internal-mqtt".to_string())),
        };

        // Register the device
        device_service
            .register_device(device_config)
            .await
            .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

        (existing_type.clone(), device_id.clone(), topic, format!(
            "Device '{}' assigned to existing type '{}'",
            device_id, existing_type
        ))
    } else {
        // New type - either use the generated type from draft or user-provided details
        let gen_type = draft.generated_type.as_ref()
            .ok_or_else(|| ErrorResponse::bad_request("Draft device has no generated type. Please analyze it first."))?;

        // Use user-provided type info if available, otherwise use generated type
        let type_id = if let Some(new_info) = new_type_info {
            new_info.device_type.clone()
        } else {
            gen_type.device_type.clone()
        };

        let type_name = if let Some(new_info) = new_type_info {
            new_info.name.clone()
        } else {
            gen_type.name.clone()
        };

        let type_description = if let Some(new_info) = new_type_info {
            new_info.description.clone()
        } else {
            gen_type.description.clone()
        };

        // Convert GeneratedDeviceType to DeviceTypeTemplate
        // Categories are not auto-generated - user can add them later if needed
        // Use Full mode if metrics were extracted, Simple mode for raw data devices
        let mode = if gen_type.metrics.is_empty() {
            DeviceTypeMode::Simple
        } else {
            DeviceTypeMode::Full
        };
        let template = DeviceTypeTemplate {
            device_type: type_id.clone(),
            name: type_name.clone(),
            description: type_description,
            categories: Vec::new(), // Empty - not auto-generated
            mode,
            metrics: convert_metrics_to_template(&gen_type.metrics),
            commands: Vec::new(), // No commands generated yet
            uplink_samples: Vec::new(), // Samples not stored in draft
        };

        // Register the device type template
        device_service
            .register_template(template)
            .await
            .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

        tracing::info!("Registered device type '{}'", type_id);

        // Use the original MQTT topic as the telemetry topic
        // The device publishes to this topic (e.g., "ashuau" - the topic where it was discovered)
        // We cannot change the device's topic, so we subscribe to what it's using
        // Prefer original_topic if available, otherwise fall back to source
        let topic = draft.original_topic.clone().unwrap_or_else(|| draft.source.clone());

        // Create device config
        let device_config = DeviceConfig {
            device_id: device_id.clone(),
            name: device_instance_name.clone(),
            device_type: type_id.clone(),
            adapter_type: "mqtt".to_string(),
            connection_config: ConnectionConfig {
                telemetry_topic: Some(topic.clone()),
                json_path: Some("$.value".to_string()),
                ..Default::default()
            },
            adapter_id: draft.adapter_id.clone().or_else(|| Some("internal-mqtt".to_string())),
        };

        // Register the device
        device_service
            .register_device(device_config)
            .await
            .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

        tracing::info!("Registered device '{}' with type '{}'", device_id, type_id);

        (type_id.clone(), device_id.clone(), topic, format!(
            "Device '{}' registered as type '{}'",
            device_id, type_id
        ))
    };

    // Remove draft completely after successful registration (same behavior as reject)
    // This allows the device to be re-discovered if needed and keeps the pending list clean
    manager.remove_draft(&device_id).await
        .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

    ok(ApproveDraftResponse {
        original_device_id: device_id.clone(),
        system_device_id,
        device_type,
        recommended_topic,
        registered: true,
        message,
    })
}

/// Convert discovered metrics to MetricDefinition format
fn convert_metrics_to_template(metrics: &[DiscoveredMetric]) -> Vec<MetricDefinition> {
    use edge_ai_devices::MetricDataType;
    use edge_ai_automation::SemanticType;

    metrics.iter().map(|m| {
        // Map semantic type to data type
        let data_type = match m.semantic_type {
            SemanticType::Temperature
            | SemanticType::Humidity
            | SemanticType::Pressure
            | SemanticType::Light
            | SemanticType::Power
            | SemanticType::Energy
            | SemanticType::Speed
            | SemanticType::Flow
            | SemanticType::Level
            | SemanticType::Co2
            | SemanticType::Pm25
            | SemanticType::Voc => MetricDataType::Float,

            SemanticType::Motion
            | SemanticType::Switch => MetricDataType::Boolean,

            SemanticType::Dimmer => MetricDataType::Integer,

            SemanticType::Color => MetricDataType::String,

            SemanticType::Status
            | SemanticType::Error
            | SemanticType::Alarm
            | SemanticType::Battery
            | SemanticType::Rssi => MetricDataType::String,

            SemanticType::Unknown => match m.data_type {
                DataType::Float => MetricDataType::Float,
                DataType::Integer => MetricDataType::Integer,
                DataType::Boolean => MetricDataType::Boolean,
                DataType::String => MetricDataType::String,
                DataType::Array => MetricDataType::Array { element_type: None },
                _ => MetricDataType::String,
            },
        };

        // Use path (e.g., "data.temperature") for metric name to support nested JSON
        // Use name (e.g., "temperature") for display name
        let metric_name = if !m.path.is_empty() {
            m.path.clone()
        } else {
            m.name.clone()
        };

        MetricDefinition {
            name: metric_name,
            display_name: if !m.display_name.is_empty() {
                m.display_name.clone()
            } else {
                m.name.clone()
            },
            data_type,
            unit: m.unit.clone().unwrap_or_default(),
            min: m.value_range.as_ref().map(|r| r.min),
            max: m.value_range.as_ref().map(|r| r.max),
            required: false,
        }
    }).collect()
}

/// Reject a draft device
pub async fn reject_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<RejectDraftDeviceRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    manager
        .reject_device(&device_id, &request.reason)
        .await
        .map_err(|e| ErrorResponse::internal(&e.to_string()))?;

    ok(SuccessResponse {
        message: format!("Draft device '{}' rejected", device_id),
    })
}

/// Trigger manual analysis of a draft device
pub async fn trigger_draft_analysis(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let draft = manager
        .get_draft(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(&format!("Draft device '{}' not found", device_id)))?;

    let draft_id = draft.id.clone();
    let samples = draft.json_samples();

    if samples.is_empty() {
        return Err(ErrorResponse::bad_request("No samples available for analysis"));
    }

    // Trigger analysis in background
    let manager_clone = manager.clone();
    let device_id_clone = device_id.clone();
    tokio::spawn(async move {
        let _ = manager_clone.analyze_device(&draft_id, &device_id_clone, samples).await;
    });

    ok(SuccessResponse {
        message: format!("Analysis triggered for device '{}'", device_id),
    })
}

/// Enhance a draft device with LLM (manual trigger)
///
/// This endpoint is called when user explicitly requests LLM enhancement
/// for a draft device. It generates Chinese display names, descriptions, and units.
pub async fn enhance_draft_with_llm(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<DraftDeviceDto> {
    let manager = get_auto_onboard_manager(&state).await;

    // Get the draft
    let draft = manager
        .get_draft(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(&format!("Draft device '{}' not found", device_id)))?;

    let gen_type = draft.generated_type.as_ref()
        .ok_or_else(|| ErrorResponse::bad_request("Draft device has no generated type yet. Please analyze it first."))?;

    // Trigger LLM enhancement in background
    let manager_clone = manager.clone();
    let device_id_clone = device_id.clone();
    let metrics_clone = gen_type.metrics.clone();
    tokio::spawn(async move {
        // Use the manager's public method for LLM enhancement
        let enhancements: Vec<(String, edge_ai_automation::discovery::MetricEnhancement)> =
            manager_clone.enhance_draft_with_llm(
                &device_id_clone,
                "sensor", // default category
                &metrics_clone,
            ).await;

        // Apply enhancements to metrics
        let enhancement_map: std::collections::HashMap<String, _> = enhancements.into_iter().collect();

        let enhanced_metrics: Vec<edge_ai_automation::DiscoveredMetric> = metrics_clone.into_iter().map(|mut m| {
            if let Some(ref enhancement) = enhancement_map.get(&m.name) {
                m.display_name = enhancement.display_name.clone();
                m.description = enhancement.description.clone();
                m.unit = enhancement.unit.clone();
                // confidence field no longer exists in DiscoveredMetric
            }
            m
        }).collect();

        // Update draft with enhanced metrics using public method
        let _ = manager_clone.update_draft_metrics(&device_id_clone, enhanced_metrics).await;
    });

    // Return current draft immediately (enhancement happens in background)
    ok(DraftDeviceDto::from(draft))
}

/// Clean up old draft devices
pub async fn cleanup_draft_devices(
    State(state): State<ServerState>,
) -> HandlerResult<CleanupResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let count = manager.cleanup_old_drafts().await;

    ok(CleanupResponse {
        cleaned: count,
        message: format!("Cleaned up {} old draft devices", count),
    })
}

/// Get all registered type signatures
///
/// Returns a mapping of signature hashes to device type IDs.
/// This is used for type reusability - devices with matching signatures
/// can reuse the same device type.
pub async fn get_type_signatures(
    State(state): State<ServerState>,
) -> HandlerResult<TypeSignaturesResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let signatures = manager.get_all_type_signatures().await;
    let count = signatures.len().to_string();

    ok(TypeSignaturesResponse {
        signatures,
        count,
    })
}

/// Get suggested device types for a draft device
///
/// Analyzes the draft's metrics and finds existing device types
/// that match based on metric signatures. Returns a list of
/// suggested types with match scores.
pub async fn suggest_device_types(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<SuggestedTypesResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    let device_service = state.device_service.clone();

    // Get the draft
    let draft = manager
        .get_draft(&device_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(&format!("Draft device '{}' not found", device_id)))?;

    let gen_type = draft.generated_type.as_ref()
        .ok_or_else(|| ErrorResponse::bad_request("Draft device has no generated type. Please analyze it first."))?;

    // Find matching type using signature
    let category = gen_type.category.clone();

    let exact_match = manager.find_matching_type(&gen_type.metrics, &category).await;

    // Get all device types to find partial matches
    let all_types = device_service.list_templates().await;

    // Build suggestions list - include all types with their match scores
    let mut suggestions: Vec<SuggestedType> = Vec::new();

    // If no types exist at all, return empty list
    if all_types.is_empty() {
        return ok(SuggestedTypesResponse {
            suggestions: vec![],
            exact_match: exact_match,
        });
    }

    // Add all types with their match scores (sorted later)
    for template in &all_types {
        let is_exact = exact_match.as_ref() == Some(&template.device_type);

        // Calculate overlap score based on (semantic_type, data_type) signatures
        // This matches the logic used in TypeSignature for type matching
        let draft_signatures: std::collections::HashSet<_> = gen_type.metrics
            .iter()
            .map(|m| (format!("{:?}", m.semantic_type), format!("{:?}", m.data_type)))
            .collect();

        let type_signatures: std::collections::HashSet<_> = template.metrics
            .iter()
            // Map data type from template format to semantic inference format
            .map(|m| {
                let semantic = infer_semantic_type_from_metric_name(&m.name);
                let data_type = format!("{:?}", m.data_type);
                (format!("{:?}", semantic), data_type)
            })
            .collect();

        // Count matching signatures (intersection over union - Jaccard similarity)
        let intersection = draft_signatures
            .intersection(&type_signatures)
            .count();

        let union = draft_signatures
            .union(&type_signatures)
            .count();

        let match_score = if union > 0 {
            ((intersection as f64) / (union as f64) * 100.0) as u32
        } else {
            0
        };

        // Include all types, but prioritize exact matches and high scores
        suggestions.push(SuggestedType {
            device_type: template.device_type.clone(),
            name: template.name.clone(),
            description: template.description.clone(),
            match_score,
            is_exact_match: is_exact,
            metric_count: template.metrics.len(),
        });
    }

    // Sort by match score descending
    suggestions.sort_by(|a, b| b.match_score.cmp(&a.match_score));

    ok(SuggestedTypesResponse {
        suggestions,
        exact_match: exact_match,
    })
}

/// Get auto-onboarding configuration
pub async fn get_onboard_config(
    State(state): State<ServerState>,
) -> HandlerResult<AutoOnboardConfig> {
    let manager = get_auto_onboard_manager(&state).await;
    let config = manager.get_config().await;
    ok(config)
}

/// Update auto-onboarding configuration
pub async fn update_onboard_config(
    State(state): State<ServerState>,
    Json(config): Json<AutoOnboardConfig>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await;
    manager.update_config(config).await;
    ok(SuccessResponse {
        message: "Configuration updated".to_string(),
    })
}

/// Upload data for auto-onboarding analysis
///
/// This endpoint allows directly uploading device data samples to be analyzed
/// and added to the pending devices list. Useful for testing or when you have
/// sample data from an unknown device.
pub async fn upload_device_data(
    State(state): State<ServerState>,
    Json(request): Json<UploadDeviceDataRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await;

    let device_id = request.device_id.clone().unwrap_or_else(|| {
        format!("unknown_{}", uuid::Uuid::new_v4().to_string().split_at(8).0)
    });

    let source = request.source.clone().unwrap_or_else(|| "manual_upload".to_string());

    // Process each data sample (manual upload is always JSON)
    let mut processed = 0;
    for data in request.data {
        match manager.process_unknown_device(&device_id, &source, &data, false).await {
            Ok(true) => processed += 1,
            Ok(false) => {
                // Device not accepted (disabled or at capacity)
            },
            Err(e) => {
                tracing::warn!("Failed to process data for device {}: {}", device_id, e);
            }
        }
    }

    ok(SuccessResponse {
        message: format!(
            "Uploaded {} data sample(s) for device '{}'. Check Pending Devices tab.",
            processed, device_id
        ),
    })
}

// ============================================================================
// Response Types
// ============================================================================

/// Response for listing draft devices
#[derive(Debug, Serialize)]
pub struct DraftDevicesResponse {
    pub total: usize,
    pub items: Vec<DraftDeviceDto>,
}

/// DTO for a draft device
#[derive(Debug, Clone, Serialize)]
pub struct DraftDeviceDto {
    pub id: String,
    pub device_id: String,
    pub source: String,
    pub original_topic: Option<String>,
    pub status: String,
    pub sample_count: usize,
    pub max_samples: usize,
    pub generated_type: Option<GeneratedDeviceTypeDto>,
    pub discovered_at: i64,
    pub updated_at: i64,
    pub error_message: Option<String>,
    pub user_name: Option<String>,
}

impl From<edge_ai_automation::DraftDevice> for DraftDeviceDto {
    fn from(draft: edge_ai_automation::DraftDevice) -> Self {
        // Convert status to snake_case string for API consistency
        let status_str = match draft.status {
            edge_ai_automation::DraftDeviceStatus::Collecting => "collecting",
            edge_ai_automation::DraftDeviceStatus::Analyzing => "analyzing",
            edge_ai_automation::DraftDeviceStatus::WaitingProcessing => "waiting_processing",
            edge_ai_automation::DraftDeviceStatus::Registering => "registering",
            edge_ai_automation::DraftDeviceStatus::Registered => "registered",
            edge_ai_automation::DraftDeviceStatus::Rejected => "rejected",
            edge_ai_automation::DraftDeviceStatus::Failed => "failed",
        };

        Self {
            id: draft.id,
            device_id: draft.device_id,
            source: draft.source,
            original_topic: draft.original_topic,
            status: status_str.to_string(),
            sample_count: draft.samples.len(),
            max_samples: draft.max_samples,
            generated_type: draft.generated_type.map(GeneratedDeviceTypeDto::from),
            discovered_at: draft.discovered_at,
            updated_at: draft.updated_at,
            error_message: draft.error_message,
            user_name: draft.user_name,
        }
    }
}

/// DTO for a generated device type
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedDeviceTypeDto {
    pub device_type: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub metrics: Vec<MetricSummaryDto>,
    pub confidence: f32,
    pub summary: ProcessingSummaryDto,
}

impl From<edge_ai_automation::GeneratedDeviceType> for GeneratedDeviceTypeDto {
    fn from(gen_type: edge_ai_automation::GeneratedDeviceType) -> Self {
        Self {
            device_type: gen_type.device_type,
            name: gen_type.name,
            description: gen_type.description,
            category: gen_type.category.display_name().to_string(),
            metrics: gen_type.metrics.into_iter().map(MetricSummaryDto::from).collect(),
            confidence: 1.0, // Default confidence
            summary: ProcessingSummaryDto::from(&gen_type.summary),
        }
    }
}

/// DTO for metric summary
#[derive(Debug, Clone, Serialize)]
pub struct MetricSummaryDto {
    pub name: String,
    pub path: String,
    pub semantic_type: String,
    pub display_name: String,
    pub confidence: f32,
}

impl From<edge_ai_automation::DiscoveredMetric> for MetricSummaryDto {
    fn from(metric: edge_ai_automation::DiscoveredMetric) -> Self {
        Self {
            name: metric.name,
            path: metric.path,
            semantic_type: metric.semantic_type.display_name().to_string(),
            display_name: metric.display_name,
            confidence: 1.0, // Default confidence
        }
    }
}

/// DTO for processing summary
#[derive(Debug, Clone, Serialize)]
pub struct ProcessingSummaryDto {
    pub samples_analyzed: usize,
    pub fields_discovered: usize,
    pub metrics_generated: usize,
    pub inferred_category: String,
    pub insights: Vec<String>,
    pub warnings: Vec<String>,
    pub recommendations: Vec<String>,
}

impl From<&edge_ai_automation::ProcessingSummary> for ProcessingSummaryDto {
    fn from(summary: &edge_ai_automation::ProcessingSummary) -> Self {
        Self {
            samples_analyzed: summary.samples_analyzed,
            fields_discovered: summary.fields_discovered,
            metrics_generated: summary.metrics_generated,
            inferred_category: summary.inferred_category.clone(),
            insights: summary.insights.clone(),
            warnings: summary.warnings.clone(),
            recommendations: summary.recommendations.clone(),
        }
    }
}

/// Response for device approval
#[derive(Debug, Serialize)]
pub struct ApproveDraftResponse {
    /// Original device_id (also used as the registered device_id)
    pub original_device_id: String,
    /// The registered device_id (same as original_device_id)
    pub system_device_id: String,
    /// The device type ID
    pub device_type: String,
    /// The MQTT topic used for this device
    pub recommended_topic: String,
    /// Whether registration was successful
    pub registered: bool,
    /// Success message
    pub message: String,
}

/// Request to approve a draft device (with optional existing type)
#[derive(Debug, Deserialize)]
pub struct ApproveDraftDeviceRequest {
    /// Optional existing device type to reuse instead of creating a new one
    pub existing_type: Option<String>,
    /// Optional new type details (when creating a custom new type)
    pub new_type: Option<NewTypeDetails>,
    /// Device instance name (user-friendly name for this specific device)
    #[serde(default)]
    pub device_name: Option<String>,
}

/// Details for creating a new device type
#[derive(Debug, Deserialize)]
pub struct NewTypeDetails {
    /// Device type ID (user-specified)
    pub device_type: String,
    /// Display name for the device type
    pub name: String,
    /// Description of the device type
    #[serde(default)]
    pub description: String,
}

/// Response for type signatures
#[derive(Debug, Serialize)]
pub struct TypeSignaturesResponse {
    /// Mapping of signature hash to device type ID
    pub signatures: std::collections::HashMap<String, String>,
    /// Number of registered signatures
    pub count: String,
}

/// A suggested device type for a draft device
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedType {
    /// The device type ID
    pub device_type: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Match score (0-100)
    pub match_score: u32,
    /// Whether this is an exact signature match
    pub is_exact_match: bool,
    /// Number of metrics in this type
    pub metric_count: usize,
}

/// Response for suggested device types
#[derive(Debug, Serialize)]
pub struct SuggestedTypesResponse {
    /// List of suggested types (sorted by match score)
    pub suggestions: Vec<SuggestedType>,
    /// Exact match type ID (if any)
    pub exact_match: Option<String>,
}

/// Response for cleanup
#[derive(Debug, Serialize)]
pub struct CleanupResponse {
    pub cleaned: usize,
    pub message: String,
}

/// Request to update a draft device
#[derive(Debug, Deserialize)]
pub struct UpdateDraftDeviceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Request to reject a draft device
#[derive(Debug, Deserialize)]
pub struct RejectDraftDeviceRequest {
    pub reason: String,
}

/// Simple success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub message: String,
}

/// Request to upload device data for auto-onboarding
#[derive(Debug, Deserialize)]
pub struct UploadDeviceDataRequest {
    /// Optional device ID (will be generated if not provided)
    pub device_id: Option<String>,
    /// Optional data source identifier
    pub source: Option<String>,
    /// Array of data samples (JSON objects)
    pub data: Vec<serde_json::Value>,
}

/// Helper function to infer semantic type from metric name
/// This is a simplified version for device type matching
fn infer_semantic_type_from_metric_name(name: &str) -> SemanticType {
    let name_lower = name.to_lowercase();

    // Check for temperature-related keywords
    if name_lower.contains("temp") || name_lower.contains("temperature") {
        return SemanticType::Temperature;
    }
    // Humidity
    if name_lower.contains("hum") || name_lower.contains("humidity") {
        return SemanticType::Humidity;
    }
    // Pressure
    if name_lower.contains("press") {
        return SemanticType::Pressure;
    }
    // Light
    if name_lower.contains("lux") || name_lower.contains("light") || name_lower.contains("illuminance") {
        return SemanticType::Light;
    }
    // Motion
    if name_lower.contains("motion") || name_lower.contains("occupancy") || name_lower.contains("presence") {
        return SemanticType::Motion;
    }
    // Switch/relay
    if name_lower.contains("switch") || name_lower.contains("relay") || name_lower.contains("state") {
        return SemanticType::Switch;
    }
    // Power/energy
    if name_lower.contains("power") || name_lower.contains("energy") || name_lower.contains("watt") {
        return SemanticType::Power;
    }
    // Battery
    if name_lower.contains("batt") {
        return SemanticType::Battery;
    }
    // Speed
    if name_lower.contains("speed") || name_lower.contains("rpm") {
        return SemanticType::Speed;
    }
    // CO2
    if name_lower.contains("co2") || name_lower.contains("carbon") {
        return SemanticType::Co2;
    }
    // PM2.5
    if name_lower.contains("pm25") || name_lower.contains("pm2.5") || name_lower.contains("pm_2_5") {
        return SemanticType::Pm25;
    }

    SemanticType::Unknown
}
