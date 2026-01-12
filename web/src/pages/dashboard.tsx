import { useState, useEffect, useRef, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { ws } from "@/lib/websocket"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Send, Bot, User, ChevronDown, ChevronUp, Settings, Copy, Check } from "lucide-react"
import { cn } from "@/lib/utils"
import type { Message, ServerMessage, ToolCall } from "@/types"
import { SessionSidebar } from "@/components/chat"

export function DashboardPage() {
  const { t } = useTranslation(['common', 'dashboard'])
  const {
    messages,
    sessionId,
    setSessionId,
    addMessage,
    setWsConnected,
    wsConnected,
    llmSettings,
    fetchLlmSettings,
    setCurrentPage,
    loadSessions,
    // LLM Backend
    llmBackends,
    llmBackendLoading,
    loadBackends,
    activateBackend,
  } = useStore()
  const [input, setInput] = useState("")
  const [isStreaming, setIsStreaming] = useState(false)
  const [expandedThinking, setExpandedThinking] = useState<Set<string>>(new Set())
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  const [streamingToolCalls, setStreamingToolCalls] = useState<ToolCall[]>([])
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const streamingContentRef = useRef("")
  const streamingThinkingRef = useRef("")
  const streamingToolCallsRef = useRef<ToolCall[]>([])

  // Keep refs in sync with state
  useEffect(() => {
    streamingContentRef.current = streamingContent
  }, [streamingContent])
  useEffect(() => {
    streamingThinkingRef.current = streamingThinking
  }, [streamingThinking])
  useEffect(() => {
    streamingToolCallsRef.current = streamingToolCalls
  }, [streamingToolCalls])

  // Fetch LLM settings on mount (once)
  const hasFetchedLlm = useRef(false)
  useEffect(() => {
    if (!hasFetchedLlm.current) {
      hasFetchedLlm.current = true
      fetchLlmSettings()
    }
  }, [])

  // Load LLM backends
  const hasFetchedBackends = useRef(false)
  useEffect(() => {
    if (!hasFetchedBackends.current) {
      hasFetchedBackends.current = true
      loadBackends()
    }
  }, [])

  // Load sessions on mount (once)
  const hasFetchedSessions = useRef(false)
  useEffect(() => {
    if (!hasFetchedSessions.current) {
      hasFetchedSessions.current = true
      loadSessions()
    }
  }, [])

  // Handle backend change
  const handleBackendChange = async (backendId: string) => {
    await activateBackend(backendId)
  }

  // Initialize session
  useEffect(() => {
    if (sessionId) {
      ws.setSessionId(sessionId)
      if (!ws.isConnected()) {
        ws.connect(sessionId)
      }
    }
  }, [sessionId])

  // Setup WebSocket connection handlers
  useEffect(() => {
    const unsubscribeConn = ws.onConnection((connected) => {
      setWsConnected(connected)
    })
    return () => {
      unsubscribeConn()
    }
  }, [setWsConnected])

  // Setup WebSocket message handler
  const handleMessage = useCallback((msg: ServerMessage) => {
    switch (msg.type) {
      case 'system':
        break

      case 'session_created':
      case 'session_switched':
        if (msg.sessionId) {
          setSessionId(msg.sessionId)
          ws.setSessionId(msg.sessionId)
          // Refresh the sessions list to show the new/updated session
          loadSessions()
        }
        break

      case 'Thinking':
        setIsStreaming(true)
        setStreamingThinking((prev) => prev + (msg.content || ""))
        break

      case 'Content':
        setIsStreaming(true)
        setStreamingContent((prev) => prev + (msg.content || ""))
        break

      case 'ToolCallStart':
        setIsStreaming(true)
        setStreamingToolCalls((prev) => [
          ...prev,
          {
            id: crypto.randomUUID(),
            name: msg.tool || "",
            arguments: msg.arguments,
          },
        ])
        break

      case 'ToolCallEnd':
        setStreamingToolCalls((prev) =>
          prev.map((tc) =>
            tc.name === msg.tool
              ? { ...tc, result: msg.result }
              : tc
          )
        )
        break

      case 'Error':
        console.error('Server error:', msg.message)
        setIsStreaming(false)
        break

      case 'end':
        const finalContent = streamingContentRef.current
        const finalThinking = streamingThinkingRef.current
        const finalCalls = streamingToolCallsRef.current

        if (finalContent || finalThinking || finalCalls.length > 0) {
          const assistantMsg: Message = {
            id: crypto.randomUUID(),
            role: "assistant",
            content: finalContent,
            timestamp: Date.now(),
            thinking: finalThinking || undefined,
            tool_calls: finalCalls.length > 0 ? finalCalls : undefined,
          }
          addMessage(assistantMsg)
        }

        setStreamingContent("")
        setStreamingThinking("")
        setStreamingToolCalls([])
        setIsStreaming(false)
        break

      case 'response':
        if (msg.content) {
          const assistantMsg: Message = {
            id: crypto.randomUUID(),
            role: "assistant",
            content: msg.content,
            timestamp: Date.now(),
          }
          addMessage(assistantMsg)
        }
        setIsStreaming(false)
        break

      case 'device_update':
        break
    }
  }, [addMessage, setSessionId])

  // Register message handler once
  useEffect(() => {
    const unsubscribe = ws.onMessage(handleMessage)
    return () => {
      unsubscribe()
    }
  }, [handleMessage])

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages, streamingContent, streamingThinking, streamingToolCalls])

  const handleSend = () => {
    if (!input.trim() || isStreaming) return

    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content: input,
      timestamp: Date.now(),
    }

    addMessage(userMsg)
    ws.sendMessage(input)
    setInput("")
    setIsStreaming(true)
  }

  const toggleThinking = (msgId: string) => {
    setExpandedThinking((prev) => {
      const next = new Set(prev)
      if (next.has(msgId)) {
        next.delete(msgId)
      } else {
        next.add(msgId)
      }
      return next
    })
  }

  const handleCopy = (content: string, id: string) => {
    navigator.clipboard.writeText(content)
    setCopiedId(id)
    setTimeout(() => setCopiedId(null), 2000)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  // Show LLM setup prompt if no LLM is configured
  if (!llmSettings) {
    return (
      <div className="flex h-full flex-row">
        <SessionSidebar onNewChat={() => setInput("")} />
        <div className="flex h-full flex-1 flex-col">
          <div className="flex h-[calc(100vh-100px)] items-center justify-center">
            <div className="text-center max-w-md">
              <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-xl bg-muted">
                <Settings className="h-8 w-8 text-muted-foreground" />
              </div>
              <h2 className="mb-3 text-lg font-semibold">{t('dashboard:llmNotConfigured')}</h2>
              <p className="mb-6 text-sm text-muted-foreground">
                {t('dashboard:llmNotConfiguredDesc')}
              </p>
              <Button onClick={() => setCurrentPage('settings')}>
                <Settings className="mr-2 h-4 w-4" />
                {t('dashboard:goToSettings')}
              </Button>
            </div>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-row">
      <SessionSidebar onNewChat={() => setInput("")} />
      <div className="flex h-full flex-1 flex-col">
        <ScrollArea className="flex-1">
          <div ref={scrollRef} className="p-4">
          {messages.length === 0 && !isStreaming && (
            <div className="flex h-[calc(100vh-200px)] items-center justify-center">
              <div className="text-center">
                <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-muted">
                  <Bot className="h-6 w-6 text-muted-foreground" />
                </div>
                <h3 className="mb-2 text-sm font-medium">{t('dashboard:startChat')}</h3>
                <p className="text-xs text-muted-foreground mb-4">
                  {wsConnected ? t('dashboard:chatReady') : t('dashboard:connecting')}
                </p>
              </div>
            </div>
          )}

          <div className="space-y-4">
            {messages.map((msg) => (
              <div
                key={msg.id}
                className={cn(
                  "flex gap-3",
                  msg.role === "user" ? "justify-end" : "justify-start"
                )}
              >
                {msg.role === "assistant" && (
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted">
                    <Bot className="h-4 w-4" />
                  </div>
                )}

                <div className="max-w-[80%]">
                  {/* Thinking */}
                  {msg.role === "assistant" && msg.thinking && (
                    <div className="mb-2 rounded-md bg-muted/50 px-3 py-2 text-xs">
                      <button
                        onClick={() => toggleThinking(msg.id)}
                        className="flex items-center gap-1 font-medium text-muted-foreground"
                      >
                        思考过程
                        {expandedThinking.has(msg.id) ? (
                          <ChevronUp className="h-3 w-3" />
                        ) : (
                          <ChevronDown className="h-3 w-3" />
                        )}
                      </button>
                      {expandedThinking.has(msg.id) && (
                        <div className="mt-2 whitespace-pre-wrap text-muted-foreground">
                          {msg.thinking}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Tool Calls */}
                  {msg.role === "assistant" && msg.tool_calls && msg.tool_calls.length > 0 && (
                    <div className="mb-2 rounded-md bg-muted/50 px-3 py-2 text-xs">
                      <div className="font-medium text-muted-foreground mb-1">工具调用</div>
                      <div className="space-y-1">
                        {msg.tool_calls.map((tc, i) => (
                          <div key={i} className="flex items-center gap-2">
                            <span className="font-mono bg-background px-2 py-0.5 rounded">{tc.name}</span>
                            <span className="text-muted-foreground">
                              {tc.result ? '✓ 完成' : '执行中'}
                            </span>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Message */}
                  <div
                    className={cn(
                      "rounded-lg px-3 py-2 text-sm",
                      msg.role === "user"
                        ? "bg-primary text-primary-foreground"
                        : "bg-muted"
                    )}
                  >
                    <p className="whitespace-pre-wrap">{msg.content}</p>
                    {msg.role === "assistant" && (
                      <button
                        onClick={() => handleCopy(msg.content, msg.id)}
                        className="mt-2 text-xs text-muted-foreground hover:text-foreground"
                      >
                        {copiedId === msg.id ? <Check className="h-3 w-3 inline" /> : <Copy className="h-3 w-3 inline" />}
                      </button>
                    )}
                  </div>
                </div>

                {msg.role === "user" && (
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted">
                    <User className="h-4 w-4" />
                  </div>
                )}
              </div>
            ))}

            {/* Streaming Message */}
            {isStreaming && (streamingContent || streamingThinking || streamingToolCalls.length > 0) && (
              <div className="flex gap-3">
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted">
                  <Bot className="h-4 w-4" />
                </div>
                <div className="max-w-[80%]">
                  {streamingThinking && (
                    <div className="mb-2 rounded-md bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
                      思考过程
                      <p className="mt-2 whitespace-pre-wrap">{streamingThinking}</p>
                    </div>
                  )}
                  {streamingToolCalls.length > 0 && (
                    <div className="mb-2 rounded-md bg-muted/50 px-3 py-2 text-xs">
                      工具调用
                      {streamingToolCalls.map((tc, i) => (
                        <div key={i} className="flex items-center gap-2 mt-1">
                          <span className="font-mono bg-background px-2 py-0.5 rounded">{tc.name}</span>
                          <span className="text-muted-foreground">执行中</span>
                        </div>
                      ))}
                    </div>
                  )}
                  <div className="rounded-lg px-3 py-2 text-sm bg-muted">
                    <p className="whitespace-pre-wrap">
                      {streamingContent}
                      <span className="inline-block w-1 h-4 bg-foreground animate-pulse ml-0.5" />
                    </p>
                  </div>
                </div>
              </div>
            )}

            {/* Loading indicator */}
            {isStreaming && !streamingContent && !streamingThinking && streamingToolCalls.length === 0 && (
              <div className="flex gap-3">
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-muted">
                  <Bot className="h-4 w-4" />
                </div>
                <div className="flex items-center px-3 py-2 bg-muted rounded-lg text-sm">
                  <span className="flex gap-1">
                    <span className="w-1 h-1 bg-foreground rounded-full animate-bounce" style={{ animationDelay: "0ms" }} />
                    <span className="w-1 h-1 bg-foreground rounded-full animate-bounce" style={{ animationDelay: "150ms" }} />
                    <span className="w-1 h-1 bg-foreground rounded-full animate-bounce" style={{ animationDelay: "300ms" }} />
                  </span>
                </div>
              </div>
            )}
          </div>
        </div>
      </ScrollArea>

      <div className="border-t p-3">
        <div className="flex gap-2 items-center">
          {/* LLM Backend Selector - always visible */}
          <Select
            value={llmBackends?.find(b => b.is_active)?.id || ""}
            onValueChange={handleBackendChange}
            disabled={isStreaming || llmBackendLoading || !llmBackends || llmBackends.length === 0}
          >
            <SelectTrigger className="h-9 w-[140px] text-xs shrink-0">
              <SelectValue placeholder="选择后端" />
            </SelectTrigger>
            <SelectContent>
              {llmBackends && llmBackends.length > 0 ? (
                llmBackends.map((backend) => (
                  <SelectItem key={backend.id} value={backend.id}>
                    <div className="flex items-center gap-2">
                      <span className="text-xs">{backend.name}</span>
                      {backend.is_active && (
                        <span className="text-xs text-green-600">•</span>
                      )}
                    </div>
                  </SelectItem>
                ))
              ) : (
                <div className="p-2 text-xs text-muted-foreground text-center">
                  暂无后端
                </div>
              )}
            </SelectContent>
          </Select>

          <Input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t('dashboard:messagePlaceholder')}
            disabled={isStreaming}
            className="flex-1"
          />
          <Button
            onClick={handleSend}
            disabled={!input.trim() || isStreaming}
            size="icon"
            className="shrink-0"
          >
            <Send className="h-4 w-4" />
          </Button>
        </div>
      </div>
      </div>
    </div>
  )
}
