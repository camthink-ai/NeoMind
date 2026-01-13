//! Core AI Agent that orchestrates LLM, memory, and tools.
//!
//! ## Architecture
//!
//! The `Agent` is a high-level AI agent that integrates LLM, tools, and memory.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    Agent                            │
//! │  ┌────────────────────────────────────────────────┐ │
//! │  │  LlmInterface (LLM wrapper)                    │ │
//! │  │  - LLM runtime management                     │ │
//! │  │  - chat() / chat_stream()                     │ │
//! │  └────────────────────────────────────────────────┘ │
//! │                                                       │
//! │  + ToolRegistry (function calling)                 │
//! │  + Memory (conversation history)                    │
//! │  + SessionState (metadata tracking)                 │
//! └─────────────────────────────────────────────────────┘
//! ```

pub mod types;
pub mod fallback;
pub mod tool_parser;
pub mod streaming;
pub mod formatter;

use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};

// Re-export error types
pub use crate::error::NeoTalkError;
use serde_json::Value;

use super::error::Result;
use super::llm::{LlmInterface, ChatConfig};
use edge_ai_core::{
    llm::backend::{LlmRuntime, StreamChunk},
    Message,
};
use edge_ai_llm::{CloudConfig, CloudRuntime, OllamaConfig, OllamaRuntime};
use edge_ai_tools::registry::format_for_llm;

pub use types::{
    AgentConfig, AgentEvent, AgentMessage, AgentResponse, LlmBackend,
    SessionState, ToolCall,
};
pub use fallback::{default_fallback_rules, process_fallback, FallbackRule};
pub use streaming::{events_to_string_stream, process_stream_events};
pub use formatter::{format_tool_result, format_summary};

/// Maximum context window size in tokens (approximate)
const MAX_CONTEXT_TOKENS: usize = 8000;

/// Estimate token count for a string (rough approximation)
fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

/// Build conversation context with token-based windowing.
fn build_context_window(messages: &[AgentMessage]) -> Vec<&AgentMessage> {
    let mut selected_messages = Vec::new();
    let mut current_tokens = 0;

    for msg in messages.iter().rev() {
        let msg_tokens = estimate_tokens(&msg.content);
        let thinking_tokens = msg.thinking.as_ref().map_or(0, |t| estimate_tokens(t));

        if current_tokens + msg_tokens + thinking_tokens > MAX_CONTEXT_TOKENS {
            if msg.role == "system" || msg.role == "user" {
                let max_len = (MAX_CONTEXT_TOKENS - current_tokens) * 4;
                if max_len > 100 {
                    selected_messages.push(msg);
                    current_tokens += msg_tokens + thinking_tokens;
                }
            }
            break;
        }

        selected_messages.push(msg);
        current_tokens += msg_tokens + thinking_tokens;
    }

    selected_messages.reverse();
    selected_messages
}

/// AI Agent that orchestrates components.
pub struct Agent {
    /// Configuration
    config: AgentConfig,
    /// Session ID
    session_id: String,
    /// Tool registry
    tools: Arc<edge_ai_tools::ToolRegistry>,
    /// LLM interface
    llm_interface: Arc<LlmInterface>,
    /// Short-term memory (in-memory conversation)
    short_term_memory: Arc<tokio::sync::RwLock<Vec<AgentMessage>>>,
    /// Session state
    state: Arc<tokio::sync::RwLock<SessionState>>,
    /// Whether LLM is configured
    llm_configured: Arc<std::sync::atomic::AtomicBool>,
    /// Fallback rules for when LLM is unavailable
    fallback_rules: Vec<FallbackRule>,
}

impl Agent {
    /// Create a new agent with custom tool registry.
    pub fn with_tools(config: AgentConfig, session_id: String, tools: Arc<edge_ai_tools::ToolRegistry>) -> Self {
        let session_id_clone = session_id.clone();

        // Create LLM interface
        let llm_config = ChatConfig {
            model: config.model.clone(),
            temperature: config.temperature,
            top_p: 0.95,
            max_tokens: usize::MAX,  // No artificial limit - let model decide
            concurrent_limit: 3, // Default to 3 concurrent LLM requests
        };

        let llm_interface = Arc::new(LlmInterface::new(llm_config)
            .with_system_prompt(&config.system_prompt));

        Self {
            config,
            session_id,
            tools,
            llm_interface,
            short_term_memory: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            state: Arc::new(tokio::sync::RwLock::new(SessionState::new(session_id_clone))),
            llm_configured: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fallback_rules: default_fallback_rules(),
        }
    }

    /// Create a new agent with default (mock) tools.
    pub fn new(config: AgentConfig, session_id: String) -> Self {
        let tools = Arc::new(
            edge_ai_tools::ToolRegistryBuilder::new()
                .with_standard_tools()
                .build(),
        );
        Self::with_tools(config, session_id, tools)
    }

    /// Create with default config and mock tools.
    pub fn with_session(session_id: String) -> Self {
        Self::new(AgentConfig::default(), session_id)
    }

    /// Set custom fallback rules.
    pub fn with_fallback_rules(mut self, rules: Vec<FallbackRule>) -> Self {
        self.fallback_rules = rules;
        self
    }

    /// Configure the LLM backend.
    pub async fn configure_llm(&self, backend: LlmBackend) -> Result<()> {
        eprintln!("Agent::configure_llm called with backend: {:?}", backend);

        let (llm, model_name) = match backend {
            LlmBackend::Ollama { endpoint, model } => {
                eprintln!("Creating OllamaRuntime: endpoint={}, model={}", endpoint, model);
                let config = OllamaConfig::new(&model).with_endpoint(&endpoint);
                let runtime = OllamaRuntime::new(config)
                    .map_err(|e| NeoTalkError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
            LlmBackend::OpenAi { api_key, endpoint, model } => {
                eprintln!("Creating CloudRuntime for OpenAI: endpoint={}, model={}", endpoint, model);
                let config = CloudConfig::openai(&api_key);
                let config = if endpoint.is_empty() {
                    config.with_model(&model)
                } else {
                    // Custom endpoint
                    CloudConfig::custom(&api_key, &endpoint).with_model(&model)
                };
                let runtime = CloudRuntime::new(config)
                    .map_err(|e| NeoTalkError::llm(e.to_string()))?;
                (Arc::new(runtime) as Arc<dyn LlmRuntime>, model)
            }
        };

        // Update model override
        self.llm_interface.update_model(model_name).await;

        self.llm_interface.set_llm(llm).await;
        self.llm_configured.store(true, std::sync::atomic::Ordering::Release);

        // Set tool definitions for function calling
        self.update_tool_definitions().await;

        Ok(())
    }

    /// Update tool definitions in the LLM interface.
    pub async fn update_tool_definitions(&self) {
        use edge_ai_core::llm::backend::ToolDefinition as CoreToolDefinition;
        use edge_ai_tools::ToolDefinition as ToolsToolDefinition;

        // Get tool definitions from registry
        let tool_defs: Vec<ToolsToolDefinition> = self.tools.definitions();

        // Convert to core ToolDefinition format
        let core_defs: Vec<CoreToolDefinition> = tool_defs
            .into_iter()
            .map(|def| CoreToolDefinition {
                name: def.name,
                description: def.description,
                parameters: def.parameters,
            })
            .collect();

        let tool_count = core_defs.len();
        self.llm_interface.set_tool_definitions(core_defs).await;
        tracing::debug!("Updated {} tool definitions for LLM", tool_count);
    }

    /// Check if LLM is configured.
    pub async fn is_llm_configured(&self) -> bool {
        self.llm_interface.is_ready().await
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the session state.
    pub async fn state(&self) -> SessionState {
        self.state.read().await.clone()
    }

    /// Get the conversation history.
    pub async fn history(&self) -> Vec<AgentMessage> {
        self.short_term_memory.read().await.clone()
    }

    /// Restore conversation history from persisted data.
    pub async fn restore_history(&self, messages: Vec<AgentMessage>) {
        let mut memory = self.short_term_memory.write().await;
        memory.clear();
        for msg in messages {
            memory.push(msg);
        }
    }

    /// Clear conversation history.
    pub async fn clear_history(&self) {
        self.short_term_memory.write().await.clear();
    }

    /// Get available tools.
    pub fn available_tools(&self) -> Vec<String> {
        self.tools.list()
    }

    /// Get tool definitions for LLM.
    pub fn tool_definitions(&self) -> Value {
        self.tools.definitions_json()
    }

    /// Process a user message with real LLM.
    pub async fn process(&self, user_message: &str) -> Result<AgentResponse> {
        let start = std::time::Instant::now();

        // Add user message to history
        let user_msg = AgentMessage::user(user_message);
        self.short_term_memory.write().await.push(user_msg.clone());

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple keyword-based responses
            let (message, tool_calls, tools_used) = process_fallback(
                &self.tools,
                &self.fallback_rules,
                user_message,
            ).await;
            let processing_time = start.elapsed().as_millis() as u64;

            self.short_term_memory.write().await.push(message.clone());
            self.state.write().await.increment_messages();

            return Ok(AgentResponse {
                message,
                tool_calls,
                memory_context_used: true,
                tools_used,
                processing_time_ms: processing_time,
            });
        }

        // Try to process with real LLM
        match self.process_with_llm(user_message).await {
            Ok(response) => {
                let processing_time = start.elapsed().as_millis() as u64;
                self.state.write().await.increment_messages();
                Ok(AgentResponse {
                    processing_time_ms: processing_time,
                    ..response
                })
            }
            Err(e) => {
                // On error, fall back to simple response
                eprintln!("LLM error: {}, using fallback", e);
                let (message, tool_calls, tools_used) = process_fallback(
                    &self.tools,
                    &self.fallback_rules,
                    user_message,
                ).await;
                let processing_time = start.elapsed().as_millis() as u64;

                self.short_term_memory.write().await.push(message.clone());
                self.state.write().await.increment_messages();

                Ok(AgentResponse {
                    message,
                    tool_calls,
                    memory_context_used: true,
                    tools_used,
                    processing_time_ms: processing_time,
                })
            }
        }
    }

    /// Process with real LLM.
    async fn process_with_llm(&self, user_message: &str) -> Result<AgentResponse> {
        use tool_parser::parse_tool_calls;

        // Get existing history (user message already added by caller in `process`)
        let history: Vec<AgentMessage> = self.short_term_memory.read().await.clone();

        // Exclude the last message (which is the user message we just added) from history
        // The LLM interface will add the user message separately
        let history_without_last: Vec<AgentMessage> = if history.len() > 1 {
            history.iter().take(history.len() - 1).cloned().collect()
        } else {
            Vec::new()
        };

        // Build history for LLM (convert AgentMessage to Message)
        let core_history: Vec<Message> = history_without_last.iter()
            .map(|msg| msg.to_core())
            .collect();

        // Call LLM with conversation history (user message will be added by LLM interface)
        let chat_response = self.llm_interface.chat_with_history(user_message, &core_history).await
            .map_err(|e| super::error::AgentError::Llm(e.to_string()))?;

        // Parse response for tool calls
        let (content, tool_calls) = parse_tool_calls(&chat_response.text)?;

        // If no tool calls, return the direct response
        if tool_calls.is_empty() {
            // Save assistant response without tool calls
            let assistant_msg = AgentMessage::assistant(&content);
            self.short_term_memory.write().await.push(assistant_msg.clone());
            return Ok(AgentResponse {
                message: assistant_msg,
                tool_calls: vec![],
                memory_context_used: true,
                tools_used: vec![],
                processing_time_ms: 0,
            });
        }

        // Tool calls detected - DON'T save the initial assistant message yet
        // We'll save a complete message (with tool_calls and final response) after tool execution

        // Execute tools and get results
        let mut tool_results = Vec::new();
        let mut tools_used = Vec::new();

        for tool_call in &tool_calls {
            match self.execute_tool(&tool_call.name, &tool_call.arguments).await {
                Ok(result) => {
                    tools_used.push(tool_call.name.clone());
                    tool_results.push((tool_call.name.clone(), result));
                }
                Err(e) => {
                    let error_msg = format!("Error: {}", e);
                    tool_results.push((tool_call.name.clone(), error_msg));
                }
            }
        }

        // Format tool results directly (without calling LLM again)
        // This prevents excessive thinking and model looping
        let final_text = crate::agent::streaming::format_tool_results(&tool_results);

        // Save a SINGLE complete message with tool_calls and final response
        let final_message = AgentMessage::assistant_with_tools(&final_text, tool_calls.clone());
        self.short_term_memory.write().await.push(final_message.clone());

        Ok(AgentResponse {
            message: final_message,
            tool_calls,
            memory_context_used: true,
            tools_used,
            processing_time_ms: 0,
        })
    }

    /// Execute a tool with retry logic.
    ///
    /// Retries up to 2 times for transient errors (network issues, timeouts).
    /// Returns a user-friendly error message if all retries fail.
    async fn execute_tool(&self, name: &str, arguments: &Value) -> Result<String> {
        const MAX_RETRIES: u32 = 2;
        let mut last_error = String::new();

        for attempt in 0..=MAX_RETRIES {
            match self.tools.execute(name, arguments.clone()).await {
                Ok(output) => {
                    // Check if tool execution itself failed
                    if !output.success {
                        let error_msg = output.error.unwrap_or_else(|| "Unknown error".to_string());
                        // Don't retry on logical errors (like invalid input)
                        return Ok(format!("错误: {}", error_msg));
                    }
                    return Ok(serde_json::to_string_pretty(&output.data)
                        .unwrap_or_else(|_| "Success".to_string()));
                }
                Err(e) => {
                    last_error = e.to_string();
                    // Check if error is transient (worth retrying)
                    let is_transient = last_error.contains("timeout")
                        || last_error.contains("network")
                        || last_error.contains("connection")
                        || last_error.contains("unavailable");

                    if is_transient && attempt < MAX_RETRIES {
                        // Exponential backoff: 100ms, 200ms
                        let delay_ms = 100 * (2_u64.pow(attempt));
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                }
            }
        }

        // All retries failed
        Err(super::error::AgentError::Tool(format!(
            "工具 {} 执行失败: {}",
            name, last_error
        )))
    }

    /// Process a tool call result.
    pub async fn process_tool_result(&self, tool_call_id: &str, result: &str) -> Result<AgentResponse> {
        // Add tool result to history
        let tool_msg = AgentMessage::tool_result(tool_call_id, result);
        self.short_term_memory.write().await.push(tool_msg);

        // Get LLM response based on tool result
        let response_content = format!("工具执行完成。结果: {}", result);

        let response = AgentMessage::assistant(response_content);
        self.short_term_memory.write().await.push(response.clone());

        Ok(AgentResponse {
            message: response,
            tool_calls: Vec::new(),
            memory_context_used: true,
            tools_used: Vec::new(),
            processing_time_ms: 0,
        })
    }

    /// Process a user message with streaming response (returns AgentEvent stream).
    pub async fn process_stream_events(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> {
        // Add user message to history
        let user_msg = AgentMessage::user(user_message);
        self.short_term_memory.write().await.push(user_msg);

        // Check if LLM is configured
        if !self.llm_interface.is_ready().await {
            // Fall back to simple response
            let (message, _, _) = process_fallback(
                &self.tools,
                &self.fallback_rules,
                user_message,
            ).await;
            self.short_term_memory.write().await.push(message.clone());
            self.state.write().await.increment_messages();

            // Return a single-item stream with the fallback response
            let content = message.content;
            return Ok(Box::pin(async_stream::stream! {
                yield AgentEvent::content(content);
                yield AgentEvent::end();
            }));
        }

        match process_stream_events(
            self.llm_interface.clone(),
            self.short_term_memory.clone(),
            self.state.clone(),
            self.tools.clone(),
            user_message,
        ) {
            Ok(stream) => Ok(stream),
            Err(e) => {
                // On error, fall back to simple response
                eprintln!("LLM stream error: {}, using fallback", e);
                let (message, _, _) = process_fallback(
                    &self.tools,
                    &self.fallback_rules,
                    user_message,
                ).await;
                self.short_term_memory.write().await.push(message.clone());
                self.state.write().await.increment_messages();

                Ok(Box::pin(async_stream::stream! {
                    yield AgentEvent::content(message.content);
                    yield AgentEvent::end();
                }))
            }
        }
    }

    /// Process a user message with streaming response (legacy, returns String stream).
    pub async fn process_stream(
        &self,
        user_message: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>> {
        let event_stream = self.process_stream_events(user_message).await?;
        Ok(events_to_string_stream(event_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_creation() {
        let agent = Agent::with_session("test_session".to_string());
        assert_eq!(agent.session_id(), "test_session");

        let state = agent.state().await;
        assert_eq!(state.id, "test_session");
    }

    #[tokio::test]
    async fn test_agent_history() {
        let agent = Agent::with_session("test_session".to_string());

        // Initially empty
        assert!(agent.history().await.is_empty());

        // Clear should work
        agent.clear_history().await;
        assert!(agent.history().await.is_empty());
    }

    #[tokio::test]
    async fn test_available_tools() {
        let agent = Agent::with_session("test_session".to_string());
        let tools = agent.available_tools();

        assert!(!tools.is_empty());
        assert!(tools.contains(&"list_devices".to_string()));
        assert!(tools.contains(&"list_rules".to_string()));
    }

    #[tokio::test]
    async fn test_process_fallback() {
        let agent = Agent::with_session("test_session".to_string());
        let response = agent.process("列出所有设备").await.unwrap();

        assert!(response.message.content.contains("设备"));
        assert!(response.tools_used.contains(&"list_devices".to_string()));
    }

    #[tokio::test]
    async fn test_process_list_rules() {
        let agent = Agent::with_session("test_session".to_string());
        let response = agent.process("列出规则").await.unwrap();

        assert!(response.message.content.contains("规则"));
        assert!(response.tools_used.contains(&"list_rules".to_string()));
    }

    #[tokio::test]
    async fn test_process_query_data() {
        let agent = Agent::with_session("test_session".to_string());
        let response = agent.process("查询温度数据").await.unwrap();

        assert!(response.message.content.contains("数据"));
        assert!(response.tools_used.contains(&"query_data".to_string()));
    }

    #[tokio::test]
    async fn test_process_default() {
        let agent = Agent::with_session("test_session".to_string());
        let response = agent.process("你好").await.unwrap();

        // Should get a helpful response
        assert!(!response.message.content.is_empty());
    }

    #[tokio::test]
    async fn test_history_persistence() {
        let agent = Agent::with_session("test_session".to_string());

        // Send a message
        agent.process("列出设备").await.unwrap();

        // Check history
        let history = agent.history().await;
        assert!(history.len() >= 2); // user + assistant
    }

    #[tokio::test]
    async fn test_custom_fallback_rules() {
        // Test custom rules that don't require tool execution
        let custom_rules = vec![
            FallbackRule::new(vec!["hello", "hi", "greeting"], "greet")
                .with_response_template("Hello there!"),
        ];
        let agent = Agent::with_session("test_session".to_string())
            .with_fallback_rules(custom_rules);

        let response = agent.process("hello there").await.unwrap();
        // Since greet tool doesn't exist, we expect error handling
        // The key is that custom rules are used
        assert!(response.tools_used.contains(&"greet".to_string()));
    }
}
