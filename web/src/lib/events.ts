// Events WebSocket/SSE Manager for Real-time System Events
//
// Provides real-time event streaming from the NeoMind event bus.

import { tokenManager } from '@/lib/api'

export type EventType =
  | 'DeviceOnline'
  | 'DeviceOffline'
  | 'DeviceMetric'
  | 'DeviceCommandResult'
  | 'RuleEvaluated'
  | 'RuleTriggered'
  | 'RuleExecuted'
  | 'WorkflowTriggered'
  | 'WorkflowStepCompleted'
  | 'WorkflowCompleted'
  | 'AlertCreated'
  | 'AlertAcknowledged'
  | 'AgentExecutionStarted'
  | 'AgentThinking'
  | 'AgentDecision'
  | 'AgentProgress'
  | 'AgentExecutionCompleted'
  | 'AgentMemoryUpdated'
  | 'PeriodicReviewTriggered'
  | 'UserMessage'
  | 'LlmResponse'
  | 'ToolExecutionStart'
  | 'ToolExecutionSuccess'
  | 'ToolExecutionFailure'
  | 'ExtensionOutput'
  | 'ExtensionLifecycle'
  | 'Custom'

export interface CustomEvent extends NeoMindEvent {
  type: 'Custom'
  data: {
    event_type: string
    [key: string]: unknown
  }
}

export type EventCategory = 'device' | 'rule' | 'llm' | 'alert' | 'tool' | 'agent' | 'extension' | 'all'

export interface NeoMindEvent {
  id: string
  type: EventType
  timestamp: number
  source: string
  data: unknown
}

export interface DeviceMetricEvent extends NeoMindEvent {
  type: 'DeviceMetric'
  data: {
    device_id: string
    metric: string
    value: number | string | boolean
    quality?: number
  }
}

// Extension-related events
export interface ExtensionOutputEvent extends NeoMindEvent {
  type: 'ExtensionOutput'
  data: {
    extension_id: string
    output_name: string
    value: number | string | boolean
    quality?: number
    labels?: Record<string, string>
  }
}

export interface ExtensionLifecycleEvent extends NeoMindEvent {
  type: 'ExtensionLifecycle'
  data: {
    extension_id: string
    state: 'registered' | 'unregistered' | 'loaded' | 'started' | 'stopped' | 'error'
    message?: string
  }
}

export interface RuleTriggeredEvent extends NeoMindEvent {
  type: 'RuleTriggered'
  data: {
    rule_id: string
    rule_name: string
    trigger_value: number
    actions: string[]
  }
}

// Agent-related events
export interface AgentExecutionStartedEvent extends NeoMindEvent {
  type: 'AgentExecutionStarted'
  data: {
    agent_id: string
    agent_name: string
    execution_id: string
    trigger_type: string
  }
}

export interface AgentExecutionCompletedEvent extends NeoMindEvent {
  type: 'AgentExecutionCompleted'
  data: {
    agent_id: string
    execution_id: string
    duration_ms: number
    success: boolean
    error?: string | null
  }
}

export interface AgentThinkingEvent extends NeoMindEvent {
  type: 'AgentThinking'
  data: {
    agent_id: string
    execution_id: string
    step_number: number
    step_type: string
    description: string
    details?: unknown
  }
}

export interface AgentDecisionEvent extends NeoMindEvent {
  type: 'AgentDecision'
  data: {
    agent_id: string
    execution_id: string
    description: string
    rationale: string
    action: string
    confidence: number
  }
}

export interface AgentProgressEvent extends NeoMindEvent {
  type: 'AgentProgress'
  data: {
    agent_id: string
    execution_id: string
    stage: string // 'collecting', 'analyzing', 'executing', 'completed'
    stage_label: string // Human-readable stage name
    progress?: number // 0.0 to 1.0 if available
    details?: string // Additional details about current operation
  }
}

export interface AgentMemoryUpdatedEvent extends NeoMindEvent {
  type: 'AgentMemoryUpdated'
  data: {
    agent_id: string
    memory_type: string
  }
}

export type EventHandler = (event: NeoMindEvent) => void
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
 * EventsWebSocket - Manages real-time event streaming from NeoMind event bus
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
  private isConnecting = false  // Track if connection attempt is in progress
  private authenticated = false  // Track if authentication succeeded
  private authFailed = false  // Track if auth failed to prevent reconnection loops

  // Performance optimization: event batching
  private pendingEvents: NeoMindEvent[] = []
  private rafId: number | null = null
  private onlineHandler: (() => void) | null = null

  constructor(config?: EventsConfig) {
    this.config = config || {}
    if (this.config.maxReconnectAttempts !== undefined) {
      this.maxReconnectAttempts = this.config.maxReconnectAttempts
    }

    // Performance optimization: listen for network state to trigger immediate reconnect
    if (typeof window !== 'undefined' && window.addEventListener) {
      this.onlineHandler = () => {
        if (!this.isConnected() && !this.isConnecting) {
          this.reconnectAttempts = 0  // Reset backoff on network recovery
          this.connect()
        }
      }
      window.addEventListener('online', this.onlineHandler)
    }
  }

  /**
   * Connect to the events endpoint
   */
  connect() {
    // Don't start a new connection if one is already in progress
    if (this.isConnecting) {
      return
    }

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
      // Reset reconnect attempts and auth failed flag when token changes
      this.reconnectAttempts = 0
      this.authFailed = false
      if (this.isConnected()) {
        this.disconnect()
      }
    }

    this.lastToken = currentToken

    // Only disconnect if we're changing connection type or config significantly changed
    // Otherwise, reuse existing connection
    const currentlyConnected = this.isConnected()
    const hasActiveSocket = this.ws?.readyState === WebSocket.OPEN ||
                            this.ws?.readyState === WebSocket.CONNECTING ||
                            this.eventSource?.readyState === EventSource.OPEN
    const category = this.config.category || 'all'
    const eventTypes = this.config.eventTypes || []

    if (currentlyConnected) {
      // Already connected and authenticated, no need to reconnect
      return
    }

    // Don't create duplicate connection if socket is already active
    if (hasActiveSocket && this.isConnecting) {
      return
    }

    if (this.config.useSSE) {
      // Use Server-Sent Events
      this.connectSSE(category, eventTypes)
    } else {
      // Use WebSocket
      this.connectWebSocket(category, eventTypes)
    }
  }

  /**
   * Ensure connection is active (no-op if already connected)
   * This is a safer alternative to connect() for repeated calls
   */
  ensureConnected() {
    if (!this.isConnected()) {
      this.connect()
    }
  }

  private connectWebSocket(category: string, eventTypes: EventType[]) {
    // Mark that we're starting a connection
    this.isConnecting = true

    const params = new URLSearchParams()
    if (category !== 'all') {
      params.set('category', category)
    }
    eventTypes.forEach(type => params.append('event_type', type))

    // Build WebSocket URL
    // Note: Token is sent via Auth message after connection (more secure than URL)
    // In development, use relative URL to go through Vite proxy
    // In Tauri/production, connect directly to backend
    let wsUrl: string
    if ((window as any).__TAURI__) {
      // Tauri: backend runs on localhost:9375
      wsUrl = `ws://localhost:9375/api/events/ws?${params.toString()}`
    } else if (window.location.port === '5173') {
      // Development: use relative URL to go through Vite proxy
      // The proxy will forward to the backend
      wsUrl = `/api/events/ws?${params.toString()}`
    } else {
      // Production: use current host with ws/wss protocol
      const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
      wsUrl = `${wsProtocol}//${window.location.host}/api/events/ws?${params.toString()}`
    }

    try {
      this.ws = new WebSocket(wsUrl)
    } catch (e) {
      this.isConnecting = false
      this.notifyError(new Error(`WebSocket creation failed: ${e}`))
      this.scheduleReconnect()
      return
    }

    this.ws.onopen = () => {
      // Don't notify connection yet - wait for authentication
      this.reconnectAttempts = 0

      // Send authentication message immediately after connection is established
      // Backend expects: {"type": "Auth", "token": "jwt-token"}
      const token = tokenManager.getToken()
      const ws = this.ws
      if (token && ws && ws.readyState === WebSocket.OPEN) {
        try {
          ws.send(JSON.stringify({
            type: 'Auth',
            token: token
          }))
        } catch (e) {
          // Failed to send auth message - set a flag to prevent reconnection from onclose
          this.isConnecting = false
          this.authFailed = true  // Flag to prevent reconnection loop
          this.notifyError(new Error(`Failed to send auth message: ${e}`))
          // Use a timeout to close connection to prevent onclose from scheduling reconnect immediately
          setTimeout(() => {
            if (ws) ws.close()
          }, 0)
          return
        }
      } else {
        // No token available or not ready, close the connection
        this.isConnecting = false
        this.authFailed = true
        this.notifyError(new Error('No token available for WebSocket authentication'))
        setTimeout(() => {
          ws?.close()
        }, 0)
        return
      }
    }

    this.ws.onclose = () => {
      this.isConnecting = false
      this.authenticated = false
      this.notifyConnection(false)
      // Only schedule reconnect if auth didn't fail (e.g., token issues)
      if (!this.authFailed) {
        this.scheduleReconnect()
      }
    }

    this.ws.onerror = () => {
      this.isConnecting = false
      this.notifyError(new Error('WebSocket connection error'))
    }

    this.ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        // Handle Authenticated response
        if (data.type === 'Authenticated') {
          // Authentication successful - now we can notify connection
          this.isConnecting = false
          this.authenticated = true
          this.notifyConnection(true)
          return
        }
        // Handle error messages from server (e.g., auth failures)
        if (data.type === 'Error') {
          // If it's an auth error, clear the token and stop reconnecting
          if (data.message?.includes('token') || data.message?.includes('Authentication') || data.message?.includes('Unauthorized')) {
            this.isConnecting = false
            this.authenticated = false
            tokenManager.clearToken()
            this.disconnect()
          }
          return
        }
        // Only notify events after successful authentication
        if (this.authenticated) {
          this.notifyEvent(data as NeoMindEvent)
        }
      } catch {
        // Silent error handling for malformed messages
      }
    }
  }

  private connectSSE(category: string, eventTypes: EventType[]) {
    // Mark that we're starting a connection
    this.isConnecting = true

    const params = new URLSearchParams()
    if (category !== 'all') {
      params.set('category', category)
    }
    eventTypes.forEach(type => params.append('event_type', type))

    // Add JWT token for authentication
    const token = tokenManager.getToken()
    if (!token) {
      this.isConnecting = false
      this.notifyError(new Error('No token available for SSE connection'))
      // Start polling for token
      this.connect()
      return
    }
    params.set('token', token)

    const sseUrl = `/api/events/stream?${params.toString()}`
    this.eventSource = new EventSource(sseUrl)

    this.eventSource.onopen = () => {
      this.isConnecting = false
      this.reconnectAttempts = 0
      this.notifyConnection(true)
    }

    this.eventSource.onerror = () => {
      this.isConnecting = false
      this.notifyConnection(false)
      this.notifyError(new Error('SSE connection error'))
      // Close the errored EventSource and schedule reconnection
      if (this.eventSource) {
        this.eventSource.close()
        this.eventSource = null
      }
      this.scheduleReconnect()
    }

    // SSE messages come through the onmessage handler
    this.eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as NeoMindEvent
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
    // Reset connecting and authenticated state
    this.isConnecting = false
    this.authenticated = false
    this.authFailed = false  // Reset auth failed flag on explicit disconnect

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
    // Cancel pending RAF
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId)
      this.rafId = null
    }
    // Clear pending events
    this.pendingEvents = []
    // Remove network listener
    if (this.onlineHandler && typeof window !== 'undefined') {
      window.removeEventListener('online', this.onlineHandler)
      this.onlineHandler = null
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
  on<T extends EventType>(eventType: T, handler: (event: Extract<NeoMindEvent, { type: T }>) => void) {
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

  private notifyEvent(event: NeoMindEvent) {
    // Performance optimization: batch events using requestAnimationFrame
    // This reduces UI jank by processing all events in a single frame

    // Handle batched events from server (performance optimization)
    if ((event as any).batch === true && (event as any).events) {
      // Server sent a batch of events
      const batchedEvents = (event as any).events as NeoMindEvent[]
      this.pendingEvents.push(...batchedEvents)
    } else {
      // Single event
      this.pendingEvents.push(event)
    }

    if (this.rafId === null) {
      this.rafId = requestAnimationFrame(() => {
        this.rafId = null
        const events = this.pendingEvents.splice(0) // Clear and get all pending events

        // Process all batched events
        for (const event of events) {
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
      })
    }
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
    // For WebSocket, require authentication to be considered connected
    // For SSE (EventSource), the connection itself implies auth (token in URL)
    if (this.ws) {
      return this.ws.readyState === WebSocket.OPEN && this.authenticated
    }
    if (this.eventSource) {
      return this.eventSource.readyState === EventSource.OPEN
    }
    return false
  }

  /**
   * Update the configuration and reconnect
   */
  updateConfig(config: Partial<EventsConfig>) {
    this.config = { ...this.config, ...config }
    this.disconnect()
    this.connect()
  }

  /**
   * Update config only when category or useSSE changed to avoid disconnect on every mount
   */
  updateConfigIfChanged(config: Partial<EventsConfig>) {
    const sameCategory = this.config.category === config.category
    const sameSSE = this.config.useSSE === config.useSSE
    if (sameCategory && sameSSE) return
    this.updateConfig(config)
  }
}

// Global event connections map
const eventConnections = new Map<string, EventsWebSocket>()

/**
 * Get or create an events WebSocket connection
 * Ensures connection is active but won't reconnect if already connected
 */
export function getEventsConnection(key = 'default', config?: EventsConfig): EventsWebSocket {
  let connection = eventConnections.get(key)
  if (!connection) {
    connection = new EventsWebSocket(config)
    eventConnections.set(key, connection)
  } else if (config && Object.keys(config).length > 0) {
    connection.updateConfigIfChanged(config)
  }
  connection.ensureConnected()
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
