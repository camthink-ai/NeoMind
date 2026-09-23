// ========== Message Types ==========
// Unified message/notification system.

import type { JsonSchema } from './api'

export type MessageSeverity = 'info' | 'warning' | 'critical' | 'emergency'
// Message Type - must match backend MessageType enum
export type MessageType = 'notification'
export type MessageStatus =
  | 'active'
  | 'acknowledged'
  | 'resolved'
  | 'archived'
  // A verdict rather than a lifecycle step: the operator dismissed the alert
  // as a false positive (002 §3.4). `messages.tsx` has carried a label for it
  // since before the backend had the status.
  | 'false_positive'
// Category is a flexible string - backend can provide any category value
export type MessageCategory = string

// Channel Filter - must match backend ChannelFilter struct
export interface ChannelFilter {
  source_types: string[]
  categories: string[]
  min_severity: MessageSeverity | null
}

// Known category values for reference (not exhaustive)
export const KNOWN_CATEGORIES = {
  alert: 'alert',
  system: 'system',
  business: 'business',
  notification: 'notification',
} as const

/**
 * Message/Notification type - must match backend Message (crates/messages/src/message.rs)
 */
export interface NotificationMessage {
  id: string
  category: MessageCategory
  severity: MessageSeverity
  title: string
  message: string
  source: string
  source_type: string
  timestamp: string  // ISO 8601 string from backend
  status: MessageStatus
  metadata?: Record<string, unknown>
  tags: string[]
}

/**
 * Message list response
 */
export interface MessageListResponse {
  messages: NotificationMessage[]
  count: number
}

/**
 * Message statistics
 */
export interface MessageStats {
  total: number
  active: number
  by_category: Record<string, number>
  by_severity: Record<string, number>
  by_status: Record<string, number>
}

/**
 * Create message request
 */
export interface CreateMessageRequest {
  category?: MessageCategory
  severity?: MessageSeverity
  title: string
  message: string
  source?: string
  source_type?: string
  metadata?: Record<string, unknown>
  tags?: string[]
}

// Helper to get display label for MessageType
export function getMessageTypeLabel(type: MessageType): string {
  switch (type) {
    case 'notification':
      return '通知'
    default:
      return type
  }
}

/**
 * Bulk message operation request
 */
export interface BulkMessageRequest {
  message_ids: string[]
}

/**
 * Cleanup old messages request
 */
export interface CleanupMessagesRequest {
  older_than_days: number
}

// Message Channel Types (formerly AlertChannel for backward compatibility)
export interface AlertChannel {
  name: string
  channel_type: 'console' | 'memory' | 'webhook' | 'email' | 'telegram' | 'wecom' | 'dingtalk' | 'slack' | 'feishu'
  enabled: boolean
  config?: Record<string, unknown>
}

export interface ChannelTypeInfo {
  id: string
  name: string
  name_zh: string
  description: string
  description_zh: string
  icon: string
  category: string
}

export interface ChannelStats {
  total: number
  enabled: number
  disabled: number
  by_type: Record<string, number>
}

export interface ChannelListResponse {
  channels: AlertChannel[]
  count: number
  stats: ChannelStats
}

export interface ChannelTestResult {
  success: boolean
  message: string
  message_zh: string
  duration_ms: number
}

export interface CreateChannelRequest {
  name: string
  channel_type: string
  [key: string]: unknown  // Additional config fields
}

export interface ChannelSchemaResponse {
  id: string
  name: string
  name_zh: string
  description: string
  description_zh: string
  icon: string
  category: string
  config_schema: JsonSchema
}

// ========== Message Channel Types ==========
// For the new unified messages system

export interface MessageChannel {
  name: string
  channel_type: 'console' | 'memory' | 'webhook' | 'email' | 'telegram' | 'wecom' | 'dingtalk' | 'slack' | 'feishu'
  enabled: boolean
  config?: Record<string, unknown>
  recipients?: string[]  // For email channels
}

export interface MessageChannelListResponse {
  channels: MessageChannel[]
  count: number
  stats: ChannelStats
}

export interface CreateMessageChannelRequest {
  name: string
  channel_type: string
  [key: string]: unknown  // Additional config fields
}
