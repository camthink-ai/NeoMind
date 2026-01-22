// WebSocket Manager for Chat
import type { ServerMessage, ClientChatMessage, ChatImage } from '@/types'

type MessageHandler = (message: ServerMessage) => void
type ConnectionHandler = (connected: boolean) => void
type StateChangeHandler = (state: ConnectionState) => void

// Connection state for UI display
export interface ConnectionState {
  status: 'connected' | 'disconnected' | 'reconnecting' | 'error'
  retryCount?: number
  nextRetryIn?: number  // seconds
  errorMessage?: string
}

// Get authentication token (JWT)
function getAuthToken(): string | null {
  // Get JWT token from localStorage or sessionStorage
  return localStorage.getItem('neotalk_token') || sessionStorage.getItem('neotalk_token_session')
}

export class ChatWebSocket {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private countdownTimer: ReturnType<typeof setInterval> | null = null
  private tokenCheckTimer: ReturnType<typeof setInterval> | null = null
  private reconnectAttempts = 0
  private maxReconnectAttempts = Infinity  // 无限重连，直到用户手动刷新
  private messageHandlers: Set<MessageHandler> = new Set()
  private connectionHandlers: Set<ConnectionHandler> = new Set()
  private stateChangeHandlers: Set<StateChangeHandler> = new Set()
  private sessionId: string | null = null
  private activeBackendId: string | null = null
  private pendingMessages: ClientChatMessage[] = []
  private lastToken: string | null = null
  private currentState: ConnectionState = { status: 'disconnected' }

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

    this.sessionId = initialSessionId || null

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    let wsUrl = `${protocol}//${window.location.host}/api/chat`

    // Add JWT token as query parameter
    const token = getAuthToken()

    if (!token) {
      this.disconnect()
      this.notifyConnection(false)

      // Poll for token
      this.tokenCheckTimer = setInterval(() => {
        const newToken = getAuthToken()
        if (newToken) {
          if (this.tokenCheckTimer) {
            clearInterval(this.tokenCheckTimer)
            this.tokenCheckTimer = null
          }
          this.connect(this.sessionId || undefined)
        }
      }, 500)
      return
    }

    // If token changed, reconnect
    if (this.lastToken !== token && this.isConnected()) {
      this.disconnect()
    }

    this.lastToken = token

    if (token) {
      wsUrl += `?token=${encodeURIComponent(token)}`
    }

    this.ws = new WebSocket(wsUrl)

    this.ws.onopen = () => {
      this.reconnectAttempts = 0
      this.notifyConnection(true)
      this.setState({ status: 'connected' })

      // Send pending messages
      while (this.pendingMessages.length > 0) {
        const msg = this.pendingMessages.shift()!
        this.sendRequest(msg)
      }
    }

    this.ws.onclose = (event) => {
      this.notifyConnection(false)
      // Don't reconnect if the server rejected us (auth error)
      if (event.code !== 1000 && event.code !== 4001) {
        this.scheduleReconnect()
      } else {
        this.setState({ status: 'disconnected' })
      }
    }

    this.ws.onerror = () => {
      this.setState({ status: 'error', errorMessage: 'Connection error' })
    }

    this.ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data) as ServerMessage
        // Handle auth error message from server
        if (data.type === 'Error') {
          if (data.message?.includes('token') || data.message?.includes('Authentication')) {
            // Stop reconnecting on auth failure
            this.disconnect()
            // Trigger a page reload to show login screen
            setTimeout(() => window.location.reload(), 1000)
            return
          }
        }
        this.notifyMessage(data)
      } catch {
        // Silent error handling
      }
    }
  }

  disconnect() {
    // Clear countdown timer
    if (this.countdownTimer) {
      clearInterval(this.countdownTimer)
      this.countdownTimer = null
    }
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
    this.setState({ status: 'disconnected' })
  }

  sendRequest(request: ClientChatMessage) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(request))
    } else {
      // Queue message for when connected
      this.pendingMessages.push(request)
    }
  }

  sendMessage(content: string, images?: ChatImage[]) {
    this.sendRequest({
      message: content,
      images: images,
      sessionId: this.sessionId || undefined,
      backendId: this.activeBackendId || undefined,
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
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      this.setState({
        status: 'error',
        errorMessage: '无法连接到服务器，请刷新页面重试'
      })
      return
    }

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000)
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
    handler(this.ws?.readyState === WebSocket.OPEN)
    return () => this.connectionHandlers.delete(handler)
  }

  private notifyMessage(message: ServerMessage) {
    this.messageHandlers.forEach(handler => handler(message))
  }

  private notifyConnection(connected: boolean) {
    this.connectionHandlers.forEach(handler => handler(connected))
  }

  private setState(state: ConnectionState) {
    this.currentState = state
    this.stateChangeHandlers.forEach(handler => handler(state))
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

  isConnected() {
    return this.ws?.readyState === WebSocket.OPEN
  }
}

// Singleton instance
export const ws = new ChatWebSocket()
