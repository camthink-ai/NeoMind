export interface AiAnalystConfig {
  agentId?: string
  sessionId?: string
  modelId?: string
  modelName?: string
  systemPrompt: string
  contextWindowSize: number
}

export type AnalystMessageType = 'image' | 'data' | 'ai' | 'user' | 'error' | 'warning'

export interface AnalystMessage {
  id: string
  type: AnalystMessageType
  content: string
  timestamp: number
  dataSource?: string
  /** Inline images embedded in this message (for data messages that carry both text + images) */
  images?: string[]
  modelName?: string
  duration?: number
  isStreaming?: boolean
}

export interface AnalystModel {
  id: string
  name: string
  backendId: string
  backendName: string
  isMultimodal: boolean
}

export const DEFAULT_SYSTEM_PROMPT =
  'You are a professional data analysis assistant. Analyze the provided data — images, metrics, or structured data — describe what you observe, and point out any notable patterns, changes, or anomalies.'
