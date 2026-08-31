// Events WebSocket/SSE Manager for Real-time System Events
//
// Provides real-time event streaming from the NeoMind event bus.

import { tokenManager, isTauriEnv, getServerOrigin, getApiKey } from '@/lib/api'
import { buildWsUrl } from '@/lib/urls'

export type EventType =
  | 'DeviceOnline'
  | 'DeviceOffline'
  | 'DeviceTransportOnline'
  | 'DeviceTransportOffline'
  | 'DeviceMetric'
  | 'DeviceCommandResult'
  | 'RuleEvaluated'
  | 'RuleTriggered'
  | 'RuleExecuted'
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
  | 'FrontendComponentLifecycle'
  | 'DashboardUpdated'
  | 'DataChanged'
  | 'ModelDownloadProgress'
  | 'SystemUpgradeProgress'
  | 'Custom'

export interface DataChangedEvent extends NeoMindEvent {
  type: 'DataChanged'
  /** Data domain, from the mutating path segment (e.g. "devices", "automations") */
  domain: string
  /** HTTP method of the mutating request */
  method: string
  /** Full request path */
  path: string
}

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

// Frontend component lifecycle events
export interface FrontendComponentLifecycleEvent extends NeoMindEvent {
  type: 'FrontendComponentLifecycle'
  data: {
    component_id: string
    state: 'installed' | 'uninstalled'
  }
}

// Builtin LLM model download progress events (WS/SSE → progress bar).
// `total`/`error` are `Option` on the backend: `total` serializes as `null`
// when unknown, and `error` is `null`/absent when there is no failure — the
// handler must tolerate both null and missing fields.
export interface ModelDownloadProgressEvent extends NeoMindEvent {
  type: 'ModelDownloadProgress'
  data: {
    model_id: string
    downloaded: number
    total: number | null
    status: 'downloading' | 'complete' | 'error' | 'cancelled'
    error?: string | null
  }
}

// Web-triggered server self-upgrade progress (About page dialog). Like
// ModelDownloadProgress it belongs to no `is_*_event()` category on the
// backend — subscribe on the unfiltered 'all' stream. Optional fields
// (`skip_serializing_if` on the backend) may be absent or null.
export interface SystemUpgradeProgressEvent extends NeoMindEvent {
  type: 'SystemUpgradeProgress'
  data: {
    phase: 'checking' | 'downloading' | 'verifying' | 'staged' | 'applying' | 'restarting' | 'done' | 'error'
    current_version: string
    target_version?: string | null
    downloaded?: number | null
    total?: number | null
    message?: string | null
    error?: string | null
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

    // Get current token and API key
    const currentToken = tokenManager.getToken()
    const apiKey = getApiKey()

    // Need either JWT token or API key to authenticate
    if (!currentToken && !apiKey) {
      this.disconnect()
      this.notifyConnection(false)

      // Poll for credentials every 500ms
      this.tokenCheckTimer = setInterval(() => {
        const token = tokenManager.getToken()
        const key = getApiKey()
        if (token || key) {
          if (this.tokenCheckTimer) {
            clearInterval(this.tokenCheckTimer)
            this.tokenCheckTimer = null
          }
          this.connect()
        }
      }, 500)
      return
    }

    // If token/key changed, force reconnect
    const authId = apiKey || currentToken
    if (this.lastToken !== authId) {
      this.reconnectAttempts = 0
      this.authFailed = false
      if (this.isConnected()) {
        this.disconnect()
      }
    }

    this.lastToken = authId

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

    const apiKey = getApiKey()
    const token = tokenManager.getToken()

    // Build WebSocket URL
    let wsUrl: string
    if (!isTauriEnv() && window.location.port === '5173') {
      // Development: use relative URL to go through Vite proxy
      wsUrl = `/api/events/ws?${params.toString()}`
    } else {
      // Tauri/production: use dynamic origin
      wsUrl = `${buildWsUrl(getServerOrigin(), '/api/events/ws')}?${params.toString()}`
    }

    // For API key auth, pass it as query param (backend validates in on_upgrade)
    if (apiKey) {
      params.set('api_key', apiKey)
      wsUrl += (wsUrl.includes('?') ? '&' : '?') + `api_key=${encodeURIComponent(apiKey)}`
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

      const ws = this.ws

      // If using API key via query param, auth is handled server-side
      // Just wait for the Authenticated message from server
      if (apiKey) {
        return
      }

      // JWT auth: send Auth message
      if (token && ws && ws.readyState === WebSocket.OPEN) {
        try {
          ws.send(JSON.stringify({
            type: 'Auth',
            token: token
          }))
        } catch (e) {
          this.isConnecting = false
          this.authFailed = true
          this.notifyError(new Error(`Failed to send auth message: ${e}`))
          setTimeout(() => {
            if (ws) ws.close()
          }, 0)
          return
        }
      } else {
        // No token available, close the connection
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
        // Handle ping from server - respond with pong to keep connection alive
        if (data.type === 'ping') {
          if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify({ type: 'pong' }))
          }
          return
        }
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

    // Use API key or JWT token for authentication
    const apiKey = getApiKey()
    const token = tokenManager.getToken()
    if (apiKey) {
      params.set('api_key', apiKey)
    } else if (token) {
      params.set('token', token)
    } else {
      this.isConnecting = false
      this.notifyError(new Error('No credentials available for SSE connection'))
      this.connect()
      return
    }

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
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000) * (0.5 + Math.random() * 0.5)
    this.reconnectAttempts++

    this.reconnectTimer = setTimeout(() => {
      this.connect()
    }, delay)
  }

  private notifyEvent(event: NeoMindEvent) {
    // Performance optimization: batch events using requestAnimationFrame
    // This reduces UI jank by processing all events in a single frame

    const MAX_PENDING = 500

    // Handle batched events from server (performance optimization)
    if ((event as any).batch === true && (event as any).events) {
      // Server sent a batch of events
      const batchedEvents = (event as any).events as NeoMindEvent[]
      this.pendingEvents.push(...batchedEvents)
    } else {
      // Single event
      this.pendingEvents.push(event)
    }

    // Cap pending events to prevent unbounded growth when tab is backgrounded (RAF stops)
    if (this.pendingEvents.length > MAX_PENDING) {
      this.pendingEvents.splice(0, this.pendingEvents.length - MAX_PENDING)
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
              } catch (err) {
                console.error('[Events] Type handler error:', err)
              }
            })
          }

          // Notify generic handlers
          this.genericHandlers.forEach(handler => {
            try {
              handler(event)
            } catch (err) {
              console.error('[Events] Generic handler error:', err)
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
