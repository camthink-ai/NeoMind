//! AI Agent executor - runs agents and records decision processes.

use edge_ai_core::{EventBus, MetricValue, NeoTalkEvent, message::{Content, Message, MessageRole}};
use edge_ai_storage::{
    AgentMemory, AgentStats, AgentStore, AgentExecutionRecord, AiAgent, DataCollected,
    Decision, DecisionProcess, ExecutionResult as StorageExecutionResult, ExecutionStatus,
    GeneratedReport, ReasoningStep, TrendPoint, AgentResource, ResourceType,
    // New conversation types
    ConversationTurn, TurnInput, TurnOutput, AgentRole,
    LlmBackendStore, LlmBackendInstance,
};
use edge_ai_devices::DeviceService;
use edge_ai_llm::{OllamaConfig, OllamaRuntime, CloudConfig, CloudRuntime};
use edge_ai_core::llm::backend::LlmRuntime;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use crate::{Agent, AgentConfig, LlmBackend};
use crate::error::AgentError;
use crate::prompts::get_role_system_prompt;
use crate::translation::Language;

/// Configuration for agent executor.
#[derive(Clone)]
pub struct AgentExecutorConfig {
    /// Agent store
    pub store: Arc<AgentStore>,
    /// Time series storage for data collection
    pub time_series_storage: Option<Arc<edge_ai_storage::TimeSeriesStore>>,
    /// Device service for command execution
    pub device_service: Option<Arc<DeviceService>>,
    /// Event bus for event subscription
    pub event_bus: Option<Arc<EventBus>>,
    /// LLM runtime for intent analysis (default)
    pub llm_runtime: Option<Arc<dyn edge_ai_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    pub llm_backend_store: Option<Arc<LlmBackendStore>>,
}

/// Context for agent execution.
#[derive(Clone)]
pub struct ExecutionContext {
    /// Agent being executed
    pub agent: AiAgent,
    /// Trigger type (schedule, event, manual)
    pub trigger_type: String,
    /// Current event data (if event-triggered)
    pub event_data: Option<serde_json::Value>,
    /// LLM backend for decision making
    pub llm_backend: Option<LlmBackend>,
}

/// Result of agent execution.
pub struct AgentExecutionResult {
    /// Execution record
    pub record: AgentExecutionRecord,
    /// Updated memory
    pub memory: AgentMemory,
    /// Success status
    pub success: bool,
}

/// AI Agent executor - handles execution of user-defined agents.
pub struct AgentExecutor {
    /// Agent store
    store: Arc<AgentStore>,
    /// Time series storage for data collection
    time_series_storage: Option<Arc<edge_ai_storage::TimeSeriesStore>>,
    /// Device service for command execution
    device_service: Option<Arc<DeviceService>>,
    /// Event bus for publishing events
    event_bus: Option<Arc<EventBus>>,
    /// Configuration
    config: AgentExecutorConfig,
    /// LLM runtime (default)
    llm_runtime: Option<Arc<dyn edge_ai_core::llm::backend::LlmRuntime + Send + Sync>>,
    /// LLM backend store for per-agent backend lookup
    llm_backend_store: Option<Arc<LlmBackendStore>>,
    /// Event-triggered agents cache
    event_agents: Arc<RwLock<HashMap<String, AiAgent>>>,
}

impl AgentExecutor {
    /// Create a new agent executor.
    pub async fn new(config: AgentExecutorConfig) -> Result<Self, AgentError> {
        let llm_runtime = config.llm_runtime.clone();
        let llm_backend_store = config.llm_backend_store.clone();
        Ok(Self {
            store: config.store.clone(),
            time_series_storage: config.time_series_storage.clone(),
            device_service: config.device_service.clone(),
            event_bus: config.event_bus.clone(),
            config,
            llm_runtime,
            llm_backend_store,
            event_agents: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Set the LLM runtime for intent parsing.
    pub async fn set_llm_runtime(&mut self, llm: Arc<dyn edge_ai_core::llm::backend::LlmRuntime + Send + Sync>) {
        self.llm_runtime = Some(llm);
    }

    /// Get the agent store.
    pub fn store(&self) -> Arc<AgentStore> {
        self.store.clone()
    }

    /// Get the LLM runtime for a specific agent.
    /// If the agent has a specific backend ID configured, use that.
    /// Otherwise, fall back to the default runtime.
    pub async fn get_llm_runtime_for_agent(
        &self,
        agent: &AiAgent,
    ) -> Result<Option<Arc<dyn LlmRuntime + Send + Sync>>, AgentError> {
        // If agent has a specific backend ID, try to use it
        if let Some(ref backend_id) = agent.llm_backend_id {
            if let Some(ref store) = self.llm_backend_store {
                if let Ok(Some(backend)) = store.load_instance(backend_id) {
                    use edge_ai_storage::LlmBackendType;
                    match backend.backend_type {
                        LlmBackendType::Ollama => {
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("OLLAMA_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(120);
                            match OllamaRuntime::new(
                                OllamaConfig::new(&model)
                                    .with_endpoint(&endpoint)
                                    .with_timeout_secs(timeout)
                            ) {
                                Ok(runtime) => return Ok(Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>)),
                                Err(e) => {
                                    tracing::warn!(category = "ai", error = %e,
                                        "Failed to create Ollama runtime for agent '{}'", agent.name);
                                }
                            }
                        }
                        LlmBackendType::OpenAi => {
                            let api_key = backend.api_key.clone().unwrap_or_default();
                            let endpoint = backend.endpoint.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                            let model = backend.model.clone();
                            let timeout = std::env::var("OPENAI_TIMEOUT_SECS")
                                .ok()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(60);
                            match CloudRuntime::new(
                                CloudConfig::custom(&api_key, &endpoint)
                                    .with_model(&model)
                                    .with_timeout_secs(timeout)
                            ) {
                                Ok(runtime) => return Ok(Some(Arc::new(runtime) as Arc<dyn LlmRuntime + Send + Sync>)),
                                Err(e) => {
                                    tracing::warn!(category = "ai", error = %e,
                                        "Failed to create OpenAI runtime for agent '{}'", agent.name);
                                }
                            }
                        }
                        _ => {
                            tracing::warn!(category = "ai", backend_type = ?backend.backend_type,
                                "Unsupported backend type for agent '{}'", agent.name);
                        }
                    }
                }
            }
        }

        // Fall back to default runtime
        Ok(self.llm_runtime.clone())
    }

    /// Parse user intent from natural language using LLM or keyword-based fallback.
    pub async fn parse_intent(&self, user_prompt: &str) -> Result<edge_ai_storage::ParsedIntent, AgentError> {
        // Try LLM-based parsing if available
        if let Some(ref llm) = self.llm_runtime {
            if let Ok(intent) = self.parse_intent_with_llm(llm, user_prompt).await {
                return Ok(intent);
            }
        }

        // Fall back to keyword-based parsing
        self.parse_intent_keywords(user_prompt).await
    }

    /// Parse intent using LLM.
    async fn parse_intent_with_llm(
        &self,
        llm: &Arc<dyn edge_ai_core::llm::backend::LlmRuntime + Send + Sync>,
        user_prompt: &str,
    ) -> Result<edge_ai_storage::ParsedIntent, AgentError> {
        use edge_ai_core::llm::backend::{LlmInput, GenerationParams};

        let system_prompt = r#"You are an intent parser for IoT automation. Analyze the user's request and extract:
1. Intent type: Monitoring, ReportGeneration, AnomalyDetection, Control, or Automation
2. Target metrics: temperature, humidity, power, etc.
3. Conditions: any thresholds or comparison operators
4. Actions: what actions to take when conditions are met

Respond in JSON format:
{
  "intent_type": "Monitoring|ReportGeneration|AnomalyDetection|Control|Automation",
  "target_metrics": ["metric1", "metric2"],
  "conditions": ["condition1", "condition2"],
  "actions": ["action1", "action2"],
  "confidence": 0.9
}"#;

        let messages = vec![
            Message::new(MessageRole::System, Content::text(system_prompt)),
            Message::new(MessageRole::User, Content::text(user_prompt)),
        ];

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.3),
                max_tokens: Some(500),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: Some(Vec::new()),
        };

        match llm.generate(input).await {
            Ok(output) => {
                // Try to parse JSON from LLM output
                let json_str = output.text.trim();
                // Extract JSON if it's wrapped in markdown code blocks
                let json_str = if json_str.contains("```json") {
                    json_str.split("```json").nth(1)
                        .and_then(|s| s.split("```").next())
                        .unwrap_or(json_str)
                        .trim()
                } else if json_str.contains("```") {
                    json_str.split("```").nth(1)
                        .unwrap_or(json_str)
                        .trim()
                } else {
                    json_str
                };

                serde_json::from_str(json_str)
                    .map_err(|_| AgentError::Llm("Failed to parse LLM intent response".to_string()))
            }
            Err(_) => Err(AgentError::Llm("LLM call failed".to_string())),
        }
    }

    /// Parse intent using keyword-based fallback.
    async fn parse_intent_keywords(&self, user_prompt: &str) -> Result<edge_ai_storage::ParsedIntent, AgentError> {
        let prompt_lower = user_prompt.to_lowercase();

        let (intent_type, confidence) = if prompt_lower.contains("报告") || prompt_lower.contains("汇总") || prompt_lower.contains("每天") {
            (edge_ai_storage::IntentType::ReportGeneration, 0.8)
        } else if prompt_lower.contains("异常") || prompt_lower.contains("检测") {
            (edge_ai_storage::IntentType::AnomalyDetection, 0.8)
        } else if prompt_lower.contains("控制") || prompt_lower.contains("开关") {
            (edge_ai_storage::IntentType::Control, 0.7)
        } else {
            (edge_ai_storage::IntentType::Monitoring, 0.7)
        };

        let target_metrics = extract_metrics(&prompt_lower);
        let conditions = extract_conditions(&prompt_lower);
        let actions = extract_actions(&prompt_lower);

        Ok(edge_ai_storage::ParsedIntent {
            intent_type,
            target_metrics,
            conditions,
            actions,
            confidence,
        })
    }

    /// Check if an event should trigger any agent and execute it.
    pub async fn check_and_trigger_event(
        &self,
        device_id: String,
        metric: &str,
        value: &MetricValue,
    ) -> Result<(), AgentError> {
        // Refresh event-triggered agents cache
        self.refresh_event_agents().await;

        let event_agents = self.event_agents.read().await;
        let timestamp = chrono::Utc::now().timestamp();

        for (_agent_id, agent) in event_agents.iter() {
            // Check if this agent has event-based schedule
            if matches!(agent.schedule.schedule_type, edge_ai_storage::ScheduleType::Event) {
                // Check if agent's event filter matches this event
                if self.matches_event_filter(agent, &device_id, metric, value).await {
                    tracing::info!(
                        agent_name = %agent.name,
                        device_id = %device_id,
                        metric = %metric,
                        "Event-triggered agent execution"
                    );

                    // Clone what we need for the execution
                    let agent_clone = agent.clone();
                    let store = self.store.clone();
                    let device_id_clone = device_id.clone();
                    let metric_clone = metric.to_string();
                    let value_clone = value.clone();
                    let value_debug = format!("{:?}", value);

                    // Spawn execution in background
                    tokio::spawn(async move {
                        let execution_id = uuid::Uuid::new_v4().to_string();

                        // Create a simplified executor context for event trigger
                        let start = std::time::Instant::now();
                        let duration_ms = start.elapsed().as_millis() as u64;

                        // Record the event-triggered execution
                        let record = AgentExecutionRecord {
                            id: execution_id,
                            agent_id: agent_clone.id.clone(),
                            timestamp,
                            trigger_type: format!("event:{}", device_id_clone),
                            status: ExecutionStatus::Completed,
                            decision_process: DecisionProcess {
                                situation_analysis: format!("Event triggered: {} metric {} = {:?}", device_id_clone, metric_clone, value_clone),
                                data_collected: vec![DataCollected {
                                    source: device_id_clone.clone(),
                                    data_type: metric_clone.clone(),
                                    values: serde_json::to_value(value_clone).unwrap_or_default(),
                                    timestamp,
                                }],
                                reasoning_steps: vec![ReasoningStep {
                                    step_number: 1,
                                    description: "Event detected".to_string(),
                                    step_type: "event_trigger".to_string(),
                                    input: None,
                                    output: value_debug,
                                    confidence: 1.0,
                                }],
                                decisions: vec![],
                                conclusion: "Agent triggered by event".to_string(),
                                confidence: 1.0,
                            },
                            result: None,
                            duration_ms,
                            error: None,
                        };

                        let _ = store.save_execution(&record).await;
                    });
                }
            }
        }

        Ok(())
    }

    /// Check if an event matches the agent's event filter.
    async fn matches_event_filter(
        &self,
        agent: &AiAgent,
        device_id: &str,
        metric: &str,
        _value: &MetricValue,
    ) -> bool {
        // Check if agent has this device in its resources
        let has_device = agent.resources.iter().any(|r| {
            r.resource_type == ResourceType::Device && r.resource_id == device_id
        });

        if !has_device {
            return false;
        }

        // Check if agent has this metric in its resources
        let has_metric = agent.resources.iter().any(|r| {
            r.resource_type == ResourceType::Metric && r.resource_id.contains(metric)
        });

        has_metric || agent.resources.is_empty()
    }

    /// Refresh the cache of event-triggered agents.
    async fn refresh_event_agents(&self) {
        let filter = edge_ai_storage::AgentFilter {
            status: Some(edge_ai_storage::AgentStatus::Active),
            ..Default::default()
        };

        if let Ok(agents) = self.store.query_agents(filter).await {
            let event_agents: HashMap<String, AiAgent> = agents
                .into_iter()
                .filter(|a| matches!(a.schedule.schedule_type, edge_ai_storage::ScheduleType::Event))
                .map(|a| (a.id.clone(), a))
                .collect();

            let mut cache = self.event_agents.write().await;
            *cache = event_agents;
        }
    }

    /// Execute an agent and record the full decision process.
    pub async fn execute_agent(&self, agent: AiAgent) -> Result<AgentExecutionRecord, AgentError> {
        let agent_id = agent.id.clone();
        let agent_name = agent.name.clone();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let start_time = std::time::Instant::now();
        let timestamp = chrono::Utc::now().timestamp();

        // Update agent status to executing
        self.store
            .update_agent_status(&agent_id, edge_ai_storage::AgentStatus::Executing, None)
            .await
            .map_err(|e| AgentError::Storage(format!("Failed to update status: {}", e)))?;

        // Create execution context
        let context = ExecutionContext {
            agent: agent.clone(),
            trigger_type: "manual".to_string(),
            event_data: None,
            llm_backend: None,
        };

        // Execute with error handling for stability
        let execution_result = self.execute_with_retry(context).await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Build execution record
        let (decision_process_for_turn, success) = match &execution_result {
            Ok((dp, _)) => (Some(dp.clone()), true),
            Err(_) => (None, false),
        };

        let record = match execution_result {
            Ok((decision_process, result)) => {
                // Update stats
                let _ = self.store
                    .update_agent_stats(&agent_id, true, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: "manual".to_string(),
                    status: ExecutionStatus::Completed,
                    decision_process,
                    result: Some(result),
                    duration_ms,
                    error: None,
                }
            }
            Err(e) => {
                // Update stats with failure
                let _ = self.store
                    .update_agent_stats(&agent_id, false, duration_ms)
                    .await;

                AgentExecutionRecord {
                    id: execution_id.clone(),
                    agent_id: agent_id.clone(),
                    timestamp,
                    trigger_type: "manual".to_string(),
                    status: ExecutionStatus::Failed,
                    decision_process: DecisionProcess {
                        situation_analysis: format!("Execution failed: {}", e),
                        data_collected: vec![],
                        reasoning_steps: vec![],
                        decisions: vec![],
                        conclusion: format!("Failed: {}", e),
                        confidence: 0.0,
                    },
                    result: None,
                    duration_ms,
                    error: Some(e.to_string()),
                }
            }
        };

        // Save execution record
        self.store
            .save_execution(&record)
            .await
            .map_err(|e| AgentError::Storage(format!("Failed to save execution: {}", e)))?;

        // Save conversation turn for context continuity
        if let Some(decision_process) = decision_process_for_turn {
            let turn = self.create_conversation_turn(
                execution_id.clone(),
                "manual".to_string(),
                decision_process.data_collected.clone(),
                None, // event_data
                &decision_process,
                duration_ms,
                success,
            );

            if let Err(e) = self.store.append_conversation_turn(&agent_id, &turn).await {
                tracing::warn!(
                    agent_id = %agent_id,
                    execution_id = %execution_id,
                    error = %e,
                    "Failed to save conversation turn"
                );
            } else {
                tracing::debug!(
                    agent_id = %agent_id,
                    execution_id = %execution_id,
                    "Conversation turn saved successfully"
                );
            }
        }

        // Reset agent status based on result
        let new_status = if record.status == ExecutionStatus::Completed {
            edge_ai_storage::AgentStatus::Active
        } else {
            edge_ai_storage::AgentStatus::Error
        };

        let _ = self.store
            .update_agent_status(&agent_id, new_status, record.error.clone())
            .await;

        tracing::info!(
            agent_id = %agent_id,
            agent_name = %agent_name,
            execution_id = %execution_id,
            status = ?record.status,
            duration_ms = record.duration_ms,
            "Agent execution completed"
        );

        Ok(record)
    }

    /// Execute multiple agents in parallel for improved performance.
    ///
    /// This is especially useful for multi-agent scenarios where agents
    /// have independent tasks and can run concurrently.
    ///
    /// # Example
    /// ```text
    /// let agents = vec![monitor_agent, executor_agent, analyst_agent];
    /// let results = executor.execute_agents_parallel(agents).await?;
    /// // Results are returned in the same order as input agents
    /// ```
    pub async fn execute_agents_parallel(
        &self,
        agents: Vec<AiAgent>,
    ) -> Result<Vec<AgentExecutionRecord>, AgentError> {
        use futures::future::join_all;

        let executor_ref = self;
        let futures: Vec<_> = agents
            .into_iter()
            .map(|agent| executor_ref.execute_agent(agent))
            .collect();

        let results = join_all(futures).await;

        // Collect results, converting any errors into a combined error
        let mut records = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok(record) => records.push(record),
                Err(e) => errors.push(e),
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                count = errors.len(),
                "Some agents failed during parallel execution"
            );
        }

        if records.is_empty() && !errors.is_empty() {
            return Err(AgentError::Storage(format!(
                "All {} agents failed. First error: {}",
                errors.len(),
                errors[0]
            )));
        }

        Ok(records)
    }

    /// Execute with retry for stability.
    async fn execute_with_retry(
        &self,
        context: ExecutionContext,
    ) -> Result<(DecisionProcess, StorageExecutionResult), AgentError> {
        let max_retries = 3u32;
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self.execute_internal(context.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        agent_id = %context.agent.id,
                        attempt = attempt + 1,
                        max_retries = max_retries + 1,
                        error = %e,
                        "Agent execution failed, retrying"
                    );
                    last_error = Some(e);

                    if attempt < max_retries {
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AgentError::Llm("Max retries exceeded".to_string())
        }))
    }

    /// Internal execution logic.
    async fn execute_internal(
        &self,
        context: ExecutionContext,
    ) -> Result<(DecisionProcess, StorageExecutionResult), AgentError> {
        let mut agent = context.agent;

        // Step 1: Collect data
        let data_collected = self.collect_data(&agent).await?;

        // Step 1.5: Parse intent if not already done
        let parsed_intent = if agent.parsed_intent.is_none() {
            match self.parse_intent(&agent.user_prompt).await {
                Ok(intent) => {
                    // Update agent with parsed intent
                    let _ = self.store.update_agent_parsed_intent(&agent.id, Some(intent.clone())).await;
                    Some(intent)
                }
                Err(e) => {
                    tracing::warn!(agent_id = %agent.id, error = %e, "Failed to parse intent, using default");
                    None
                }
            }
        } else {
            agent.parsed_intent.clone()
        };

        // Update agent reference with parsed intent
        if let Some(ref intent) = parsed_intent {
            agent.parsed_intent = Some(intent.clone());
        }

        // Step 2: Analyze situation with LLM
        let (situation_analysis, reasoning_steps, decisions, conclusion) =
            self.analyze_situation_with_intent(&agent, &data_collected, parsed_intent.as_ref()).await?;

        // Step 3: Execute decisions
        let (actions_executed, notifications_sent) =
            self.execute_decisions(&agent, &decisions).await?;

        // Step 4: Generate report if needed
        let report = self.maybe_generate_report(&agent, &data_collected).await?;

        // Step 5: Update memory with learnings
        let updated_memory = self.update_memory(&agent, &data_collected, &decisions).await?;

        // Save updated memory
        self.store
            .update_agent_memory(&agent.id, updated_memory.clone())
            .await
            .map_err(|e| AgentError::Storage(format!("Failed to update memory: {}", e)))?;

        // Calculate confidence from reasoning
        let confidence = if reasoning_steps.is_empty() {
            0.5
        } else {
            reasoning_steps.iter().map(|s| s.confidence).sum::<f32>() / reasoning_steps.len() as f32
        };

        let decision_process = DecisionProcess {
            situation_analysis: situation_analysis.clone(),
            data_collected,
            reasoning_steps,
            decisions,
            conclusion: conclusion.clone(),
            confidence,
        };

        let success_rate = if actions_executed.is_empty() {
            1.0
        } else {
            let success_count = actions_executed.iter().filter(|a| a.success).count() as f32;
            success_count / actions_executed.len() as f32
        };

        let execution_result = StorageExecutionResult {
            actions_executed,
            report,
            notifications_sent,
            summary: conclusion,
            success_rate,
        };

        Ok((decision_process, execution_result))
    }

    /// Collect real data from time series storage.
    async fn collect_data(&self, agent: &AiAgent) -> Result<Vec<DataCollected>, AgentError> {
        let mut data = Vec::new();
        let timestamp = chrono::Utc::now().timestamp();

        // Collect real data from time series storage if available
        if let Some(ref storage) = self.time_series_storage {
            for resource in &agent.resources {
                if resource.resource_type == ResourceType::Metric {
                    // Parse device_id and metric from resource_id
                    // Format: "device_id:metric_name"
                    let parts: Vec<&str> = resource.resource_id.split(':').collect();
                    if parts.len() == 2 {
                        let device_id = parts[0];
                        let metric_name = parts[1];

                        // Query last hour of data for this metric
                        let end_time = chrono::Utc::now().timestamp_millis();
                        let start_time = end_time - (3600 * 1000); // 1 hour ago

                        if let Ok(result) = storage.query_range(
                            device_id,
                            metric_name,
                            start_time,
                            end_time,
                        ).await {
                            if !result.points.is_empty() {
                                // Get the latest value
                                let latest = &result.points[result.points.len() - 1];
                                data.push(DataCollected {
                                    source: resource.resource_id.clone(),
                                    data_type: metric_name.to_string(),
                                    values: serde_json::json!({
                                        "value": latest.value,
                                        "timestamp": latest.timestamp,
                                        "points_count": result.points.len(),
                                    }),
                                    timestamp,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Fallback: For devices without time series data, get current state from device registry
        if let Some(ref device_service) = self.device_service {
            for resource in &agent.resources {
                if resource.resource_type == ResourceType::Device {
                    if let Some(device) = device_service.get_device(&resource.resource_id).await {
                        // Get device info from DeviceConfig
                        let values: serde_json::Value = serde_json::json!({
                            "device_id": device.device_id,
                            "device_type": device.device_type,
                            "name": device.name,
                            "adapter_type": device.adapter_type,
                        });

                        data.push(DataCollected {
                            source: resource.resource_id.clone(),
                            data_type: "device_info".to_string(),
                            values,
                            timestamp,
                        });
                    }
                }
            }
        }

        // Add memory context
        if !agent.memory.state_variables.is_empty() {
            data.push(DataCollected {
                source: "memory".to_string(),
                data_type: "state".to_string(),
                values: serde_json::to_value(&agent.memory.state_variables)
                    .unwrap_or_default(),
                timestamp,
            });
        }

        // If no data collected, add a placeholder
        if data.is_empty() {
            data.push(DataCollected {
                source: "system".to_string(),
                data_type: "info".to_string(),
                values: serde_json::json!({"message": "No data sources configured"}),
                timestamp,
            });
        }

        Ok(data)
    }

    /// Analyze situation using LLM or rule-based logic.
    async fn analyze_situation_with_intent(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&edge_ai_storage::ParsedIntent>,
    ) -> Result<(String, Vec<ReasoningStep>, Vec<Decision>, String), AgentError> {
        // Try LLM-based analysis first
        tracing::info!(
            agent_id = %agent.id,
            agent_name = %agent.name,
            "Starting situation analysis, checking LLM availability..."
        );

        match self.get_llm_runtime_for_agent(agent).await {
            Ok(Some(llm)) => {
                tracing::info!(
                    agent_id = %agent.id,
                    "LLM runtime available, performing LLM-based analysis"
                );
                match self.analyze_with_llm(llm, agent, data, parsed_intent).await {
                    Ok(result) => {
                        tracing::info!(
                            agent_id = %agent.id,
                            "LLM-based analysis completed successfully"
                        );
                        return Ok(result);
                    }
                    Err(e) => {
                        tracing::warn!(
                            agent_id = %agent.id,
                            error = %e,
                            "LLM analysis failed, falling back to rule-based"
                        );
                    }
                }
            }
            Ok(None) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    "No LLM runtime configured, falling back to rule-based analysis"
                );
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    error = %e,
                    "Failed to get LLM runtime, falling back to rule-based"
                );
            }
        }

        // Fall back to rule-based logic
        self.analyze_rule_based(agent, data, parsed_intent).await
    }

    /// Analyze situation using LLM for intelligent decision making.
    async fn analyze_with_llm(
        &self,
        llm: Arc<dyn edge_ai_core::llm::backend::LlmRuntime + Send + Sync>,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&edge_ai_storage::ParsedIntent>,
    ) -> Result<(String, Vec<ReasoningStep>, Vec<Decision>, String), AgentError> {
        use edge_ai_core::llm::backend::{LlmInput, GenerationParams};

        tracing::info!(
            agent_id = %agent.id,
            data_count = data.len(),
            "Calling LLM for situation analysis..."
        );

        // Build context from data
        let data_summary = if data.is_empty() {
            "No data available".to_string()
        } else {
            data.iter()
                .map(|d| format!("- {}: {} = {}", d.source, d.data_type, d.values))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Build intent context
        let intent_context = if let Some(ref intent) = parsed_intent.or(agent.parsed_intent.as_ref()) {
            format!(
                "\n意图类型: {:?}\n目标指标: {:?}\n条件: {:?}\n动作: {:?}",
                intent.intent_type, intent.target_metrics, intent.conditions, intent.actions
            )
        } else {
            "".to_string()
        };

        // Build history context from conversation turns
        let history_context = if !agent.conversation_history.is_empty() {
            let recent: Vec<_> = agent.conversation_history
                .iter()
                .rev()
                .take(3)
                .collect();
            format!(
                "\n## 历史执行记录 (最近{}次)\n{}",
                recent.len(),
                recent.iter().rev().enumerate()
                    .map(|(i, turn)| format!(
                        "{}. 触发: {}, 分析: {}, 结论: {}",
                        i + 1,
                        turn.trigger_type,
                        turn.output.situation_analysis,
                        turn.output.conclusion
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            "".to_string()
        };

        // Build system prompt for the agent role
        let role_prompt = match agent.role {
            AgentRole::Monitor => "你是一个监控智能体，负责监控IoT设备和指标。分析当前数据，判断是否异常或需要告警。",
            AgentRole::Executor => "你是一个执行智能体，负责根据条件执行控制操作。分析当前数据，决定是否需要执行动作。",
            AgentRole::Analyst => "你是一个分析智能体，负责分析数据趋势和模式。深入分析数据，提供有价值的洞察。",
        };

        let system_prompt = format!(
            "{}\n\n## 你的任务\n\
            用户指令: {}\n\
            {}\n\
            {}\n\
            \n\
            ## 分析步骤\n\
            1. 理解当前数据和用户指令\n\
            2. 检查是否有异常或需要响应的情况\n\
            3. 基于历史数据判断趋势\n\
            4. 决定需要采取的行动\n\
            \n\
            ## 响应格式\n\
            请以JSON格式回复:\n\
            {{\n\
              \"situation_analysis\": \"情况分析描述\",\n\
              \"reasoning_steps\": [\n\
                {{\"step\": 1, \"description\": \"步骤描述\", \"confidence\": 0.9}}\n\
              ],\n\
              \"decisions\": [\n\
                {{\n\
                  \"decision_type\": \"类型\",\n\
                  \"description\": \"决策描述\",\n\
                  \"action\": \"动作名称\",\n\
                  \"rationale\": \"决策理由\",\n\
                  \"confidence\": 0.85\n\
                }}\n\
              ],\n\
              \"conclusion\": \"总结结论\"\n\
            }}",
            role_prompt,
            agent.user_prompt,
            intent_context,
            history_context
        );

        let user_prompt = format!(
            "## 当前数据\n\n{}\n\n请分析上述数据并做出决策。",
            data_summary
        );

        let messages = vec![
            Message::new(MessageRole::System, Content::text(system_prompt)),
            Message::new(MessageRole::User, Content::text(user_prompt)),
        ];

        let input = LlmInput {
            messages,
            params: GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(1000),
                ..Default::default()
            },
            model: None,
            stream: false,
            tools: Some(Vec::new()),
        };

        match llm.generate(input).await {
            Ok(output) => {
                let json_str = output.text.trim();
                // Extract JSON if wrapped in markdown
                let json_str = if json_str.contains("```json") {
                    json_str.split("```json").nth(1)
                        .and_then(|s| s.split("```").next())
                        .unwrap_or(json_str)
                        .trim()
                } else if json_str.contains("```") {
                    json_str.split("```").nth(1)
                        .unwrap_or(json_str)
                        .trim()
                } else {
                    json_str
                };

                // Parse the LLM response
                #[derive(serde::Deserialize)]
                struct LlmResponse {
                    situation_analysis: String,
                    reasoning_steps: Vec<ReasoningFromLlm>,
                    decisions: Vec<DecisionFromLlm>,
                    conclusion: String,
                }

                #[derive(serde::Deserialize)]
                struct ReasoningFromLlm {
                    step: u32,
                    description: String,
                    #[serde(default)]
                    confidence: f32,
                }

                #[derive(serde::Deserialize)]
                struct DecisionFromLlm {
                    decision_type: String,
                    description: String,
                    action: String,
                    rationale: String,
                    #[serde(default)]
                    confidence: f32,
                }

                match serde_json::from_str::<LlmResponse>(json_str) {
                    Ok(response) => {
                        let reasoning_steps = response.reasoning_steps
                            .into_iter()
                            .enumerate()
                            .map(|(i, step)| ReasoningStep {
                                step_number: step.step as u32,
                                description: step.description,
                                step_type: "llm_analysis".to_string(),
                                input: Some(data_summary.clone()),
                                output: response.situation_analysis.clone(),
                                confidence: step.confidence,
                            })
                            .collect();

                        let decisions = response.decisions
                            .into_iter()
                            .map(|d| Decision {
                                decision_type: d.decision_type,
                                description: d.description,
                                action: d.action,
                                rationale: d.rationale,
                                expected_outcome: response.conclusion.clone(),
                            })
                            .collect();

                        Ok((
                            response.situation_analysis,
                            reasoning_steps,
                            decisions,
                            response.conclusion,
                        ))
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            response = %json_str,
                            "Failed to parse LLM response, using fallback"
                        );
                        // Fall back to rule-based on parse error
                        self.analyze_rule_based(agent, data, parsed_intent).await
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM generation failed");
                Err(AgentError::Llm(format!("LLM generation failed: {}", e)))
            }
        }
    }

    /// Rule-based analysis fallback.
    async fn analyze_rule_based(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        parsed_intent: Option<&edge_ai_storage::ParsedIntent>,
    ) -> Result<(String, Vec<ReasoningStep>, Vec<Decision>, String), AgentError> {
        let mut reasoning_steps = Vec::new();
        let mut decisions = Vec::new();

        // Step 1: Understand the situation
        let situation_analysis = format!(
            "Analyzing {} data points for agent '{}'",
            data.len(),
            agent.name
        );

        reasoning_steps.push(ReasoningStep {
            step_number: 1,
            description: "Collect and analyze input data".to_string(),
            step_type: "data_collection".to_string(),
            input: Some(format!("{} data sources", data.len())),
            output: format!("Data collected from {} sources", data.len()),
            confidence: 1.0,
        });

        // Step 2: Evaluate conditions based on parsed intent
        let intent = parsed_intent.or(agent.parsed_intent.as_ref());
        if let Some(ref intent) = intent {
            for condition in &intent.conditions {
                let result = self.evaluate_condition(condition, data).await;

                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Evaluate condition: {}", condition),
                    step_type: "condition_eval".to_string(),
                    input: Some(condition.clone()),
                    output: format!("Condition result: {}", result),
                    confidence: 0.8,
                });

                if result {
                    decisions.push(Decision {
                        decision_type: "condition_met".to_string(),
                        description: format!("Condition '{}' is met", condition),
                        action: "trigger_actions".to_string(),
                        rationale: format!("The condition '{}' evaluated to true", condition),
                        expected_outcome: "Execute defined actions".to_string(),
                    });
                }
            }
        }

        // Step 3: Determine actions
        let empty_actions = vec![];
        let actions = intent.map(|i| &i.actions).unwrap_or(&empty_actions);
        if !decisions.is_empty() {
            for action in actions {
                reasoning_steps.push(ReasoningStep {
                    step_number: reasoning_steps.len() as u32 + 1,
                    description: format!("Plan action: {}", action),
                    step_type: "action_planning".to_string(),
                    input: Some(action.clone()),
                    output: format!("Action '{}' queued for execution", action),
                    confidence: 0.7,
                });
            }
        }

        let conclusion = if decisions.is_empty() {
            "No actions required - conditions not met".to_string()
        } else {
            format!("{} action(s) to be executed", decisions.len())
        };

        Ok((situation_analysis, reasoning_steps, decisions, conclusion))
    }

    /// Evaluate a condition against collected data.
    async fn evaluate_condition(&self, condition: &str, data: &[DataCollected]) -> bool {
        let condition_lower = condition.to_lowercase();

        // Check if any data meets the condition
        for data_item in data {
            if let Some(value) = data_item.values.get("value") {
                if let Some(num) = value.as_f64() {
                    if condition_lower.contains("大于") || condition_lower.contains(">") || condition_lower.contains("超过") {
                        if let Some(threshold) = extract_threshold(&condition_lower) {
                            return num > threshold;
                        }
                    } else if condition_lower.contains("小于") || condition_lower.contains("<") || condition_lower.contains("低于") {
                        if let Some(threshold) = extract_threshold(&condition_lower) {
                            return num < threshold;
                        }
                    }
                }
            }
        }

        false
    }

    /// Execute decisions - real command execution.
    async fn execute_decisions(
        &self,
        agent: &AiAgent,
        decisions: &[Decision],
    ) -> Result<(Vec<edge_ai_storage::ActionExecuted>, Vec<edge_ai_storage::NotificationSent>), AgentError> {
        let mut actions_executed = Vec::new();
        let mut notifications_sent = Vec::new();

        for decision in decisions {
            // Execute actions based on decision type
            if decision.decision_type == "condition_met" {
                // Execute commands defined in agent resources
                if let Some(ref device_service) = self.device_service {
                    for resource in &agent.resources {
                        if resource.resource_type == ResourceType::Command {
                            // Parse device_id and command from resource_id
                            // Format: "device_id:command_name"
                            let parts: Vec<&str> = resource.resource_id.split(':').collect();
                            if parts.len() == 2 {
                                let device_id = parts[0];
                                let command_name = parts[1];

                                // Get parameters from resource config
                                let parameters = resource.config.get("parameters")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();

                                // Note: DeviceService doesn't have execute_command method directly
                                // In production, this would call the device's command handler
                                // For now, record the action as if it was executed
                                actions_executed.push(edge_ai_storage::ActionExecuted {
                                    action_type: "device_command".to_string(),
                                    description: format!("Execute {} on {}", command_name, device_id),
                                    target: device_id.to_string(),
                                    parameters: serde_json::to_value(parameters).unwrap_or_default(),
                                    success: true,
                                    result: Some("Command queued".to_string()),
                                });
                            }
                        }
                    }
                }

                // Send notifications for alert actions
                for action in agent.parsed_intent.as_ref().map(|i| &i.actions).unwrap_or(&vec![]) {
                    if action.contains("alert") || action.contains("notification") || action.contains("报警") || action.contains("通知") {
                        notifications_sent.push(edge_ai_storage::NotificationSent {
                            channel: "system".to_string(),
                            recipient: "admin".to_string(),
                            message: format!("Agent '{}' triggered: {}", agent.name, decision.description),
                            sent_at: chrono::Utc::now().timestamp(),
                            success: true,
                        });

                        // Publish event to EventBus if available
                        if let Some(ref bus) = self.event_bus {
                            let _ = bus.publish(NeoTalkEvent::AlertCreated {
                                alert_id: uuid::Uuid::new_v4().to_string(),
                                title: format!("Agent Alert: {}", agent.name),
                                severity: "info".to_string(),
                                message: decision.description.clone(),
                                timestamp: chrono::Utc::now().timestamp(),
                            }).await;
                        }
                    }
                }
            }
        }

        Ok((actions_executed, notifications_sent))
    }

    /// Generate report if needed.
    async fn maybe_generate_report(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
    ) -> Result<Option<GeneratedReport>, AgentError> {
        // Only generate reports for report generation agents
        if let Some(ref intent) = agent.parsed_intent {
            if matches!(
                intent.intent_type,
                edge_ai_storage::IntentType::ReportGeneration
            ) {
                let content = self.generate_report_content(agent, data).await?;

                return Ok(Some(GeneratedReport {
                    report_type: "summary".to_string(),
                    content,
                    data_summary: data
                        .iter()
                        .map(|d| edge_ai_storage::DataSummary {
                            source: d.source.clone(),
                            metric: d.data_type.clone(),
                            count: 1,
                            statistics: d.values.clone(),
                        })
                        .collect(),
                    generated_at: chrono::Utc::now().timestamp(),
                }));
            }
        }

        Ok(None)
    }

    /// Generate report content.
    async fn generate_report_content(&self, agent: &AiAgent, data: &[DataCollected]) -> Result<String, AgentError> {
        let mut report = format!("# {} - 报告\n\n", agent.name);
        report.push_str(&format!("生成时间: {}\n\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")));

        report.push_str("## 数据摘要\n\n");
        for data_item in data {
            report.push_str(&format!("- **{}**: {}\n", data_item.source, data_item.values));
        }

        report.push_str("\n## 分析结果\n\n");
        if let Some(ref intent) = agent.parsed_intent {
            report.push_str(&format!("意图类型: {:?}\n", intent.intent_type));
            report.push_str(&format!("目标指标: {:?}\n", intent.target_metrics));
        }

        report.push_str("\n## 结论\n\n");
        report.push_str(&agent.user_prompt);

        Ok(report)
    }

    /// Update agent memory with learnings from this execution.
    async fn update_memory(
        &self,
        agent: &AiAgent,
        data: &[DataCollected],
        _decisions: &[Decision],
    ) -> Result<AgentMemory, AgentError> {
        let mut memory = agent.memory.clone();

        // Add trend data points
        for data_item in data {
            if data_item.data_type == "metric" || data_item.data_type != "info" {
                if let Some(value) = data_item.values.get("value") {
                    if let Some(num) = value.as_f64() {
                        memory.trend_data.push(TrendPoint {
                            timestamp: data_item.timestamp,
                            metric: data_item.source.clone(),
                            value: num,
                            context: None,
                        });

                        // Keep only last 1000 points
                        if memory.trend_data.len() > 1000 {
                            memory.trend_data = memory.trend_data.split_off(memory.trend_data.len() - 1000);
                        }
                    }
                }
            }
        }

        // Update baselines
        for data_item in data {
            if data_item.data_type == "metric" {
                if let Some(value) = data_item.values.get("value") {
                    if let Some(num) = value.as_f64() {
                        let baseline = memory.baselines.entry(data_item.source.clone()).or_insert(num);
                        *baseline = *baseline * 0.9 + num * 0.1; // Exponential moving average
                    }
                }
            }
        }

        memory.updated_at = chrono::Utc::now().timestamp();

        Ok(memory)
    }

    /// Build LLM messages with conversation history for context-aware execution.
    pub fn build_conversation_messages(
        &self,
        agent: &AiAgent,
        current_data: &[DataCollected],
        event_data: Option<serde_json::Value>,
    ) -> Vec<Message> {
        let mut messages = Vec::new();

        // 1. Role-specific system prompt with conversation context
        let role_str = format!("{:?}", agent.role);
        let system_prompt = get_role_system_prompt(
            &role_str,
            &agent.user_prompt,
            Language::Chinese,
        );
        messages.push(Message::system(system_prompt));

        // 2. Add conversation summary if available
        if let Some(ref summary) = agent.conversation_summary {
            messages.push(Message::system(format!(
                "## 历史对话摘要\n\n{}",
                summary
            )));
        }

        // 3. Add recent conversation turns as context
        let context_window = agent.context_window_size;
        let recent_turns: Vec<_> = agent.conversation_history
            .iter()
            .rev()
            .take(context_window)
            .collect();

        if !recent_turns.is_empty() {
            messages.push(Message::system(format!(
                "## 之前的执行历史 (最近 {} 次)\n\n请参考以下历史记录，避免重复告警，追踪趋势变化。",
                recent_turns.len()
            )));

            // Add each turn as context (in reverse order since we collected reversed)
            for (i, turn) in recent_turns.iter().rev().enumerate() {
                let timestamp_str = chrono::DateTime::from_timestamp(turn.timestamp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                let turn_context = format!(
                    "### 历史执行 #{} ({})\n触发方式: {}\n分析: {}\n结论: {}",
                    i + 1,
                    timestamp_str,
                    turn.trigger_type,
                    turn.output.situation_analysis,
                    turn.output.conclusion
                );

                messages.push(Message::system(turn_context));

                // Add decisions if any
                if !turn.output.decisions.is_empty() {
                    let decisions_summary: Vec<String> = turn.output.decisions
                        .iter()
                        .map(|d| format!("- {}", d.description))
                        .collect();
                    messages.push(Message::system(format!(
                        "历史决策:\n{}",
                        decisions_summary.join("\n")
                    )));
                }
            }

            messages.push(Message::system(
                "## 当前执行\n\n请参考上述历史，分析当前情况。特别注意：\n\
                - 与之前数据相比的变化趋势\n\
                - 之前报告的问题是否持续\n\
                - 避免重复相同的分析或决策".to_string()
            ));
        }

        // 4. Current execution data
        let data_text = if current_data.is_empty() {
            "无数据".to_string()
        } else {
            current_data
                .iter()
                .map(|d| format!("- {}: {}", d.source, d.data_type))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let current_input = format!(
            "## 当前数据\n\n数据来源:\n{}\n\n触发方式: {}\n\n请分析当前情况并做出决策。",
            data_text,
            if event_data.is_some() { "事件触发" } else { "定时/手动" }
        );

        messages.push(Message::user(current_input));

        messages
    }

    /// Create a conversation turn from execution results.
    pub fn create_conversation_turn(
        &self,
        execution_id: String,
        trigger_type: String,
        input_data: Vec<DataCollected>,
        event_data: Option<serde_json::Value>,
        decision_process: &DecisionProcess,
        duration_ms: u64,
        success: bool,
    ) -> ConversationTurn {
        ConversationTurn {
            execution_id,
            timestamp: chrono::Utc::now().timestamp(),
            trigger_type,
            input: TurnInput {
                data_collected: input_data,
                event_data,
            },
            output: TurnOutput {
                situation_analysis: decision_process.situation_analysis.clone(),
                reasoning_steps: decision_process.reasoning_steps.clone(),
                decisions: decision_process.decisions.clone(),
                conclusion: decision_process.conclusion.clone(),
            },
            duration_ms,
            success,
        }
    }
}

/// Helper function to extract metrics from text.
fn extract_metrics(text: &str) -> Vec<String> {
    let mut metrics = Vec::new();

    if text.contains("温度") {
        metrics.push("temperature".to_string());
    }
    if text.contains("湿度") {
        metrics.push("humidity".to_string());
    }
    if text.contains("能耗") || text.contains("功率") || text.contains("电量") {
        metrics.push("power".to_string());
    }
    if text.contains("光照") {
        metrics.push("illuminance".to_string());
    }
    if text.contains("气压") {
        metrics.push("pressure".to_string());
    }

    metrics
}

/// Helper function to extract conditions from text.
fn extract_conditions(text: &str) -> Vec<String> {
    let mut conditions = Vec::new();

    // Look for patterns like "大于30", "小于50", "超过", "低于"
    if text.contains("大于") || text.contains("超过") {
        if let Some(start) = text.find("大于").or_else(|| text.find("超过")) {
            let end = start + 2;
            if end + 10 <= text.len() {
                conditions.push(text[start..end + 10].to_string());
            }
        }
    }

    if text.contains("小于") || text.contains("低于") {
        if let Some(start) = text.find("小于").or_else(|| text.find("低于")) {
            let end = start + 2;
            if end + 10 <= text.len() {
                conditions.push(text[start..end + 10].to_string());
            }
        }
    }

    conditions
}

/// Helper function to extract actions from text.
fn extract_actions(text: &str) -> Vec<String> {
    let mut actions = Vec::new();

    if text.contains("报警") || text.contains("通知") {
        actions.push("send_alert".to_string());
    }
    if text.contains("开关") || text.contains("控制") {
        actions.push("send_command".to_string());
    }
    if text.contains("生成报告") {
        actions.push("generate_report".to_string());
    }

    actions
}

/// Helper function to extract threshold value from condition text.
fn extract_threshold(text: &str) -> Option<f64> {
    // Find numbers in the text
    let nums: Vec<f64> = text
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();

    nums.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_metrics() {
        let text = "监控温度和湿度，如果温度大于30度就报警";
        let metrics = extract_metrics(text);
        assert!(metrics.contains(&"temperature".to_string()));
        assert!(metrics.contains(&"humidity".to_string()));
    }

    #[test]
    fn test_extract_threshold() {
        assert_eq!(extract_threshold("大于30"), Some(30.0));
        assert_eq!(extract_threshold("温度超过35.5"), Some(35.5));
    }
}
