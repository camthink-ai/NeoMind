// ============================================================================
// Extension Streaming Types
// ============================================================================
//
// Generic streaming support for extensions
// Supports: Image analysis, Video processing, Audio streaming, Sensor data, etc.

/**
 * Stream direction - matches backend StreamDirection enum
 */
export type StreamDirection = 'upload' | 'download' | 'bidirectional'

/**
 * Stream mode - matches backend StreamMode enum
 */
export type StreamMode = 'stateless' | 'stateful' | 'push'

/**
 * Stream data type - matches backend StreamDataType enum
 */
export type StreamDataTypeBase = 'binary' | 'text' | 'json'

export interface StreamDataTypeImage {
  type: 'image'
  format: string  // e.g., 'jpeg', 'png', 'webp'
}

export interface StreamDataTypeAudio {
  type: 'audio'
  format: string  // e.g., 'mp3', 'wav', 'aac'
  sample_rate: number
  channels: number
}

export interface StreamDataTypeVideo {
  type: 'video'
  codec: string  // e.g., 'h264', 'h265', 'vp9'
  width: number
  height: number
  fps: number
}

export interface StreamDataTypeSensor {
  type: 'sensor'
  sensor_type: string
}

export interface StreamDataTypeCustom {
  type: 'custom'
  mime_type: string
}

export type StreamDataType = StreamDataTypeBase | StreamDataTypeImage | StreamDataTypeAudio | StreamDataTypeVideo | StreamDataTypeSensor | StreamDataTypeCustom

/**
 * Flow control settings - matches backend FlowControl struct
 */
export interface FlowControl {
  supports_backpressure: boolean
  window_size: number
  supports_throttling: boolean
  max_rate: number
}

/**
 * Stream capability - matches backend StreamCapability struct
 */
export interface StreamCapability {
  direction: StreamDirection
  mode: StreamMode
  supported_data_types: StreamDataType[]
  max_chunk_size: number
  preferred_chunk_size: number
  max_concurrent_sessions: number
  flow_control: FlowControl
  config_schema?: Record<string, unknown>
}

/**
 * Client message types for extension streaming WebSocket
 */
export interface ExtensionClientMessageHello {
  type: 'hello'
}

export interface ExtensionClientMessageInit {
  type: 'init'
  config?: Record<string, unknown>
}

export interface ExtensionClientMessageAck {
  type: 'ack'
  sequence: number
}

export interface ExtensionClientMessageClose {
  type: 'close'
}

export type ExtensionClientMessage = ExtensionClientMessageHello | ExtensionClientMessageInit | ExtensionClientMessageAck | ExtensionClientMessageClose

/**
 * Server message types for extension streaming WebSocket
 */
export interface ExtensionServerMessageCapability {
  type: 'capability'
  capability: StreamCapability
}

export interface ExtensionServerMessageSessionCreated {
  type: 'session_created'
  session_id: string
  server_time: number
}

export interface ExtensionServerMessageResult {
  type: 'result'
  input_sequence: number | null
  output_sequence: number
  data: string  // base64 encoded
  data_type: string
  processing_ms: number
  metadata?: Record<string, unknown>
}

export interface ExtensionServerMessageError {
  type: 'error'
  code: string
  message: string
  retryable: boolean
}

export interface ExtensionServerMessageSessionClosed {
  type: 'session_closed'
  session_id: string
  total_frames: number
  duration_ms: number
  stats: ExtensionSessionStats
}

export interface ExtensionServerMessagePushOutput {
  type: 'push_output'
  session_id: string
  sequence: number
  data: string // base64 encoded
  data_type: string
  timestamp: number
  metadata?: Record<string, unknown> | null
}

export interface ExtensionServerMessageHeartbeat {
  type: 'heartbeat'
  timestamp: number
}

export type ExtensionServerMessage = ExtensionServerMessageCapability | ExtensionServerMessageSessionCreated | ExtensionServerMessageResult | ExtensionServerMessageError | ExtensionServerMessageSessionClosed | ExtensionServerMessagePushOutput | ExtensionServerMessageHeartbeat

/**
 * Session statistics - matches backend SessionStats struct
 */
export interface ExtensionSessionStats {
  input_chunks: number
  output_chunks: number
  input_bytes: number
  output_bytes: number
  errors: number
  last_activity: number
}

/**
 * Stream connection state for extensions
 */
export interface ExtensionStreamConnectionState {
  status: 'connecting' | 'connected' | 'disconnected' | 'error'
  sessionId: string | null
  errorMessage?: string
  capability: StreamCapability | null
}

/**
 * Stream result - processed data from extension
 */
export interface ExtensionStreamResult {
  inputSequence: number | null
  outputSequence: number
  data: Uint8Array
  dataType: string
  processingMs: number
  metadata?: Record<string, unknown>
}

/**
 * Options for extension stream hooks (formerly consumed by useExtensionStream).
 */
export interface UseExtensionStreamOptions {
  /**
   * Extension ID to connect to
   */
  extensionId: string

  /**
   * Initial configuration for stateful sessions
   */
  config?: Record<string, unknown>

  /**
   * Enable auto-connect
   */
  enabled?: boolean

  /**
   * Result callback - receives processed data
   */
  onResult?: (result: ExtensionStreamResult) => void

  /**
   * Error callback
   */
  onError?: (error: Error) => void

  /**
   * Connection state change callback
   */
  onConnectionChange?: (state: ExtensionStreamConnectionState) => void

  /**
   * Session closed callback
   */
  onSessionClosed?: (stats: ExtensionSessionStats) => void
}
