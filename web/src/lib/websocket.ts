// WebSocket Manager for Chat
import type { ServerMessage, ClientChatMessage, ChatImage } from '@/types'
import { tokenManager } from './auth'
import { storage } from './utils/storage'
import { isTauriEnv, getApiKey } from './api'
import { buildWsUrl } from './urls'
import { getServerOrigin } from './api'

type MessageHandler = (message: ServerMessage) => void
type ConnectionHandler = (connected: boolean, isReconnect?: boolean) => void
type StateChangeHandler = (state: ConnectionState) => void

// Connection state for UI display
export interface ConnectionState {
  status: 'connected' | 'disconnected' | 'reconnecting' | 'error'
  retryCount?: number
  nextRetryIn?: number  // seconds
  errorMessage?: string
  wasConnected?: boolean  // true if we ever had a successful connection
}

// Persistence configuration
const STORAGE_KEY = 'neomind_ws_pending_messages'
const MAX_STORED_MESSAGES = 100  // Persistent storage limit
const MAX_MESSAGE_AGE_MS = 24 * 60 * 60 * 1000  // 24 hours

// Stored message with metadata
interface StoredMessage {
  message: ClientChatMessage
  timestamp: number
  retries: number
}

export class ChatWebSocket {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private countdownTimer: ReturnType<typeof setInterval> | null = null
  private tokenCheckTimer: ReturnType<typeof setInterval> | null = null
  private reconnectAttempts = 0
  private maxReconnectAttempts = 15  // 快速重连最多15次
  private baseReconnectDelay = 1000  // 初始重连延迟1秒
  private maxReconnectDelay = 30000   // 最大重连延迟30秒
  private slowReconnectDelay = 30000  // 超过上限后每30s轮询一次，无限重试
  private isManualDisconnect = false  // 是否用户主动断开
  private wasConnected = false  // 跟踪是否曾经连接过（用于区分初始连接和重连）
  private messageHandlers: Set<MessageHandler> = new Set()
  private connectionHandlers: Set<ConnectionHandler> = new Set()
  private stateChangeHandlers: Set<StateChangeHandler> = new Set()
  private sessionId: string | null = null
  private activeBackendId: string | null = null
  private pendingMessages: ClientChatMessage[] = []
  private readonly MAX_PENDING_MESSAGES = 50  // P0: Limit pending messages to prevent memory leak
  private lastToken: string | null = null
  private currentState: ConnectionState = { status: 'disconnected' }
  private onlineHandler: (() => void) | null = null

  constructor() {
    this.loadPendingMessages()

    // Listen for network recovery to trigger immediate reconnect
    if (typeof window !== 'undefined' && window.addEventListener) {
      this.onlineHandler = () => {
        if (!this.isConnected() && !this.isManualDisconnect && !this.isConnecting) {
          this.reconnectAttempts = 0  // Reset backoff on network recovery
          this.connect(this.sessionId || undefined)
        }
      }
      window.addEventListener('online', this.onlineHandler)
    }
  }

  private get isConnecting(): boolean {
    return this.ws?.readyState === WebSocket.CONNECTING
  }

  private loadPendingMessages() {
    const stored = storage.get<StoredMessage[]>(STORAGE_KEY, []) || []
    const now = Date.now()

    // Filter expired messages (24 hours)
    const valid = stored.filter(m =>
      now - m.timestamp < MAX_MESSAGE_AGE_MS
    )

    // Restore to memory queue (up to MAX_PENDING_MESSAGES)
    this.pendingMessages = valid
      .slice(0, this.MAX_PENDING_MESSAGES)
      .map(m => m.message)

  }

  connect(initialSessionId?: string) {
    // Clear any existing timers
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.tokenCheckTimer) {
      clearInterval(this.tokenCheckTimer)
      this.tokenCheckTimer = null
    }

    // Calling connect() is an explicit intent to connect — reset manual disconnect flag
    this.isManualDisconnect = false

    this.sessionId = initialSessionId || null

    // Build WebSocket URL using dynamic server origin (supports instance switching)
    let wsUrl = buildWsUrl(getServerOrigin(), '/api/chat')

    // Determine authentication method: API Key (remote instance) or JWT (local instance)
    const apiKey = getApiKey()
    const token = tokenManager.getToken()

    if (apiKey) {
      // Remote instance: authenticate with API key
      wsUrl += `?api_key=${encodeURIComponent(apiKey)}`
    } else if (token) {
      // Local instance: authenticate with JWT token
      // If token changed, reconnect with new token
      if (this.lastToken !== token && this.isConnected()) {
        // Update lastToken BEFORE closing so the reconnect uses the new token
        this.lastToken = token
        if (this.ws) {
          this.ws.close(4000, 'Token changed - reconnecting')
        }
        // onclose handler will call scheduleReconnect() which calls connect() with the new token
        return
      }
      this.lastToken = token
      wsUrl += `?token=${encodeURIComponent(token)}`
    } else {
      // No credentials available — poll for token
      this.disconnect()
      this.notifyConnection(false)

      this.tokenCheckTimer = setInterval(() => {
        const newToken = tokenManager.getToken()
        const newApiKey = getApiKey()
        if (newToken || newApiKey) {
          if (this.tokenCheckTimer) {
            clearInterval(this.tokenCheckTimer)
            this.tokenCheckTimer = null
          }
          this.connect(this.sessionId || undefined)
        }
      }, 500)
      return
    }

    // Don't create a new WebSocket if we're already connected with the same token
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      // Already connected, no need to reconnect
      return
    }

    // Close existing WebSocket if it's in a bad state
    if (this.ws) {
      this.ws.close()
      this.ws = null
    }

    this.ws = new WebSocket(wsUrl)

    this.ws.onopen = () => {
      const isReconnect = this.wasConnected
      this.wasConnected = true  // 标记已连接过

      this.resetReconnectState()  // 重置重连状态
      this.notifyConnection(true, isReconnect)
      this.setState({ status: 'connected' })

      // Send pending messages
      while (this.pendingMessages.length > 0) {
        const msg = this.pendingMessages.shift()!
        this.sendRequest(msg)
      }

      // If all messages sent successfully, clear storage
      if (this.pendingMessages.length === 0) {
        storage.remove(STORAGE_KEY)
      }
    }

    this.ws.onclose = (event) => {
      this.notifyConnection(false)
      // Only stop reconnecting on auth rejection (4001)
      // Reconnect on everything else including normal close (server restart)
      // Code 4000 is used for token change - we DO want to reconnect
      if (event.code === 4001) {
        // Auth error - stop reconnecting
        if (this.pendingMessages.length > 0) {
          console.warn(`[WebSocket] Auth error, clearing ${this.pendingMessages.length} pending messages`)
          this.pendingMessages = []
          storage.remove(STORAGE_KEY)
        }
        this.setState({ status: 'disconnected' })
      } else {
        this.scheduleReconnect()
      }
    }

    this.ws.onerror = () => {
      this.setState({ status: 'error', errorMessage: 'Connection error' })
    }

    this.ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as ServerMessage

        // Auto-respond to ping from server
        if (data.type === 'ping') {
          if (this.ws?.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify({ type: 'pong' }))
          }
          return // Don't notify handlers about ping/pong
        }

        // Handle auth error message from server
        // IMPORTANT: Only trigger reload for JWT-related auth errors where
        // the user needs to re-login. For API key errors (remote instance),
        // just disconnect — reloading won't fix an invalid key.
        if (data.type === 'Error') {
          const msg = (data.message || '').toLowerCase()
          const isJwtError = msg.includes('jwt') ||
            (msg.includes('token') && (msg.includes('expired') || msg.includes('invalid')))
          const isApiKeyError = msg.includes('invalid api key')

          if (isApiKeyError) {
            // API key auth failed — disconnect without reload.
            // The user needs to fix the API key in instance settings.
            console.warn('[WebSocket] API key auth failed, stopping reconnect')
            this.disconnect()
            return
          }

          if (isJwtError || msg.includes('authentication') ||
            msg.includes('unauthorized') || msg.includes('access denied')) {
            // JWT expired/invalid — disconnect + let UI handle redirect
            // (aligned with events.ts: no forced reload that disrupts the user)
            this.disconnect()
            return
          }
        }
        this.notifyMessage(data)
      } catch (err) {
        console.warn('[WebSocket] Failed to parse message:', typeof event.data === 'string' ? event.data.slice(0, 100) : '(non-string)')
      }
    }
  }

  sendRequest(request: ClientChatMessage) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(request))
      this.removeFromStorage(request)  // Remove from storage on successful send
    } else {
      // Queue message for when connected
      // P0: Prevent unbounded growth - evict oldest if limit reached
      if (this.pendingMessages.length >= this.MAX_PENDING_MESSAGES) {
        // Remove oldest message (FIFO)
        const dropped = this.pendingMessages.shift()
        if (dropped) {
          const preview = typeof dropped.message === 'string'
            ? dropped.message.slice(0, 50)
            : '(non-text message)'
          console.warn(`[WebSocket] Pending limit (${this.MAX_PENDING_MESSAGES}) reached, dropping oldest: "${preview}..."`)
          // Notify UI that a message was dropped
          this.setState({
            ...this.currentState,
            errorMessage: `A queued message was dropped due to connection issues: "${preview}"`,
          })
        }
      }
      this.pendingMessages.push(request)
      this.persistPendingMessages()  // Immediately persist to storage
    }
  }

  sendMessage(content: string, images?: ChatImage[], selectedSkills?: string[], pageContext?: string) {
    this.sendRequest({
      message: content,
      images: images,
      sessionId: this.sessionId || undefined,
      backendId: this.activeBackendId || undefined,
      selectedSkills: selectedSkills && selectedSkills.length > 0 ? selectedSkills : undefined,
      pageContext: pageContext || undefined,
    })
  }

  setActiveBackend(backendId: string | null) {
    this.activeBackendId = backendId
  }

  setSessionId(sessionId: string) {
    this.sessionId = sessionId
  }

  getSessionId() {
    return this.sessionId
  }

  private scheduleReconnect() {
    // 如果是用户主动断开，不重连
    if (this.isManualDisconnect) {
      this.setState({
        status: 'disconnected',
        errorMessage: undefined
      })
      return
    }

    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      // Never had a successful connection = the backend never came up at all
      // (port conflict, crashed at startup, wrong endpoint). Stop silently
      // spinning "Reconnecting" forever and surface a clear error so the user
      // knows to act (manualReconnect stays available). A backend that was up
      // and briefly dropped (server restart) still gets slow-polling recovery
      // below so it auto-reconnects when it returns.
      if (!this.wasConnected) {
        this.setState({
          status: 'error',
          retryCount: this.reconnectAttempts,
        })
        return
      }
      // Switch to slow polling mode - keep retrying every 30s indefinitely
      // instead of giving up permanently
      this.reconnectAttempts++ // Keep incrementing to track total attempts

      this.setState({
        status: 'reconnecting',
        retryCount: this.reconnectAttempts,
        nextRetryIn: Math.ceil(this.slowReconnectDelay / 1000)
      })

      let countdown = Math.ceil(this.slowReconnectDelay / 1000)
      if (this.countdownTimer) {
        clearInterval(this.countdownTimer)
      }
      this.countdownTimer = setInterval(() => {
        countdown--
        this.setState({
          status: 'reconnecting',
          retryCount: this.reconnectAttempts,
          nextRetryIn: countdown
        })
        if (countdown <= 0) {
          if (this.countdownTimer) {
            clearInterval(this.countdownTimer)
            this.countdownTimer = null
          }
        }
      }, 1000)

      this.reconnectTimer = setTimeout(() => {
        if (this.countdownTimer) {
          clearInterval(this.countdownTimer)
          this.countdownTimer = null
        }
        this.connect(this.sessionId || undefined)
      }, this.slowReconnectDelay)
      return
    }

    // 使用指数退避算法，但有最大延迟限制
    const delay = Math.min(
      this.baseReconnectDelay * Math.pow(2, this.reconnectAttempts),
      this.maxReconnectDelay
    ) * (0.5 + Math.random() * 0.5)
    this.reconnectAttempts++

    // Set reconnecting state
    this.setState({
      status: 'reconnecting',
      retryCount: this.reconnectAttempts,
      nextRetryIn: Math.ceil(delay / 1000)
    })

    // Start countdown
    let countdown = Math.ceil(delay / 1000)
    if (this.countdownTimer) {
      clearInterval(this.countdownTimer)
    }
    this.countdownTimer = setInterval(() => {
      countdown--
      this.setState({
        status: 'reconnecting',
        retryCount: this.reconnectAttempts,
        nextRetryIn: countdown
      })
      if (countdown <= 0) {
        if (this.countdownTimer) {
          clearInterval(this.countdownTimer)
          this.countdownTimer = null
        }
      }
    }, 1000)

    this.reconnectTimer = setTimeout(() => {
      if (this.countdownTimer) {
        clearInterval(this.countdownTimer)
        this.countdownTimer = null
      }
      this.connect(this.sessionId || undefined)
    }, delay)
  }

  onMessage(handler: MessageHandler) {
    this.messageHandlers.add(handler)
    return () => this.messageHandlers.delete(handler)
  }

  onConnection(handler: ConnectionHandler) {
    this.connectionHandlers.add(handler)
    handler(this.ws?.readyState === WebSocket.OPEN, false)
    return () => this.connectionHandlers.delete(handler)
  }

  private notifyMessage(message: ServerMessage) {
    this.messageHandlers.forEach(handler => handler(message))
  }

  private notifyConnection(connected: boolean, isReconnect = false) {
    this.connectionHandlers.forEach(handler => handler(connected, isReconnect))
  }

  private setState(state: ConnectionState) {
    this.currentState = { ...state, wasConnected: this.wasConnected }
    this.stateChangeHandlers.forEach(handler => handler(this.currentState))
  }

  // Public API for connection state
  getState(): ConnectionState {
    return this.currentState
  }

  onStateChange(handler: StateChangeHandler): () => void {
    this.stateChangeHandlers.add(handler)
    handler(this.currentState) // Immediately call with current state
    return () => this.stateChangeHandlers.delete(handler)
  }

  /**
   * 手动断开连接（不会触发重连）
   */
  disconnect() {
    this.isManualDisconnect = true
    this.reconnectAttempts = 0

    // Clear timers
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.countdownTimer) {
      clearInterval(this.countdownTimer)
      this.countdownTimer = null
    }
    if (this.tokenCheckTimer) {
      clearInterval(this.tokenCheckTimer)
      this.tokenCheckTimer = null
    }

    // Close WebSocket
    if (this.ws) {
      this.ws.close(1000, 'User disconnected')
      this.ws = null
    }

    // Remove network listener
    if (this.onlineHandler && typeof window !== 'undefined') {
      window.removeEventListener('online', this.onlineHandler)
      this.onlineHandler = null
    }

    // Persist pending messages on manual disconnect (don't clear)
    if (this.pendingMessages.length > 0) {
      this.persistPendingMessages()
    }

    this.setState({ status: 'disconnected' })
  }

  /**
   * 手动重新连接（重置重连计数器）
   */
  manualReconnect() {
    this.isManualDisconnect = false
    this.reconnectAttempts = 0

    // Clear existing timers
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.countdownTimer) {
      clearInterval(this.countdownTimer)
      this.countdownTimer = null
    }

    // Close existing WebSocket and clear reference before connecting
    // This prevents race conditions where the old socket triggers events
    if (this.ws) {
      if (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING) {
        this.ws.close()
      }
      this.ws = null  // Clear immediately to prevent race conditions
    }

    this.connect(this.sessionId || undefined)
  }

  /**
   * 重置重连状态（用于连接成功后）
   */
  private resetReconnectState() {
    this.reconnectAttempts = 0
    this.isManualDisconnect = false
  }

  /**
   * Persist pending messages to localStorage
   */
  private persistPendingMessages() {
    const now = Date.now()
    const toStore: StoredMessage[] = this.pendingMessages.map(msg => ({
      message: msg,
      timestamp: now,
      retries: 0
    }))

    // Limit storage to MAX_STORED_MESSAGES
    const limited = toStore.slice(-MAX_STORED_MESSAGES)
    storage.set(STORAGE_KEY, limited)
  }

  /**
   * Remove a specific message from storage after successful send
   */
  private removeFromStorage(request: ClientChatMessage) {
    const stored = storage.get<StoredMessage[]>(STORAGE_KEY, []) || []
    const filtered = stored.filter(m =>
      m.message.sessionId !== request.sessionId ||
      m.message.message !== request.message
    )

    if (filtered.length < stored.length) {
      storage.set(STORAGE_KEY, filtered)
    }
  }

  isConnected() {
    return this.ws?.readyState === WebSocket.OPEN
  }
}

// Singleton instance
export const ws = new ChatWebSocket()
