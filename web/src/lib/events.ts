// Events WebSocket/SSE Manager for Real-time System Events
//
// Provides real-time event streaming from the NeoTalk event bus.

import { tokenManager } from '@/lib/api'

export type EventType =
  | 'DeviceOnline'
  | 'DeviceOffline'
  | 'DeviceMetric'
  | 'DeviceCommandResult'
  | 'RuleEvaluated'
  | 'RuleTriggered'
  | 'RuleExecuted'
  | 'AlertCreated'
  | 'AlertAcknowledged'
  | 'PeriodicReviewTriggered'
  | 'LlmDecisionProposed'
  | 'LlmDecisionExecuted'
  | 'UserMessage'
  | 'LlmResponse'
  | 'ToolExecutionStart'
  | 'ToolExecutionSuccess'
  | 'ToolExecutionFailure'
  | 'Custom'

export interface CustomEvent extends NeoTalkEvent {
  type: 'Custom'
  data: {
    event_type: string
    [key: string]: unknown
  }
}

export type EventCategory = 'device' | 'rule' | 'llm' | 'alert' | 'tool' | 'all'

export interface NeoTalkEvent {
  id: string
  type: EventType
  timestamp: number
  source: string
  data: unknown
}

export interface DeviceMetricEvent extends NeoTalkEvent {
  type: 'DeviceMetric'
  data: {
    device_id: string
    metric: string
    value: number | string | boolean
    quality?: number
  }
}

export interface RuleTriggeredEvent extends NeoTalkEvent {
  type: 'RuleTriggered'
  data: {
    rule_id: string
    rule_name: string
    trigger_value: number
    actions: string[]
  }
}

export interface LlmDecisionProposedEvent extends NeoTalkEvent {
  type: 'LlmDecisionProposed'
  data: {
    decision_id: string
    title: string
    description: string
    reasoning: string
    actions: Array<{
      action_type: string
      description: string
      parameters: unknown
    }>
    confidence: number
  }
}

export type EventHandler = (event: NeoTalkEvent) => void
export type ConnectionHandler = (connected: boolean) => void
export type ErrorHandler = (error: Error) => void

interface EventsConfig {
  category?: EventCategory
  eventTypes?: EventType[]
  useSSE?: boolean  // Use Server-Sent Events instead of WebSocket
  reconnectInterval?: number
  maxReconnectAttempts?: number
}

/**
 * EventsWebSocket - Manages real-time event streaming from NeoTalk event bus
 *
 * Supports both WebSocket and SSE (Server-Sent Events) connections.
 */
export class EventsWebSocket {
  private ws: WebSocket | null = null
  private eventSource: EventSource | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private reconnectAttempts = 0
  private maxReconnectAttempts = Infinity  // Use infinite retry with backoff
  private eventHandlers: Map<EventType, Set<EventHandler>> = new Map()
  private genericHandlers: Set<EventHandler> = new Set()
  private connectionHandlers: Set<ConnectionHandler> = new Set()
  private errorHandlers: Set<ErrorHandler> = new Set()
  private config: EventsConfig = {}
  private lastToken: string | null = null  // Track token for reconnection
  private tokenCheckTimer: ReturnType<typeof setInterval> | null = null  // Poll for token

  constructor(config?: EventsConfig) {
    this.config = config || {}
    if (this.config.maxReconnectAttempts !== undefined) {
      this.maxReconnectAttempts = this.config.maxReconnectAttempts
    }
  }

  /**
   * Connect to the events endpoint
   */
  connect() {
    // Clear any existing token check timer
    if (this.tokenCheckTimer) {
      clearInterval(this.tokenCheckTimer)
      this.tokenCheckTimer = null
    }

    // Get current token
    const currentToken = tokenManager.getToken()

    // If no token, start polling for token
    if (!currentToken) {
      this.disconnect()
      this.notifyConnection(false)

      // Poll for token every 500ms
      this.tokenCheckTimer = setInterval(() => {
        const token = tokenManager.getToken()
        if (token) {
          if (this.tokenCheckTimer) {
            clearInterval(this.tokenCheckTimer)
            this.tokenCheckTimer = null
          }
          this.connect()
        }
      }, 500)
      return
    }

    // If token changed, force reconnect
    if (this.lastToken !== currentToken) {
      // Reset reconnect attempts when token changes
      this.reconnectAttempts = 0
      if (this.isConnected()) {
        this.disconnect()
      }
    }

    this.lastToken = currentToken

    // Only disconnect if we're changing connection type or config significantly changed
    // Otherwise, reuse existing connection
    const currentlyConnected = this.isConnected()
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const category = this.config.category || 'all'
    const eventTypes = this.config.eventTypes || []

    if (currentlyConnected) {
      // Already connected, no need to reconnect
      return
    }

    if (this.config.useSSE) {
      // Use Server-Sent Events
      this.connectSSE(category, eventTypes)
    } else {
      // Use WebSocket
      this.connectWebSocket(protocol, category, eventTypes)
    }
  }

  private connectWebSocket(protocol: string, category: string, eventTypes: EventType[]) {
    const params = new URLSearchParams()
    if (category !== 'all') {
      params.set('category', category)
    }
    eventTypes.forEach(type => params.append('event_type', type))

    // Add JWT token for authentication
    const token = tokenManager.getToken()
    if (token) {
      params.set('token', token)
    }

    // Build WebSocket URL
    // In development, use direct connection to backend server (port 3000)
    // In production, use the same host as the frontend
    let wsHost = window.location.host
    if (window.location.port === '5173' || window.location.hostname === 'localhost') {
      // Development: connect directly to backend server
      wsHost = 'localhost:3000'
    }

    const wsUrl = `${protocol}//${wsHost}/api/events/ws?${params.toString()}`

    try {
      this.ws = new WebSocket(wsUrl)
    } catch (e) {
      this.notifyError(new Error(`WebSocket creation failed: ${e}`))
      this.scheduleReconnect()
      return
    }

    this.ws.onopen = () => {
      this.reconnectAttempts = 0
      this.notifyConnection(true)
    }

    this.ws.onclose = () => {
      this.notifyConnection(false)
      this.scheduleReconnect()
    }

    this.ws.onerror = () => {
      this.notifyError(new Error('WebSocket connection error'))
    }

    this.ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        // Handle error messages from server (e.g., auth failures)
        if (data.type === 'Error') {
          // If it's an auth error, clear the token and stop reconnecting
          if (data.message?.includes('token') || data.message?.includes('Authentication') || data.message?.includes('Unauthorized')) {
            tokenManager.clearToken()
            this.disconnect()
            return
          }
        }
        this.notifyEvent(data as NeoTalkEvent)
      } catch {
        // Silent error handling for malformed messages
      }
    }
  }

  private connectSSE(category: string, eventTypes: EventType[]) {
    const params = new URLSearchParams()
    if (category !== 'all') {
      params.set('category', category)
    }
    eventTypes.forEach(type => params.append('event_type', type))

    // Add JWT token for authentication
    const token = tokenManager.getToken()
    if (token) {
      params.set('token', token)
    }

    const sseUrl = `/api/events/stream?${params.toString()}`
    this.eventSource = new EventSource(sseUrl)

    this.eventSource.onopen = () => {
      this.reconnectAttempts = 0
      this.notifyConnection(true)
    }

    this.eventSource.onerror = () => {
      this.notifyConnection(false)
      this.notifyError(new Error('SSE connection error'))
    }

    // SSE messages come through the onmessage handler
    this.eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as NeoTalkEvent
        this.notifyEvent(data)
      } catch {
        // Silent error handling
      }
    }
  }

  /**
   * Disconnect from the events endpoint
   */
  disconnect() {
    // Clear token check timer
    if (this.tokenCheckTimer) {
      clearInterval(this.tokenCheckTimer)
      this.tokenCheckTimer = null
    }
    // Clear reconnect timer
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    // Close WebSocket
    if (this.ws) {
      this.ws.close()
      this.ws = null
    }
    // Close EventSource
    if (this.eventSource) {
      this.eventSource.close()
      this.eventSource = null
    }
  }

  /**
   * Subscribe to all events
   */
  onEvent(handler: EventHandler) {
    this.genericHandlers.add(handler)
    return () => this.genericHandlers.delete(handler)
  }

  /**
   * Subscribe to a specific event type
   */
  on<T extends EventType>(eventType: T, handler: (event: Extract<NeoTalkEvent, { type: T }>) => void) {
    if (!this.eventHandlers.has(eventType)) {
      this.eventHandlers.set(eventType, new Set())
    }
    this.eventHandlers.get(eventType)!.add(handler as EventHandler)
    return () => {
      this.eventHandlers.get(eventType)?.delete(handler as EventHandler)
    }
  }

  /**
   * Subscribe to device events
   */
  onDeviceEvent(handler: (event: DeviceMetricEvent) => void) {
    return this.on('DeviceMetric', handler)
  }

  /**
   * Subscribe to rule events
   */
  onRuleEvent(handler: (event: RuleTriggeredEvent) => void) {
    return this.on('RuleTriggered', handler)
  }

  /**
   * Subscribe to LLM decision events
   */
  onLlmDecisionEvent(handler: (event: LlmDecisionProposedEvent) => void) {
    return this.on('LlmDecisionProposed', handler)
  }

  /**
   * Subscribe to connection state changes
   */
  onConnection(handler: ConnectionHandler) {
    this.connectionHandlers.add(handler)
    handler(this.isConnected())
    return () => this.connectionHandlers.delete(handler)
  }

  /**
   * Subscribe to errors
   */
  onError(handler: ErrorHandler) {
    this.errorHandlers.add(handler)
    return () => this.errorHandlers.delete(handler)
  }

  private scheduleReconnect() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      return
    }

    // Exponential backoff with jitter: 2^n * 1000ms, capped at 30s
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000)
    this.reconnectAttempts++

    this.reconnectTimer = setTimeout(() => {
      this.connect()
    }, delay)
  }

  private notifyEvent(event: NeoTalkEvent) {
    // Notify type-specific handlers
    const typeHandlers = this.eventHandlers.get(event.type as EventType)
    if (typeHandlers) {
      typeHandlers.forEach(handler => {
        try {
          handler(event)
        } catch {
          // Silent error handling
        }
      })
    }

    // Notify generic handlers
    this.genericHandlers.forEach(handler => {
      try {
        handler(event)
      } catch {
        // Silent error handling
      }
    })
  }

  private notifyConnection(connected: boolean) {
    this.connectionHandlers.forEach(handler => {
      try {
        handler(connected)
      } catch {
        // Silent error handling
      }
    })
  }

  private notifyError(error: Error) {
    this.errorHandlers.forEach(handler => {
      try {
        handler(error)
      } catch {
        // Silent error handling
      }
    })
  }

  isConnected() {
    return this.ws?.readyState === WebSocket.OPEN ||
           this.eventSource?.readyState === EventSource.OPEN
  }

  /**
   * Update the configuration and reconnect
   */
  updateConfig(config: Partial<EventsConfig>) {
    this.config = { ...this.config, ...config }
    this.disconnect()
    this.connect()
  }
}

// Global event connections map
const eventConnections = new Map<string, EventsWebSocket>()

/**
 * Get or create an events WebSocket connection
 * Always attempts to connect (will reconnect if token changed)
 */
export function getEventsConnection(key = 'default', config?: EventsConfig): EventsWebSocket {
  let connection = eventConnections.get(key)
  if (!connection) {
    connection = new EventsWebSocket(config)
    eventConnections.set(key, connection)
  }
  // Always try to connect (will check token and reconnect if needed)
  connection.connect()
  return connection
}

/**
 * Refresh all event connections (call after login/logout)
 */
export function refreshEventConnections() {
  eventConnections.forEach((connection) => {
    connection.connect()
  })
}

/**
 * Close an events WebSocket connection
 */
export function closeEventsConnection(key = 'default') {
  const connection = eventConnections.get(key)
  if (connection) {
    connection.disconnect()
    eventConnections.delete(key)
  }
}

/**
 * Close all events WebSocket connections
 */
export function closeAllEventsConnections() {
  eventConnections.forEach((connection) => {
    connection.disconnect()
  })
  eventConnections.clear()
}
