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
import { Send, Bot, User, ChevronDown, ChevronUp, Settings, Copy, Check, CheckCircle2, Wrench, Loader2 } from "lucide-react"
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
  const [expandedToolResults, setExpandedToolResults] = useState<Set<string>>(new Set())
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  const [streamingToolCalls, setStreamingToolCalls] = useState<ToolCall[]>([])
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const streamingContentRef = useRef("")
  const streamingThinkingRef = useRef("")
  const streamingToolCallsRef = useRef<ToolCall[]>([])

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

  // Auto-create session when connected and no session exists
  useEffect(() => {
    if (wsConnected && !sessionId) {
      // Create a new session via API
      fetch('/api/sessions', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
      })
        .then(res => res.json())
        .then(data => {
          if (data.data?.id) {
            setSessionId(data.data.id)
            ws.setSessionId(data.data.id)
          }
        })
        .catch(err => {
          console.error('Failed to create session:', err)
        })
    }
  }, [wsConnected, sessionId, setSessionId])

  // Reset streaming states when sessionId changes
  useEffect(() => {
    // Reset all streaming states when switching sessions
    setIsStreaming(false)
    setStreamingContent("")
    setStreamingThinking("")
    setStreamingToolCalls([])
  }, [sessionId])

  // Setup WebSocket message handler
  // Use ref to avoid re-subscription when addMessage changes
  const addMessageRef = useRef(addMessage)
  const loadSessionsRef = useRef(loadSessions)
  const setSessionIdRef = useRef(setSessionId)

  useEffect(() => {
    addMessageRef.current = addMessage
    loadSessionsRef.current = loadSessions
    setSessionIdRef.current = setSessionId
  }, [addMessage, loadSessions, setSessionId])

  const handleMessage = useCallback((msg: ServerMessage) => {
    switch (msg.type) {
      case 'system':
        break

      case 'session_created':
      case 'session_switched':
        if (msg.sessionId) {
          // Update BOTH React state and WebSocket instance
          setSessionIdRef.current(msg.sessionId)
          ws.setSessionId(msg.sessionId)
          // Refresh the sessions list to show the new/updated session
          loadSessionsRef.current()
        }
        break

      case 'Thinking':
        setIsStreaming(true)
        // Update ref synchronously to avoid stale data in end event
        streamingThinkingRef.current += (msg.content || "")
        setStreamingThinking(streamingThinkingRef.current)
        break

      case 'Content':
        setIsStreaming(true)
        // Update ref synchronously to avoid stale data in end event
        streamingContentRef.current += (msg.content || "")
        setStreamingContent(streamingContentRef.current)
        break

      case 'ToolCallStart':
        setIsStreaming(true)
        const newToolCall: ToolCall = {
          id: crypto.randomUUID(),
          name: msg.tool || "",
          arguments: msg.arguments,
        }
        streamingToolCallsRef.current.push(newToolCall)
        setStreamingToolCalls([...streamingToolCallsRef.current])
        break

      case 'ToolCallEnd':
        streamingToolCallsRef.current = streamingToolCallsRef.current.map((tc) =>
          tc.name === msg.tool
            ? { ...tc, result: msg.result }
            : tc
        )
        setStreamingToolCalls(streamingToolCallsRef.current)
        break

      case 'Error':
        console.error('Server error:', msg.message)
        setIsStreaming(false)
        break

      case 'end':
        // Use refs directly since they're updated synchronously now
        const finalContent = streamingContentRef.current
        const finalThinking = streamingThinkingRef.current
        const finalCalls = streamingToolCallsRef.current

        console.log('[dashboard] end event received', {
          contentLength: finalContent.length,
          thinkingLength: finalThinking.length,
          toolCallsCount: finalCalls.length,
        })

        if (finalContent || finalThinking || finalCalls.length > 0) {
          const assistantMsg: Message = {
            id: crypto.randomUUID(),
            role: "assistant",
            content: finalContent,
            timestamp: Date.now(),
            thinking: finalThinking || undefined,
            tool_calls: finalCalls.length > 0 ? finalCalls : undefined,
          }
          addMessageRef.current(assistantMsg)
        } else {
          console.warn('[dashboard] end event received but no content to save')
        }

        // Clear refs immediately
        streamingContentRef.current = ""
        streamingThinkingRef.current = ""
        streamingToolCallsRef.current = []

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
          addMessageRef.current(assistantMsg)
        }
        setIsStreaming(false)
        break

      case 'device_update':
        break
    }
  }, []) // No dependencies - use refs inside

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

  const toggleToolResult = (key: string) => {
    setExpandedToolResults((prev) => {
      const next = new Set(prev)
      if (next.has(key)) {
        next.delete(key)
      } else {
        next.add(key)
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
                        {t('dashboard:thinkingProcess')}
                        {expandedThinking.has(msg.id) ? (
                          <ChevronUp className="h-3 w-3" />
                        ) : (
                          <ChevronDown className="h-3 w-3" />
                        )}
                      </button>
                      {expandedThinking.has(msg.id) && (
                        <div className="mt-2 max-h-[200px] overflow-y-auto whitespace-pre-wrap text-muted-foreground scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
                          {msg.thinking}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Tool Calls - Improved Visual Flow */}
                  {msg.role === "assistant" && msg.tool_calls && msg.tool_calls.length > 0 && (
                    <div className="mb-3 rounded-lg border border-border/50 bg-muted/30 overflow-hidden">
                      {/* Header */}
                      <div className="flex items-center gap-2 px-3 py-2 bg-muted/50 border-b border-border/50">
                        <Wrench className="h-3.5 w-3.5 text-muted-foreground" />
                        <span className="font-medium text-xs text-muted-foreground">
                          {t('dashboard:toolCalls')} ({msg.tool_calls.length})
                        </span>
                      </div>

                      {/* Tool List with Timeline */}
                      <div className="divide-y divide-border/30">
                        {msg.tool_calls.map((tc, i) => {
                          const resultKey = `${msg.id}-${i}`
                          const isExpanded = expandedToolResults.has(resultKey)
                          const hasResult = tc.result !== undefined && tc.result !== null

                          return (
                            <div key={i} className="px-3 py-2">
                              {/* Tool Name Row */}
                              <div className="flex items-center justify-between gap-2">
                                <div className="flex items-center gap-2 flex-1 min-w-0">
                                  {/* Timeline indicator */}
                                  <div className="flex flex-col items-center">
                                    <div className={`h-5 w-5 rounded-full flex items-center justify-center ${
                                      hasResult ? 'bg-green-500/20 text-green-600' : 'bg-amber-500/20 text-amber-600'
                                    }`}>
                                      {hasResult ? (
                                        <CheckCircle2 className="h-3 w-3" />
                                      ) : (
                                        <Loader2 className="h-3 w-3 animate-spin" />
                                      )}
                                    </div>
                                    {i < (msg.tool_calls?.length ?? 0) - 1 && (
                                      <div className="w-px h-4 bg-border/50 mt-1" />
                                    )}
                                  </div>

                                  {/* Tool info */}
                                  <div className="flex items-center gap-2 flex-1 min-w-0">
                                    <span className="font-mono text-xs font-medium bg-background px-2 py-0.5 rounded border border-border/50 truncate">
                                      {tc.name}
                                    </span>
                                    <span className={`text-xs ${
                                      hasResult ? 'text-green-600' : 'text-amber-600'
                                    }`}>
                                      {hasResult ? t('completed') : t('executing')}
                                    </span>
                                  </div>
                                </div>

                                {/* Expand button for results */}
                                {hasResult && (
                                  <button
                                    onClick={() => toggleToolResult(resultKey)}
                                    className="p-1 hover:bg-background/50 rounded transition-colors"
                                  >
                                    {isExpanded ? (
                                      <ChevronUp className="h-3.5 w-3.5 text-muted-foreground" />
                                    ) : (
                                      <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
                                    )}
                                  </button>
                                )}
                              </div>

                              {/* Expandable Result */}
                              {isExpanded && hasResult && (
                                <div className="mt-2 ml-7 p-2 bg-background rounded border border-border/50">
                                  <div className="text-xs text-muted-foreground mb-1">{t('executionResult')}:</div>
                                  <pre className="text-xs font-mono text-foreground overflow-x-auto whitespace-pre-wrap break-words">
                                    {typeof tc.result === 'string' ? tc.result : JSON.stringify(tc.result, null, 2)}
                                  </pre>
                                </div>
                              )}
                            </div>
                          )
                        })}
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
                      {t('dashboard:thinkingProcess')}
                      <p className="mt-2 max-h-[200px] overflow-y-auto whitespace-pre-wrap scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">{streamingThinking}</p>
                    </div>
                  )}
                  {streamingToolCalls.length > 0 && (
                    <div className="mb-3 rounded-lg border border-border/50 bg-muted/30 overflow-hidden">
                      <div className="flex items-center gap-2 px-3 py-2 bg-muted/50 border-b border-border/50">
                        <Wrench className="h-3.5 w-3.5 text-muted-foreground" />
                        <span className="font-medium text-xs text-muted-foreground">
                          {t('dashboard:toolCalls')} ({streamingToolCalls.length})
                        </span>
                      </div>
                      <div className="divide-y divide-border/30">
                        {streamingToolCalls.map((tc, i) => (
                          <div key={i} className="px-3 py-2">
                            <div className="flex items-center gap-2">
                              <div className="h-5 w-5 rounded-full flex items-center justify-center bg-amber-500/20 text-amber-600">
                                <Loader2 className="h-3 w-3 animate-spin" />
                              </div>
                              <span className="font-mono text-xs font-medium bg-background px-2 py-0.5 rounded border border-border/50">
                                {tc.name}
                              </span>
                              <span className="text-xs text-amber-600">{t('executing')}</span>
                            </div>
                          </div>
                        ))}
                      </div>
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
              <SelectValue placeholder={t('dashboard:selectBackend')} />
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
                  {t('dashboard:noBackends')}
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
