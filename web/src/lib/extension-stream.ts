// Extension Stream WebSocket Client
//
// Provides WebSocket-based streaming communication with extensions.
// Supports generic data types: images, video, audio, sensor data, etc.

import type {
  ExtensionClientMessage,
  ExtensionServerMessage,
  StreamCapability,
  ExtensionStreamConnectionState,
  ExtensionSessionStats,
} from '@/types'
import { tokenManager } from './auth'
import { getServerOrigin } from './api'
import { buildWsUrl } from './urls'

type MessageHandler = (message: ExtensionServerMessage) => void
type ConnectionHandler = (state: ExtensionStreamConnectionState) => void
type ResultHandler = (data: Uint8Array, dataType: string, sequence: number) => void
type SessionClosedHandler = (stats: ExtensionSessionStats) => void
type ErrorHandler = (error: Error) => void

export class ExtensionStreamClient {
  private ws: WebSocket | null = null
  private extensionId: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private reconnectAttempts = 0
  private maxReconnectAttempts = 5
  private baseReconnectDelay = 1000
  private isManualDisconnect = false
  private messageHandlers: Set<MessageHandler> = new Set()
  private connectionHandlers: Set<ConnectionHandler> = new Set()
  private resultHandlers: Set<ResultHandler> = new Set()
  private sessionClosedHandlers: Set<SessionClosedHandler> = new Set()
  private errorHandlers: Set<ErrorHandler> = new Set()
  private currentState: ExtensionStreamConnectionState = {
    status: 'disconnected',
    sessionId: null,
    capability: null,
  }
  private sessionId: string | null = null
  private outputSequence = 0
  private capability: StreamCapability | null = null
  private pendingMessages: ExtensionClientMessage[] = []

  /** Cap for messages queued while disconnected (FIFO eviction past it). */
  private static readonly MAX_PENDING_MESSAGES = 100

  constructor(extensionId: string) {
    this.extensionId = extensionId
  }

  /**
   * Get current connection state
   */
  getState(): ExtensionStreamConnectionState {
    return { ...this.currentState }
  }

  /**
   * Get stream capability (available after connection)
   */
  getCapability(): StreamCapability | null {
    return this.capability
  }

  /**
   * Get session ID (available after init in stateful mode)
   */
  getSessionId(): string | null {
    return this.sessionId
  }

  /**
   * Check if connected
   */
  isConnected(): boolean {
    return this.currentState.status === 'connected' && this.ws?.readyState === WebSocket.OPEN
  }

  /**
   * Connect to the extension stream WebSocket
   */
  connect(config?: Record<string, unknown>): void {
    // Clear any existing timers
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    // Update state
    this.currentState = {
      status: 'connecting',
      sessionId: null,
      capability: null,
    }
    this.notifyConnectionChange()

    // Build WebSocket URL using dynamic server origin (supports instance switching)
    const wsUrl = buildWsUrl(getServerOrigin(), `/api/extensions/${this.extensionId}/stream`)

    // Add JWT token as query parameter
    const token = tokenManager.getToken()
    if (!token) {
      this.handleError(new Error('No authentication token available'))
      this.currentState = {
        status: 'error',
        sessionId: null,
        capability: null,
        errorMessage: 'No authentication token',
      }
      this.notifyConnectionChange()
      return
    }

    const fullUrl = `${wsUrl}?token=${encodeURIComponent(token)}`

    // Close existing connection if any
    if (this.ws) {
      this.ws.close()
      this.ws = null
    }

    try {
      this.ws = new WebSocket(fullUrl)
      this.isManualDisconnect = false

      this.ws.onopen = () => {
        this.reconnectAttempts = 0
        this.currentState = {
          status: 'connected',
          sessionId: this.sessionId,
          capability: this.capability,
        }
        this.notifyConnectionChange()

        // Send hello message
        this.sendMessage({ type: 'hello' })

        // If config provided and mode is stateful, initialize session
        if (config && this.capability?.mode === 'stateful') {
          this.sendMessage({ type: 'init', config })
        }

        // Send pending messages
        while (this.pendingMessages.length > 0) {
          const msg = this.pendingMessages.shift()!
          this.sendMessage(msg)
        }
      }

      this.ws.onclose = (event) => {
        this.currentState = {
          status: 'disconnected',
          sessionId: null,
          capability: this.capability,
        }
        this.notifyConnectionChange()

        // Auto-reconnect if not manual disconnect and not a normal close
        if (!this.isManualDisconnect && event.code !== 1000) {
          this.scheduleReconnect()
        }
      }

      this.ws.onerror = (event) => {
        const error = new Error(`WebSocket error: ${event}`)
        this.handleError(error)
        this.currentState = {
          status: 'error',
          sessionId: null,
          capability: this.capability,
          errorMessage: error.message,
        }
        this.notifyConnectionChange()
      }

      this.ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data) as ExtensionServerMessage
          this.handleMessage(message)
        } catch (err) {
          console.error('[ExtensionStream] Failed to parse message:', err)
        }
      }
    } catch (err) {
      this.handleError(err instanceof Error ? err : new Error(String(err)))
      this.currentState = {
        status: 'error',
        sessionId: null,
        capability: null,
        errorMessage: err instanceof Error ? err.message : String(err),
      }
      this.notifyConnectionChange()
    }
  }

  /**
   * Disconnect from the stream
   */
  disconnect(): void {
    this.isManualDisconnect = true

    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    if (this.ws) {
      this.ws.onclose = null  // Prevent reconnect on close
      this.ws.onerror = null  // Prevent error callbacks after disconnect
      this.ws.close(1000, 'Client disconnect')
      this.ws = null
    }

    this.currentState = {
      status: 'disconnected',
      sessionId: null,
      capability: null,
    }
    this.notifyConnectionChange()
  }

  /**
   * Full teardown — disconnects and clears all handlers to release references.
   * Call this when the client will no longer be used.
   */
  destroy(): void {
    this.disconnect()
    this.messageHandlers.clear()
    this.connectionHandlers.clear()
    this.resultHandlers.clear()
    this.sessionClosedHandlers.clear()
    this.errorHandlers.clear()
    this.pendingMessages = []
  }

  /**
   * Initialize a stateful session
   */
  initSession(config?: Record<string, unknown>): void {
    this.sendMessage({ type: 'init', config })
  }

  /**
   * Send binary data chunk to the extension
   * @param data Binary data to send
   * @param sequence Sequence number for the chunk
   */
  sendChunk(data: ArrayBuffer | Uint8Array, sequence: number): void {
    if (!this.isConnected()) {
      console.warn('[ExtensionStream] Not connected, chunk not sent')
      return
    }

    // Prepare binary frame: [sequence: u64 (8 bytes, big endian)][data...]
    const dataArray = data instanceof Uint8Array ? data : new Uint8Array(data)
    const frame = new Uint8Array(8 + dataArray.length)

    // Write sequence number in big-endian
    const view = new DataView(frame.buffer)
    view.setUint32(0, Math.floor(sequence / 0x100000000), false) // High 32 bits
    view.setUint32(4, sequence & 0xFFFFFFFF, false) // Low 32 bits

    // Copy data
    frame.set(dataArray, 8)

    this.ws?.send(frame)
  }

  /**
   * Send acknowledgment for a received chunk (flow control)
   */
  sendAck(sequence: number): void {
    this.sendMessage({ type: 'ack', sequence })
  }

  /**
   * Request close of the stream/session
   */
  close(): void {
    this.sendMessage({ type: 'close' })
    // Actual disconnect will happen when server responds with close
  }

  /**
   * Subscribe to server messages
   */
  onMessage(handler: MessageHandler): () => void {
    this.messageHandlers.add(handler)
    return () => this.messageHandlers.delete(handler)
  }

  /**
   * Subscribe to connection state changes
   */
  onConnection(handler: ConnectionHandler): () => void {
    this.connectionHandlers.add(handler)
    return () => this.connectionHandlers.delete(handler)
  }

  /**
   * Subscribe to processed results
   */
  onResult(handler: ResultHandler): () => void {
    this.resultHandlers.add(handler)
    return () => this.resultHandlers.delete(handler)
  }

  /**
   * Subscribe to session closed events
   */
  onSessionClosed(handler: SessionClosedHandler): () => void {
    this.sessionClosedHandlers.add(handler)
    return () => this.sessionClosedHandlers.delete(handler)
  }

  /**
   * Subscribe to errors
   */
  onError(handler: ErrorHandler): () => void {
    this.errorHandlers.add(handler)
    return () => this.errorHandlers.delete(handler)
  }

  // Private methods

  private sendMessage(message: ExtensionClientMessage): void {
    if (this.isConnected()) {
      try {
        this.ws?.send(JSON.stringify(message))
      } catch (err) {
        console.error('[ExtensionStream] Failed to send message:', err)
      }
    } else {
      // Queue message for when connected — bounded, and evictions are
      // surfaced through the error channel (the same discipline the chat
      // websocket applies): an unbounded queue on a long disconnect OOMs
      // the tab, and a silent drop loses capability invocations without
      // a trace.
      if (this.pendingMessages.length >= ExtensionStreamClient.MAX_PENDING_MESSAGES) {
        const dropped = this.pendingMessages.shift()
        const preview = dropped ? JSON.stringify(dropped).slice(0, 120) : '(none)'
        console.warn(`[ExtensionStream] Pending limit (${ExtensionStreamClient.MAX_PENDING_MESSAGES}) reached, dropping oldest: ${preview}…`)
        this.errorHandlers.forEach(handler => {
          try {
            handler(new Error(`A queued extension message was dropped (connection issues): ${preview}…`))
          } catch {
            /* handler errors must not break the eviction loop */
          }
        })
      }
      this.pendingMessages.push(message)
    }
  }

  private handleMessage(message: ExtensionServerMessage): void {
    // Notify all message handlers
    this.messageHandlers.forEach(handler => {
      try {
        handler(message)
      } catch (err) {
        console.error('[ExtensionStream] Message handler error:', err)
      }
    })

    // Handle specific message types
    switch (message.type) {
      case 'capability':
        this.capability = message.capability
        this.currentState.capability = message.capability
        this.notifyConnectionChange()
        break

      case 'session_created':
        this.sessionId = message.session_id
        this.currentState.sessionId = message.session_id
        this.notifyConnectionChange()
        break

      case 'result': {
        // Decode base64 data
        const binaryData = this.base64ToUint8Array(message.data)
        this.outputSequence = message.output_sequence

        // Notify result handlers
        this.resultHandlers.forEach(handler => {
          try {
            handler(binaryData, message.data_type, message.output_sequence)
          } catch (err) {
            console.error('[ExtensionStream] Result handler error:', err)
          }
        })
        break
      }

      case 'error':
        this.handleError(new Error(`${message.code}: ${message.message}`))
        break

      case 'session_closed':
        this.sessionId = null
        this.currentState.sessionId = null
        this.notifyConnectionChange()

        // Notify session closed handlers
        this.sessionClosedHandlers.forEach(handler => {
          try {
            handler(message.stats)
          } catch (err) {
            console.error('[ExtensionStream] Session closed handler error:', err)
          }
        })
        break

      case 'push_output': {
        // Push mode: extension-initiated data
        const pushData = this.base64ToUint8Array(message.data)
        this.outputSequence = message.sequence

        // Notify result handlers (same path as stateless/stateful results)
        this.resultHandlers.forEach(handler => {
          try {
            handler(pushData, message.data_type, message.sequence)
          } catch (err) {
            console.error('[ExtensionStream] Push output handler error:', err)
          }
        })
        break
      }

      case 'heartbeat':
        // Just keep-alive, no action needed
        break
    }
  }

  private notifyConnectionChange(): void {
    this.connectionHandlers.forEach(handler => {
      try {
        handler(this.getState())
      } catch (err) {
        console.error('[ExtensionStream] Connection handler error:', err)
      }
    })
  }

  private handleError(error: Error): void {
    this.errorHandlers.forEach(handler => {
      try {
        handler(error)
      } catch (err) {
        console.error('[ExtensionStream] Error handler error:', err)
      }
    })
  }

  private scheduleReconnect(): void {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.warn('[ExtensionStream] Max reconnect attempts reached')
      return
    }

    const delay = Math.min(
      this.baseReconnectDelay * Math.pow(2, this.reconnectAttempts),
      30000
    )

    this.reconnectTimer = setTimeout(() => {
      this.reconnectAttempts++
      this.connect()
    }, delay)
  }

  private base64ToUint8Array(base64: string): Uint8Array {
    const binaryString = atob(base64)
    const bytes = new Uint8Array(binaryString.length)
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i)
    }
    return bytes
  }
}

// Global map of stream clients (one per extension)
const streamClients = new Map<string, ExtensionStreamClient>()

/**
 * Get or create a stream client for an extension
 */
export function getExtensionStreamClient(extensionId: string): ExtensionStreamClient {
  let client = streamClients.get(extensionId)
  if (!client) {
    client = new ExtensionStreamClient(extensionId)
    streamClients.set(extensionId, client)
  }
  return client
}

/**
 * Close and remove a stream client
 */
export function closeExtensionStreamClient(extensionId: string): void {
  const client = streamClients.get(extensionId)
  if (client) {
    client.destroy()
    streamClients.delete(extensionId)
  }
}
