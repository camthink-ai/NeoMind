// WebSocket Manager for Chat
import type { ServerMessage, ClientChatMessage } from '@/types'

type MessageHandler = (message: ServerMessage) => void
type ConnectionHandler = (connected: boolean) => void

// Get authentication token (JWT)
function getAuthToken(): string | null {
  // Get JWT token from localStorage or sessionStorage
  return localStorage.getItem('neotalk_token') || sessionStorage.getItem('neotalk_token_session')
}

export class ChatWebSocket {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private tokenCheckTimer: ReturnType<typeof setInterval> | null = null
  private reconnectAttempts = 0
  private maxReconnectAttempts = Infinity  // 无限重连，直到用户手动刷新
  private messageHandlers: Set<MessageHandler> = new Set()
  private connectionHandlers: Set<ConnectionHandler> = new Set()
  private sessionId: string | null = null
  private pendingMessages: ClientChatMessage[] = []
  private lastToken: string | null = null

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
      }
    }

    this.ws.onerror = () => {
      // Silent error handling
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
  }

  sendRequest(request: ClientChatMessage) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(request))
    } else {
      // Queue message for when connected
      this.pendingMessages.push(request)
    }
  }

  sendMessage(content: string) {
    this.sendRequest({
      message: content,
      sessionId: this.sessionId || undefined,
    })
  }

  setSessionId(sessionId: string) {
    this.sessionId = sessionId
  }

  getSessionId() {
    return this.sessionId
  }

  private scheduleReconnect() {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log('Max reconnect attempts reached')
      return
    }

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000)
    this.reconnectAttempts++

    console.log(`Reconnecting in ${delay}ms... (attempt ${this.reconnectAttempts})`)

    this.reconnectTimer = setTimeout(() => {
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

  isConnected() {
    return this.ws?.readyState === WebSocket.OPEN
  }
}

// Singleton instance
export const ws = new ChatWebSocket()
