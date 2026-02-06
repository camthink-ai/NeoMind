//! Zero-Config Auto-Onboarding for Unknown Devices
//!
//! This module handles the automatic discovery and onboarding of unknown devices:
//! 1. Collects data samples from unknown devices
//! 2. Uses AI to analyze samples and generate device types
//! 3. Creates draft devices for user review
//! 4. Auto-registers high-confidence devices

use crate::discovery::types::*;
use crate::discovery::{DataPathExtractor, SemanticInference, VirtualMetricGenerator, MetricEnhancement};
use neomind_core::{EventBus, LlmRuntime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use base64::{Engine as _, engine::general_purpose};

/// Helper to decode base64 string
fn decode_base64(s: &str) -> Vec<u8> {
    general_purpose::STANDARD.decode(s).unwrap_or_default()
}

/// Constants for auto-onboarding limits
const MAX_DRAFT_DEVICES: usize = 50;  // Maximum concurrent draft devices
const MIN_SAMPLES_FOR_ANALYSIS: usize = 1;  // Minimum samples before analysis
const MAX_MESSAGES_PER_DRAFT: usize = 20;  // Max messages to process per draft (security)
const MESSAGE_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);  // Rate limit window
const MAX_MESSAGES_PER_WINDOW: usize = 10;  // Max messages per minute per device

/// Rate limiter for device messages
#[derive(Debug, Clone)]
struct MessageRateLimiter {
    /// Message timestamps within the current window
    timestamps: Arc<RwLock<Vec<Instant>>>,
    /// Total messages processed (for lifetime limit)
    total_count: Arc<RwLock<usize>>,
}

impl MessageRateLimiter {
    fn new() -> Self {
        Self {
            timestamps: Arc::new(RwLock::new(Vec::new())),
            total_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Check if a message should be allowed based on rate limits
    /// Returns true if allowed, false if rate limited
    async fn check_rate_limit(&self) -> bool {
        let now = Instant::now();

        // Clean up old timestamps outside the window
        let mut timestamps = self.timestamps.write().await;
        timestamps.retain(|&ts| now.duration_since(ts) < MESSAGE_RATE_LIMIT_WINDOW);

        // Check rate limit
        if timestamps.len() >= MAX_MESSAGES_PER_WINDOW {
            return false;
        }

        // Add current timestamp
        timestamps.push(now);
        drop(timestamps);

        // Check total count
        let mut total = self.total_count.write().await;
        if *total >= MAX_MESSAGES_PER_DRAFT {
            return false;
        }
        *total += 1;

        true
    }

    /// Get the total count of messages processed
    async fn total_count(&self) -> usize {
        *self.total_count.read().await
    }
}

/// Type signature for matching similar device types
///
/// A type signature is a fingerprint based on the semantic and data types
/// of metrics, allowing devices with the same data structure to share types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeSignature {
    /// Ordered list of metric signatures (semantic_type, data_type pairs)
    pub metric_signatures: Vec<(String, String)>,
    /// Device category for coarse filtering
    pub category: String,
}

impl TypeSignature {
    /// Compute a hash from the signature
    pub fn to_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.metric_signatures.hash(&mut hasher);
        self.category.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Configuration for auto-onboarding behavior
///
/// Simplified configuration for the new flow:
/// - Fast analysis without LLM
/// - Manual LLM enhancement trigger
/// - No auto-registration based on confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoOnboardConfig {
    /// Enable/disable auto-onboarding
    pub enabled: bool,
    /// Maximum number of samples to collect per device
    pub max_samples: usize,
    /// Draft retention time in seconds (default: 24 hours)
    pub draft_retention_secs: u64,
}

impl Default for AutoOnboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_samples: 10,  // Collect 10 samples max
            draft_retention_secs: 86400, // 24 hours
        }
    }
}

/// Auto-onboarding manager for zero-config device discovery
pub struct AutoOnboardManager {
    /// LLM runtime for AI analysis
    llm: Arc<dyn LlmRuntime>,
    /// Event bus for publishing events
    event_bus: Arc<EventBus>,
    /// Configuration
    config: Arc<RwLock<AutoOnboardConfig>>,
    /// Draft devices being tracked
    drafts: Arc<RwLock<HashMap<String, DraftDevice>>>,
    /// Type signatures mapped to device_type IDs
    /// signature_hash -> device_type
    type_signatures: Arc<RwLock<HashMap<String, String>>>,
    /// Reverse mapping: device_type -> signature_hash
    device_type_signatures: Arc<RwLock<HashMap<String, String>>>,
    /// Full type signatures for similarity matching
    /// signature_hash -> TypeSignature
    full_type_signatures: Arc<RwLock<HashMap<String, TypeSignature>>>,
    /// Rate limiters per device ID (for DoS protection)
    rate_limiters: Arc<RwLock<HashMap<String, MessageRateLimiter>>>,
    /// Blocked devices (spam detected)
    blocked_devices: Arc<RwLock<HashMap<String, (Instant, String)>>>,
    /// Path extractor for analyzing samples
    path_extractor: DataPathExtractor,
    /// Semantic inference
    semantic_inference: SemanticInference,
    /// Virtual metric generator
    metric_generator: VirtualMetricGenerator,
}

/// Result of device registration with enhancement
///
/// Contains the system-generated device_id and recommended topic format
#[derive(Debug, Clone, Serialize)]
pub struct RegistrationResult {
    /// The system-generated device ID (NOT the original MQTT device_id)
    pub system_device_id: String,
    /// The generated device type ID
    pub device_type: String,
    /// The recommended MQTT topic format for this device
    pub recommended_topic: String,
    /// The MDL definition with enhanced metrics
    pub mdl_definition: serde_json::Value,
    /// Original MQTT device_id (for reference/mapping)
    pub original_device_id: String,
}

impl AutoOnboardManager {
    /// Create a new auto-onboard manager
    pub fn new(llm: Arc<dyn LlmRuntime>, event_bus: Arc<EventBus>) -> Self {
        let config = Arc::new(RwLock::new(AutoOnboardConfig::default()));
        let path_extractor = DataPathExtractor::new(llm.clone());
        let semantic_inference = SemanticInference::new(llm.clone());
        let metric_generator = VirtualMetricGenerator::new(llm.clone());

        Self {
            llm,
            event_bus,
            config,
            drafts: Arc::new(RwLock::new(HashMap::new())),
            type_signatures: Arc::new(RwLock::new(HashMap::new())),
            device_type_signatures: Arc::new(RwLock::new(HashMap::new())),
            full_type_signatures: Arc::new(RwLock::new(HashMap::new())),
            rate_limiters: Arc::new(RwLock::new(HashMap::new())),
            blocked_devices: Arc::new(RwLock::new(HashMap::new())),
            path_extractor,
            semantic_inference,
            metric_generator,
        }
    }

    /// Set configuration (returns a new manager with updated config)
    pub fn with_config(self, config: AutoOnboardConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            ..self
        }
    }

    /// Get current configuration
    pub async fn get_config(&self) -> AutoOnboardConfig {
        self.config.read().await.clone()
    }

    /// Update configuration
    pub async fn update_config(&self, config: AutoOnboardConfig) {
        let mut cfg = self.config.write().await;
        *cfg = config;
    }

    /// Process incoming data from an unknown device
    ///
    /// Returns whether the data was accepted for collection
    pub async fn process_unknown_device(
        &self,
        device_id: &str,
        source: &str,
        data: &serde_json::Value,
        is_binary: bool,
    ) -> Result<bool> {
        self.process_unknown_device_with_topic(device_id, source, data, is_binary, None, None).await
    }

    /// Process incoming data from an unknown device with original topic
    ///
    /// Returns whether the data was accepted for collection
    pub async fn process_unknown_device_with_topic(
        &self,
        device_id: &str,
        source: &str,
        data: &serde_json::Value,
        is_binary: bool,
        original_topic: Option<String>,
        adapter_id: Option<String>,
    ) -> Result<bool> {
        // Check if device is blocked (spam detected)
        {
            let blocked = self.blocked_devices.read().await;
            if let Some((blocked_at, reason)) = blocked.get(device_id) {
                let elapsed = blocked_at.elapsed();
                // Block for 1 hour, then allow retry
                if elapsed < Duration::from_secs(3600) {
                    tracing::warn!(
                        "Rejected message from blocked device '{}' ({}s remaining): {}",
                        device_id,
                        3600 - elapsed.as_secs(),
                        reason
                    );
                    return Ok(false);
                } else {
                    // Unblock after timeout
                    drop(blocked);
                    let mut blocked = self.blocked_devices.write().await;
                    blocked.remove(device_id);
                    tracing::info!("Unblocked device '{}' after timeout", device_id);
                }
            }
        }

        // Get or create rate limiter for this device
        let rate_limiter = {
            let mut limiters = self.rate_limiters.write().await;
            if !limiters.contains_key(device_id) {
                limiters.insert(device_id.to_string(), MessageRateLimiter::new());
            }
            limiters.get(device_id).cloned().unwrap()
        };

        // Check rate limits
        if !rate_limiter.check_rate_limit().await {
            let count = rate_limiter.total_count().await;
            tracing::warn!(
                "Rate limit exceeded for device '{}': {} messages processed (max: {})",
                device_id, count, MAX_MESSAGES_PER_DRAFT
            );

            // Block the device if it's clearly spamming
            if count >= MAX_MESSAGES_PER_DRAFT {
                let mut blocked = self.blocked_devices.write().await;
                blocked.insert(device_id.to_string(), (Instant::now(), "Rate limit exceeded".to_string()));
                tracing::warn!("Device '{}' blocked due to excessive messages", device_id);
            }
            return Ok(false);
        }

        let config = self.config.read().await;
        if !config.enabled {
            return Ok(false);
        }
        drop(config);

        let drafts = self.drafts.read().await;

        // Check if there's an existing draft for this device
        if let Some(_draft) = drafts.get(device_id) {
            drop(drafts);
            return self.add_sample_to_draft(device_id, data, is_binary).await;
        }

        // Check if we're at capacity
        if drafts.len() >= MAX_DRAFT_DEVICES {
            return Ok(false);
        }
        drop(drafts);

        // Create new draft device
        self.create_draft_with_topic(device_id, source, data, is_binary, original_topic, adapter_id).await
    }

    /// Create a new draft device with original topic
    async fn create_draft_with_topic(
        &self,
        device_id: &str,
        source: &str,
        data: &serde_json::Value,
        is_binary: bool,
        original_topic: Option<String>,
        adapter_id: Option<String>,
    ) -> Result<bool> {
        let mut drafts = self.drafts.write().await;

        let mut draft = DraftDevice::with_original_topic(
            device_id.to_string(),
            source.to_string(),
            self.config.read().await.max_samples,
            original_topic,
        );

        // Store adapter_id for external broker devices
        draft.adapter_id = adapter_id;

        // Mark as binary if applicable
        if is_binary {
            draft.is_binary = true;
        }

        let sample = if is_binary {
            // For binary data, extract base64 string and decode to raw bytes
            let base64_str = data.as_str().unwrap_or("");
            let raw_bytes = decode_base64(base64_str);
            DeviceSample {
                raw_data: raw_bytes,
                parsed: None,  // Binary data has no parsed JSON
                source: source.to_string(),
                timestamp: chrono::Utc::now().timestamp(),
            }
        } else {
            DeviceSample::from_json(data.clone(), source)
        };
        draft.add_sample(sample);

        // Clone values needed for event and analysis before moving draft
        let draft_id = draft.id.clone();
        let device_id_owned = device_id.to_string();
        let source_owned = source.to_string();
        let samples = draft.json_samples();

        drafts.insert(device_id.to_string(), draft.clone());
        drop(drafts);

        // Publish DraftCreated event (fire-and-forget)
        let manager_for_event = self.clone();
        let draft_id_for_event = draft_id.clone();
        let device_id_for_event = device_id_owned.clone();
        tokio::spawn(async move {
            manager_for_event.publish_event(AutoOnboardEvent::DraftCreated {
                draft_id: draft_id_for_event,
                device_id: device_id_for_event,
                source: source_owned,
            }).await;
        });

        tracing::info!(
            "Created draft device '{}' from source {} (1/{} samples, is_binary={})",
            device_id,
            source,
            self.config.read().await.max_samples,
            is_binary
        );

        // Trigger analysis immediately since MIN_SAMPLES_FOR_ANALYSIS = 1
        let manager_for_analysis = self.clone();
        tokio::spawn(async move {
            let _ = manager_for_analysis.analyze_device(&draft_id, &device_id_owned, samples).await;
        });

        Ok(true)
    }

    /// Add a sample to an existing draft
    async fn add_sample_to_draft(
        &self,
        device_id: &str,
        data: &serde_json::Value,
        is_binary: bool,
    ) -> Result<bool> {
        let mut drafts = self.drafts.write().await;
        let draft = drafts.get_mut(device_id).ok_or_else(|| {
            DiscoveryError::InvalidData(format!("Draft device not found: {}", device_id))
        })?;

        // Don't add samples if already analyzing
        if draft.status != DraftDeviceStatus::Collecting {
            return Ok(false);
        }

        // Update binary flag if any sample is binary
        if is_binary {
            draft.is_binary = true;
        }

        let source = draft.source.clone();
        let sample = if is_binary {
            let base64_str = data.as_str().unwrap_or("");
            let raw_bytes = decode_base64(base64_str);
            DeviceSample {
                raw_data: raw_bytes,
                parsed: None,
                source,
                timestamp: chrono::Utc::now().timestamp(),
            }
        } else {
            DeviceSample::from_json(data.clone(), &source)
        };

        let added = draft.add_sample(sample);

        if added {
            let count = draft.samples.len();
            tracing::debug!(
                "Added sample to draft '{}' ({}/{} collected)",
                device_id,
                count,
                draft.max_samples
            );

            // Publish event (fire-and-forget)
            let manager = self.clone();
            let draft_id = draft.id.clone();
            tokio::spawn(async move {
                manager.publish_event(AutoOnboardEvent::SampleCollected {
                    draft_id,
                    sample_count: count,
                }).await;
            });

            // Check if ready for analysis
            if draft.ready_for_analysis(MIN_SAMPLES_FOR_ANALYSIS) {
                let draft_id = draft.id.clone();
                let device_id = draft.device_id.clone();
                let samples = draft.json_samples();
                drop(drafts);

                // Trigger analysis in background
                let manager = self.clone_for_task();
                tokio::spawn(async move {
                    let _ = manager.analyze_device(&draft_id, &device_id, samples).await;
                });
            }
        }

        Ok(added)
    }

    /// Analyze collected samples and generate device type
    pub async fn analyze_device(
        &self,
        draft_id: &str,
        device_id: &str,
        samples: Vec<serde_json::Value>,
    ) -> Result<GeneratedDeviceType> {
        // Update status to Analyzing
        {
            let mut drafts = self.drafts.write().await;
            if let Some(draft) = drafts.get_mut(device_id) {
                draft.set_status(DraftDeviceStatus::Analyzing);
            }
        }

        // Publish event (fire-and-forget)
        let manager = self.clone();
        let draft_id_str = draft_id.to_string();
        let sample_count = samples.len();
        tokio::spawn(async move {
            manager.publish_event(AutoOnboardEvent::AnalysisStarted {
                draft_id: draft_id_str,
                sample_count,
            }).await;
        });

        tracing::info!(
            "Analyzing draft device '{}' with {} samples (fast mode, no LLM)",
            device_id,
            samples.len()
        );

        // Check if this is a binary device
        let is_binary = {
            let drafts = self.drafts.read().await;
            drafts.get(device_id).map(|d| d.is_binary).unwrap_or(false)
        };

        // For binary devices, generate simple mode MDL directly
        if is_binary {
            tracing::info!("Draft device '{}' is binary, generating simple mode MDL", device_id);

            let category = DeviceCategory::Unknown;
            let type_id = format!("auto_{}", device_id.replace('-', "_"));

            let mdl_definition = self.generate_simple_mdl(&type_id, &type_id, &category).await?;

            let generated_type = GeneratedDeviceType::from_discovered(
                type_id.clone(),
                vec![], // No metrics for binary devices
                vec![], // Commands
                mdl_definition,
                samples.len(),
            );

            let type_id_for_event = type_id.clone();

            // Update draft with generated type
            {
                let mut drafts = self.drafts.write().await;
                if let Some(draft) = drafts.get_mut(device_id) {
                    draft.generated_type = Some(generated_type.clone());
                    draft.set_status(DraftDeviceStatus::WaitingProcessing);
                }
            }

            // Publish event
            let manager = self.clone_for_task();
            let draft_id_str = draft_id.to_string();
            tokio::spawn(async move {
                manager.publish_event(AutoOnboardEvent::AnalysisCompleted {
                    draft_id: draft_id_str,
                    device_type: type_id_for_event,
                }).await;
            });

            tracing::info!(
                "Analysis complete for binary device '{}': type={}, simple mode",
                device_id,
                type_id
            );

            return Ok(generated_type);
        }

        // For JSON devices, use fast analysis pipeline
        // Use fast analysis pipeline (no LLM calls) for draft creation
        // LLM enhancement happens only when user approves the device
        let metrics = self
            .semantic_inference
            .analyze_samples_fast(device_id, &samples)
            .await;

        if metrics.is_empty() {
            return Err(DiscoveryError::Parse(
                "No metrics could be extracted from samples".to_string()
            ));
        }

        tracing::info!(
            "Fast analysis extracted {} metrics from {} samples",
            metrics.len(),
            samples.len()
        );

        // Convert to DeviceSample format for record keeping
        let device_samples: Vec<DeviceSample> = samples
            .into_iter()
            .enumerate()
            .map(|(i, v)| DeviceSample {
                raw_data: serde_json::to_vec(&v).unwrap_or_default(),
                parsed: Some(v),
                source: format!("sample_{}", i),
                timestamp: chrono::Utc::now().timestamp(),
            })
            .collect();

        // Infer device category from metrics
        let category = self.infer_category(&metrics);

        // First, try exact match
        let existing_type = self.find_matching_type(&metrics, &category).await;

        // If no exact match, try similarity match with threshold 0.7 (70%)
        let similar_type = if existing_type.is_none() {
            self.find_similar_type(&metrics, &category, 0.7).await
        } else {
            None
        };

        let (type_id, reusing_type) = existing_type
            .or(similar_type)
            .map(|t| (t, true))
            .unwrap_or_else(|| {
                let new_type_id = format!("auto_{}", device_id.replace('-', "_"));
                (new_type_id, false)
            });

        if reusing_type {
            tracing::info!(
                "Reusing existing type '{}' for device '{}'",
                type_id,
                device_id
            );
        }

        // Generate MDL definition
        let mdl_definition = self.generate_mdl(&type_id, &type_id, &metrics, &category).await?;

        let generated_type = GeneratedDeviceType::from_discovered(
            type_id.clone(),
            metrics.clone(),
            vec![], // Commands inferred from discovered metrics
            mdl_definition,
            device_samples.len(),
        );

        let type_id_for_event = type_id.clone(); // Clone for event publishing

        // Register type signature if this is a new type
        if !reusing_type {
            self.register_type_signature(&metrics, &category, &type_id).await;
        }

        // Update draft with generated type
        {
            let mut drafts = self.drafts.write().await;
            if let Some(draft) = drafts.get_mut(device_id) {
                draft.generated_type = Some(generated_type.clone());
                // Always set to PendingReview - user must approve manually
                draft.set_status(DraftDeviceStatus::WaitingProcessing);
            }
        }

        // Publish event (fire-and-forget)
        let manager = self.clone();
        let draft_id_str = draft_id.to_string();
        tokio::spawn(async move {
            manager.publish_event(AutoOnboardEvent::AnalysisCompleted {
                draft_id: draft_id_str,
                device_type: type_id_for_event,
            }).await;
        });

        tracing::info!(
            "Analysis complete for '{}': type={}, reusing={}",
            device_id,
            type_id,
            reusing_type
        );

        // Get the generated type from drafts
        let drafts = self.drafts.read().await;
        if let Some(draft) = drafts.get(device_id)
            && let Some(ref gen_type) = draft.generated_type {
                return Ok(gen_type.clone());
            }

        Err(DiscoveryError::Parse("Generated type not found".to_string()))
    }

    /// Register a device from its draft
    pub async fn register_device(
        &self,
        draft_id: &str,
        device_id: &str,
    ) -> Result<()> {
        let (device_type, _mdl_def) = {
            let drafts = self.drafts.read().await;
            let draft = drafts.get(device_id).ok_or_else(|| {
                DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
            })?;

            let gen_type = draft.generated_type.as_ref().ok_or_else(|| {
                DiscoveryError::InvalidData("No generated type for draft".to_string())
            })?;

            (gen_type.device_type.clone(), gen_type.mdl_definition.clone())
        };

        tracing::info!(
            "Registering device '{}' with type '{}'",
            device_id,
            device_type
        );

        // Note: Actual registration with MDL registry and device registry
        // is handled by the caller via the API
        // 1. mdl_registry.register(mdl_def)
        // 2. device_registry.add(device_id, device_type)

        // Update draft status
        {
            let mut drafts = self.drafts.write().await;
            if let Some(draft) = drafts.get_mut(device_id) {
                draft.set_status(DraftDeviceStatus::Registered);
            }
        }

        // Publish event (fire-and-forget)
        let manager = self.clone();
        let draft_id_str = draft_id.to_string();
        let device_id_str = device_id.to_string();
        let device_type_clone = device_type.clone();
        tokio::spawn(async move {
            manager.publish_event(AutoOnboardEvent::DeviceRegistered {
                draft_id: draft_id_str,
                device_id: device_id_str,
                device_type: device_type_clone,
            }).await;
        });

        tracing::info!("Device '{}' successfully registered", device_id);

        Ok(())
    }

    /// Register a device from its draft with LLM enhancement
    ///
    /// This method enhances the generated type with LLM before registration:
    /// - Generates a NEW system device_id (not the original MQTT device_id)
    /// - Generates Chinese display names
    /// - Creates proper descriptions
    /// - Recommends units
    ///
    /// This should be called when user explicitly approves the device.
    pub async fn register_device_with_enhancement(
        &self,
        draft_id: &str,
        device_id: &str,
    ) -> Result<RegistrationResult> {
        tracing::info!(
            "Registering device '{}' with LLM enhancement",
            device_id
        );

        // Get the draft and its metrics
        let (_draft, metrics) = {
            let drafts = self.drafts.read().await;
            let draft = drafts.get(device_id).ok_or_else(|| {
                DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
            })?;

            let gen_type = draft.generated_type.as_ref().ok_or_else(|| {
                DiscoveryError::InvalidData("No generated type for draft".to_string())
            })?;

            (draft.clone(), gen_type.metrics.clone())
        };

        let category = self.infer_category(&metrics);
        let category_name = category.display_name();

        // Step 1: Enhance metrics with LLM (generate Chinese names, descriptions, units)
        tracing::info!("Enhancing {} metrics with LLM for device '{}'", metrics.len(), device_id);
        let enhancements = self.semantic_inference.enhance_metrics_with_llm(
            device_id,
            category_name,
            &metrics
        ).await;

        // Step 2: Apply enhancements to metrics
        let enhancement_map: std::collections::HashMap<String, _> = enhancements.into_iter().collect();

        let enhanced_metrics: Vec<DiscoveredMetric> = metrics.into_iter().map(|mut m| {
            if let Some(enhancement) = enhancement_map.get(&m.name) {
                m.display_name = enhancement.display_name.clone();
                m.description = enhancement.description.clone();
                m.unit = enhancement.unit.clone();
            }
            m
        }).collect();

        // Step 3: Generate the type ID
        let type_id = format!("auto_{}", device_id.replace('-', "_"));
        let display_name = self.generate_device_name(device_id, &category, &enhanced_metrics);

        // Step 3.5: Generate a NEW system device_id (not the original MQTT device_id)
        // This allows the system to assign our own device_id while tracking the original
        let system_device_id = format!("dev_{}", uuid::Uuid::new_v4().to_string().split_at(8).0);
        let recommended_topic = format!("device/{}/{}", type_id, system_device_id);

        tracing::info!(
            "Device '{}' assigned system device_id '{}' with recommended topic '{}'",
            device_id, system_device_id, recommended_topic
        );

        // Step 4: Generate MDL definition with enhanced metrics
        let mdl_definition = self.generate_mdl(&type_id, &display_name, &enhanced_metrics, &category).await?;

        tracing::info!(
            "Device '{}' enhanced and registered as type '{}', system device_id: '{}'",
            device_id, type_id, system_device_id
        );

        // Update draft status and store system_device_id
        {
            let mut drafts = self.drafts.write().await;
            if let Some(draft) = drafts.get_mut(device_id) {
                draft.set_status(DraftDeviceStatus::Registered);
                // Store the system device_id for future reference
                draft.device_id = system_device_id.clone();
                // Update the generated type with enhanced metrics
                if let Some(ref mut gen_type) = draft.generated_type {
                    gen_type.metrics = enhanced_metrics.clone();
                    gen_type.name = display_name.clone();
                    gen_type.mdl_definition = mdl_definition.clone();
                }
            }
        }

        // Publish event with system_device_id
        let manager = self.clone();
        let draft_id_str = draft_id.to_string();
        let _original_device_id = device_id.to_string();
        let system_device_id_clone = system_device_id.clone();
        let type_id_clone = type_id.clone();
        tokio::spawn(async move {
            manager.publish_event(AutoOnboardEvent::DeviceRegistered {
                draft_id: draft_id_str,
                device_id: system_device_id_clone,
                device_type: type_id_clone,
            }).await;
        });

        Ok(RegistrationResult {
            system_device_id,
            device_type: type_id,
            recommended_topic,
            mdl_definition,
            original_device_id: device_id.to_string(),
        })
    }

    /// Reject a draft device - completely removes it so it can be re-discovered later
    pub async fn reject_device(&self, device_id: &str, reason: &str) -> Result<()> {
        let mut drafts = self.drafts.write().await;

        // Get the draft info before removing (for logging and event)
        let draft = drafts.get(device_id).ok_or_else(|| {
            DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
        })?;
        let draft_id = draft.id.clone();

        // Completely remove the draft instead of just marking as rejected
        // This allows the device to be re-discovered when it sends data again
        drafts.remove(device_id);

        // Publish event (fire-and-forget)
        let manager = self.clone();
        let reason_str = reason.to_string();
        tokio::spawn(async move {
            manager.publish_event(AutoOnboardEvent::DeviceRejected {
                draft_id,
                reason: reason_str,
            }).await;
        });

        tracing::info!("Draft device '{}' rejected and removed: {}", device_id, reason);

        Ok(())
    }

    /// Remove a draft device completely (used after successful registration)
    pub async fn remove_draft(&self, device_id: &str) -> Result<()> {
        let mut drafts = self.drafts.write().await;

        // Check if draft exists before removing
        if drafts.contains_key(device_id) {
            drafts.remove(device_id);
            tracing::info!("Draft device '{}' removed after registration", device_id);
        }
        drop(drafts);

        // Clean up rate limiter for this device
        let mut limiters = self.rate_limiters.write().await;
        limiters.remove(device_id);

        Ok(())
    }


    /// Set the status of a draft device
    pub async fn set_draft_status(&self, device_id: &str, status: DraftDeviceStatus) -> Result<()> {
        let mut drafts = self.drafts.write().await;
        let draft = drafts.get_mut(device_id).ok_or_else(|| {
            DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
        })?;

        draft.set_status(status);
        draft.updated_at = chrono::Utc::now().timestamp();

        Ok(())
    }

    /// Get all draft devices
    pub async fn get_drafts(&self) -> Vec<DraftDevice> {
        let drafts = self.drafts.read().await;
        drafts.values().cloned().collect()
    }

    /// Get a specific draft device
    pub async fn get_draft(&self, device_id: &str) -> Option<DraftDevice> {
        let drafts = self.drafts.read().await;
        drafts.get(device_id).cloned()
    }

    /// Update draft device (user edits)
    pub async fn update_draft(
        &self,
        device_id: &str,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let mut drafts = self.drafts.write().await;
        let draft = drafts.get_mut(device_id).ok_or_else(|| {
            DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
        })?;

        if let Some(n) = name {
            draft.user_name = Some(n);
        }
        if let Some(d) = description {
            draft.user_description = Some(d);
        }
        draft.updated_at = chrono::Utc::now().timestamp();

        Ok(())
    }

    /// Update draft device metrics after LLM enhancement
    pub async fn update_draft_metrics(
        &self,
        device_id: &str,
        enhanced_metrics: Vec<DiscoveredMetric>,
    ) -> Result<()> {
        let mut drafts = self.drafts.write().await;
        let draft = drafts.get_mut(device_id).ok_or_else(|| {
            DiscoveryError::InvalidData(format!("Draft not found: {}", device_id))
        })?;

        if let Some(ref mut gen_type) = draft.generated_type {
            gen_type.metrics = enhanced_metrics;
        }
        draft.updated_at = chrono::Utc::now().timestamp();

        Ok(())
    }

    /// Enhance draft device metrics with LLM (manual trigger)
    /// Returns the enhanced metrics that can be applied via update_draft_metrics
    pub async fn enhance_draft_with_llm(
        &self,
        device_id: &str,
        device_category: &str,
        metrics: &[DiscoveredMetric],
    ) -> Vec<(String, MetricEnhancement)> {
        self.semantic_inference.enhance_metrics_with_llm(
            device_id,
            device_category,
            metrics,
        ).await
    }

    /// Clean up old draft devices
    pub async fn cleanup_old_drafts(&self) -> usize {
        let now = chrono::Utc::now().timestamp();
        let config = self.config.read().await;
        let retention_secs = config.draft_retention_secs;
        drop(config);

        let mut drafts = self.drafts.write().await;

        let to_remove: Vec<String> = drafts
            .iter()
            .filter(|(_, d)| {
                now - d.updated_at > retention_secs as i64
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in &to_remove {
            drafts.remove(id);
        }

        to_remove.len()
    }

    /// Infer device category from metrics
    fn infer_category(&self, metrics: &[DiscoveredMetric]) -> DeviceCategory {
        use SemanticType::*;

        let has_temp = metrics.iter().any(|m| m.semantic_type == Temperature);
        let has_humid = metrics.iter().any(|m| m.semantic_type == Humidity);
        let has_motion = metrics.iter().any(|m| m.semantic_type == Motion);
        let has_light = metrics.iter().any(|m| m.semantic_type == Light);
        let has_switch = metrics.iter().any(|m| m.semantic_type == Switch);
        let has_power = metrics.iter().any(|m| m.semantic_type == Power || m.semantic_type == Energy);
        let has_battery = metrics.iter().any(|m| m.semantic_type == Battery);

        // Detection-related patterns
        let has_detections = metrics.iter().any(|m| m.path.contains("detection") || m.path.contains("object"));
        let has_image = metrics.iter().any(|m| m.path.contains("image") || m.path.contains("frame"));

        match () {
            _ if has_image && has_detections => DeviceCategory::Camera,
            _ if has_temp && has_humid => DeviceCategory::MultiSensor,
            _ if has_temp => DeviceCategory::TemperatureSensor,
            _ if has_humid => DeviceCategory::HumiditySensor,
            _ if has_motion => DeviceCategory::MotionSensor,
            _ if has_light => DeviceCategory::LightSensor,
            _ if has_switch => DeviceCategory::Switch,
            _ if has_power => DeviceCategory::EnergyMonitor,
            _ if has_battery => DeviceCategory::Unknown, // Generic sensor
            _ => DeviceCategory::Unknown,
        }
    }

    /// Generate a display name for the device
    fn generate_device_name(
        &self,
        device_id: &str,
        category: &DeviceCategory,
        _metrics: &[DiscoveredMetric],
    ) -> String {
        // Try to extract a meaningful name from device_id
        let base_name = device_id
            .split(['-', '_'])
            .next()
            .unwrap_or("device")
            .to_string();

        format!("{} {}", category.display_name(), base_name)
    }

    /// Generate MDL definition
    async fn generate_mdl(
        &self,
        type_id: &str,
        name: &str,
        metrics: &[DiscoveredMetric],
        category: &DeviceCategory,
    ) -> Result<serde_json::Value> {
        self.generate_mdl_impl(type_id, name, metrics, category, false).await
    }

    /// Generate MDL definition for binary devices (simple mode)
    async fn generate_simple_mdl(
        &self,
        type_id: &str,
        name: &str,
        category: &DeviceCategory,
    ) -> Result<serde_json::Value> {
        self.generate_mdl_impl(type_id, name, &[], category, true).await
    }

    /// Internal MDL generation implementation
    async fn generate_mdl_impl(
        &self,
        type_id: &str,
        name: &str,
        metrics: &[DiscoveredMetric],
        category: &DeviceCategory,
        is_simple_mode: bool,
    ) -> Result<serde_json::Value> {
        use serde_json::json;

        // Build metrics definition (empty for simple mode)
        let metrics_def: Vec<serde_json::Value> = if is_simple_mode {
            Vec::new()
        } else {
            metrics.iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "path": m.path,
                        "data_type": m.data_type.display_name(),
                        "unit": m.unit,
                        "display_name": m.display_name,
                        "description": m.description,
                    })
                })
                .collect()
        };

        let mode = if is_simple_mode { "simple" } else { "full" };
        let description = if is_simple_mode {
            format!("Auto-generated {} definition (Raw Data Mode - requires Transform for decoding)", category.display_name())
        } else {
            format!("Auto-generated {} definition", category.display_name())
        };

        Ok(json!({
            "device_type": type_id,
            "name": name,
            "description": description,
            "category": category.display_name(),
            "mode": mode,
            "version": "1.0.0",
            "metrics": metrics_def,
            "commands": [],
            "generated_by": "neoalk-auto-onboard",
            "generated_at": chrono::Utc::now().to_rfc3339(),
        }))
    }

    /// Publish an event
    async fn publish_event(&self, event: AutoOnboardEvent) {
        if let Ok(event_json) = serde_json::to_value(&event) {
            // Publish to event bus as a custom event
            // This will be forwarded to WebSocket clients
            let event = neomind_core::NeoMindEvent::Custom {
                event_type: "auto_onboard".to_string(),
                data: event_json,
            };
            self.event_bus.publish(event).await;
        }
    }

    /// Compute type signature from discovered metrics
    ///
    /// The signature is based on the ordered list of (semantic_type, data_type) pairs
    /// and the device category. This allows devices with the same data structure
    /// to be identified as the same type.
    pub fn compute_type_signature(&self, metrics: &[DiscoveredMetric], category: &DeviceCategory) -> TypeSignature {
        // Sort metrics by semantic type to ensure consistent ordering
        let mut sorted_metrics = metrics.to_vec();
        sorted_metrics.sort_by(|a, b| {
            // Sort by string representations since semantic_type and data_type don't implement Ord
            let a_semantic = format!("{:?}", a.semantic_type);
            let b_semantic = format!("{:?}", b.semantic_type);
            match a_semantic.cmp(&b_semantic) {
                std::cmp::Ordering::Equal => {
                    let a_data = format!("{:?}", a.data_type);
                    let b_data = format!("{:?}", b.data_type);
                    a_data.cmp(&b_data)
                }
                other => other,
            }
        });

        let metric_signatures = sorted_metrics
            .iter()
            .map(|m| {
                // Convert semantic_type and data_type to strings for consistent hashing
                let semantic_str = format!("{:?}", m.semantic_type);
                let data_type_str = format!("{:?}", m.data_type);
                (semantic_str, data_type_str)
            })
            .collect();

        TypeSignature {
            metric_signatures,
            category: format!("{:?}", category),
        }
    }

    /// Find an existing device type that matches the given metrics
    ///
    /// Returns None if no matching type exists, or Some(device_type_id) if found.
    pub async fn find_matching_type(&self, metrics: &[DiscoveredMetric], category: &DeviceCategory) -> Option<String> {
        let signature = self.compute_type_signature(metrics, category);
        let signature_hash = signature.to_hash();

        let signatures = self.type_signatures.read().await;
        signatures.get(&signature_hash).cloned()
    }

    /// Find a similar device type using similarity matching
    ///
    /// Returns the most similar device type if similarity >= threshold (default 0.7)
    /// Uses Jaccard similarity on metric signatures
    pub async fn find_similar_type(&self, metrics: &[DiscoveredMetric], category: &DeviceCategory, threshold: f64) -> Option<String> {
        let signature = self.compute_type_signature(metrics, category);

        // Get all registered signatures
        let full_signatures = self.full_type_signatures.read().await;
        let type_signatures = self.type_signatures.read().await;

        if full_signatures.is_empty() {
            return None;
        }

        let mut best_match: Option<(String, f64)> = None;

        for (sig_hash, existing_signature) in full_signatures.iter() {
            // Skip if category doesn't match
            if existing_signature.category != signature.category {
                continue;
            }

            // Compute similarity
            let similarity = self.compute_signature_similarity(&signature, existing_signature);

            if similarity >= threshold {
                match &best_match {
                    None => {
                        best_match = Some((sig_hash.clone(), similarity));
                    }
                    Some((_, current_best)) if similarity > *current_best => {
                        best_match = Some((sig_hash.clone(), similarity));
                    }
                    _ => {}
                }
            }
        }

        // Look up the device_type from the signature hash
        if let Some((sig_hash, similarity)) = best_match
            && let Some(device_type) = type_signatures.get(&sig_hash) {
                tracing::info!(
                    "Found similar type '{}' with similarity {:.2}%",
                    device_type,
                    similarity * 100.0
                );
                return Some(device_type.clone());
            }

        None
    }

    /// Compute similarity between two type signatures using Jaccard index
    ///
    /// Jaccard similarity = |A ∩ B| / |A ∪ B|
    /// Where A and B are sets of metric signatures
    fn compute_signature_similarity(&self, sig1: &TypeSignature, sig2: &TypeSignature) -> f64 {
        use std::collections::HashSet;

        let set1: HashSet<_> = sig1.metric_signatures.iter().collect();
        let set2: HashSet<_> = sig2.metric_signatures.iter().collect();

        if set1.is_empty() && set2.is_empty() {
            return 1.0;
        }

        let intersection = set1.intersection(&set2).count();
        let union = set1.union(&set2).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f64 / union as f64
    }

    /// Register a new type signature mapping
    ///
    /// Stores the relationship between a signature hash and device_type_id.
    pub async fn register_type_signature(&self, metrics: &[DiscoveredMetric], category: &DeviceCategory, device_type: &str) {
        let signature = self.compute_type_signature(metrics, category);
        let signature_hash = signature.to_hash();

        // Store signature -> device_type mapping
        {
            let mut signatures = self.type_signatures.write().await;
            signatures.insert(signature_hash.clone(), device_type.to_string());
        }

        // Store device_type -> signature mapping (for reverse lookup)
        {
            let mut reverse = self.device_type_signatures.write().await;
            reverse.insert(device_type.to_string(), signature_hash.clone());
        }

        // Store full signature for similarity matching
        {
            let mut full_signatures = self.full_type_signatures.write().await;
            full_signatures.insert(signature_hash.clone(), signature);
        }

        tracing::debug!(
            "Registered type signature: {} -> {}",
            signature_hash,
            device_type
        );
    }

    /// Get all registered type signatures
    ///
    /// Returns a map of signature_hash -> device_type_id
    pub async fn get_all_type_signatures(&self) -> HashMap<String, String> {
        let signatures = self.type_signatures.read().await;
        signatures.clone()
    }

    /// Get the signature hash for a specific device type
    pub async fn get_type_signature(&self, device_type: &str) -> Option<String> {
        let reverse = self.device_type_signatures.read().await;
        reverse.get(device_type).cloned()
    }

    /// Rename a device type in the signature registry
    ///
    /// Updates the signature mappings when a device type is renamed.
    pub async fn rename_type_signature(&self, old_name: &str, new_name: &str) -> Result<()> {
        let signature_hash = {
            let reverse = self.device_type_signatures.read().await;
            reverse.get(old_name).cloned().ok_or_else(|| {
                DiscoveryError::InvalidData(format!("Device type not found in signatures: {}", old_name))
            })?
        };

        // Update signature -> device_type mapping
        {
            let mut signatures = self.type_signatures.write().await;
            if let Some(existing_type) = signatures.get(&signature_hash)
                && existing_type == old_name {
                    signatures.insert(signature_hash.clone(), new_name.to_string());
                }
        }

        // Update device_type -> signature mapping
        {
            let mut reverse = self.device_type_signatures.write().await;
            reverse.remove(old_name);
            reverse.insert(new_name.to_string(), signature_hash);
        }

        tracing::info!("Renamed type signature: {} -> {}", old_name, new_name);
        Ok(())
    }

    /// Remove a device type from the signature registry
    pub async fn remove_type_signature(&self, device_type: &str) -> Result<()> {
        let signature_hash = {
            let reverse = self.device_type_signatures.read().await;
            reverse.get(device_type).cloned().ok_or_else(|| {
                DiscoveryError::InvalidData(format!("Device type not found in signatures: {}", device_type))
            })?
        };

        // Remove from signature -> device_type mapping
        {
            let mut signatures = self.type_signatures.write().await;
            signatures.remove(&signature_hash);
        }

        // Remove from device_type -> signature mapping
        {
            let mut reverse = self.device_type_signatures.write().await;
            reverse.remove(device_type);
        }

        tracing::info!("Removed type signature for: {}", device_type);
        Ok(())
    }

    /// Check if a device type exists for the given signature
    pub async fn has_matching_type(&self, metrics: &[DiscoveredMetric], category: &DeviceCategory) -> bool {
        self.find_matching_type(metrics, category).await.is_some()
    }

    /// Clone for task spawning
    fn clone_for_task(&self) -> Self {
        Self {
            llm: self.llm.clone(),
            event_bus: self.event_bus.clone(),
            config: self.config.clone(),
            drafts: self.drafts.clone(),
            type_signatures: self.type_signatures.clone(),
            device_type_signatures: self.device_type_signatures.clone(),
            full_type_signatures: self.full_type_signatures.clone(),
            rate_limiters: self.rate_limiters.clone(),
            blocked_devices: self.blocked_devices.clone(),
            path_extractor: DataPathExtractor::new(self.llm.clone()),
            semantic_inference: SemanticInference::new(self.llm.clone()),
            metric_generator: VirtualMetricGenerator::new(self.llm.clone()),
        }
    }
}

/// Clone implementation for task spawning
impl Clone for AutoOnboardManager {
    fn clone(&self) -> Self {
        self.clone_for_task()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draft_device_creation() {
        let draft = DraftDevice::new("test-device".to_string(), "mqtt".to_string(), 5);

        assert_eq!(draft.device_id, "test-device");
        assert_eq!(draft.source, "mqtt");
        assert_eq!(draft.status, DraftDeviceStatus::Collecting);
        assert_eq!(draft.samples.len(), 0);
        assert_eq!(draft.max_samples, 5);
    }

    #[test]
    fn test_add_sample() {
        let mut draft = DraftDevice::new("test-device".to_string(), "mqtt".to_string(), 3);
        let sample = DeviceSample::from_json(serde_json::json!({"temp": 25.5}), "test");

        assert!(draft.add_sample(sample));
        assert_eq!(draft.samples.len(), 1);
        assert!(draft.ready_for_analysis(1));
    }

    #[test]
    fn test_max_samples() {
        let mut draft = DraftDevice::new("test-device".to_string(), "mqtt".to_string(), 2);

        assert!(draft.add_sample(DeviceSample::from_json(
            serde_json::json!({"temp": 25.5}),
            "test",
        )));
        assert!(draft.add_sample(DeviceSample::from_json(
            serde_json::json!({"temp": 26.0}),
            "test",
        )));
        assert!(!draft.add_sample(DeviceSample::from_json(
            serde_json::json!({"temp": 26.5}),
            "test",
        )));

        assert_eq!(draft.samples.len(), 2);
    }

    #[test]
    fn test_status_transitions() {
        let mut draft = DraftDevice::new("test-device".to_string(), "mqtt".to_string(), 3);

        assert_eq!(draft.status, DraftDeviceStatus::Collecting);

        draft.set_status(DraftDeviceStatus::Analyzing);
        assert_eq!(draft.status, DraftDeviceStatus::Analyzing);

        draft.set_status(DraftDeviceStatus::WaitingProcessing);
        assert_eq!(draft.status, DraftDeviceStatus::WaitingProcessing);

        draft.set_status(DraftDeviceStatus::Registered);
        assert_eq!(draft.status, DraftDeviceStatus::Registered);
    }

    #[test]
    fn test_category_inference() {
        let manager = create_test_manager();

        // Temperature + Humidity = MultiSensor
        let metrics = vec![
            DiscoveredMetric {
                semantic_type: SemanticType::Temperature,
                ..Default::default()
            },
            DiscoveredMetric {
                semantic_type: SemanticType::Humidity,
                ..Default::default()
            },
        ];
        assert_eq!(
            manager.infer_category(&metrics),
            DeviceCategory::MultiSensor
        );

        // Only temperature = TemperatureSensor
        let metrics = vec![DiscoveredMetric {
            semantic_type: SemanticType::Temperature,
            ..Default::default()
        }];
        assert_eq!(
            manager.infer_category(&metrics),
            DeviceCategory::TemperatureSensor
        );
    }

    fn create_test_manager() -> AutoOnboardManager {
        use neomind_core::{EventBus, LlmRuntime};
        use futures::Stream;

        // Create a simple test helper - the manager only needs the reference
        // for infer_category which is a pure function
        struct DummyLlm;
        #[async_trait::async_trait]
        impl LlmRuntime for DummyLlm {
            fn backend_id(&self) -> neomind_core::llm::backend::BackendId {
                neomind_core::llm::backend::BackendId::new("ollama")
            }
            fn model_name(&self) -> &str {
                "dummy"
            }
            fn capabilities(&self) -> neomind_core::llm::backend::BackendCapabilities {
                neomind_core::llm::backend::BackendCapabilities::default()
            }
            fn max_context_length(&self) -> usize {
                4096
            }
            async fn generate(&self, _input: neomind_core::llm::backend::LlmInput) -> std::result::Result<neomind_core::llm::backend::LlmOutput, neomind_core::llm::backend::LlmError> {
                Ok(neomind_core::llm::backend::LlmOutput {
                    text: String::new(),
                    thinking: None,
                    finish_reason: neomind_core::llm::backend::FinishReason::Stop,
                    usage: Some(neomind_core::llm::backend::TokenUsage::new(0, 0)),
                })
            }
            async fn generate_stream(
                &self,
                _input: neomind_core::llm::backend::LlmInput,
            ) -> std::result::Result<Pin<Box<dyn futures::Stream<Item = std::result::Result<(String, bool), neomind_core::llm::backend::LlmError>> + Send>>, neomind_core::llm::backend::LlmError> {
                Ok(Box::pin(futures::stream::empty()))
            }
        }

        let llm = Arc::new(DummyLlm) as Arc<dyn LlmRuntime>;
        let event_bus = Arc::new(EventBus::new());

        AutoOnboardManager::new(llm, event_bus)
    }
}
