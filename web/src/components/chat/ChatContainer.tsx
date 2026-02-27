/**
 * ChatContainer - Main chat interface component
 * A simplified, chat-centric UI with minimal navigation
 */

import { useState, useRef, useEffect, useCallback, useReducer } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { generateId } from "@/lib/id"
import { ws } from "@/lib/websocket"
import type { Message, ServerMessage } from "@/types"
import type { StreamProgress as StreamProgressType } from "@/types"
import { filterPartialMessages } from "@/lib/messageUtils"
import {
  selectSessionId,
  selectMessages,
  selectUser,
  selectLlmBackendState,
  selectChatActions,
} from "@/store/selectors"
import { SessionDrawer } from "../session/SessionDrawer"
import { InputSuggestions } from "./InputSuggestions"
import { MergedMessageList } from "./MergedMessageList"
import { StreamProgress } from "./StreamProgress"
import { forceViewportReset } from "@/hooks/useVisualViewport"
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

// Performance optimization: useReducer to batch stream state updates
// This prevents multiple re-renders during streaming response
interface StreamState {
  isStreaming: boolean
  streamingContent: string
  streamingThinking: string
  streamingToolCalls: any[]
  streamProgress: StreamProgressType
  currentPlanStep: string
  errorMessage: string | null
}

type StreamAction =
  | { type: 'START_STREAM' }
  | { type: 'THINKING'; content: string }
  | { type: 'CONTENT'; content: string }
  | { type: 'TOOL_START'; tool: string; arguments?: any }
  | { type: 'TOOL_END'; tool: string; result: any }
  | { type: 'PROGRESS'; progress: Partial<StreamProgressType> }
  | { type: 'PLAN'; step: string }
  | { type: 'WARNING'; message: string }
  | { type: 'END_STREAM' }
  | { type: 'ERROR'; message?: string }
  | { type: 'RESET' }
  | { type: 'CLEAR_ERROR' }

const initialStreamState: StreamState = {
  isStreaming: false,
  streamingContent: "",
  streamingThinking: "",
  streamingToolCalls: [],
  streamProgress: {
    elapsed: 0,
    stage: 'thinking',
    warnings: [],
    remainingTime: 300
  },
  currentPlanStep: "",
  errorMessage: null
}

function streamReducer(state: StreamState, action: StreamAction): StreamState {
  switch (action.type) {
    case 'START_STREAM':
      return { ...state, isStreaming: true }

    case 'THINKING':
      return {
        ...state,
        isStreaming: true,
        streamingThinking: state.streamingThinking + action.content,
        streamProgress: {
          ...state.streamProgress,
          stage: 'thinking',
          elapsed: Math.floor((Date.now() - Date.now()) / 1000) // Simplified
        }
      }

    case 'CONTENT':
      return {
        ...state,
        isStreaming: true,
        streamingContent: state.streamingContent + action.content,
        streamProgress: {
          ...state.streamProgress,
          stage: 'generating',
        }
      }

    case 'TOOL_START':
      return {
        ...state,
        isStreaming: true,
        streamingToolCalls: [
          ...state.streamingToolCalls,
          {
            id: generateId(),
            name: action.tool,
            arguments: action.arguments,
            result: null
          }
        ],
        streamProgress: {
          ...state.streamProgress,
          stage: 'tool_execution',
        }
      }

    case 'TOOL_END':
      return {
        ...state,
        streamingToolCalls: state.streamingToolCalls.map(tc =>
          tc.name === action.tool
            ? { ...tc, result: action.result }
            : tc
        )
      }

    case 'PROGRESS':
      return {
        ...state,
        streamProgress: {
          ...state.streamProgress,
          ...action.progress,
          warnings: action.progress.warnings ?? state.streamProgress.warnings
        }
      }

    case 'PLAN':
      return {
        ...state,
        currentPlanStep: action.step
      }

    case 'WARNING':
      return {
        ...state,
        streamProgress: {
          ...state.streamProgress,
          warnings: [...state.streamProgress.warnings, action.message]
        }
      }

    case 'END_STREAM':
      return {
        ...initialStreamState,
        isStreaming: false
      }

    case 'ERROR':
      return {
        ...initialStreamState,
        isStreaming: false,
        errorMessage: action.message || null
      }

    case 'CLEAR_ERROR':
      return {
        ...state,
        errorMessage: null
      }

    case 'RESET':
      return initialStreamState

    default:
      return state
  }
}

export function ChatContainer({ className = "" }: ChatContainerProps) {
  const { t } = useTranslation("chat")
  const navigate = useNavigate()

  // Store state - using optimized selectors to prevent unnecessary re-renders
  // Single values don't need shallow comparison
  const sessionId = useStore(selectSessionId)
  const messages = useStore(selectMessages)
  const user = useStore(selectUser)

  // Arrays/objects use shallow comparison to avoid re-renders on reference changes
  const { llmBackends, activeBackendId } = useStore(selectLlmBackendState, shallow)

  // Actions are stable functions, no need for shallow comparison
  const { addMessage, createSession, switchSession, activateBackend } = useStore(selectChatActions)

  // Store reference for WebSocket handlers (avoids dependency issues)
  const storeRef = useRef(useStore())
  useEffect(() => {
    storeRef.current = useStore()
  }, [])

  // Performance optimization: useReducer for stream state to batch updates
  const [streamState, dispatch] = useReducer(streamReducer, initialStreamState)

  // Local state
  const [input, setInput] = useState("")
  const [sessionDrawerOpen, setSessionDrawerOpen] = useState(false)
  const [showSuggestions, setShowSuggestions] = useState(false)

  // Track the ID of the current streaming message (generated once per stream)
  const [currentStreamMessageId, setCurrentStreamMessageId] = useState<string | null>(null)
  // Track the ID of the last completed assistant message for tool call result updates
  const [lastAssistantMessageId, setLastAssistantMessageId] = useState<string | null>(null)

  // Refs for tracking stream content during reducer updates
  const streamStartRef = useRef<number>(Date.now())
  const isStreamingRef = useRef(false)
  const messagesRef = useRef<Message[]>([])  // Store latest messages for WebSocket handlers

  // Auto-scroll to bottom when messages change
  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [])

  // Ref for messages end and input
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    scrollToBottom()
  }, [messages, streamState.streamingContent, scrollToBottom])

  // Sync isStreaming ref with state
  useEffect(() => {
    isStreamingRef.current = streamState.isStreaming
  }, [streamState.isStreaming])

  // Sync messages to ref for WebSocket handlers (prevents re-subscription on every message)
  useEffect(() => {
    messagesRef.current = messages
  }, [messages])

  // Handle WebSocket events - optimized with useReducer for batched state updates
  useEffect(() => {
    // Track stream content for final message assembly
    let streamingContentAccumulator = ""
    let streamingThinkingAccumulator = ""
    let streamingToolCallsAccumulator: any[] = []

    const handleMessage = (data: ServerMessage) => {
      switch (data.type) {
        case "Thinking":
          streamingThinkingAccumulator += (data.content || "")
          dispatch({ type: 'THINKING', content: data.content || "" })
          break

        case "Content":
          streamingContentAccumulator += (data.content || "")
          dispatch({ type: 'CONTENT', content: data.content || "" })
          break

        case "ToolCallStart":
          dispatch({ type: 'TOOL_START', tool: data.tool, arguments: data.arguments })
          // Track locally for message assembly
          streamingToolCallsAccumulator.push({
            id: generateId(),
            name: data.tool,
            arguments: data.arguments,
            result: null
          })
          break

        case "ToolCallEnd":
          // Update local tracking
          streamingToolCallsAccumulator = streamingToolCallsAccumulator.map(tc =>
            tc.name === data.tool ? { ...tc, result: data.result } : tc
          )
          dispatch({ type: 'TOOL_END', tool: data.tool, result: data.result })

          // If not streaming (stream ended before tool execution),
          // update the saved assistant message's tool_calls
          if (!isStreamingRef.current && lastAssistantMessageId) {
            const lastMessage = messagesRef.current.find(m => m.id === lastAssistantMessageId)
            if (lastMessage && lastMessage.role === "assistant" && lastMessage.tool_calls) {
              const updatedToolCalls = lastMessage.tool_calls.map(tc =>
                tc.name === data.tool ? { ...tc, result: data.result } : tc
              )
              addMessage({
                ...lastMessage,
                tool_calls: updatedToolCalls
              })
            }
          }
          break

        case "Progress":
          dispatch({
            type: 'PROGRESS',
            progress: {
              elapsed: data.elapsed,
              stage: data.stage,
              remainingTime: data.remainingTime ?? 300
            }
          })
          // Update plan step from progress message if available
          if (data.message) {
            dispatch({ type: 'PLAN', step: data.message })
          }
          break

        case "Plan":
          dispatch({ type: 'PLAN', step: data.step })
          break

        case "Warning":
          dispatch({ type: 'WARNING', message: data.message })
          break

        case "end":
          // Save the complete message using the ID generated at stream start
          if (streamingContentAccumulator || streamingThinkingAccumulator || streamingToolCallsAccumulator.length > 0) {
            const messageId = currentStreamMessageId || generateId()
            const completeMessage: Message = {
              id: messageId,
              role: "assistant",
              content: streamingContentAccumulator,
              timestamp: Math.floor(Date.now() / 1000),
              thinking: streamingThinkingAccumulator || undefined,
              tool_calls: streamingToolCallsAccumulator.length > 0 ? streamingToolCallsAccumulator : undefined,
            }
            addMessage(completeMessage)
            setLastAssistantMessageId(messageId)
          }
          // Reset local accumulators
          streamingContentAccumulator = ""
          streamingThinkingAccumulator = ""
          streamingToolCallsAccumulator = []
          setCurrentStreamMessageId(null)
          dispatch({ type: 'END_STREAM' })
          break

        case "Error":
          dispatch({ type: 'ERROR', message: data.message })
          break

        case "session_created":
        case "session_switched":
          if (data.sessionId) {
            storeRef.current.switchSession(data.sessionId)
          }
          break
      }
    }

    // ws.onMessage returns an unsubscribe function - we MUST call it in cleanup
    const unsubscribe = ws.onMessage(handleMessage)
    return () => { void unsubscribe() }
  }, [lastAssistantMessageId, currentStreamMessageId])  // Stable dependencies only

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
    if (!trimmedInput || streamState.isStreaming) return

    // Add user message directly to current session
    // Backend handles session continuity - no need to create new session
    const userMessage: Message = {
      id: generateId(),
      role: "user",
      content: trimmedInput,
      timestamp: Math.floor(Date.now() / 1000), // Use seconds (matches backend)
    }
    addMessage(userMessage)

    // Clear input and reset textarea height
    setInput("")
    setShowSuggestions(false)

    // Reset textarea height to initial state
    if (inputRef.current) {
      inputRef.current.style.height = "auto"
    }

    // Start streaming - generate message ID once for the entire stream
    dispatch({ type: 'START_STREAM' })
    const newMessageId = generateId()
    setCurrentStreamMessageId(newMessageId)
    streamStartRef.current = Date.now()  // Reset stream start time
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

  // Handle tap outside to dismiss keyboard (mobile)
  const handleBackdropClick = () => {
    forceViewportReset()
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  }

  // Get user initials
  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  // Filter out partial messages - they're handled separately during streaming
  // Use shared utility for consistency
  const filteredMessages = filterPartialMessages(messages)

  return (
    <div className={`flex flex-col h-full bg-[var(--background)] ${className}`}>
      {/* Header */}
      <header className="flex items-center justify-between px-4 py-3 border-b border-[var(--border)] bg-[var(--card)] flex-shrink-0">
        <div className="flex items-center gap-3">
          {/* Session history trigger */}
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setSessionDrawerOpen(true)}
            className="rounded-full"
            aria-label="Open chat history"
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
      <div
        className="flex-1 overflow-y-auto px-4 py-6 min-h-0"
        onClick={(e) => {
          // If clicking outside interactive elements, dismiss keyboard
          if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return
          handleBackdropClick()
        }}
      >
        <div className="max-w-3xl mx-auto space-y-6">
          {/* Welcome message if no messages */}
          {filteredMessages.length === 0 && !streamState.isStreaming && (
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
            isStreaming={streamState.isStreaming}
            streamingContent={streamState.streamingContent}
            streamingThinking={streamState.streamingThinking}
            streamingToolCalls={streamState.streamingToolCalls}
          />

          {/* Stream progress indicator - always show during streaming */}
          {streamState.isStreaming && (
            <StreamProgress
              elapsed={streamState.streamProgress.elapsed}
              totalDuration={300}
              stage={streamState.streamProgress.stage}
              warning={streamState.streamProgress.warnings[streamState.streamProgress.warnings.length - 1]}
              currentStep={streamState.currentPlanStep}
            />
          )}

          {/* Error message display */}
          {streamState.errorMessage && (
            <div className="flex items-center gap-3 p-4 rounded-lg bg-destructive/10 border border-destructive/20 text-sm">
              <div className="w-2 h-2 rounded-full bg-destructive shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="font-medium text-destructive">{t("errors.llmError")}</p>
                <p className="text-muted-foreground text-xs mt-1 break-all">{streamState.errorMessage}</p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="shrink-0 text-xs"
                onClick={() => dispatch({ type: 'CLEAR_ERROR' })}
              >
                {t("errors.dismiss")}
              </Button>
            </div>
          )}

          {/* Scroll anchor */}
          <div ref={messagesEndRef} />
        </div>
      </div>

      {/* Input area */}
      <div className="border-t border-[var(--border)] bg-[var(--card)] px-3 sm:px-4 py-3 sm:py-4 pb-6 sm:pb-4 safe-bottom flex-shrink-0">
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
                {/* Streaming indicator overlay */}
                {streamState.isStreaming && (
                  <div className="absolute left-4 top-1/2 -translate-y-1/2 z-10 flex items-center gap-1.5 pointer-events-none">
                    <span className="w-1.5 h-1.5 rounded-full bg-blue-500 animate-ping" />
                    <span className="text-xs text-muted-foreground">{t("status.typing", "正在输入...")}</span>
                  </div>
                )}
                <textarea
                  ref={inputRef}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder={streamState.isStreaming ? t("status.wait", "请等待...") : t("input.placeholder")}
                  rows={1}
                  disabled={streamState.isStreaming}
                  className={cn(
                    "w-full px-3 sm:px-4 py-3 rounded-2xl resize-none text-base",
                    "bg-[var(--input-focus-bg)] border border-[var(--border)]",
                    "text-[var(--foreground)] placeholder:text-muted-foreground",
                    "focus:outline-none focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500/50",
                    "transition-all duration-200",
                    "max-h-32 scroll-mb-32",
                    streamState.isStreaming && "opacity-60 cursor-wait"
                  )}
                  style={{
                    minHeight: "44px",
                    height: "auto"
                  }}
                  onInput={(e) => {
                    const target = e.target as HTMLTextAreaElement
                    target.style.height = "auto"
                    target.style.height = Math.min(target.scrollHeight, 128) + "px"
                  }}
                />
              </div>

              {/* Send button - shows loading spinner when streaming */}
              <Button
                onClick={handleSend}
                disabled={!input.trim() || streamState.isStreaming}
                className={cn(
                  "h-12 w-12 rounded-full flex-shrink-0",
                  "bg-blue-600 hover:bg-blue-700 text-white",
                  "transition-all duration-200",
                  "disabled:opacity-50 disabled:cursor-not-allowed",
                  streamState.isStreaming && "relative overflow-hidden"
                )}
              >
                {streamState.isStreaming ? (
                  <>
                    <span className="absolute inset-0 bg-blue-600/20 animate-ping" />
                    <Send className="h-5 w-5 relative" />
                  </>
                ) : (
                  <Send className="h-5 w-5" />
                )}
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
