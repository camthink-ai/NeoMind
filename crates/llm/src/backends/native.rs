//! Native Rust LLM inference backend.
//!
//! This module provides native Rust LLM inference using the candle library,
//! allowing models to run directly in the same process without external dependencies.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use edge_ai_core::llm::backend::{
    BackendCapabilities, BackendId, BackendMetrics, FinishReason, LlmError, LlmInput,
    LlmOutput, LlmRuntime, StreamChunk, TokenUsage, ToolDefinition,
};
use edge_ai_core::message::{Message, MessageRole};

#[cfg(feature = "native")]
use candle_core::{Device, DType, Tensor};

#[cfg(feature = "native")]
use candle_nn::VarBuilder;

#[cfg(feature = "native")]
use candle_transformers::models::qwen2::{Config as Qwen2Config, ModelForCausalLM};

#[cfg(feature = "native")]
use tokenizers::Tokenizer;

#[cfg(feature = "native")]
use rand::Rng;

/// Configuration for native LLM backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeConfig {
    /// Model name/identifier (e.g., "qwen2:1.5b-instruct", "qwen3:1.7b")
    pub model: String,

    /// Path to the model files.
    /// If None, will download from Hugging Face.
    pub model_path: Option<PathBuf>,

    /// Device to run on ("cpu", "cuda", "metal")
    #[serde(default = "default_device")]
    pub device: String,

    /// Number of threads for CPU inference
    #[serde(default = "default_threads")]
    pub threads: usize,

    /// Max sequence length
    #[serde(default = "default_max_seq_len")]
    pub max_seq_len: usize,

    /// Temperature for sampling
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Top-p sampling
    #[serde(default = "default_top_p")]
    pub top_p: f32,

    /// Top-k sampling
    #[serde(default = "default_top_k")]
    pub top_k: usize,

    /// Repeat penalty
    #[serde(default = "default_repeat_penalty")]
    pub repeat_penalty: f32,

    /// Seed for reproducibility
    pub seed: Option<u64>,
}

fn default_device() -> String {
    // Always default to CPU since CUDA/Metal features may not be enabled
    "cpu".to_string()
}

fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn default_max_seq_len() -> usize { 2048 }
fn default_temperature() -> f32 { 0.7 }
fn default_top_p() -> f32 { 0.9 }
fn default_top_k() -> usize { 40 }
fn default_repeat_penalty() -> f32 { 1.0 }

impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            model: "qwen3:1.7b".to_string(),
            model_path: None,
            device: default_device(),
            threads: default_threads(),
            max_seq_len: default_max_seq_len(),
            temperature: default_temperature(),
            top_p: default_top_p(),
            top_k: default_top_k(),
            repeat_penalty: default_repeat_penalty(),
            seed: None,
        }
    }
}

impl NativeConfig {
    /// Create a new native config with the specified model.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// Set the device for inference.
    pub fn with_device(mut self, device: impl Into<String>) -> Self {
        self.device = device.into();
        self
    }

    /// Set the model path.
    pub fn with_model_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_path = Some(path.into());
        self
    }

    /// Set the max sequence length.
    pub fn with_max_seq_len(mut self, len: usize) -> Self {
        self.max_seq_len = len;
        self
    }
}

/// Loaded model with tokenizer
#[cfg(feature = "native")]
struct LoadedModel {
    model: Arc<Mutex<ModelForCausalLM>>,
    tokenizer: Tokenizer,
    device: Device,
}

/// Internal metrics for the native backend.
#[derive(Debug, Default)]
struct NativeMetrics {
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    total_tokens: u64,
    total_latency_ms: u64,
    last_latency_ms: Option<u64>,
}

/// Native LLM runtime using candle.
pub struct NativeRuntime {
    config: NativeConfig,
    model_name: String,
    max_context: usize,
    device: Device,
    // The model and tokenizer are loaded lazily on first use
    model: Arc<RwLock<Option<Arc<LoadedModel>>>>,
    metrics: Arc<RwLock<NativeMetrics>>,
}

impl NativeRuntime {
    /// Create a new native runtime.
    pub fn new(config: NativeConfig) -> Result<Self, LlmError> {
        let device = Self::create_device(&config.device)?;

        let model_name = config.model.clone();
        let max_context = config.max_seq_len;

        Ok(Self {
            config,
            model_name,
            max_context,
            device,
            model: Arc::new(RwLock::new(None)),
            metrics: Arc::new(RwLock::new(NativeMetrics::default())),
        })
    }

    fn create_device(device_str: &str) -> Result<Device, LlmError> {
        match device_str {
            "cpu" => Ok(Device::Cpu),
            "cuda" => Self::create_cuda_device(0),
            "metal" => Self::create_metal_device(0),
            _ => Err(LlmError::InvalidInput(format!(
                "Unknown device: {} (supported: cpu, cuda, metal)",
                device_str
            ))),
        }
    }

    #[cfg(any(feature = "cuda", feature = "metal"))]
    fn create_cuda_device(ordinal: usize) -> Result<Device, LlmError> {
        #[cfg(feature = "cuda")]
        {
            Ok(Device::Cuda(ordinal))
        }
        #[cfg(not(feature = "cuda"))]
        {
            let _ = ordinal;
            Err(LlmError::InvalidInput(
                "CUDA feature not enabled".to_string()
            ))
        }
    }

    #[cfg(any(feature = "cuda", feature = "metal"))]
    fn create_metal_device(ordinal: usize) -> Result<Device, LlmError> {
        #[cfg(feature = "metal")]
        {
            Ok(Device::Metal(ordinal))
        }
        #[cfg(not(feature = "metal"))]
        {
            let _ = ordinal;
            Err(LlmError::InvalidInput(
                "Metal feature not enabled".to_string()
            ))
        }
    }

    #[cfg(not(any(feature = "cuda", feature = "metal")))]
    fn create_cuda_device(_ordinal: usize) -> Result<Device, LlmError> {
        Err(LlmError::InvalidInput(
            "CUDA feature not enabled".to_string()
        ))
    }

    #[cfg(not(any(feature = "cuda", feature = "metal")))]
    fn create_metal_device(_ordinal: usize) -> Result<Device, LlmError> {
        Err(LlmError::InvalidInput(
            "Metal feature not enabled".to_string()
        ))
    }

    /// Load the model (lazy loading on first use).
    async fn ensure_model_loaded(&self) -> Result<(), LlmError> {
        let model_lock = self.model.read().await;
        if model_lock.is_some() {
            return Ok(());
        }
        drop(model_lock);

        tracing::info!("Loading native LLM model: {}", self.config.model);

        #[cfg(feature = "native")]
        {
            use hf_hub::api::sync::Api;

            let repo_id = self.get_model_repo_id()?;
            tracing::info!("Loading model from Hugging Face: {}", repo_id);

            // Cache directory
            let cache_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
                .join("neotalk")
                .join("models")
                .join(&self.config.model.replace(':', "-"));

            std::fs::create_dir_all(&cache_dir)
                .map_err(|e| LlmError::Generation(format!("Failed to create cache dir: {}", e)))?;

            let api = Api::new()
                .map_err(|e| LlmError::Generation(format!("Failed to create HF Hub API: {}", e)))?;
            let api_repo = api.model(repo_id.clone());

            // Download tokenizer
            let tokenizer_path = api_repo.get("tokenizer.json")
                .map_err(|e| LlmError::Generation(format!("Failed to get tokenizer: {}", e)))?;
            let target_tokenizer = cache_dir.join("tokenizer.json");
            if !target_tokenizer.exists() {
                std::fs::copy(&tokenizer_path, &target_tokenizer)
                    .map_err(|e| LlmError::Generation(format!("Failed to copy tokenizer: {}", e)))?;
            }

            // Load tokenizer
            let tokenizer = Tokenizer::from_file(&target_tokenizer)
                .map_err(|e| LlmError::Generation(format!("Failed to load tokenizer: {}", e)))?;

            // Load config
            let config_path = api_repo.get("config.json")
                .map_err(|e| LlmError::Generation(format!("Failed to get config: {}", e)))?;
            let target_config = cache_dir.join("config.json");
            if !target_config.exists() {
                std::fs::copy(&config_path, &target_config)
                    .map_err(|e| LlmError::Generation(format!("Failed to copy config: {}", e)))?;
            }

            let config_content = std::fs::read_to_string(&target_config)
                .map_err(|e| LlmError::Generation(format!("Failed to read config: {}", e)))?;

            let model_config: serde_json::Value = serde_json::from_str(&config_content)
                .map_err(|e| LlmError::Generation(format!("Failed to parse config: {}", e)))?;

            // Create Qwen2 config from the loaded config
            let qwen2_config = Qwen2Config {
                vocab_size: model_config["vocab_size"].as_u64().unwrap_or(151936) as usize,
                hidden_size: model_config["hidden_size"].as_u64().unwrap_or(1536) as usize,
                intermediate_size: model_config["intermediate_size"].as_u64().unwrap_or(8960) as usize,
                num_hidden_layers: model_config["num_hidden_layers"].as_u64().unwrap_or(24) as usize,
                num_attention_heads: model_config["num_attention_heads"].as_u64().unwrap_or(12) as usize,
                num_key_value_heads: model_config["num_key_value_heads"].as_u64().unwrap_or(2) as usize,
                max_position_embeddings: model_config["max_position_embeddings"].as_u64().unwrap_or(32768) as usize,
                sliding_window: model_config["sliding_window"].as_u64().unwrap_or(0) as usize,
                max_window_layers: model_config["max_window_layers"].as_u64().unwrap_or(0) as usize,
                tie_word_embeddings: model_config["tie_word_embeddings"].as_bool().unwrap_or(false),
                rope_theta: model_config["rope_theta"].as_f64().unwrap_or(10000.0) as f64,
                rms_norm_eps: model_config["rms_norm_eps"].as_f64().unwrap_or(1e-6) as f64,
                use_sliding_window: model_config["use_sliding_window"].as_bool().unwrap_or(false),
                hidden_act: candle_nn::Activation::Gelu,
            };

            // Download and load model weights
            let model_filename = "model.safetensors";
            let weights_path = api_repo.get(model_filename)
                .map_err(|e| LlmError::Generation(format!("Failed to get model weights: {}", e)))?;
            let target_weights = cache_dir.join(model_filename);

            if !target_weights.exists() {
                std::fs::copy(&weights_path, &target_weights)
                    .map_err(|e| LlmError::Generation(format!("Failed to copy model weights: {}", e)))?;
            }

            let start = Instant::now();

            // Load model
            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(
                    &[target_weights.clone()],
                    DType::F32,
                    &self.device,
                )
            }.map_err(|e| LlmError::Generation(format!("Failed to load safetensors: {}", e)))?;

            let model = ModelForCausalLM::new(&qwen2_config, vb)
                .map_err(|e| LlmError::Generation(format!("Failed to create model: {}", e)))?;

            tracing::info!("Model loaded in {:?}", start.elapsed());

            let loaded_model = LoadedModel {
                model: Arc::new(Mutex::new(model)),
                tokenizer,
                device: self.device.clone(),
            };

            let mut model_lock = self.model.write().await;
            *model_lock = Some(Arc::new(loaded_model));
        }

        #[cfg(not(feature = "native"))]
        {
            return Err(LlmError::BackendUnavailable(
                "Native feature not enabled. Build with --features native".to_string()
            ));
        }

        Ok(())
    }

    /// Get the Hugging Face repo ID for the model
    fn get_model_repo_id(&self) -> Result<String, LlmError> {
        let model = &self.config.model;

        let repo = match model.as_str() {
            "qwen3:1.7b" | "qwen3:1.7b-instruct" => "Qwen/Qwen2.5-1.5B-Instruct".to_string(),
            "qwen2:1.5b" | "qwen2:1.5b-instruct" => "Qwen/Qwen2-1.5B-Instruct".to_string(),
            "qwen2:0.5b" | "qwen2:0.5b-instruct" => "Qwen/Qwen2-0.5B-Instruct".to_string(),
            "qwen2:7b" | "qwen2:7b-instruct" => "Qwen/Qwen2-7B-Instruct".to_string(),
            s if s.contains('/') => s.to_string(),
            s => format!("Qwen/{}", s),
        };

        Ok(repo)
    }

    /// Format tools for text-based tool calling (JSON format).
    ///
    /// This injects tool definitions into the system prompt for models
    /// that don't have native function calling support.
    fn format_tools_for_text_calling(tools: &[ToolDefinition]) -> String {
        let mut result = String::from("## Tool Calling Requirements\n");
        result.push_str("You must call tools using JSON format. Do not just describe what to do.\n\n");
        result.push_str("Format:\n");
        result.push_str("[{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}]\n\n");

        result.push_str("## Examples\n");
        result.push_str("User: 有哪些设备？\n");
        result.push_str("Assistant: [{\"name\": \"list_devices\", \"arguments\": {}}]\n\n");

        result.push_str("User: 把客厅灯打开\n");
        result.push_str("Assistant: [{\"name\": \"control_device\", \"arguments\": {\"device_id\": \"light_living\", \"action\": \"on\"}}]\n\n");

        result.push_str("## Available Tools\n\n");

        for tool in tools {
            result.push_str(&format!("### {}\n", tool.name));
            result.push_str(&format!("Description: {}\n", tool.description));

            if let Some(props) = tool.parameters.get("properties")
                && let Some(obj) = props.as_object()
                    && !obj.is_empty() {
                        result.push_str("Parameters:\n");
                        for (name, prop) in obj {
                            let desc = prop
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("No description");
                            let type_name = prop
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("unknown");
                            result.push_str(&format!("- {}: {} ({})\n", name, desc, type_name));
                        }
                    }

            if let Some(required) = tool.parameters.get("required")
                && let Some(arr) = required.as_array()
                    && !arr.is_empty() {
                        let required_names: Vec<&str> =
                            arr.iter().filter_map(|v| v.as_str()).collect();
                        result.push_str(&format!("Required: {}\n", required_names.join(", ")));
                    }

            result.push('\n');
        }

        result.push_str("## Important Rules\n");
        result.push_str("1. ALWAYS output tool calls as a JSON array\n");
        result.push_str("2. Output ONLY the JSON array, no other text\n");
        result.push_str("3. If multiple tools are needed, include all in the same array\n");
        result.push_str("4. If no tools are needed, respond normally to the user\n");

        result
    }

    async fn generate_inner(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
        self.ensure_model_loaded().await?;

        let start = Instant::now();
        let prompt = self.format_messages(&input.messages, input.tools.as_deref())?;

        tracing::debug!("Generating with prompt length: {}", prompt.len());

        // Update metrics - increment total requests
        {
            let mut metrics = self.metrics.write().await;
            metrics.total_requests += 1;
        }

        #[cfg(feature = "native")]
        {
            let model_lock = self.model.read().await;
            let loaded_model = model_lock.as_ref()
                .ok_or_else(|| LlmError::Generation("Model not loaded".to_string()))?;

            // Tokenize input
            let tokens = loaded_model.tokenizer.encode(prompt.as_str(), false)
                .map_err(|e| LlmError::Generation(format!("Tokenization failed: {}", e)))?;

            let input_ids: Vec<u32> = tokens.get_ids()
                .iter()
                .map(|&id| id as u32)
                .collect();

            let input_tensor = Tensor::from_vec(input_ids.clone(), 1, &loaded_model.device)
                .map_err(|e| LlmError::Generation(format!("Failed to create input tensor: {}", e)))?;

            let prompt_tokens = input_ids.len();

            // Generate tokens
            let sample_len = input.params.max_tokens.unwrap_or(512).min(self.config.max_seq_len);

            let generated_text = self.generate_tokens(
                loaded_model,
                input_tensor,
                sample_len,
            ).await?;

            let token_count = self.estimate_tokens(&generated_text);

            // Update metrics - success
            let latency = start.elapsed().as_millis() as u64;
            {
                let mut metrics = self.metrics.write().await;
                metrics.successful_requests += 1;
                metrics.total_tokens += token_count as u64;
                metrics.total_latency_ms += latency;
                metrics.last_latency_ms = Some(latency);
            }

            return Ok(LlmOutput {
                text: generated_text,
                finish_reason: FinishReason::Stop,
                usage: Some(TokenUsage {
                    prompt_tokens: prompt_tokens as u32,
                    completion_tokens: token_count as u32,
                    total_tokens: (prompt_tokens + token_count) as u32,
                }),
                thinking: None,
            });
        }

        #[cfg(not(feature = "native"))]
        {
            let _ = (input, start);
            return Err(LlmError::BackendUnavailable(
                "Native feature not enabled".to_string()
            ));
        }
    }

    /// Generate tokens using the model
    #[cfg(feature = "native")]
    async fn generate_tokens(
        &self,
        loaded_model: &LoadedModel,
        input: Tensor,
        sample_len: usize,
    ) -> Result<String, LlmError> {
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        let tokenizer = loaded_model.tokenizer.clone();
        let device = &loaded_model.device;
        let model = &loaded_model.model;

        let mut input_ids = input;
        let mut generated_tokens = Vec::with_capacity(sample_len);

        let mut rng = match self.config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };

        let temp = self.config.temperature;
        let top_p = self.config.top_p;
        let top_k = self.config.top_k;

        for _idx in 0..sample_len {
            // Lock the model for forward pass
            let logits = {
                let mut model_guard = model.try_lock()
                    .map_err(|_| LlmError::Generation("Model lock poisoned".to_string()))?;

                // The ModelForCausalLM::forward expects &mut self and input_ids as Tensor
                model_guard.forward(&input_ids, 0)
                    .map_err(|e| LlmError::Generation(format!("Forward pass failed: {}", e)))?
            };

            // Get logits as vector
            let logits_vec = logits.to_vec1::<f32>()
                .map_err(|e| LlmError::Generation(format!("Failed to convert logits: {}", e)))?;

            let logits_vec: Vec<f32> = if temp > 0.0 {
                logits_vec.iter().map(|x| x / temp).collect()
            } else {
                logits_vec
            };

            let next_token = if temp <= 0.0 || top_p >= 1.0 && top_k >= logits_vec.len() {
                logits_vec
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                    .map(|(i, _)| i as u32)
                    .unwrap()
            } else {
                self.sample_token(&logits_vec, top_p, top_k, &mut rng)?
            };

            // Check for EOS tokens
            let eos_tokens = [
                tokenizer.token_to_id("<|im_end|>").unwrap_or(151644),
                tokenizer.token_to_id("").unwrap_or(151643),
            ];
            if eos_tokens.contains(&next_token) {
                break;
            }

            generated_tokens.push(next_token);

            // Append token to input_ids for next iteration
            let new_tokens = vec![next_token];
            let new_token_tensor = Tensor::from_vec(new_tokens.clone(), 1, device)
                .map_err(|e| LlmError::Generation(format!("Failed to create token tensor: {}", e)))?;

            input_ids = Tensor::cat(&[&input_ids, &new_token_tensor], 1)
                .map_err(|e| LlmError::Generation(format!("Failed to append token: {}", e)))?;
        }

        let text = tokenizer.decode(&generated_tokens, true)
            .map_err(|e| LlmError::Generation(format!("Decoding failed: {}", e)))?;

        let text = text
            .replace("<|im_end|>", "")
            .trim()
            .to_string();

        Ok(text)
    }

    /// Sample a token using top-p and top-k sampling
    #[allow(dead_code)]
    fn sample_token(
        &self,
        logits: &[f32],
        top_p: f32,
        top_k: usize,
        rng: &mut impl rand::Rng,
    ) -> Result<u32, LlmError> {
        use std::f32;

        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp: Vec<f32> = logits.iter().map(|x| (x - max_logit).exp()).collect();
        let sum: f32 = exp.iter().sum();
        let mut probs: Vec<(usize, f32)> = exp.iter().map(|x| x / sum).enumerate().collect();

        probs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let probs = if top_k < probs.len() {
            &probs[..top_k]
        } else {
            &probs
        };

        let mut cumsum = 0.0;
        let mut last_idx = 0;
        for (i, p) in probs.iter().enumerate() {
            cumsum += p.1;
            last_idx = i;
            if cumsum >= top_p {
                break;
            }
        }

        let candidates = &probs[..=last_idx];
        let sum: f32 = candidates.iter().map(|(_, p)| p).sum();
        let mut rng_val = rng.r#gen::<f32>();
        let mut selected_token = 0;

        for (token, p) in candidates {
            rng_val -= p / sum;
            if rng_val <= 0.0 {
                selected_token = *token;
                break;
            }
        }

        Ok(selected_token as u32)
    }

    fn format_messages(&self, messages: &[Message], tools: Option<&[ToolDefinition]>) -> Result<String, LlmError> {
        let mut prompt = String::new();

        // Pre-format tool instructions if tools are provided
        let tool_instructions = tools.is_some_and(|t| !t.is_empty())
            .then(|| Self::format_tools_for_text_calling(tools.unwrap()));

        for msg in messages {
            match msg.role {
                MessageRole::System => {
                    prompt.push_str("<|im_start|>system\n");
                    let mut text = msg.text();
                    // Inject tool instructions into system message
                    if let Some(instructions) = &tool_instructions {
                        text = format!("{}\n\n{}", text, instructions);
                    }
                    prompt.push_str(text.as_str());
                    prompt.push_str("<|im_end|>\n");
                }
                MessageRole::User => {
                    prompt.push_str("<|im_start|>user\n");
                    prompt.push_str(msg.text().as_str());
                    prompt.push_str("<|im_end|>\n");
                }
                MessageRole::Assistant => {
                    prompt.push_str("<|im_start|>assistant\n");
                    prompt.push_str(msg.text().as_str());
                    prompt.push_str("<|im_end|>\n");
                }
            }
        }

        prompt.push_str("<|im_start|>assistant");

        Ok(prompt)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        text.chars().count() / 4
    }

    async fn generate_stream_inner(
        &self,
        input: LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        self.ensure_model_loaded().await?;

        use tokio::sync::mpsc;
        let (tx, rx) = mpsc::unbounded_channel();

        let model = self.model.clone();
        let metrics = self.metrics.clone();
        let config = self.config.clone();
        let prompt = self.format_messages(&input.messages, input.tools.as_deref())?;

        tokio::spawn(async move {
            let start = Instant::now();

            let model_lock = model.read().await;
            let loaded_model = match model_lock.as_ref() {
                Some(m) => m,
                None => {
                    let _ = tx.send(Err(LlmError::Generation("Model not loaded".to_string())));
                    return;
                }
            };

            let tokens = match loaded_model.tokenizer.encode(prompt.as_str(), false) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(Err(LlmError::Generation(format!("Tokenization failed: {}", e))));
                    return;
                }
            };

            let input_ids: Vec<u32> = tokens.get_ids().iter().map(|&id| id as u32).collect();

            #[cfg(feature = "native")]
            {
                let device = &loaded_model.device;
                let mut input = match Tensor::from_vec(input_ids.clone(), 1, device) {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = tx.send(Err(LlmError::Generation(format!("Failed to create input tensor: {}", e))));
                        return;
                    }
                };

                let tokenizer = loaded_model.tokenizer.clone();
                let sample_len = 512.min(config.max_seq_len);
                let mut full_text = String::new();

                for _ in 0..sample_len {
                    let logits = {
                        let mut model_guard = match loaded_model.model.try_lock() {
                            Ok(g) => g,
                            Err(_) => {
                                let _ = tx.send(Err(LlmError::Generation("Model lock poisoned".to_string())));
                                return;
                            }
                        };

                        match model_guard.forward(&input, 0) {
                            Ok(l) => l,
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::Generation(format!("Forward pass failed: {}", e))));
                                return;
                            }
                        }
                    };

                    let logits_vec = match logits.to_vec1::<f32>() {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = tx.send(Err(LlmError::Generation(format!("Failed to convert logits: {}", e))));
                            return;
                        }
                    };

                    let next_token = logits_vec
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                        .map(|(i, _)| i as u32)
                        .unwrap();

                    let eos_tokens = [
                        tokenizer.token_to_id("<|im_end|>").unwrap_or(151644),
                        tokenizer.token_to_id("").unwrap_or(151643),
                    ];
                    if eos_tokens.contains(&next_token) {
                        break;
                    }

                    let token_text = match tokenizer.decode(&[next_token], true) {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = tx.send(Err(LlmError::Generation(format!("Decoding failed: {}", e))));
                            return;
                        }
                    };

                    if !token_text.is_empty() {
                        full_text.push_str(&token_text);
                        let _ = tx.send(Ok((token_text, false)));
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                    let new_token = match Tensor::from_vec(vec![next_token], 1, device) {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = tx.send(Err(LlmError::Generation(format!("Failed to create token tensor: {}", e))));
                            return;
                        }
                    };

                    input = match Tensor::cat(&[&input, &new_token], 1) {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = tx.send(Err(LlmError::Generation(format!("Failed to append token: {}", e))));
                            return;
                        }
                    };
                }

                let _ = tx.send(Ok(("".to_string(), true)));

                let latency = start.elapsed().as_millis() as u64;
                let mut m = metrics.write().await;
                m.total_requests += 1;
                m.successful_requests += 1;
                m.total_tokens += full_text.chars().count() as u64 / 4;
                m.total_latency_ms += latency;
                m.last_latency_ms = Some(latency);
            }

            #[cfg(not(feature = "native"))]
            {
                let _ = tx.send(Err(LlmError::Generation("Native feature not enabled".to_string())));
                return;
            }
        });

        let stream = stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Some(chunk) => Some((chunk, rx)),
                None => None,
            }
        });

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl LlmRuntime for NativeRuntime {
    fn backend_id(&self) -> BackendId {
        BackendId::new("native")
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    async fn is_available(&self) -> bool {
        #[cfg(feature = "native")]
        {
            self.ensure_model_loaded().await.is_ok()
        }
        #[cfg(not(feature = "native"))]
        {
            false
        }
    }

    async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
        self.generate_inner(input).await
    }

    async fn generate_stream(
        &self,
        input: LlmInput,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        self.generate_stream_inner(input).await
    }

    fn max_context_length(&self) -> usize {
        self.max_context
    }

    fn supports_multimodal(&self) -> bool {
        false
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            streaming: true,
            multimodal: false,
            function_calling: true,
            multiple_models: false,
            max_context: Some(self.max_context),
            modalities: vec!["text".to_string()],
            thinking_display: false,
            supports_images: false,
            supports_audio: false,
        }
    }

    fn metrics(&self) -> BackendMetrics {
        match self.metrics.try_read() {
            Ok(metrics) => {
                let avg_latency = if metrics.total_requests > 0 {
                    metrics.total_latency_ms as f64 / metrics.total_requests as f64
                } else {
                    0.0
                };

                BackendMetrics {
                    total_requests: metrics.total_requests,
                    successful_requests: metrics.successful_requests,
                    failed_requests: metrics.failed_requests,
                    total_tokens: metrics.total_tokens,
                    avg_latency_ms: avg_latency,
                    last_request: Some(std::time::SystemTime::now()),
                }
            }
            Err(_) => BackendMetrics::default(),
        }
    }
}

/// Create a native runtime from configuration.
pub fn create_native_runtime(config: NativeConfig) -> Result<NativeRuntime, LlmError> {
    NativeRuntime::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = NativeConfig::default();
        assert_eq!(config.temperature, 0.7);
        assert_eq!(config.top_p, 0.9);
        assert_eq!(config.top_k, 40);
        assert_eq!(config.model, "qwen3:1.7b");
    }

    #[test]
    fn test_config_builder() {
        let config = NativeConfig::new("qwen2:1.5b")
            .with_device("cpu")
            .with_max_seq_len(4096);

        assert_eq!(config.model, "qwen2:1.5b");
        assert_eq!(config.device, "cpu");
        assert_eq!(config.max_seq_len, 4096);
    }

    #[test]
    fn test_model_repo_mapping() {
        let config = NativeConfig::default();
        let runtime = NativeRuntime::new(config).unwrap();
        let repo = runtime.get_model_repo_id();
        assert!(repo.is_ok());
        assert_eq!(repo.unwrap(), "Qwen/Qwen2.5-1.5B-Instruct");
    }

    #[test]
    fn test_estimate_tokens() {
        let config = NativeConfig::default();
        let runtime = NativeRuntime::new(config).unwrap();
        let text = "Hello world! This is a test.";
        let tokens = runtime.estimate_tokens(text);
        assert!(tokens > 0);
        assert!(tokens < text.len());
    }

    #[test]
    fn test_format_messages() {
        let config = NativeConfig::default();
        let runtime = NativeRuntime::new(config).unwrap();

        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
        ];

        let prompt = runtime.format_messages(&messages, None).unwrap();
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("You are a helpful assistant"));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.contains("Hello"));
        assert!(prompt.contains("<|im_start|>assistant"));
    }
}
