// ========== LLM Backend Management Types ==========

export type LlmBackendType = 'ollama' | 'openai' | 'anthropic' | 'google' | 'xai' | 'llamacpp'

export interface BackendCapabilities {
  supports_streaming: boolean
  supports_multimodal: boolean
  /** User-set override for multimodal; when set, auto-detection is skipped. */
  multimodal_user_override?: boolean | null
  /** Provenance of `supports_multimodal`: 'user_override' | 'runtime_api' | 'registry' | 'heuristic' | 'default'. */
  multimodal_source?: string | null
  supports_thinking: boolean
  supports_tools: boolean
  max_context: number
  /** Declared reasoning/thinking capabilities (drives the effort UI). */
  reasoning?: ReasoningCapabilities
}

export interface ReasoningCapabilities {
  /** Effort levels this backend can honor. Empty = unknown. */
  supported_efforts?: ThinkingEffort[]
  /** The model's default effort when nothing is set. */
  default_effort?: ThinkingEffort
  /** Whether thinking is mandatory (cannot be turned off). */
  mandatory?: boolean
  /** How the backend controls thinking. */
  control?: 'readonly' | 'boolean' | 'level' | 'effort'
}

export interface LlmBackendInstance {
  id: string
  name: string
  backend_type: LlmBackendType
  endpoint?: string
  model: string
  api_key_configured: boolean
  is_active: boolean
  temperature: number
  top_p: number
  top_k: number
  max_tokens: number
  thinking_enabled: boolean  // Enable thinking/reasoning mode for models that support it
  thinking_effort?: ThinkingEffort  // Unified reasoning effort (preferred over thinking_enabled)
  capabilities: BackendCapabilities
  updated_at: number
  healthy?: boolean  // Health check result (from API)
}

export type ThinkingEffort =
  | 'none'
  | 'low'
  | 'medium'
  | 'high'
  | 'xhigh'
  | 'max'

export interface CreateLlmBackendRequest {
  name: string
  backend_type: LlmBackendType
  endpoint?: string
  model: string
  api_key?: string
  temperature?: number
  top_p?: number
  top_k?: number
  thinking_enabled?: boolean  // Enable thinking/reasoning mode for models that support it
  thinking_effort?: ThinkingEffort  // Unified reasoning effort (preferred over thinking_enabled)
  capabilities?: BackendCapabilities  // Model capabilities (from Ollama model detection)
}

export interface UpdateLlmBackendRequest {
  name?: string
  endpoint?: string
  model?: string
  api_key?: string
  temperature?: number
  top_p?: number
  top_k?: number
  thinking_enabled?: boolean  // Enable thinking/reasoning mode for models that support it
  thinking_effort?: ThinkingEffort  // Unified reasoning effort (preferred over thinking_enabled)
  capabilities?: BackendCapabilities  // Model capabilities (from Ollama model detection)
}

export interface LlmBackendListResponse {
  backends: LlmBackendInstance[]
  count: number
  active_id: string | null
}

export interface BackendTypeDefinition {
  id: string
  name: string
  description: string
  default_model: string
  default_endpoint?: string
  requires_api_key: boolean
  supports_streaming: boolean
  supports_thinking: boolean
  supports_multimodal: boolean
  config_schema?: Record<string, unknown>  // JSON Schema for configuration
}

export interface BackendTestResult {
  success: boolean
  latency_ms?: number
  error?: string
}

export interface LlmBackendStats {
  total_backends: number
  active_backends: number
  by_type: Record<string, number>
  total_requests: number
  successful_requests: number
  failed_requests: number
  average_latency_ms: number
}

// ========== Device Adapter Types ==========
// Similar to LLM backend types, device adapters are now dynamically loaded

export interface AdapterType {
  id: string  // e.g., "mqtt", "webhook"
  name: string  // e.g., "MQTT", "HTTP (Polling)", "Webhook"
  description: string
  icon: string  // Icon name for lucide-react
  icon_bg: string  // Tailwind CSS classes for icon background
  mode: 'push' | 'pull' | 'hybrid'  // Connection mode
  can_add_multiple: boolean  // Whether multiple instances can be created
  builtin: boolean  // Whether this is a built-in adapter
}

/**
 * Request to validate LLM backend
 */
export interface ValidateLlmRequest {
  backend_id?: string
  model?: string
  test_prompt?: string
}

/**
 * Response from LLM validation
 */
export interface ValidateLlmResponse {
  valid: boolean
  backend_name?: string
  model?: string
  error?: string
  response_time_ms?: number
}
