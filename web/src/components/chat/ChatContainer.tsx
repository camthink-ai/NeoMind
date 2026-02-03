/**
 * ChatContainer - Main chat interface component
 * A simplified, chat-centric UI with minimal navigation
 */

import { useState, useRef, useEffect, useCallback } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { ws } from "@/lib/websocket"
import type { Message, ServerMessage } from "@/types"
import type { StreamProgress as StreamProgressType } from "@/types"
import { SessionDrawer } from "../session/SessionDrawer"
import { InputSuggestions } from "./InputSuggestions"
import { MergedMessageList } from "./MergedMessageList"
import { StreamProgress } from "./StreamProgress"
import {
  Menu,
  Send,
  Paperclip,
  MoreVertical,
  Zap,
  ChevronDown
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { BrandName } from "@/components/shared/BrandName"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu"
import { cn } from "@/lib/utils"

interface ChatContainerProps {
  className?: string
}

export function ChatContainer({ className = "" }: ChatContainerProps) {
  const { t } = useTranslation("chat")
  const navigate = useNavigate()
  // Store state
  const {
    sessionId,
    messages,
    addMessage,
    createSession,
    switchSession,
    user,
    llmBackends,
    activeBackendId,
    activateBackend
  } = useStore()

  // Local state
  const [input, setInput] = useState("")
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  const [streamingToolCalls, setStreamingToolCalls] = useState<any[]>([])
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false)
  const [showSuggestions, setShowSuggestions] = useState(false)
  // Track the ID of the last assistant message for tool call result updates
  const [lastAssistantMessageId, setLastAssistantMessageId] = useState<string | null>(null)

  // Stream progress state (P0.1: Progress tracking)
  const [streamProgress, setStreamProgress] = useState<StreamProgressType>({
    elapsed: 0,
    stage: 'thinking',
    warnings: [],
    remainingTime: 300
  })

  // Current execution plan step from Plan events
  const [currentPlanStep, setCurrentPlanStep] = useState<string>("")

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  const streamingContentRef = useRef("")
  const streamingThinkingRef = useRef("")
  const streamingToolCallsRef = useRef<any[]>([])
  const streamStartRef = useRef<number>(Date.now())
  const isStreamingRef = useRef(false)

  // Auto-scroll to bottom when messages change
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
          // Update progress stage
          setStreamProgress(prev => ({
            ...prev,
            stage: 'thinking',
            elapsed: Math.floor((Date.now() - streamStartRef.current) / 1000)
          }))
          break

        case "Content":
          setIsStreaming(true)
          streamingContentRef.current += (data.content || "")
          setStreamingContent(streamingContentRef.current)
          // Update progress stage
          setStreamProgress(prev => ({
            ...prev,
            stage: 'generating',
            elapsed: Math.floor((Date.now() - streamStartRef.current) / 1000)
          }))
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
          // Update progress stage
          setStreamProgress(prev => ({
            ...prev,
            stage: 'tool_execution',
            elapsed: Math.floor((Date.now() - streamStartRef.current) / 1000)
          }))
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

        // P0.1: Handle progress events from backend
        case "Progress": {
          setStreamProgress({
            elapsed: data.elapsed,
            stage: data.stage,
            warnings: streamProgress.warnings,
            remainingTime: data.remainingTime ?? 300
          })
          // Update plan step from progress message if available
          if (data.message) {
            setCurrentPlanStep(data.message)
          }
          break
        }

        // Handle Plan events - shows execution step
        case "Plan": {
          setCurrentPlanStep(data.step)
          break
        }

        // P0.1: Handle warning events
        case "Warning": {
          setStreamProgress(prev => ({
            ...prev,
            warnings: [...prev.warnings, data.message],
            elapsed: data.elapsed ?? prev.elapsed,
            remainingTime: data.remainingTime ?? prev.remainingTime
          }))
          break
        }

        case "end":
          // Save the complete message
          if (streamingContentRef.current || streamingThinkingRef.current || streamingToolCallsRef.current.length > 0) {
            const messageId = streamingMessageIdRef.current || crypto.randomUUID()
            const completeMessage: Message = {
              id: messageId,
              role: "assistant",
              content: streamingContentRef.current,
              timestamp: Math.floor(Date.now() / 1000), // Use seconds (matches backend)
              thinking: streamingThinkingRef.current || undefined,
              tool_calls: streamingToolCallsRef.current.length > 0 ? streamingToolCallsRef.current : undefined,
            }
            addMessage(completeMessage)
            // Track this message for potential tool call result updates
            setLastAssistantMessageId(messageId)
          }
          // Reset streaming state
          setIsStreaming(false)
          setStreamingContent("")
          setStreamingThinking("")
          setStreamingToolCalls([])
          streamingContentRef.current = ""
          streamingThinkingRef.current = ""
          streamingToolCallsRef.current = []
          streamingMessageIdRef.current = null
          // Reset progress state
          setStreamProgress({
            elapsed: 0,
            stage: 'thinking',
            warnings: [],
            remainingTime: 300
          })
          setCurrentPlanStep("")
          break

        case "Error":
          setIsStreaming(false)
          break

        case "session_created":
        case "session_switched":
          if (data.sessionId) {
            switchSession(data.sessionId)
          }
          break
      }
    }

    // ws.onMessage returns an unsubscribe function - we MUST call it in cleanup
    const unsubscribe = ws.onMessage(handleMessage)
    return () => { void unsubscribe() }
  }, [addMessage, switchSession, lastAssistantMessageId, messages])

  // Initialize session if none exists
  useEffect(() => {
    if (!sessionId) {
      createSession().then((newSessionId) => {
        if (newSessionId) {
          navigate(`/chat/${newSessionId}`)
        }
      })
    }
  }, [sessionId, createSession, navigate])

  // Send message
  const handleSend = async () => {
    const trimmedInput = input.trim()
    if (!trimmedInput || isStreaming) return

    // Add user message directly to current session
    // Backend handles session continuity - no need to create new session
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content: trimmedInput,
      timestamp: Math.floor(Date.now() / 1000), // Use seconds (matches backend)
    }
    addMessage(userMessage)

    // Clear input
    setInput("")
    setShowSuggestions(false)

    // Start streaming
    setIsStreaming(true)
    streamingMessageIdRef.current = crypto.randomUUID()
    streamStartRef.current = Date.now()  // Reset stream start time
    // Reset progress state
    setStreamProgress({
      elapsed: 0,
      stage: 'thinking',
      warnings: [],
      remainingTime: 300
    })
    // Reset last assistant message ID (new response incoming)
    setLastAssistantMessageId(null)

    // Send via WebSocket
    ws.sendMessage(trimmedInput)

    // Focus input after sending
    setTimeout(() => {
      inputRef.current?.focus()
    }, 100)
  }

  // Handle suggestion select
  const handleSuggestionSelect = (prompt: string) => {
    setInput(prompt)
    setShowSuggestions(false)
    inputRef.current?.focus()
  }

  // Handle keyboard shortcuts
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      if (!showSuggestions) {
        handleSend()
      }
    }
    // Show suggestions on slash
    if (e.key === "/" && input.length === 0) {
      setShowSuggestions(true)
    }
    // Escape to hide suggestions
    if (e.key === "Escape") {
      setShowSuggestions(false)
    }
  }

  // Get user initials
  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  // Filter out partial messages - they're handled separately during streaming
  const filteredMessages = messages.filter(msg => !msg.isPartial)

  return (
    <div className={`flex flex-col h-screen bg-[var(--background)] ${className}`}>
      {/* Header */}
      <header className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)] bg-[var(--card)]">
        <div className="flex items-center gap-3">
          {/* Session history trigger */}
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setSessionDrawerOpen(true)}
            className="rounded-full"
          >
            <Menu className="h-5 w-5" />
          </Button>

          {/* Logo and title */}
          <div className="flex items-center gap-2">
            <img src="/logo-square.png" alt="NeoMind" className="w-8 h-8 rounded-lg" />
            <div>
              <h1 className="text-lg font-semibold text-[var(--foreground)]"><BrandName /></h1>
              <p className="text-xs text-muted-foreground">{t("title")}</p>
            </div>
          </div>
        </div>

        {/* User avatar */}
        {user && (
          <Avatar className="h-8 w-8">
            <AvatarFallback className="bg-blue-600 text-white text-xs font-medium">
              {getUserInitials(user.username)}
            </AvatarFallback>
          </Avatar>
        )}
      </header>

      {/* Messages area */}
      <div className="flex-1 overflow-y-auto px-4 py-6">
        <div className="max-w-3xl mx-auto space-y-6">
          {/* Welcome message if no messages */}
          {filteredMessages.length === 0 && !isStreaming && (
            <div className="text-center py-16">
              <img src="/logo-square.png" alt="NeoMind" className="w-16 h-16 rounded-2xl mx-auto mb-6" />
              <h2 className="text-2xl font-semibold mb-2 text-[var(--foreground)]">
                {t("welcome.greeting")}
              </h2>
              <p className="text-muted-foreground mb-8">
                {t("welcome.description")}
              </p>
              <div className="flex flex-wrap justify-center gap-2">
                {[
                  t("welcome.suggestions.checkDevices"),
                  t("welcome.suggestions.createRule"),
                  t("welcome.suggestions.viewAlerts")
                ].map((suggestion) => (
                  <button
                    key={suggestion}
                    onClick={() => setInput(suggestion)}
                    className="px-4 py-2 rounded-lg bg-[var(--card-hover-bg)] hover:bg-[var(--session-item-active)] text-sm text-foreground transition-colors border border-[var(--border)]"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Messages - with automatic merging at render time */}
          <MergedMessageList
            messages={filteredMessages}
            isStreaming={isStreaming}
            streamingContent={streamingContent}
            streamingThinking={streamingThinking}
            streamingToolCalls={streamingToolCalls}
          />

          {/* Stream progress indicator - always show during streaming */}
          {isStreaming && (
            <StreamProgress
              elapsed={streamProgress.elapsed}
              totalDuration={300}
              stage={streamProgress.stage}
              warning={streamProgress.warnings[streamProgress.warnings.length - 1]}
              currentStep={currentPlanStep}
            />
          )}

          {/* Scroll anchor */}
          <div ref={messagesEndRef} />
        </div>
      </div>

      {/* Input area */}
      <div className="border-t border-[var(--border)] bg-[var(--card)] px-4 py-4">
        <div className="max-w-3xl mx-auto">
          <div className="relative">
            {/* Suggestions */}
            {showSuggestions && (
              <InputSuggestions
                input={input}
                onSelect={handleSuggestionSelect}
                visible={showSuggestions}
              />
            )}

            {/* Attachment toolbar */}
            <div className="flex items-center gap-2 mb-2 px-1">
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
                         t("input.selectModel")}
                      </span>
                      <ChevronDown className="h-3 w-3 shrink-0" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-56">
                    <DropdownMenuLabel className="text-xs text-muted-foreground">
                      {t("input.selectLLMModel")}
                    </DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    {llmBackends.map((backend) => (
                      <DropdownMenuItem
                        key={backend.id}
                        onClick={() => activateBackend(backend.id)}
                        className={cn(
                          "flex items-center gap-2",
                          backend.id === activeBackendId && "bg-muted"
                        )}
                      >
                        <div className={cn(
                          "w-1.5 h-1.5 rounded-full shrink-0",
                          backend.healthy ? "bg-green-500" : "bg-muted-foreground"
                        )} />
                        <div className="flex-1 min-w-0">
                          <p className="text-sm truncate">{backend.name || backend.model}</p>
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

              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
              >
                <Paperclip className="h-4 w-4" />
              </Button>
              <span
                className="text-xs text-muted-foreground"
                dangerouslySetInnerHTML={{ __html: t("input.sendHint") }}
              />
              <div className="flex-1" />
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
              >
                <MoreVertical className="h-4 w-4" />
              </Button>
            </div>

            {/* Input */}
            <div className="flex items-end gap-2">
              <div className="flex-1 relative">
                <textarea
                  ref={inputRef}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder={t("input.placeholder")}
                  rows={1}
                  className={cn(
                    "w-full px-4 py-3 rounded-2xl resize-none",
                    "bg-[var(--input-focus-bg)] border border-[var(--border)]",
                    "text-[var(--foreground)] placeholder:text-muted-foreground",
                    "focus:outline-none focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500/50",
                    "transition-all duration-200",
                    "max-h-32"
                  )}
                  style={{
                    minHeight: "48px",
                    height: "auto"
                  }}
                  onInput={(e) => {
                    const target = e.target as HTMLTextAreaElement
                    target.style.height = "auto"
                    target.style.height = Math.min(target.scrollHeight, 128) + "px"
                  }}
                />
              </div>

              {/* Send button */}
              <Button
                onClick={handleSend}
                disabled={!input.trim() || isStreaming}
                className={cn(
                  "h-12 w-12 rounded-full flex-shrink-0",
                  "bg-blue-600 hover:bg-blue-700 text-white",
                  "transition-all duration-200",
                  "disabled:opacity-50 disabled:cursor-not-allowed"
                )}
              >
                <Send className="h-5 w-5" />
              </Button>
            </div>

            {/* Footer hint */}
            <p className="text-xs text-muted-foreground text-center mt-2">
              {t("footer.disclaimer")}
            </p>
          </div>
        </div>
      </div>

      {/* Session drawer */}
      <SessionDrawer
        open={sessionDrawerOpen}
        onClose={() => setSessionDrawerOpen(false)}
        onNewSession={async () => {
          const newSessionId = await createSession()
          if (newSessionId) {
            navigate(`/chat/${newSessionId}`)
          }
        }}
        onSelectSession={async (sessionId) => {
          await switchSession(sessionId)
          navigate(`/chat/${sessionId}`)
        }}
        currentSessionId={sessionId}
      />
    </div>
  )
}
