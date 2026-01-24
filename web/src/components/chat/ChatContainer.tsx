/**
 * ChatContainer - Main chat interface component
 * A simplified, chat-centric UI with minimal navigation
 */

import { useState, useRef, useEffect, useCallback } from "react"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { ws } from "@/lib/websocket"
import type { Message, ServerMessage } from "@/types"
import { SessionDrawer } from "../session/SessionDrawer"
import { InputSuggestions } from "./InputSuggestions"
import { MergedMessageList } from "./MergedMessageList"
import {
  Menu,
  Send,
  Paperclip,
  MoreVertical,
  Sparkles,
  Zap,
  ChevronDown
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
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

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  const streamingContentRef = useRef("")
  const streamingThinkingRef = useRef("")
  const streamingToolCallsRef = useRef<any[]>([])

  // Auto-scroll to bottom when messages change
  const scrollToBottom = useCallback(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [])

  useEffect(() => {
    scrollToBottom()
  }, [messages, streamingContent, scrollToBottom])

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
          setStreamingToolCalls(prev =>
            prev.map(tc =>
              tc.name === data.tool
                ? { ...tc, result: data.result }
                : tc
            )
          )
          break
        }

        case "end":
          // Save the complete message
          if (streamingContentRef.current || streamingThinkingRef.current || streamingToolCallsRef.current.length > 0) {
            const completeMessage: Message = {
              id: streamingMessageIdRef.current || crypto.randomUUID(),
              role: "assistant",
              content: streamingContentRef.current,
              timestamp: Date.now(),
              thinking: streamingThinkingRef.current || undefined,
              tool_calls: streamingToolCallsRef.current.length > 0 ? streamingToolCallsRef.current : undefined,
            }
            addMessage(completeMessage)
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
  }, [addMessage, switchSession])

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

    // Check if we're viewing a historical session with existing messages
    // If so, create a new session first to avoid adding message to old session
    const hasExistingMessages = messages.length > 0

    if (hasExistingMessages) {
      // Create new session first - this will clear messages and update sessionId
      const newSessionId = await createSession()
      if (newSessionId) {
        // Navigate to the new session URL
        navigate(`/chat/${newSessionId}`)
      }
    }

    // Add user message to the (now empty or current) session
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content: trimmedInput,
      timestamp: Date.now(),
    }
    addMessage(userMessage)

    // Clear input
    setInput("")
    setShowSuggestions(false)

    // Start streaming
    setIsStreaming(true)
    streamingMessageIdRef.current = crypto.randomUUID()

    // Send via WebSocket (now using the correct session)
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
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center">
              <Sparkles className="h-4 w-4 text-white" />
            </div>
            <div>
              <h1 className="text-lg font-semibold text-[var(--foreground)]">NeoTalk</h1>
              <p className="text-xs text-muted-foreground">智能物联网助手</p>
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
              <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center mx-auto mb-6">
                <Sparkles className="h-8 w-8 text-white" />
              </div>
              <h2 className="text-2xl font-semibold mb-2 text-[var(--foreground)]">
                你好！我是 NeoTalk
              </h2>
              <p className="text-muted-foreground mb-8">
                智能物联网助手，帮你管理设备和自动化
              </p>
              <div className="flex flex-wrap justify-center gap-2">
                {["查看设备状态", "创建自动化规则", "查看告警"].map((suggestion) => (
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
                         '选择模型'}
                      </span>
                      <ChevronDown className="h-3 w-3 shrink-0" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-56">
                    <DropdownMenuLabel className="text-xs text-muted-foreground">
                      选择 LLM 模型
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
              <span className="text-xs text-muted-foreground">
                按 <kbd className="px-1 py-0.5 rounded bg-[var(--muted)]">/</kbd> 显示建议
              </span>
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
                  placeholder="输入消息... (Shift+Enter 换行)"
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
              NeoTalk 使用 AI 提供智能物联网管理 • 回复可能不准确
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
