// ========== Chat Types ==========

export interface Message {
  id: string
  role: 'user' | 'assistant' | 'system' | 'tool'
  content: string
  timestamp: number
  /** Reply wall time (first streamed event → end), ms — assistant messages. */
  generationMs?: number
  thinking?: string // Legacy: combined thinking across all rounds
  tool_calls?: ToolCall[]
  images?: ChatImage[]  // Images sent with user messages (multimodal)
  // Indicates if this message is still being streamed (partial)
  isPartial?: boolean
  // Per-round intermediate text for multi-round tool calling
  round_contents?: Record<number, string>
  // Per-round thinking content for grouped rendering
  round_thinking?: Record<number, string>
}

/**
 * All possible message roles including internal tool messages
 */
export type MessageRole = Message['role']

/**
 * Type guard to check if a message is a tool message (internal LLM context).
 * Tool messages should be filtered out from display.
 */
export function isToolMessage(msg: Message): msg is Message & { role: 'tool' } {
  return msg.role === 'tool'
}

export interface ToolCall {
  id: string
  name: string
  arguments: Record<string, unknown>
  result?: unknown
  round?: number // 1-based round number for multi-round tool calling
}

// Build Meta types for AI Build Mode rich cards
export type BuildEntityType = 'device' | 'dashboard' | 'rule' | 'extension' | 'widget' | 'transform' | 'agent' | 'message'
export type BuildAction = 'create' | 'update' | 'delete' | 'install'

export interface BuildMeta {
  type: BuildEntityType
  action: BuildAction
  entity_id: string
  entity_name?: string
  undo_command: string
}

/** CLI JSON response structure with optional build_meta */
export interface CliBuildResponse {
  success: boolean
  data?: Record<string, unknown>
  message?: string
  error?: string
  code?: string
  build_meta?: BuildMeta
}

// Session Types
export interface ChatSession {
  sessionId: string
  id: string
  createdAt: number
  updatedAt?: number
  messageCount?: number
  // User-defined title for the session
  title?: string | null
  // Preview text from last message
  preview?: string
  // Whether memory injection is enabled
  memoryEnabled?: boolean
}

export interface SessionListResponse {
  sessions: ChatSession[]
  meta: {
    page: number
    pageSize: number
    totalCount: number
    totalPages: number
  }
}

export interface SessionHistoryResponse {
  messages: Message[]
  count: number
}

/** Planning mode - how the plan was generated */
export type PlanningMode = 'keyword' | 'llm'

/** A single step in an execution plan */
export interface PlanStep {
  id: number
  tool_name: string
  action: string
  params: Record<string, unknown>
  depends_on: number[]
  description: string
}

/** An execution plan produced by the planner */
export interface ExecutionPlan {
  steps: PlanStep[]
  mode: PlanningMode
}

// Server WebSocket message types (matching backend)
//
// NOTE: These types must match the AgentEvent serialization in crates/agent/src/agent/types.rs
// and the WebSocket message format in crates/api/src/handlers/sessions.rs
export type ServerMessage =
  | { type: 'system'; content?: string; sessionId?: string }
  | { type: 'session_created'; sessionId: string }
  | { type: 'session_switched'; sessionId: string }
  // Thinking content - AI reasoning process
  | { type: 'Thinking'; content: string; sessionId: string }
  // Actual response content
  | { type: 'Content'; content: string; sessionId: string }
  // Tool call starting
  | { type: 'ToolCallStart'; tool: string; arguments: Record<string, unknown>; sessionId: string; round?: number }
  // Tool call completed - result is a string (JSON or plain text)
  | { type: 'ToolCallEnd'; tool: string; result: string; sessionId: string; success?: boolean; round?: number }
  // Intermediate end for multi-round tool calling
  | { type: 'IntermediateEnd'; sessionId: string }
  // Progress event during long-running operations
  // Note: stage is optional and can be any string (collecting, analyzing, executing, etc.)
  | { type: 'Progress'; elapsed: number; stage?: string; message?: string; remainingTime?: number; sessionId: string }
  // Warning event when approaching timeout
  | { type: 'Warning'; message: string; elapsed?: number; remainingTime?: number; sessionId: string }
  // Intent classification result (informational, not displayed in current UI)
  | { type: 'Intent'; category: string; displayName: string; confidence?: number; keywords?: string[]; sessionId: string }
  // Execution plan step (informational, not displayed in current UI)
  | { type: 'Plan'; step: string; stage: string; sessionId: string }
  // Execution plan created - full plan with all steps
  | { type: 'ExecutionPlanCreated'; plan: ExecutionPlan; sessionId: string }
  // A plan step has started executing
  | { type: 'PlanStepStarted'; stepId: number; description: string; sessionId: string }
  // A plan step has completed
  | { type: 'PlanStepCompleted'; stepId: number; success: boolean; summary: string; sessionId: string }
  // Heartbeat to keep connection alive (not displayed)
  | { type: 'Heartbeat'; timestamp: number; sessionId: string }
  // Ping from server - client auto-responds with pong
  | { type: 'ping'; timestamp: number }
  // Error occurred - sessionId is always included when sent from backend
  | { type: 'Error'; message: string; sessionId: string }
  // Stream ended
  | { type: 'end'; sessionId: string; tokenUsage?: { promptTokens: number } }
  // Intermediate end for multi-round tool calling (indicates more content coming)
  | { type: 'intermediate_end'; sessionId: string }
  // Non-streaming response (fallback)
  | { type: 'response'; content: string; sessionId: string; toolsUsed?: string[]; processingTimeMs?: number }
  // Device status update
  | { type: 'device_update'; updateType: string; deviceId: string; status?: string; lastSeen?: number }

// Stream configuration types (matching backend StreamConfig)
export interface StreamConfig {
  maxThinkingChars: number
  maxThinkingTimeSecs: number
  maxStreamDurationSecs: number
  warningThresholds: number[]
  maxThinkingLoop: number
  progressEnabled: boolean
}

// Stream progress state for UI
export interface StreamProgress {
  elapsed: number
  stage?: string  // Optional stage from backend (collecting, analyzing, executing, etc.)
  warnings: string[]
  remainingTime: number
}

// Image data for multimodal messages
export interface ChatImage {
  data: string  // Base64 data URL (e.g., "data:image/png;base64,...")
  mimeType?: string  // e.g., "image/png", "image/jpeg"
}

// Client WebSocket message types
export interface ClientChatMessage {
  message: string
  images?: ChatImage[]  // Optional images for multimodal models
  sessionId?: string
  backendId?: string  // Optional LLM backend ID to use for this message
  selectedSkills?: string[]  // Skill IDs pinned by user for this session
  pageContext?: string  // Short neutral page context, sent only on first message
}

// ========== Chat Input Types ==========

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  timestamp: number
  thinking?: string
  isStreaming?: boolean
}

// ========== Voice Recognition Types ==========

export interface VoiceRecognitionState {
  isListening: boolean
  transcript: string
  isSupported: boolean
  error?: string
}
