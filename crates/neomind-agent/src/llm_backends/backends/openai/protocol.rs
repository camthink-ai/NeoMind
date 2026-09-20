//! `protocol` — split from the former openai.rs monolith.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) presence_penalty: Option<f32>,
    pub(crate) stream: bool,
    /// Tools for function calling (OpenAI-compatible format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<OpenAiTool>>,
    /// Request usage data in streaming response (OpenAI stream_options)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream_options: Option<StreamOptions>,
    /// DashScope (Qwen) hybrid-thinking toggle. qwen3.x-plus defaults to
    /// thinking ON; without this knob the model burns tokens on hidden CoT
    /// during non-chat LLM calls (memory extraction, intent parsing, Phase 2
    /// fallback — gotcha #7) and risks gateway idle timeouts on long
    /// reasoning under non-streaming mode. Only emitted for
    /// `CloudProvider::Qwen`; other OpenAI-compatible servers may reject
    /// unknown fields. Mirrors the Ollama path's `thinking_enabled` handling
    /// (ollama.rs:826-844).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) enable_thinking: Option<bool>,
    /// OpenAI/GPT-5-style reasoning effort (`none`/`minimal`/`low`/`medium`/
    /// `high`/`xhigh`). Emitted for OpenAI-compatible providers that accept it
    /// (OpenAI, Custom, GLM); others reject unknown fields so it's skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<String>,
    /// DeepSeek thinking-mode toggle: `{"type":"enabled"|"disabled"}`.
    /// DeepSeek defaults thinking ON at `high` effort, so an explicit
    /// "disabled" is required to turn it off. Only emitted for DeepSeek.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<Thinking>,
}

/// DeepSeek thinking-mode toggle (`thinking: {"type": "enabled"|"disabled"}`).
#[derive(Debug, Serialize)]
pub(crate) struct Thinking {
    #[serde(rename = "type")]
    pub(crate) thinking_type: String,
}

/// Stream options to request usage data in final chunk
#[derive(Debug, Serialize)]
pub(crate) struct StreamOptions {
    pub(crate) include_usage: bool,
}

/// Tool definition in OpenAI format
#[derive(Debug, Serialize)]
pub(crate) struct OpenAiTool {
    #[serde(rename = "type")]
    pub(crate) tool_type: String, // Always "function"
    pub(crate) function: OpenAiFunction,
}

/// Function definition for tool calling
#[derive(Debug, Serialize)]
pub(crate) struct OpenAiFunction {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: serde_json::Value,
}

impl From<neomind_core::llm::backend::ToolDefinition> for OpenAiTool {
    fn from(tool: neomind_core::llm::backend::ToolDefinition) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: OpenAiFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiMessage {
    pub(crate) role: String,
    pub(crate) content: ApiContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ApiContent {
    Text(String),
    Parts(Vec<ApiContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum ApiContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl {
        #[serde(rename = "image_url")]
        image_url: ImageUrlContent,
    },
    /// Anthropic-style image format: {"type": "image", "source": {"type": "base64", "media_type": "...", "data": "..."}}
    #[serde(rename = "image")]
    AnthropicImage {
        #[serde(rename = "source")]
        source: AnthropicImageSource,
    },
}

/// Image URL content for OpenAI format
#[derive(Debug, Serialize)]
pub(crate) struct ImageUrlContent {
    pub(crate) url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

/// Anthropic image source format
#[derive(Debug, Serialize)]
pub(crate) struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub(crate) typ: String, // "base64"
    #[serde(rename = "media_type")]
    pub(crate) media_type: String, // "image/png", "image/jpeg", etc.
    pub(crate) data: String, // base64 data without prefix
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionResponse {
    pub(crate) choices: Vec<Choice>,
    #[serde(default)]
    pub(crate) usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Choice {
    pub(crate) message: ApiMessageResponse,
    pub(crate) finish_reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiMessageResponse {
    /// Content can be null when model makes tool calls
    #[serde(default)]
    pub(crate) content: Option<String>,
    /// Tool calls made by the model (for function calling)
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<OpenAiToolCallResponse>>,
    /// Reasoning chain emitted by thinking/reasoning models (DeepSeek-R1,
    /// Qwen3.x-plus, GLM-4.6 thinking, Moonshot K2, etc.). This is the
    /// de-facto industry standard field originated by DeepSeek-R1 and adopted
    /// by vLLM/SGLang/LMDeploy/SiliconFlow. Silently dropping it loses the
    /// model's chain-of-thought — must be captured into `LlmOutput.thinking`
    /// to mirror the llamacpp path.
    #[serde(default)]
    pub(crate) reasoning_content: Option<String>,
}

/// Tool call in OpenAI response format
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub(crate) struct OpenAiToolCallResponse {
    /// Tool call ID
    pub(crate) id: Option<String>,
    /// Tool type (always "function")
    #[serde(rename = "type")]
    pub(crate) call_type: Option<String>,
    /// Function call details
    pub(crate) function: OpenAiFunctionCall,
}

/// Function call details in response
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OpenAiFunctionCall {
    /// Function name
    pub(crate) name: String,
    /// Function arguments as JSON string
    pub(crate) arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Usage {
    pub(crate) prompt_tokens: u32,
    pub(crate) completion_tokens: u32,
    pub(crate) total_tokens: u32,
}

/// Accumulated tool call from streaming chunks
#[derive(Debug, Clone)]
pub(crate) struct AccumulatedToolCall {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamChunkEvent {
    #[serde(default)]
    pub(crate) choices: Vec<StreamChoice>,
    /// Usage data - only present in the final chunk when stream_options.include_usage=true
    #[serde(default)]
    pub(crate) usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamChoice {
    pub(crate) delta: StreamDelta,
    #[serde(default)]
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamDelta {
    /// Content can be null when model makes tool calls
    #[serde(default)]
    pub(crate) content: Option<String>,
    /// Tool calls in streaming format (incremental updates)
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<StreamToolCall>>,
}

/// Tool call in streaming response (incremental)
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub(crate) struct StreamToolCall {
    /// Index of this tool call in the array
    pub(crate) index: u32,
    /// Tool call ID (only in first chunk)
    pub(crate) id: Option<String>,
    /// Tool type (only in first chunk)
    #[serde(rename = "type")]
    pub(crate) call_type: Option<String>,
    /// Function call details (incremental)
    pub(crate) function: Option<StreamFunctionCall>,
}

/// Function call in streaming response (incremental)
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StreamFunctionCall {
    /// Function name (only in first chunk)
    pub(crate) name: Option<String>,
    /// Function arguments (incremental, JSON string fragments)
    pub(crate) arguments: Option<String>,
}

// --- Anthropic-native API types ---

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicRequest {
    pub(crate) model: String,
    pub(crate) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system: Option<String>,
    pub(crate) messages: Vec<AnthropicApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_sequences: Option<Vec<String>>,
    pub(crate) stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<AnthropicTool>>,
    /// Extended thinking config. `{type: "disabled"}` when the caller
    /// explicitly disables thinking; `{type: "enabled", budget_tokens: N}`
    /// when enabling. Omitted → Anthropic model default (adaptive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<AnthropicThinking>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum AnthropicThinking {
    #[serde(rename = "enabled")]
    Enabled { budget_tokens: u32 },
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicApiMessage {
    pub(crate) role: String,
    pub(crate) content: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicTool {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicResponse {
    pub(crate) content: Vec<AnthropicContentBlock>,
    pub(crate) stop_reason: Option<String>,
    pub(crate) usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Extended-thinking blocks — emitted by the official API (and
    /// Anthropic-compatible providers like GLM) when thinking is enabled.
    /// Not part of the visible text; skipped during extraction — the fields
    /// exist for deserialization tolerance only, hence dead-code-allowed.
    #[serde(rename = "thinking")]
    #[allow(dead_code)]
    Thinking {
        #[serde(default)]
        thinking: Option<String>,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "redacted_thinking")]
    #[allow(dead_code)]
    RedactedThinking {
        #[serde(default)]
        data: Option<String>,
    },
    /// Future/unknown block types — tolerate instead of failing the whole
    /// response (a provider adding a block kind must not break chat).
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicUsage {
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicMessageStart },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: serde_json::Value,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: AnthropicDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDeltaBody,
        usage: Option<AnthropicUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicDelta {
    #[serde(rename = "type")]
    pub(crate) delta_type: String,
    pub(crate) text: Option<String>,
    pub(crate) partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessageStart {
    #[allow(dead_code)]
    pub(crate) id: Option<String>,
    #[allow(dead_code)]
    pub(crate) model: Option<String>,
    #[allow(dead_code)]
    pub(crate) usage: Option<AnthropicUsage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessageDeltaBody {
    pub(crate) stop_reason: Option<String>,
}
