//! Auto-onboarding API handlers for zero-config device discovery
//!
//! This module provides REST endpoints for:
//! - Listing draft devices
//! - Getting draft device details
//! - Approving/rejecting draft devices
//! - Managing auto-onboarding configuration

use crate::automation::discovery::auto_onboard::AutoOnboardConfig;
use crate::automation::discovery::types::DataType;
use crate::automation::SemanticType;
use crate::automation::{AutoOnboardManager, DiscoveredMetric};

/// The manager reports a missing draft as InvalidData("Draft not found: X")
/// (stringly typed) — surface it as 404 instead of 500.
fn draft_error_to_response(
    e: crate::automation::discovery::types::DiscoveryError,
) -> ErrorResponse {
    let msg = e.to_string();
    if msg.contains("Draft not found") {
        ErrorResponse::not_found(msg)
    } else {
        ErrorResponse::internal(msg)
    }
}
use neomind_agent::llm_backends::backends::{OllamaConfig, OllamaRuntime};
use neomind_core::llm::backend::LlmRuntime;
use neomind_devices::{
    AssociationBindInput, ConnectionConfig, ConnectionStatus, DeviceConfig, DeviceError,
    DeviceTypeMode, DeviceTypeTemplate, MdlMetricDefinition as MetricDefinition,
};

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::types::ServerState;
use axum::{
    extract::{Path, State},
    Json,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;

/// Helper to create an LLM runtime from a configuration
/// This creates a simple default runtime for auto-onboarding when LLM is not fully configured
fn create_default_llm_runtime() -> Arc<dyn LlmRuntime> {
    // Create a default Ollama runtime with standard settings
    let config = OllamaConfig::new(neomind_core::config::models::OLLAMA_DEFAULT)
        .with_endpoint("http://localhost:11434")
        .with_timeout_secs(120);

    match OllamaRuntime::new(config) {
        Ok(runtime) => Arc::new(runtime) as Arc<dyn LlmRuntime>,
        Err(_) => {
            // Fallback: create a mock runtime that returns empty results
            struct DummyRuntime;
            #[async_trait::async_trait]
            impl LlmRuntime for DummyRuntime {
                fn backend_id(&self) -> neomind_core::llm::backend::BackendId {
                    neomind_core::llm::backend::BackendId::new("dummy")
                }
                fn model_name(&self) -> &str {
                    "dummy"
                }
                fn capabilities(&self) -> neomind_core::llm::backend::BackendCapabilities {
                    neomind_core::llm::backend::BackendCapabilities::default()
                }
                async fn generate(
                    &self,
                    _input: neomind_core::llm::backend::LlmInput,
                ) -> Result<
                    neomind_core::llm::backend::LlmOutput,
                    neomind_core::llm::backend::LlmError,
                > {
                    Ok(neomind_core::llm::backend::LlmOutput {
                        text: String::new(),
                        thinking: None,
                        finish_reason: neomind_core::llm::backend::FinishReason::Stop,
                        usage: Some(neomind_core::llm::backend::TokenUsage::new(0, 0)),
                        tool_calls: None,
                    })
                }
                async fn generate_stream(
                    &self,
                    _input: neomind_core::llm::backend::LlmInput,
                ) -> Result<
                    Pin<
                        Box<
                            dyn Stream<
                                    Item = Result<
                                        (String, bool),
                                        neomind_core::llm::backend::LlmError,
                                    >,
                                > + Send,
                        >,
                    >,
                    neomind_core::llm::backend::LlmError,
                > {
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
async fn get_auto_onboard_manager(
    state: &ServerState,
) -> Result<Arc<AutoOnboardManager>, ErrorResponse> {
    // First check: read lock (fast path)
    {
        let manager_guard = state.auto_onboard_manager.read().await;
        if let Some(manager) = manager_guard.as_ref() {
            return Ok(manager.clone());
        }
    }

    // Second check: write lock (create if needed)
    let mut manager_guard = state.auto_onboard_manager.write().await;
    if let Some(manager) = manager_guard.as_ref() {
        return Ok(manager.clone());
    }

    // Create the manager
    let llm = create_default_llm_runtime();
    let event_bus = state
        .core
        .event_bus
        .as_ref()
        .ok_or_else(|| ErrorResponse::internal("EventBus not initialized"))?
        .clone();
    let manager = Arc::new(AutoOnboardManager::new(llm, event_bus));

    // Store in state
    *manager_guard = Some(manager.clone());

    tracing::info!("AutoOnboardManager initialized and cached");
    Ok(manager)
}

/// List all draft devices
#[utoipa::path(
    get,
    path = "/api/devices/drafts",
    tag = "drafts",
    responses(
        (status = 200, description = "Auto-discovered draft devices awaiting approval"),
    )
)]
pub async fn list_draft_devices(
    State(state): State<ServerState>,
) -> HandlerResult<DraftDevicesResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let drafts = manager.get_drafts().await;

    let items: Vec<DraftDeviceDto> = drafts.into_iter().map(DraftDeviceDto::from).collect();

    ok(DraftDevicesResponse {
        count: items.len(),
        items,
    })
}

/// Get a specific draft device by ID
#[utoipa::path(
    get,
    path = "/api/devices/drafts/{device_id}",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    responses(
        (status = 200, description = "One draft device with its analysis"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn get_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<DraftDeviceDto> {
    let manager = get_auto_onboard_manager(&state).await?;
    let draft = manager.get_draft(&device_id).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", device_id))
    })?;

    ok(DraftDeviceDto::from(draft))
}

/// Update a draft device (user edits)
#[utoipa::path(
    put,
    path = "/api/devices/drafts/{device_id}",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    request_body = UpdateDraftDeviceRequest,
    responses(
        (status = 200, description = "Draft fields updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn update_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<UpdateDraftDeviceRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    manager
        .update_draft(&device_id, request.name, request.description)
        .await
        .map_err(draft_error_to_response)?;

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
#[utoipa::path(
    post,
    path = "/api/devices/drafts/{device_id}/approve",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    request_body = ApproveDraftDeviceRequest,
    responses(
        (status = 200, description = "Draft approved; device registered"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn approve_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<Option<ApproveDraftDeviceRequest>>,
) -> HandlerResult<ApproveDraftResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let device_service = state.devices.service.clone();

    // Get the draft
    let draft = manager.get_draft(&device_id).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", device_id))
    })?;

    // Association-exception drafts use a placeholder id (`pending-assoc-*`).
    // Ordinary approve would register that placeholder as a real device and
    // leave later traffic still A/B-ambiguous. Require the confirm-association
    // path with an explicit real identity + parse field instead.
    if draft.association_exception
        || neomind_devices::adapters::mqtt::is_pending_association_candidate_id(&device_id)
    {
        return Err(ErrorResponse::bad_request(
            "This draft is an MQTT association exception. Use POST \
             /api/devices/drafts/{id}/confirm-association with an explicit \
             real device_id and identity_field (never pending-assoc-*). \
             Ordinary approve is refused so the placeholder cannot become a \
             registered device.",
        )
        .with_hint(
            "Open the pending conflict UI, confirm the real identity and parse \
             field, or dismiss/reject the draft.",
        ));
    }

    let _draft_id = draft.id.clone();

    // Get device name from request (user-provided name for this device instance)
    let device_instance_name = request
        .as_ref()
        .and_then(|r| r.device_name.as_ref())
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("Device {}", device_id));

    // Check if user wants to reuse an existing type
    let use_existing_type = request.as_ref().and_then(|r| r.existing_type.as_ref());

    // Check if user provided new type details
    let new_type_info = request.as_ref().and_then(|r| r.new_type.as_ref());

    let (device_type, system_device_id, recommended_topic, message) =
        if let Some(existing_type) = use_existing_type {
            // User wants to reuse an existing type - no need to create a new one
            tracing::info!(
                "Device {} being assigned to existing type {}",
                device_id,
                existing_type
            );

            // Use the original topic as the telemetry topic (for MQTT devices)
            // The device publishes to this topic (e.g., "ashuau" - the topic where it was discovered)
            // We cannot change the device's topic, so we subscribe to what it's using
            // Prefer original_topic if available, otherwise fall back to source
            let topic = draft
                .original_topic
                .clone()
                .unwrap_or_else(|| draft.source.clone());

            // Determine adapter_type and connection_config based on source
            let adapter_type = draft.source.clone();
            let default_adapter_id = format!("internal-{}", adapter_type);
            let connection_config = if adapter_type == "webhook" {
                // Webhook devices don't need MQTT topic config
                ConnectionConfig::default()
            } else {
                let mut connection_config = ConnectionConfig {
                    telemetry_topic: Some(topic.clone()),
                    ..Default::default()
                };
                if let Some(ref src) = draft.mqtt_identity_source {
                    connection_config.extra.insert(
                        "mqtt_identity_source".to_string(),
                        serde_json::Value::String(src.clone()),
                    );
                }
                if let Some(ref field) = draft.mqtt_identity_field {
                    connection_config.extra.insert(
                        "mqtt_identity_field".to_string(),
                        serde_json::Value::String(field.clone()),
                    );
                }
                connection_config
            };

            // Create device config with the existing type
            let device_config = DeviceConfig {
                device_id: device_id.clone(),
                name: device_instance_name.clone(),
                device_type: existing_type.clone(),
                adapter_type,
                connection_config,
                adapter_id: draft.adapter_id.clone().or(Some(default_adapter_id)),
                last_seen: chrono::Utc::now().timestamp(),
                offline_timeout_secs: None,
            };

            // Register the device (MQTT subscribe honesty → BindIncomplete)
            device_service
                .register_device(device_config)
                .await
                .map_err(bind_error_to_response)?;

            (
                existing_type.clone(),
                device_id.clone(),
                topic,
                format!(
                    "Device '{}' assigned to existing type '{}'",
                    device_id, existing_type
                ),
            )
        } else {
            // New type - either use the generated type from draft or user-provided details
            let gen_type = draft.generated_type.as_ref().ok_or_else(|| {
                ErrorResponse::bad_request(
                    "Draft device has no generated type. Please analyze it first.",
                )
            })?;

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
                commands: Vec::new(),       // No commands generated yet
                uplink_samples: Vec::new(), // Samples not stored in draft
                default_offline_timeout_secs: None,
                store_raw: None,
                builtin_version: None, // auto-onboard types are user-domain, never builtin
                ct_prov: None,
            };

            // Register the device type template
            device_service
                .register_template(template)
                .await
                .map_err(|e| ErrorResponse::internal(e.to_string()))?;

            tracing::info!("Registered device type '{}'", type_id);

            // Use the original topic as the telemetry topic (for MQTT devices)
            // Prefer original_topic if available, otherwise fall back to source
            let topic = draft
                .original_topic
                .clone()
                .unwrap_or_else(|| draft.source.clone());

            // Determine adapter_type and connection_config based on source
            let adapter_type = draft.source.clone();
            let default_adapter_id = format!("internal-{}", adapter_type);
            let connection_config = if adapter_type == "webhook" {
                ConnectionConfig::default()
            } else {
                let mut connection_config = ConnectionConfig {
                    telemetry_topic: Some(topic.clone()),
                    ..Default::default()
                };
                if let Some(ref src) = draft.mqtt_identity_source {
                    connection_config.extra.insert(
                        "mqtt_identity_source".to_string(),
                        serde_json::Value::String(src.clone()),
                    );
                }
                if let Some(ref field) = draft.mqtt_identity_field {
                    connection_config.extra.insert(
                        "mqtt_identity_field".to_string(),
                        serde_json::Value::String(field.clone()),
                    );
                }
                connection_config
            };

            // Create device config
            let device_config = DeviceConfig {
                device_id: device_id.clone(),
                name: device_instance_name.clone(),
                device_type: type_id.clone(),
                adapter_type,
                connection_config,
                adapter_id: draft.adapter_id.clone().or(Some(default_adapter_id)),
                last_seen: chrono::Utc::now().timestamp(),
                offline_timeout_secs: None,
            };

            // Register the device (MQTT subscribe honesty → BindIncomplete)
            device_service
                .register_device(device_config)
                .await
                .map_err(bind_error_to_response)?;

            tracing::info!("Registered device '{}' with type '{}'", device_id, type_id);

            (
                type_id.clone(),
                device_id.clone(),
                topic,
                format!("Device '{}' registered as type '{}'", device_id, type_id),
            )
        };

    let is_webhook = draft.source == "webhook";
    let subscription_ready = if is_webhook {
        // Webhook unchanged: no MQTT subscribe gate.
        false
    } else {
        // MQTT (and other transport): register_device only returns Ok after
        // subscribe succeeded (or non-mqtt adapters). Config ≠ online.
        true
    };

    if is_webhook {
        // Legacy webhook path: mark Connected + DeviceOnline from discovery traffic.
        device_service
            .update_device_status(&device_id, ConnectionStatus::Connected)
            .await;
        if let Some(event_bus) = state.core.event_bus.as_ref() {
            let _ = event_bus
                .publish(neomind_core::NeoMindEvent::DeviceOnline {
                    device_id: device_id.clone(),
                    device_type: device_type.clone(),
                    timestamp: chrono::Utc::now().timestamp(),
                })
                .await;
        }
    }
    // MQTT: do NOT mark Connected / publish DeviceOnline here — online follows
    // real telemetry or transport evidence (same as association confirm).

    // Draft consumed ONLY after register (+ MQTT subscribe) succeeded.
    manager
        .remove_draft(&device_id)
        .await
        .map_err(draft_error_to_response)?;

    ok(ApproveDraftResponse {
        original_device_id: device_id.clone(),
        system_device_id,
        device_type,
        recommended_topic,
        registered: true,
        subscription_ready,
        online: false,
        message,
    })
}

/// Convert discovered metrics to MetricDefinition format
fn convert_metrics_to_template(metrics: &[DiscoveredMetric]) -> Vec<MetricDefinition> {
    use crate::automation::SemanticType;
    use neomind_devices::MetricDataType;

    metrics
        .iter()
        .map(|m| {
            // Data type comes from the OBSERVED values; the semantic type only
            // fills the gap when the observed type carries no information
            // (unknown/all-null fields). The old mapping let the semantic type
            // override real values — numeric battery/rssi fields were forced
            // to String (SemanticType::Battery/Rssi arm), breaking charts and
            // numeric transforms.
            let data_type = match m.data_type {
                DataType::Float => MetricDataType::Float,
                DataType::Integer => MetricDataType::Integer,
                DataType::Boolean => MetricDataType::Boolean,
                DataType::String => MetricDataType::String,
                DataType::Array => MetricDataType::Array { element_type: None },
                DataType::Binary => MetricDataType::Binary,
                DataType::Object | DataType::Null | DataType::Unknown => match m.semantic_type {
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

                    SemanticType::Motion | SemanticType::Switch => MetricDataType::Boolean,

                    SemanticType::Dimmer => MetricDataType::Integer,

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
        })
        .collect()
}

/// Reject a draft device
#[utoipa::path(
    post,
    path = "/api/devices/drafts/{device_id}/reject",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    request_body = RejectDraftDeviceRequest,
    responses(
        (status = 200, description = "Draft rejected and removed"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn reject_draft_device(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
    Json(request): Json<RejectDraftDeviceRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    manager
        .reject_device(&device_id, &request.reason)
        .await
        .map_err(draft_error_to_response)?;

    ok(SuccessResponse {
        message: format!("Draft device '{}' rejected", device_id),
    })
}

/// Confirm a real device identity for an MQTT association-exception draft.
///
/// Ordinary `/approve` is refused for these drafts (placeholder `pending-assoc-*`
/// must never become a registered device). This endpoint requires an explicit
/// real `device_id` + payload `identity_field`, registers/attaches that device
/// on the draft's topic, and leaves blocked devices untouched unless
/// `overwrite_blocked` is set.
#[utoipa::path(
    post,
    path = "/api/devices/drafts/{device_id}/confirm-association",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Association-exception draft id"),
    ),
    request_body = ConfirmAssociationRequest,
    responses(
        (status = 200, description = "Real identity confirmed; device attached"),
        (status = 400, description = "Missing/invalid identity or ordinary approve path"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn confirm_association_draft(
    State(state): State<ServerState>,
    Path(draft_key): Path<String>,
    Json(request): Json<ConfirmAssociationRequest>,
) -> HandlerResult<ApproveDraftResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let device_service = state.devices.service.clone();

    // Serialize same-draft confirms; release on both Ok and Err so failed
    // attempts remain retryable. Same-device bind coordination lives inside
    // `apply_association_bind` (per-device mutex).
    let draft_lock = manager.confirm_draft_lock_for(&draft_key);
    let _draft_guard = draft_lock.lock().await;

    // Re-read after lock — a concurrent confirm may have consumed the draft.
    let draft = manager.get_draft(&draft_key).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", draft_key))
    })?;

    if !draft.association_exception
        && !neomind_devices::adapters::mqtt::is_pending_association_candidate_id(&draft_key)
    {
        return Err(ErrorResponse::bad_request(
            "Draft is not an association exception — use POST .../approve instead",
        ));
    }

    use neomind_devices::adapters::mqtt::{
        is_pending_association_candidate_id, normalize_confirm_device_id,
        verify_confirm_identity_against_raw, MqttIdentitySource,
    };

    let identity_source_raw = request
        .identity_source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(draft.mqtt_identity_source.as_deref())
        .unwrap_or("auto_payload");
    let identity_source = MqttIdentitySource::parse(identity_source_raw).ok_or_else(|| {
        ErrorResponse::bad_request(format!(
            "identity_source '{identity_source_raw}' is invalid"
        ))
        .with_hint(
            "Use one of: auto_payload, configured_payload, topic_strong, topic_weak, topic_hash",
        )
    })?;

    let identity_field = {
        let trimmed = request.identity_field.trim();
        let from_draft = draft
            .mqtt_identity_field
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let effective = if trimmed.is_empty() {
            from_draft.unwrap_or("")
        } else {
            trimmed
        };
        if identity_source.requires_identity_field() && effective.is_empty() {
            return Err(ErrorResponse::bad_request(
                "identity_field is required — name the JSON parse field that identifies the device",
            )
            .with_hint("Example: sn, data.sn, device_id, mac"));
        }
        // Persist only nonempty fields (topic sources may omit).
        if effective.is_empty() {
            None
        } else {
            Some(effective.to_string())
        }
    };

    let real_id = {
        let trimmed = request.device_id.trim();
        if trimmed.is_empty() {
            return Err(ErrorResponse::bad_request(
                "device_id is required — confirm the real device identity explicitly",
            )
            .with_hint("Do not reuse the pending-assoc-* placeholder."));
        }
        normalize_confirm_device_id(trimmed).ok_or_else(|| {
            ErrorResponse::bad_request(
                "device_id is empty or invalid after normalization (ascii alnum, -, _, :, . only)",
            )
            .with_hint(
                "Enter the real payload identity (e.g. sn value), not a path or placeholder.",
            )
        })?
    };
    if is_pending_association_candidate_id(&real_id) {
        return Err(ErrorResponse::bad_request(
            "device_id must not be a pending-assoc-* placeholder",
        )
        .with_hint(
            "Enter the real device identity from the payload (e.g. sn value) or an existing id.",
        ));
    }

    let overwrite_blocked = request.overwrite_blocked.unwrap_or(false);
    if draft.blocked_device_ids.iter().any(|id| id == &real_id) && !overwrite_blocked {
        return Err(ErrorResponse::bad_request(format!(
            "Device '{}' is blocked by this association exception — confirming it would \
             overwrite that device's identity binding. Pass overwrite_blocked=true only if \
             you intentionally want to retarget that device.",
            real_id
        ))
        .with_hint(
            "Prefer confirming a different real identity (e.g. the conflicting sn value) \
             so blocked devices keep their current binding.",
        ));
    }

    let topic = draft
        .original_topic
        .clone()
        .unwrap_or_else(|| draft.source.clone());

    // Fail closed on the full raw sample before mutating registry / consuming draft.
    if let Err(reason) = verify_confirm_identity_against_raw(
        draft.samples.first().map(|s| s.raw_data.as_slice()),
        &topic,
        identity_source,
        identity_field.as_deref(),
        &real_id,
    ) {
        return Err(ErrorResponse::bad_request(format!(
            "identity config does not match the draft sample: {reason}"
        ))
        .with_hint(
            "Check identity_field / device_id against the sample payload before confirming.",
        ));
    }

    let adapter_type = draft.source.clone();
    let default_adapter_id = format!("internal-{}", adapter_type);
    // Confirmed draft source constrains broker — never silently keep a stale adapter_id.
    let confirmed_adapter_id = draft
        .adapter_id
        .clone()
        .or(Some(default_adapter_id.clone()));

    let device_instance_name = request
        .device_name
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Device {}", real_id));

    let identity_field_label = identity_field.as_deref().unwrap_or("(topic)");

    let existing = device_service.get_device(&real_id);
    let device_type = if let Some(existing_cfg) = existing.as_ref() {
        existing_cfg.device_type.clone()
    } else {
        // Resolve type for a new device before the unified bind.
        let use_existing_type = request.existing_type.as_ref().filter(|s| !s.is_empty());
        let new_type_info = request.new_type.as_ref();

        if let Some(existing_type) = use_existing_type {
            existing_type.clone()
        } else if let Some(new_info) = new_type_info {
            let gen_type = draft.generated_type.as_ref();
            let type_id = new_info.device_type.clone();
            let type_name = if new_info.name.trim().is_empty() {
                type_id.clone()
            } else {
                new_info.name.clone()
            };
            let type_description = new_info.description.clone();
            let mode = match gen_type {
                Some(g) if !g.metrics.is_empty() => DeviceTypeMode::Full,
                _ => DeviceTypeMode::Simple,
            };
            let metrics = gen_type
                .map(|g| convert_metrics_to_template(&g.metrics))
                .unwrap_or_default();
            let template = DeviceTypeTemplate {
                device_type: type_id.clone(),
                name: type_name,
                description: type_description,
                categories: Vec::new(),
                mode,
                metrics,
                commands: Vec::new(),
                uplink_samples: Vec::new(),
                default_offline_timeout_secs: None,
                store_raw: None,
                builtin_version: None,
                ct_prov: None,
            };
            device_service
                .register_template(template)
                .await
                .map_err(|e| ErrorResponse::internal(e.to_string()))?;
            type_id
        } else if let Some(blocked) = draft.blocked_device_ids.first() {
            if let Some(peer) = device_service.get_device(blocked) {
                peer.device_type.clone()
            } else {
                return Err(ErrorResponse::bad_request(
                    "existing_type or new_type is required when confirming a new device",
                ));
            }
        } else {
            return Err(ErrorResponse::bad_request(
                "existing_type or new_type is required when confirming a new device",
            ));
        }
    };

    // Unified business op: apply config + subscribe switch; return actual result.
    let bind = device_service
        .apply_association_bind(AssociationBindInput {
            device_id: real_id.clone(),
            name: device_instance_name,
            device_type: device_type.clone(),
            adapter_type,
            adapter_id: confirmed_adapter_id,
            telemetry_topic: topic.clone(),
            identity_source: identity_source.as_str().to_string(),
            identity_field: identity_field.clone(),
        })
        .await
        .map_err(bind_error_to_response)?;

    // Draft consumed ONLY after config+subscribe both succeeded.
    manager
        .remove_draft(&draft_key)
        .await
        .map_err(draft_error_to_response)?;

    // Config ≠ online: do not mark Connected / publish DeviceOnline here.
    // Online/connected follows real telemetry or transport evidence.

    let action = if bind.was_existing {
        "Attached"
    } else {
        "Registered"
    };
    let message = format!(
        "{action} device '{}' on topic '{}' with identity_field='{}' via adapter {:?} (subscription ready; not marked online — waiting for telemetry)",
        real_id, topic, identity_field_label, bind.subscribed_adapter_id
    );

    ok(ApproveDraftResponse {
        original_device_id: draft_key,
        system_device_id: real_id,
        device_type,
        recommended_topic: topic,
        registered: true,
        subscription_ready: true,
        online: false,
        message,
    })
}

fn bind_error_to_response(e: DeviceError) -> ErrorResponse {
    match e {
        DeviceError::BindIncomplete {
            message,
            config_applied,
            retry_hint,
        } => ErrorResponse::service_unavailable(format!(
            "{message}; config_applied={config_applied}"
        ))
        .with_hint(retry_hint),
        DeviceError::InvalidParameter(msg) => ErrorResponse::bad_request(msg),
        other => ErrorResponse::from(other),
    }
}

/// Trigger manual analysis of a draft device
#[utoipa::path(
    post,
    path = "/api/devices/drafts/{device_id}/analyze",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    responses(
        (status = 200, description = "Analysis re-run for the draft"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn trigger_draft_analysis(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let draft = manager.get_draft(&device_id).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", device_id))
    })?;

    let draft_id = draft.id.clone();
    let samples = draft.json_samples();

    if samples.is_empty() {
        return Err(ErrorResponse::bad_request(
            "No samples available for analysis",
        ));
    }

    // Trigger analysis in background
    let manager_clone = manager.clone();
    let device_id_clone = device_id.clone();
    tokio::spawn(async move {
        let _ = manager_clone
            .analyze_device(&draft_id, &device_id_clone, samples)
            .await;
    });

    ok(SuccessResponse {
        message: format!("Analysis triggered for device '{}'", device_id),
    })
}

/// Enhance a draft device with LLM (manual trigger)
///
/// This endpoint is called when user explicitly requests LLM enhancement
/// for a draft device. It generates Chinese display names, descriptions, and units.
#[utoipa::path(
    post,
    path = "/api/devices/drafts/{device_id}/enhance",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    responses(
        (status = 200, description = "LLM enrichment of the draft MDL"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn enhance_draft_with_llm(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<DraftDeviceDto> {
    let manager = get_auto_onboard_manager(&state).await?;

    // Get the draft
    let draft = manager.get_draft(&device_id).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", device_id))
    })?;

    let gen_type = draft.generated_type.as_ref().ok_or_else(|| {
        ErrorResponse::bad_request(
            "Draft device has no generated type yet. Please analyze it first.",
        )
    })?;

    // Trigger LLM enhancement in background
    let manager_clone = manager.clone();
    let device_id_clone = device_id.clone();
    let metrics_clone = gen_type.metrics.clone();
    tokio::spawn(async move {
        // Use the manager's public method for LLM enhancement
        let enhancements: Vec<(String, crate::automation::discovery::MetricEnhancement)> =
            manager_clone
                .enhance_draft_with_llm(
                    &device_id_clone,
                    "sensor", // default category
                    &metrics_clone,
                )
                .await;

        // Apply enhancements to metrics
        let enhancement_map: std::collections::HashMap<String, _> =
            enhancements.into_iter().collect();

        let enhanced_metrics: Vec<crate::automation::DiscoveredMetric> = metrics_clone
            .into_iter()
            .map(|mut m| {
                if let Some(enhancement) = enhancement_map.get(&m.name) {
                    m.display_name = enhancement.display_name.clone();
                    m.description = enhancement.description.clone();
                    m.unit = enhancement.unit.clone();
                    // confidence field no longer exists in DiscoveredMetric
                }
                m
            })
            .collect();

        // Update draft with enhanced metrics using public method
        let _ = manager_clone
            .update_draft_metrics(&device_id_clone, enhanced_metrics)
            .await;
    });

    // Return current draft immediately (enhancement happens in background)
    ok(DraftDeviceDto::from(draft))
}

/// Clean up old draft devices
#[utoipa::path(
    post,
    path = "/api/devices/drafts/cleanup",
    tag = "drafts",
    responses(
        (status = 200, description = "Expired drafts removed"),
    )
)]
pub async fn cleanup_draft_devices(
    State(state): State<ServerState>,
) -> HandlerResult<CleanupResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
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
#[utoipa::path(
    get,
    path = "/api/devices/drafts/type-signatures",
    tag = "drafts",
    responses(
        (status = 200, description = "Observed telemetry type signatures (draft input hints)"),
    )
)]
pub async fn get_type_signatures(
    State(state): State<ServerState>,
) -> HandlerResult<TypeSignaturesResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let signatures = manager.get_all_type_signatures().await;
    let count = signatures.len().to_string();

    ok(TypeSignaturesResponse { signatures, count })
}

/// Get suggested device types for a draft device
///
/// Analyzes the draft's metrics and finds existing device types
/// that match based on metric signatures. Returns a list of
/// suggested types with match scores.
#[utoipa::path(
    get,
    path = "/api/devices/drafts/{device_id}/suggest-types",
    tag = "drafts",
    params(
        ("device_id" = String, Path, description = "Draft device id"),
    ),
    responses(
        (status = 200, description = "Candidate device types for the draft"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn suggest_device_types(
    State(state): State<ServerState>,
    Path(device_id): Path<String>,
) -> HandlerResult<SuggestedTypesResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
    let device_service = state.devices.service.clone();

    // Get the draft
    let draft = manager.get_draft(&device_id).await.ok_or_else(|| {
        ErrorResponse::not_found(format!("Draft device '{}' not found", device_id))
    })?;

    let gen_type = draft.generated_type.as_ref().ok_or_else(|| {
        ErrorResponse::bad_request("Draft device has no generated type. Please analyze it first.")
    })?;

    // Find matching type using signature
    let category = gen_type.category.clone();

    let exact_match = manager
        .find_matching_type(&gen_type.metrics, &category)
        .await;

    // Get all device types to find partial matches
    let all_types = device_service.list_templates();

    // Build suggestions list - include all types with their match scores
    let mut suggestions: Vec<SuggestedType> = Vec::new();

    // If no types exist at all, return empty list
    if all_types.is_empty() {
        return ok(SuggestedTypesResponse {
            suggestions: vec![],
            exact_match,
        });
    }

    // Add all types with their match scores (sorted later)
    for template in &all_types {
        let is_exact = exact_match.as_ref() == Some(&template.device_type);

        // Calculate overlap score based on (semantic_type, data_type) signatures
        // This matches the logic used in TypeSignature for type matching
        let draft_signatures: std::collections::HashSet<_> = gen_type
            .metrics
            .iter()
            .map(|m| {
                (
                    format!("{:?}", m.semantic_type),
                    format!("{:?}", m.data_type),
                )
            })
            .collect();

        let type_signatures: std::collections::HashSet<_> = template
            .metrics
            .iter()
            // Map data type from template format to semantic inference format
            .map(|m| {
                let semantic = infer_semantic_type_from_metric_name(&m.name);
                let data_type = format!("{:?}", m.data_type);
                (format!("{:?}", semantic), data_type)
            })
            .collect();

        // Count matching signatures (intersection over union - Jaccard similarity)
        let intersection = draft_signatures.intersection(&type_signatures).count();

        let union = draft_signatures.union(&type_signatures).count();

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
    suggestions.sort_by_key(|a| std::cmp::Reverse(a.match_score));

    ok(SuggestedTypesResponse {
        suggestions,
        exact_match,
    })
}

/// Get auto-onboarding configuration
#[utoipa::path(
    get,
    path = "/api/devices/drafts/config",
    tag = "drafts",
    responses(
        (status = 200, description = "Auto-onboarding configuration"),
    )
)]
pub async fn get_onboard_config(
    State(state): State<ServerState>,
) -> HandlerResult<AutoOnboardConfig> {
    let manager = get_auto_onboard_manager(&state).await?;
    let config = manager.get_config().await;
    ok(config)
}

/// Update auto-onboarding configuration
#[utoipa::path(
    put,
    path = "/api/devices/drafts/config",
    tag = "drafts",
    request_body = AutoOnboardConfig,
    responses(
        (status = 200, description = "Auto-onboarding configuration saved"),
    )
)]
pub async fn update_onboard_config(
    State(state): State<ServerState>,
    Json(config): Json<AutoOnboardConfig>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await?;
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
#[utoipa::path(
    post,
    path = "/api/devices/drafts/upload",
    tag = "drafts",
    request_body = UploadDeviceDataRequest,
    responses(
        (status = 200, description = "Sample payload uploaded; draft created from it"),
    )
)]
pub async fn upload_device_data(
    State(state): State<ServerState>,
    Json(request): Json<UploadDeviceDataRequest>,
) -> HandlerResult<SuccessResponse> {
    let manager = get_auto_onboard_manager(&state).await?;

    let device_id = request
        .device_id
        .clone()
        .unwrap_or_else(|| format!("unknown_{}", uuid::Uuid::new_v4().to_string().split_at(8).0));

    let source = request
        .source
        .clone()
        .unwrap_or_else(|| "manual_upload".to_string());

    // Process each data sample (manual upload is always JSON)
    let mut processed = 0;
    for data in request.data {
        match manager
            .process_unknown_device(&device_id, &source, &data, false)
            .await
        {
            Ok(true) => processed += 1,
            Ok(false) => {
                // Device not accepted (disabled or at capacity)
            }
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
    pub count: usize,
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
    /// MQTT association exception pending human confirm (not silent drop).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub association_exception: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub association_exception_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_device_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_identity: Option<String>,
    /// First sample payload preview (association-exception conflict UX).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_preview: Option<serde_json::Value>,
    /// Sticky identity source discovered with the sample (confirm prefill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mqtt_identity_source: Option<String>,
    /// Sticky identity field path (e.g. `data.sn`) for confirm prefill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mqtt_identity_field: Option<String>,
    /// Source adapter id from the draft (cross-broker confirm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

impl From<crate::automation::DraftDevice> for DraftDeviceDto {
    fn from(draft: crate::automation::DraftDevice) -> Self {
        // Convert status to snake_case string for API consistency
        let status_str = match draft.status {
            crate::automation::DraftDeviceStatus::Collecting => "collecting",
            crate::automation::DraftDeviceStatus::Analyzing => "analyzing",
            crate::automation::DraftDeviceStatus::WaitingProcessing => "waiting_processing",
            crate::automation::DraftDeviceStatus::Registering => "registering",
            crate::automation::DraftDeviceStatus::Registered => "registered",
            crate::automation::DraftDeviceStatus::Rejected => "rejected",
            crate::automation::DraftDeviceStatus::Failed => "failed",
        };

        let sample_preview = draft.samples.first().and_then(|s| s.parsed.clone());

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
            association_exception: draft.association_exception,
            association_exception_kind: draft.association_exception_kind,
            blocked_device_ids: draft.blocked_device_ids,
            claimed_identity: draft.claimed_identity,
            sample_preview,
            mqtt_identity_source: draft.mqtt_identity_source,
            mqtt_identity_field: draft.mqtt_identity_field,
            adapter_id: draft.adapter_id,
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
    pub summary: ProcessingSummaryDto,
}

impl From<crate::automation::GeneratedDeviceType> for GeneratedDeviceTypeDto {
    fn from(gen_type: crate::automation::GeneratedDeviceType) -> Self {
        Self {
            device_type: gen_type.device_type,
            name: gen_type.name,
            description: gen_type.description,
            category: gen_type.category.display_name().to_string(),
            metrics: gen_type
                .metrics
                .into_iter()
                .map(MetricSummaryDto::from)
                .collect(),
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
}

impl From<crate::automation::DiscoveredMetric> for MetricSummaryDto {
    fn from(metric: crate::automation::DiscoveredMetric) -> Self {
        Self {
            name: metric.name,
            path: metric.path,
            semantic_type: metric.semantic_type.display_name().to_string(),
            display_name: metric.display_name,
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

impl From<&crate::automation::ProcessingSummary> for ProcessingSummaryDto {
    fn from(summary: &crate::automation::ProcessingSummary) -> Self {
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

/// Response for device approval / association confirm
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
    /// True when adapter subscribe switch completed (confirm path).
    #[serde(default)]
    pub subscription_ready: bool,
    /// Always false on confirm — online requires real telemetry/transport evidence.
    #[serde(default)]
    pub online: bool,
    /// Success message
    pub message: String,
}

/// Request to approve a draft device (with optional existing type)
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct ApproveDraftDeviceRequest {
    /// Optional existing device type to reuse instead of creating a new one
    pub existing_type: Option<String>,
    /// Optional new type details (when creating a custom new type)
    pub new_type: Option<NewTypeDetails>,
    /// Device instance name (user-friendly name for this specific device)
    #[serde(default)]
    pub device_name: Option<String>,
}

/// Confirm real identity for an MQTT association-exception draft.
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize)]
pub struct ConfirmAssociationRequest {
    /// Real device identity to register or attach (never `pending-assoc-*`).
    pub device_id: String,
    /// JSON field used to parse identity on later messages (e.g. `sn`).
    pub identity_field: String,
    /// Sticky identity source; defaults to `auto_payload` when omitted.
    #[serde(default)]
    pub identity_source: Option<String>,
    /// Optional existing device type when registering a new device.
    #[serde(default)]
    pub existing_type: Option<String>,
    /// Optional new type details when registering a new device.
    #[serde(default)]
    pub new_type: Option<NewTypeDetails>,
    /// Display name for the device instance.
    #[serde(default)]
    pub device_name: Option<String>,
    /// Explicitly allow targeting a blocked device id (default false — never
    /// silently overwrite A).
    #[serde(default)]
    pub overwrite_blocked: Option<bool>,
}

/// Details for creating a new device type
#[derive(utoipa::ToSchema, Debug, Clone, Deserialize)]
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
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct UpdateDraftDeviceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Request to reject a draft device
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct RejectDraftDeviceRequest {
    pub reason: String,
}

/// Simple success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub message: String,
}

/// Request to upload device data for auto-onboarding
#[derive(utoipa::ToSchema, Debug, Deserialize)]
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
    if name_lower.contains("lux")
        || name_lower.contains("light")
        || name_lower.contains("illuminance")
    {
        return SemanticType::Light;
    }
    // Motion
    if name_lower.contains("motion")
        || name_lower.contains("occupancy")
        || name_lower.contains("presence")
    {
        return SemanticType::Motion;
    }
    // Switch/relay
    if name_lower.contains("switch") || name_lower.contains("relay") || name_lower.contains("state")
    {
        return SemanticType::Switch;
    }
    // Power/energy
    if name_lower.contains("power") || name_lower.contains("energy") || name_lower.contains("watt")
    {
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
    if name_lower.contains("pm25") || name_lower.contains("pm2.5") || name_lower.contains("pm_2_5")
    {
        return SemanticType::Pm25;
    }

    SemanticType::Unknown
}

#[cfg(test)]
mod convert_metrics_tests {
    use super::*;
    use neomind_devices::MetricDataType;

    fn metric(path: &str, data_type: DataType, semantic_type: SemanticType) -> DiscoveredMetric {
        DiscoveredMetric {
            name: path.to_string(),
            display_name: path.to_string(),
            description: String::new(),
            path: path.to_string(),
            data_type,
            semantic_type,
            unit: None,
            value_range: None,
            is_readable: true,
            is_writable: false,
        }
    }

    #[test]
    fn observed_data_type_wins_over_semantic_type() {
        // Numeric battery/rssi fields used to be forced to String by the
        // semantic-type arm (SemanticType::Battery/Rssi → String), breaking
        // charts and numeric transforms.
        let metrics = vec![
            metric(
                "device_info.battery_percent",
                DataType::Integer,
                SemanticType::Battery,
            ),
            metric("rssi", DataType::Integer, SemanticType::Rssi),
            metric("status", DataType::String, SemanticType::Status),
        ];
        let defs = convert_metrics_to_template(&metrics);

        assert!(matches!(defs[0].data_type, MetricDataType::Integer));
        assert!(matches!(defs[1].data_type, MetricDataType::Integer));
        assert!(matches!(defs[2].data_type, MetricDataType::String));
        // Dotted path is kept as the metric name (it doubles as the
        // extraction path).
        assert_eq!(defs[0].name, "device_info.battery_percent");
    }

    #[test]
    fn semantic_type_fills_uninformative_observed_types() {
        let metrics = vec![
            metric("temp", DataType::Unknown, SemanticType::Temperature),
            metric("flag", DataType::Null, SemanticType::Switch),
        ];
        let defs = convert_metrics_to_template(&metrics);

        assert!(matches!(defs[0].data_type, MetricDataType::Float));
        assert!(matches!(defs[1].data_type, MetricDataType::Boolean));
    }
}
