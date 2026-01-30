import { useEffect, useRef, useState, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { useParams, useNavigate } from "react-router-dom"
import { Settings, Send, Sparkles, PanelLeft, Plus, Zap, ChevronDown, X, Image as ImageIcon, Loader2, Eye, Brain } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu"
import { SessionSidebar } from "@/components/session/SessionSidebar"
import { WelcomeArea } from "@/components/chat/WelcomeArea"
import { MarkdownMessage } from "@/components/chat/MarkdownMessage"
import { ThinkingBlock } from "@/components/chat/ThinkingBlock"
import { ToolCallVisualization } from "@/components/chat/ToolCallVisualization"
import { QuickActions } from "@/components/chat/QuickActions"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { ws } from "@/lib/websocket"
import type { Message, ServerMessage, ChatImage } from "@/types"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"

/** Image gallery component for user messages */
function MessageImages({ images }: { images: ChatImage[] }) {
  if (!images || images.length === 0) return null

  return (
    <div className={images.length === 1 ? "mb-2" : "mb-2 grid grid-cols-2 gap-2"}>
      {images.map((img, idx) => (
        <img
          key={idx}
          src={img.data}
          alt={`Image ${idx + 1}`}
          className="rounded-lg max-w-full max-h-64 object-cover"
        />
      ))}
    </div>
  )
}

/**
 * Merge fragmented assistant messages for display.
 * Same logic as MergedMessageList component.
 */
function mergeMessagesForDisplay(messages: Message[]): Message[] {
  const result: Message[] = []

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i]

    // Skip tool messages (internal use)
    if ((msg as any).role === "tool") continue

    // User messages and system messages are kept as-is
    if (msg.role !== "assistant") {
      result.push(msg)
      continue
    }

    // Assistant messages - check if we should merge with following assistant messages
    const mergedAssistant: Message = { ...msg }
    const contentParts: string[] = []
    if (msg.content) {
      contentParts.push(msg.content)
    }

    // Look ahead for consecutive assistant messages to merge
    let j = i + 1
    while (j < messages.length && messages[j].role === "assistant") {
      const nextMsg = messages[j]

      // Collect content
      if (nextMsg.content) {
        contentParts.push(nextMsg.content)
      }

      // Use thinking from first message that has it
      if (!mergedAssistant.thinking && nextMsg.thinking) {
        mergedAssistant.thinking = nextMsg.thinking
      }

      // Use tool_calls from first message that has them
      if (!mergedAssistant.tool_calls && nextMsg.tool_calls) {
        mergedAssistant.tool_calls = nextMsg.tool_calls
      }

      j++
    }

    // Set merged content
    mergedAssistant.content = contentParts.join("")

    // Only add if there's something to show
    if (mergedAssistant.content || mergedAssistant.thinking || mergedAssistant.tool_calls) {
      result.push(mergedAssistant)
    }

    // Skip the merged messages
    i = j - 1
  }

  return result
}

// Hook to detect desktop breakpoint (lg: 1024px)
function useIsDesktop() {
  const [isDesktop, setIsDesktop] = useState(false)

  useEffect(() => {
    const checkIsDesktop = () => setIsDesktop(window.innerWidth >= 1024)
    checkIsDesktop()
    window.addEventListener("resize", checkIsDesktop)
    return () => window.removeEventListener("resize", checkIsDesktop)
  }, [])

  return isDesktop
}

// Check if active backend supports multimodal
function getActiveBackendSupportsMultimodal(llmBackends: any[], activeBackendId: string | null): boolean {
  if (!activeBackendId) return false
  const activeBackend = llmBackends.find(b => b.id === activeBackendId)
  return activeBackend?.capabilities?.supports_multimodal ?? false
}

// Convert file to base64 data URL
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

export function ChatPage() {
  const { t } = useTranslation(['common', 'chat'])
  const { sessionId: urlSessionId } = useParams<{ sessionId?: string }>()
  const navigate = useNavigate()
  const llmBackends = useStore((state) => state.llmBackends)
  const activeBackendId = useStore((state) => state.activeBackendId)
  const activateBackend = useStore((state) => state.activateBackend)
  const loadBackends = useStore((state) => state.loadBackends)
  const hasLoadedBackends = useRef(false)

  // Chat state from store
  const {
    sessionId,
    messages,
    addMessage,
    createSession,
    switchSession,
    loadSessions,
    user
  } = useStore()

  // Local state
  const [input, setInput] = useState("")
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  const [streamingToolCalls, setStreamingToolCalls] = useState<any[]>([])
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  // Track the ID of the last assistant message for tool call result updates
  const [lastAssistantMessageId, setLastAssistantMessageId] = useState<string | null>(null)

  // Image upload state
  const [attachedImages, setAttachedImages] = useState<ChatImage[]>([])
  const [isUploadingImage, setIsUploadingImage] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Responsive
  const isDesktop = useIsDesktop()

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  const streamingContentRef = useRef("")
  const streamingThinkingRef = useRef("")
  const streamingToolCallsRef = useRef<any[]>([])
  const isStreamingRef = useRef(false)

  // Load LLM backends and sessions on mount
  useEffect(() => {
    if (!hasLoadedBackends.current) {
      hasLoadedBackends.current = true
      loadBackends()
      loadSessions()
    }
  }, [loadBackends, loadSessions])

  // Get sessions from store for navigation logic
  const sessions = useStore((state) => state.sessions)

  // Load session from URL parameter (only when on /chat/:sessionId)
  useEffect(() => {
    if (urlSessionId && urlSessionId !== sessionId) {
      // Only switch if it's a different session
      switchSession(urlSessionId).catch((err) => {
        console.error('Failed to load session from URL:', err)
      })
    }
  }, [urlSessionId, sessionId, switchSession])

  // Auto-navigate to latest session when on /chat without sessionId
  // Also redirect to /chat when current session is deleted or no sessions exist
  useEffect(() => {
    // Only run this after sessions are loaded
    if (!hasLoadedBackends.current) return

    const currentPath = window.location.pathname

    // If current sessionId in URL is not in sessions list (session was deleted)
    // redirect to /chat
    if (urlSessionId && sessions.length > 0 && !sessions.find(s => s.sessionId === urlSessionId)) {
      navigate('/chat', { replace: true })
      return
    }

    // If sessions become empty, redirect to /chat
    if (urlSessionId && sessions.length === 0) {
      navigate('/chat', { replace: true })
      return
    }

    const isOnChatPath = currentPath === '/chat' || currentPath === '/'

    // If on /chat or / with no sessionId in URL, and we have sessions
    if (isOnChatPath && !urlSessionId && sessions.length > 0) {
      // Navigate to the most recent session
      const latestSession = sessions[0] // sessions are sorted by updatedAt desc
      if (latestSession?.sessionId) {
        navigate(`/chat/${latestSession.sessionId}`, { replace: true })
        return
      }
    }

    // Redirect root path to /chat
    if (currentPath === '/') {
      navigate('/chat', { replace: true })
    }
  }, [urlSessionId, sessions, navigate])

  // Sync WebSocket sessionId when store sessionId changes
  useEffect(() => {
    if (sessionId) {
      ws.setSessionId(sessionId)
    }
  }, [sessionId])

  // Determine mode: welcome mode (no sessionId in URL) or chat mode (has sessionId in URL)
  const isWelcomeMode = !urlSessionId

  // Auto-scroll to bottom
  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [])

  useEffect(() => {
    scrollToBottom()
  }, [messages, streamingContent, scrollToBottom])

  // Sync isStreaming ref with state
  useEffect(() => {
    isStreamingRef.current = isStreaming
  }, [isStreaming])

  // Handle WebSocket events
  useEffect(() => {
    const handleMessage = (data: ServerMessage) => {
      switch (data.type) {
        case "Thinking":
          setIsStreaming(true)
          streamingThinkingRef.current += (data.content || "")
          setStreamingThinking(streamingThinkingRef.current)
          break

        case "Content":
          setIsStreaming(true)
          streamingContentRef.current += (data.content || "")
          setStreamingContent(streamingContentRef.current)
          break

        case "ToolCallStart": {
          const toolCall = {
            id: crypto.randomUUID(),
            name: data.tool,
            arguments: data.arguments,
            result: null
          }
          streamingToolCallsRef.current.push(toolCall)
          setStreamingToolCalls([...streamingToolCallsRef.current])
          break
        }

        case "ToolCallEnd": {
          setStreamingToolCalls(prev => {
            const updated = prev.map(tc =>
              tc.name === data.tool
                ? { ...tc, result: data.result }
                : tc
            )
            // Also update the ref for consistency
            streamingToolCallsRef.current = updated

            // If not streaming (stream ended before tool execution),
            // update the saved assistant message's tool_calls
            if (!isStreamingRef.current && lastAssistantMessageId) {
              const lastMessage = messages.find(m => m.id === lastAssistantMessageId)
              if (lastMessage && lastMessage.role === "assistant" && lastMessage.tool_calls) {
                const updatedToolCalls = lastMessage.tool_calls.map(tc =>
                  tc.name === data.tool
                    ? { ...tc, result: data.result }
                    : tc
                )
                // Update the message in store
                addMessage({
                  ...lastMessage,
                  tool_calls: updatedToolCalls
                })
              }
            }

            return updated
          })
          break
        }

        case "end":
          if (streamingContentRef.current || streamingThinkingRef.current || streamingToolCallsRef.current.length > 0) {
            const messageId = streamingMessageIdRef.current || crypto.randomUUID()
            const completeMessage: Message = {
              id: messageId,
              role: "assistant",
              content: streamingContentRef.current,
              timestamp: Date.now(),
              thinking: streamingThinkingRef.current || undefined,
              tool_calls: streamingToolCallsRef.current.length > 0 ? streamingToolCallsRef.current : undefined,
            }
            addMessage(completeMessage)
            // Track this message for potential tool call result updates
            setLastAssistantMessageId(messageId)
          }
          setIsStreaming(false)
          setStreamingContent("")
          setStreamingThinking("")
          setStreamingToolCalls([])
          streamingContentRef.current = ""
          streamingThinkingRef.current = ""
          streamingToolCallsRef.current = []
          streamingMessageIdRef.current = null
          break

        case "Error":
          setIsStreaming(false)
          break

        case "session_created":
        case "session_switched":
          // Only switch if it's a different session to avoid unnecessary API calls
          if (data.sessionId && data.sessionId !== sessionId) {
            switchSession(data.sessionId)
          }
          break
      }
    }

    const unsubscribe = ws.onMessage(handleMessage)
    return () => { void unsubscribe() }
  }, [addMessage, switchSession, lastAssistantMessageId, messages, sessionId])

  // Send message - in welcome mode, create session and navigate
  const handleSend = async () => {
    const trimmedInput = input.trim()
    if ((!trimmedInput && attachedImages.length === 0) || isStreaming) return

    // Check if images are attached but current model doesn't support vision
    if (attachedImages.length > 0 && !supportsMultimodal) {
      alert(t('chat:model.visionError'))
      return
    }

    // In welcome mode, create session first, then navigate
    if (isWelcomeMode) {
      const newSessionId = await createSession()
      if (!newSessionId) {
        console.error('Failed to create session')
        return
      }
      // Navigate to the new session URL
      navigate(`/chat/${newSessionId}`)
    }

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content: trimmedInput || (attachedImages.length > 0 ? "[Image]" : ""),
      timestamp: Date.now(),
      images: attachedImages.length > 0 ? [...attachedImages] : undefined,
    }
    addMessage(userMessage)

    setInput("")
    setAttachedImages([])
    setIsStreaming(true)
    streamingMessageIdRef.current = crypto.randomUUID()
    // Reset last assistant message ID (new response incoming)
    setLastAssistantMessageId(null)

    ws.sendMessage(trimmedInput, attachedImages.length > 0 ? attachedImages : undefined)

    setTimeout(() => {
      inputRef.current?.focus()
    }, 100)
  }

  // Handle image file selection
  const handleImageSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return

    setIsUploadingImage(true)
    try {
      const newImages: ChatImage[] = []
      for (let i = 0; i < files.length; i++) {
        const file = files[i]
        if (!file.type.startsWith('image/')) continue

        // Limit file size to 10MB
        if (file.size > 10 * 1024 * 1024) {
          alert(`Image ${file.name} is too large. Maximum size is 10MB.`)
          continue
        }

        const dataUrl = await fileToBase64(file)
        newImages.push({
          data: dataUrl,
          mimeType: file.type,
        })
      }

      if (newImages.length > 0) {
        setAttachedImages(prev => [...prev, ...newImages])
      }
    } catch (error) {
      console.error('Failed to process images:', error)
      alert(t('imageProcessFailed'))
    } finally {
      setIsUploadingImage(false)
      // Reset file input
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    }
  }

  // Remove attached image
  const removeAttachedImage = (index: number) => {
    setAttachedImages(prev => prev.filter((_, i) => i !== index))
  }

  // Check if multimodal is supported
  const supportsMultimodal = getActiveBackendSupportsMultimodal(llmBackends, activeBackendId)

  // Handle quick action
  const handleQuickAction = (prompt: string) => {
    setInput(prompt)
    inputRef.current?.focus()
  }

  // Handle keyboard shortcuts
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  // Filter out partial messages and merge fragmented assistant messages
  const filteredMessages = messages.filter(msg => !msg.isPartial)
  const displayMessages = mergeMessagesForDisplay(filteredMessages)

  // Show chat area if there are messages or currently streaming
  const hasMessages = filteredMessages.length > 0 || isStreaming

  // Show LLM setup prompt if not configured
  if (!llmBackends || llmBackends.length === 0) {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <div className="text-center max-w-md px-6">
          <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-xl bg-muted">
            <Settings className="h-8 w-8 text-muted-foreground" />
          </div>
          <h2 className="mb-3 text-lg font-semibold">{t('chat:notConfigured.title')}</h2>
          <p className="text-sm text-muted-foreground mb-6">
            {t('chat:notConfigured.description')}
          </p>
          <Button
            onClick={() => navigate('/plugins')}
            className="gap-2"
            size="default"
          >
            <Settings className="h-4 w-4" />
            {t('chat:notConfigured.goToSettings')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full">
      {/* Desktop Sidebar - always show when there are sessions or in chat mode */}
      {isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
        <SessionSidebar
          open={true}
          onClose={() => {}}
          collapsed={sidebarCollapsed}
          onToggleCollapse={() => setSidebarCollapsed(!sidebarCollapsed)}
          isDesktop={true}
        />
      )}

      {/* Mobile Sidebar - drawer */}
      {!isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
        <SessionSidebar
          open={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
          isDesktop={false}
        />
      )}

      {/* Main Content */}
      <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        {/* Mobile Header - show when there are sessions or in chat mode */}
        {!isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
          <div className="h-11 flex items-center px-3 gap-2 bg-background/50 backdrop-blur-sm border-b border-border/30">
            {/* Menu button */}
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setSidebarOpen(true)}
              className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            >
              <PanelLeft className="h-4 w-4" />
            </Button>

            <div className="flex-1" />

            {/* New session button */}
            <Button
              variant="ghost"
              size="sm"
              onClick={async () => {
                const newSessionId = await createSession()
                if (newSessionId) {
                  navigate(`/chat/${newSessionId}`)
                }
              }}
              className="h-8 gap-1.5 rounded-lg text-muted-foreground hover:text-foreground"
            >
              <Plus className="h-4 w-4" />
              <span className="text-xs">{t('chat:input.newChat')}</span>
            </Button>
          </div>
        )}

        {/* Chat Content Area */}
        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        {isWelcomeMode ? (
          /* Welcome Area - shown on /chat (no sessionId) */
          <WelcomeArea onQuickAction={handleQuickAction} />
        ) : hasMessages ? (
          /* Chat Messages - shown on /chat/:sessionId with messages */
          <div className="flex-1 overflow-y-auto px-2 sm:px-4 py-4 sm:py-6">
            <div className="max-w-3xl mx-auto space-y-4 sm:space-y-6">
              {displayMessages.map((message) => (
                <div
                  key={message.id}
                  className={`flex gap-2 sm:gap-3 ${message.role === "user" ? "justify-end" : "justify-start"}`}
                >
                  {message.role === "assistant" && (
                    <div className="flex-shrink-0 w-7 h-7 sm:w-8 sm:h-8 rounded-lg bg-foreground flex items-center justify-center">
                      <Sparkles className="h-3.5 w-3.5 sm:h-4 sm:w-4 text-background" />
                    </div>
                  )}

                  <div className={`max-w-[85%] sm:max-w-[80%] ${message.role === "user" ? "order-1" : ""}`}>
                    <div
                      className={cn(
                        "rounded-2xl px-3 py-2 sm:px-4 sm:py-3",
                        message.role === "user"
                          ? "bg-foreground text-background"
                          : "bg-muted text-foreground"
                      )}
                    >
                      <div className={message.role === "user" ? "message-bubble-user" : "message-bubble-assistant"}>
                      {/* Images for user messages */}
                      {message.role === "user" && message.images && message.images.length > 0 && (
                        <MessageImages images={message.images} />
                      )}
                      {message.thinking && <ThinkingBlock thinking={message.thinking} />}
                      {message.tool_calls && message.tool_calls.length > 0 && (
                        <ToolCallVisualization toolCalls={message.tool_calls} isStreaming={false} />
                      )}
                      {message.content && (
                        <MarkdownMessage content={message.content} variant={message.role as 'user' | 'assistant'} />
                      )}
                      </div>
                    </div>

                    {message.role === "assistant" && (
                      <QuickActions message={message} onActionClick={handleQuickAction} />
                    )}

                    <p className="text-xs text-muted-foreground mt-1 px-1">
                      {formatTimestamp(message.timestamp, false)}
                    </p>
                  </div>

                  {message.role === "user" && user && (
                    <Avatar className="h-7 w-7 sm:h-8 sm:w-8 order-2">
                      <AvatarFallback className="bg-muted text-muted-foreground text-[10px] sm:text-xs">
                        {getUserInitials(user.username)}
                      </AvatarFallback>
                    </Avatar>
                  )}
                </div>
              ))}

              {/* Streaming message */}
              {isStreaming && (
                <div className="flex gap-2 sm:gap-3 justify-start">
                  <div className="flex-shrink-0 w-7 h-7 sm:w-8 sm:h-8 rounded-lg bg-foreground flex items-center justify-center">
                    <Sparkles className="h-3.5 w-3.5 sm:h-4 sm:w-4 text-background animate-pulse" />
                  </div>
                  <div className="max-w-[85%] sm:max-w-[80%]">
                    <div className="rounded-2xl px-3 py-2 sm:px-4 sm:py-3 bg-muted text-foreground">
                      <div className="message-bubble-assistant">
                      {streamingThinking && <ThinkingBlock thinking={streamingThinking} />}
                      {streamingToolCalls.length > 0 && (
                        <ToolCallVisualization toolCalls={streamingToolCalls} isStreaming={true} />
                      )}
                      {streamingContent && (
                        <MarkdownMessage content={streamingContent} variant="assistant" />
                      )}
                      {!streamingContent && !streamingThinking && streamingToolCalls.length === 0 && (
                        <div className="flex items-center gap-1">
                          <span key="dot-1" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "0ms" }} />
                          <span key="dot-2" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "150ms" }} />
                          <span key="dot-3" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "300ms" }} />
                        </div>
                      )}
                      </div>
                    </div>
                  </div>
                </div>
              )}

              <div ref={messagesEndRef} />
            </div>
          </div>
        ) : (
          /* Empty chat - shown on /chat/:sessionId with no messages yet */
          <div className="flex-1 flex items-center justify-center px-4">
            <div className="text-center space-y-4 max-w-md">
              <div className="w-16 h-16 rounded-2xl bg-muted flex items-center justify-center mx-auto">
                <Sparkles className="h-8 w-8 text-muted-foreground" />
              </div>
              <div>
                <h3 className="text-lg font-semibold">{t('chat:input.newChat')}</h3>
                <p className="text-sm text-muted-foreground mt-1">
                  {t('chat:input.startNewConversation')}
                </p>
              </div>
            </div>
          </div>
        )}
        </div>

        {/* Input Area */}
        <div className="bg-background px-3 sm:px-4 py-3">
          <div className="max-w-3xl mx-auto">
            {/* Input toolbar with model selector and image preview */}
            <div className="flex items-center gap-2 mb-2">
              {/* Model selector */}
              {llmBackends.length > 0 && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 rounded-lg text-muted-foreground hover:text-foreground text-xs gap-1 max-w-[140px]"
                    >
                      <Zap className="h-3 w-3 shrink-0" />
                      <span className="truncate">
                        {llmBackends.find(b => b.id === activeBackendId)?.name ||
                         llmBackends.find(b => b.id === activeBackendId)?.model ||
                         t('chat:input.selectModel')}
                      </span>
                      <ChevronDown className="h-3 w-3 shrink-0" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-64">
                    <DropdownMenuLabel className="text-xs text-muted-foreground">
                      {t('chat:input.selectLLMModel')}
                    </DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    {llmBackends.map((backend) => (
                      <DropdownMenuItem
                        key={backend.id}
                        onClick={() => activateBackend(backend.id)}
                        className={cn(
                          "flex items-center gap-2 py-2",
                          backend.id === activeBackendId && "bg-muted"
                        )}
                      >
                        <div className={cn(
                          "w-1.5 h-1.5 rounded-full shrink-0",
                          backend.healthy ? "bg-green-500" : "bg-muted-foreground"
                        )} />
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-1.5">
                            <p className="text-sm truncate">{backend.name || backend.model}</p>
                            {/* Capability badges */}
                            {backend.capabilities?.supports_multimodal && (
                              <span className="text-[10px] px-1 rounded bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300 flex items-center" title={t('chat:model.supportsVision')}>
                                <Eye className="h-3 w-3" />
                              </span>
                            )}
                            {backend.capabilities?.supports_thinking && (
                              <span className="text-[10px] px-1 rounded bg-purple-100 dark:bg-purple-900 text-purple-700 dark:text-purple-300 flex items-center" title={t('chat:model.supportsThinking')}>
                                <Brain className="h-3 w-3" />
                              </span>
                            )}
                          </div>
                          <p className="text-[10px] text-muted-foreground truncate">
                            {backend.backend_type} · {backend.model}
                          </p>
                        </div>
                        {backend.id === activeBackendId && (
                          <span className="text-[10px] text-muted-foreground">✓</span>
                        )}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              )}

              {/* Image preview - shown next to model selector */}
              {attachedImages.length > 0 && (
                <div className="flex items-center gap-1.5">
                  {attachedImages.map((image, index) => (
                    <div key={index} className="relative group">
                      <img
                        src={image.data}
                        alt={`Attached ${index + 1}`}
                        className="h-7 w-7 object-cover rounded-md border border-border"
                      />
                      <Button
                        variant="destructive"
                        size="icon"
                        className="absolute -top-1.5 -right-1.5 h-4 w-4 rounded-full p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={() => removeAttachedImage(index)}
                      >
                        <X className="h-2.5 w-2.5" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}

              <div className="flex-1" />
            </div>

            <div className="flex items-center gap-2">
              {/* Image upload button */}
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                className="hidden"
                onChange={handleImageSelect}
                disabled={isStreaming || !supportsMultimodal}
              />
              <Button
                variant="ghost"
                size="icon"
                onClick={() => fileInputRef.current?.click()}
                disabled={isStreaming || !supportsMultimodal}
                className={cn(
                  "h-11 w-11 rounded-full flex-shrink-0",
                  "text-muted-foreground hover:text-foreground hover:bg-muted",
                  "transition-all disabled:opacity-50 disabled:cursor-not-allowed",
                  !supportsMultimodal && "opacity-50"
                )}
                title={supportsMultimodal ? t('chat:model.addImage') : t('chat:model.notSupportImage')}
              >
                {isUploadingImage ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : attachedImages.length > 0 ? (
                  <div className="relative">
                    <ImageIcon className="h-5 w-5" />
                    <span className="absolute -top-1 -right-1 bg-primary text-primary-foreground text-xs rounded-full h-4 w-4 flex items-center justify-center">
                      {attachedImages.length}
                    </span>
                  </div>
                ) : (
                  <ImageIcon className="h-5 w-5" />
                )}
              </Button>

              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t('chat:input.placeholder')}
                rows={1}
                className={cn(
                  "flex-1 px-4 py-2.5 rounded-2xl resize-none text-sm",
                  "bg-muted/50 text-foreground placeholder:text-muted-foreground",
                  "focus:outline-none focus:ring-2 focus:ring-foreground/20",
                  "transition-all max-h-32"
                )}
                style={{ minHeight: "44px", height: "44px" }}
                onInput={(e) => {
                  const target = e.target as HTMLTextAreaElement
                  target.style.height = "44px"
                  target.style.height = Math.min(target.scrollHeight, 128) + "px"
                }}
              />

              <Button
                onClick={handleSend}
                disabled={(!input.trim() && attachedImages.length === 0) || isStreaming}
                className={cn(
                  "h-11 w-11 rounded-full flex-shrink-0",
                  "bg-foreground hover:bg-foreground/90 text-background",
                  "transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                )}
              >
                <Send className="h-5 w-5" />
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
