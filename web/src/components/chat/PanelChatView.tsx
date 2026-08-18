/**
 * PanelChatView - Independent chat interface for the global side panel
 *
 * Has its own session and messages, completely independent from the main chat page.
 * Handles WebSocket streaming, message rendering, and input.
 * No model selector, skill selector, or session history.
 */

import { useState, useRef, useEffect, useCallback, useReducer, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { generateId } from "@/lib/id"
import { ws } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { ServerMessage, ExecutionPlan, Message } from "@/types"
import type { StreamProgress as StreamProgressType } from "@/types"
import { filterPartialMessages, mergeMessagesForDisplay as mergeAssistantMessages } from "@/lib/messageUtils"
import {
  selectLlmBackendState,
  selectChatActions,
} from "@/store/selectors"
import { usePageContext } from "@/hooks/usePageContext"
import { MergedMessageList } from "./MergedMessageList"
import { Send, X, Minimize2, Bot, Plus, Settings } from "lucide-react"
import { Button } from "@/components/ui/button"

// Shared with GlobalChatFab for panel session persistence
export const PANEL_SESSION_KEY = "neomind:panelSessionId"

interface PanelChatViewProps {
  onClose: () => void
  onStreamingChange: (streaming: boolean) => void
  showMinimize?: boolean
  onNavigateToSettings?: () => void
}

// Stream state - same structure as ChatContainer
interface StreamState {
  isStreaming: boolean
  streamingContent: string
  streamingThinking: string
  streamingToolCalls: any[]
  streamProgress: StreamProgressType
  currentPlanStep: string
  executionPlan: ExecutionPlan | null
  planStepStates: Record<number, 'pending' | 'running' | 'completed' | 'failed'>
  roundContents: Record<number, string>
  currentRound: number
}

type StreamAction =
  | { type: 'START_STREAM' }
  | { type: 'THINKING'; content: string }
  | { type: 'CONTENT'; content: string }
  | { type: 'TOOL_START'; tool: string; arguments?: any; round?: number }
  | { type: 'TOOL_END'; tool: string; result: any }
  | { type: 'PROGRESS'; progress: Partial<StreamProgressType> }
  | { type: 'PLAN'; step: string }
  | { type: 'WARNING'; message: string }
  | { type: 'EXECUTION_PLAN'; plan: ExecutionPlan }
  | { type: 'PLAN_STEP_STARTED'; stepId: number; description: string }
  | { type: 'PLAN_STEP_COMPLETED'; stepId: number; success: boolean; summary: string }
  | { type: 'ROUND_END' }
  | { type: 'END_STREAM' }
  | { type: 'ERROR' }
  | { type: 'RESET' }

const initialStreamState: StreamState = {
  isStreaming: false,
  streamingContent: "",
  streamingThinking: "",
  streamingToolCalls: [],
  streamProgress: {
    elapsed: 0,
    stage: 'thinking',
    warnings: [],
    remainingTime: 300,
  },
  currentPlanStep: "",
  executionPlan: null,
  planStepStates: {},
  roundContents: {},
  currentRound: 1,
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
        streamProgress: { ...state.streamProgress, stage: 'thinking' },
      }
    case 'CONTENT':
      return {
        ...state,
        isStreaming: true,
        streamingContent: state.streamingContent + action.content,
        streamProgress: { ...state.streamProgress, stage: 'generating' },
      }
    case 'TOOL_START':
      return {
        ...state,
        isStreaming: true,
        streamingToolCalls: [
          ...state.streamingToolCalls,
          { id: generateId(), name: action.tool, arguments: action.arguments, result: null, round: action.round },
        ],
        streamProgress: { ...state.streamProgress, stage: 'tool_execution' },
      }
    case 'TOOL_END': {
      const idx = state.streamingToolCalls.findIndex(
        tc => tc.name === action.tool && tc.result === null
      )
      if (idx === -1) return state
      const updated = [...state.streamingToolCalls]
      updated[idx] = { ...updated[idx], result: action.result }
      return { ...state, streamingToolCalls: updated }
    }
    case 'PROGRESS':
      return {
        ...state,
        streamProgress: {
          ...state.streamProgress,
          ...action.progress,
          warnings: action.progress.warnings ?? state.streamProgress.warnings,
        },
      }
    case 'PLAN':
      return { ...state, currentPlanStep: action.step }
    case 'WARNING':
      return {
        ...state,
        streamProgress: {
          ...state.streamProgress,
          warnings: [...state.streamProgress.warnings, action.message],
        },
      }
    case 'ROUND_END':
      return {
        ...state,
        roundContents: {
          ...state.roundContents,
          [state.currentRound]: state.streamingContent,
        },
        streamingContent: "",
        streamingThinking: "",
        currentRound: state.currentRound + 1,
      }
    case 'EXECUTION_PLAN':
      return { ...state, executionPlan: action.plan, planStepStates: {} }
    case 'PLAN_STEP_STARTED':
      return { ...state, planStepStates: { ...state.planStepStates, [action.stepId]: 'running' } }
    case 'PLAN_STEP_COMPLETED':
      return { ...state, planStepStates: { ...state.planStepStates, [action.stepId]: action.success ? 'completed' : 'failed' } }
    case 'END_STREAM':
      return { ...initialStreamState, isStreaming: false }
    case 'ERROR':
      return { ...initialStreamState, isStreaming: false }
    case 'RESET':
      return initialStreamState
    default:
      return state
  }
}

export function PanelChatView({ onClose, onStreamingChange, showMinimize, onNavigateToSettings }: PanelChatViewProps) {
  const { t } = useTranslation("chat")

  // Only read LLM backend state from global store (read-only, never affects chat page)
  const { llmBackends, llmBackendLoading } = useStore(selectLlmBackendState)
  const { loadBackends } = useStore(selectChatActions)

  // Independent panel state — does NOT touch global messages/sessionId
  const [panelMessages, setPanelMessages] = useState<Message[]>([])
  const [isHistoryLoading, setIsHistoryLoading] = useState(true)
  const panelSessionIdRef = useRef<string | null>(null)

  // Streaming state
  const [streamState, dispatch] = useReducer(streamReducer, initialStreamState)
  const [currentStreamMessageId, setCurrentStreamMessageId] = useState<string | null>(null)
  const currentStreamMessageIdRef = useRef<string | null>(null)
  const [input, setInput] = useState("")
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Page context — reactive, only read when sending first message
  const pageContext = usePageContext()
  const hasInjectedContextRef = useRef(false)

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const isStreamingRef = useRef(false)
  const onStreamingChangeRef = useRef(onStreamingChange)
  useEffect(() => { onStreamingChangeRef.current = onStreamingChange }, [onStreamingChange])

  // Sync streaming state to parent
  useEffect(() => {
    isStreamingRef.current = streamState.isStreaming
    onStreamingChangeRef.current(streamState.isStreaming)
  }, [streamState.isStreaming])

  // Auto-scroll
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({
      behavior: streamState.isStreaming ? "smooth" : "instant",
    })
  }, [panelMessages, streamState.streamingContent, streamState.isStreaming])

  // Add message to local panel state (NOT global store)
  const addPanelMessage = useCallback((msg: Message) => {
    setPanelMessages(prev => [...prev, msg])
  }, [])

  // Initialize panel: load backends + create/load independent session
  useEffect(() => {
    loadBackends()

    // Try to reuse persisted panel session, otherwise create new
    const persistedId = localStorage.getItem(PANEL_SESSION_KEY)
    if (persistedId) {
      // Load history for persisted session
      api.getSessionHistory(persistedId, { skipErrorToast: true }).then(result => {
        panelSessionIdRef.current = persistedId
        ws.setSessionId(persistedId)
        const merged = mergeAssistantMessages(result.messages || [])
        setPanelMessages(merged)
        setIsHistoryLoading(false)
      }).catch(() => {
        // Session no longer exists — silently create a new one
        localStorage.removeItem(PANEL_SESSION_KEY)
        createPanelSession()
      })
    } else {
      createPanelSession()
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Create a new panel session
  const createPanelSession = useCallback(async () => {
    try {
      const result = await api.createSession()
      if (result?.sessionId) {
        panelSessionIdRef.current = result.sessionId
        localStorage.setItem(PANEL_SESSION_KEY, result.sessionId)
        ws.setSessionId(result.sessionId)
        setPanelMessages([])
      }
    } catch { /* ignore — panel just won't work until backend is available */ }
    setIsHistoryLoading(false)
  }, [])

  // New conversation handler
  const handleNewConversation = useCallback(async () => {
    if (streamState.isStreaming) return
    localStorage.removeItem(PANEL_SESSION_KEY)
    panelSessionIdRef.current = null
    setPanelMessages([])
    hasInjectedContextRef.current = false
    dispatch({ type: 'RESET' })
    await createPanelSession()
  }, [streamState.isStreaming, createPanelSession])

  // Handle WebSocket events — all messages go to local panel state
  useEffect(() => {
    let streamingContentAcc = ""
    let streamingThinkingAcc = ""
    let streamingToolCallsAcc: any[] = []
    let roundContentsAcc: Record<number, string> = {}
    let currentRound = 1

    const handleMessage = (data: ServerMessage) => {
      switch (data.type) {
        case "Thinking":
          streamingThinkingAcc += (data.content || "")
          dispatch({ type: 'THINKING', content: data.content || "" })
          break
        case "Content":
          streamingContentAcc += (data.content || "")
          dispatch({ type: 'CONTENT', content: data.content || "" })
          break
        case "ToolCallStart":
          dispatch({ type: 'TOOL_START', tool: data.tool, arguments: data.arguments, round: data.round ?? currentRound })
          streamingToolCallsAcc.push({
            id: generateId(), name: data.tool, arguments: data.arguments, result: null, round: data.round ?? currentRound,
          })
          break
        case "ToolCallEnd": {
          const idx = streamingToolCallsAcc.findIndex(tc => tc.name === data.tool && tc.result === null)
          if (idx !== -1) {
            streamingToolCallsAcc[idx] = { ...streamingToolCallsAcc[idx], result: data.result }
          }
          dispatch({ type: 'TOOL_END', tool: data.tool, result: data.result })
          break
        }
        case "IntermediateEnd":
        case "intermediate_end":
          if (streamingContentAcc) roundContentsAcc[currentRound] = streamingContentAcc
          streamingContentAcc = ""
          streamingThinkingAcc = ""
          currentRound += 1
          dispatch({ type: 'ROUND_END' })
          break
        case "Progress":
          dispatch({ type: 'PROGRESS', progress: { elapsed: data.elapsed, stage: data.stage, remainingTime: data.remainingTime ?? 300 } })
          if (data.message) dispatch({ type: 'PLAN', step: data.message })
          break
        case "Plan":
          dispatch({ type: 'PLAN', step: data.step })
          break
        case "ExecutionPlanCreated":
          dispatch({ type: 'EXECUTION_PLAN', plan: data.plan })
          break
        case "PlanStepStarted":
          dispatch({ type: 'PLAN_STEP_STARTED', stepId: data.stepId, description: data.description })
          break
        case "PlanStepCompleted":
          dispatch({ type: 'PLAN_STEP_COMPLETED', stepId: data.stepId, success: data.success ?? true, summary: data.summary ?? '' })
          break
        case "Warning":
          dispatch({ type: 'WARNING', message: data.message })
          break
        case "end":
          if (streamingContentAcc || streamingThinkingAcc || streamingToolCallsAcc.length > 0) {
            if (streamingContentAcc) roundContentsAcc[currentRound] = streamingContentAcc
            const hasMultipleRounds = Object.keys(roundContentsAcc).length > 1
            // Use currentStreamMessageId as the message ID so the streaming block
            // transitions smoothly to the saved message without flash
            const msgId = currentStreamMessageIdRef.current || generateId()
            addPanelMessage({
              id: msgId,
              role: "assistant",
              content: streamingContentAcc,
              timestamp: Math.floor(Date.now() / 1000),
              thinking: streamingThinkingAcc || undefined,
              tool_calls: streamingToolCallsAcc.length > 0 ? streamingToolCallsAcc : undefined,
              round_contents: hasMultipleRounds ? roundContentsAcc : undefined,
            })
          }
          dispatch({ type: 'END_STREAM' })
          setCurrentStreamMessageId(null)
          currentStreamMessageIdRef.current = null
          streamingContentAcc = ""
          streamingThinkingAcc = ""
          streamingToolCallsAcc = []
          roundContentsAcc = {}
          currentRound = 1
          break
        case "Error":
          addPanelMessage({
            id: generateId(),
            role: "assistant",
            content: `**${t("errors.llmError")}**\n\n${data.message}`,
            timestamp: Math.floor(Date.now() / 1000),
          })
          dispatch({ type: 'ERROR' })
          break
      }
    }

    const unsubscribe = ws.onMessage(handleMessage)
    return () => { void unsubscribe() }
  }, [addPanelMessage, t])

  // Send message — ensure session is ready before sending
  const handleSend = useCallback(async () => {
    const text = input.trim()
    if (!text || streamState.isStreaming) return

    // Ensure we have a session before sending
    if (!panelSessionIdRef.current) {
      await createPanelSession()
    }

    addPanelMessage({
      id: generateId(),
      role: "user",
      content: text,
      timestamp: Math.floor(Date.now() / 1000),
    })

    setInput("")
    if (inputRef.current) inputRef.current.style.height = "auto"
    dispatch({ type: 'START_STREAM' })
    const streamMsgId = generateId()
    setCurrentStreamMessageId(streamMsgId)
    currentStreamMessageIdRef.current = streamMsgId
    // Inject page context on first message only
    const contextToSend = !hasInjectedContextRef.current && pageContext ? pageContext : undefined
    if (!hasInjectedContextRef.current) hasInjectedContextRef.current = true
    ws.sendMessage(text, undefined, undefined, contextToSend)
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [input, streamState.isStreaming, addPanelMessage, createPanelSession, pageContext])

  const filteredMessages = useMemo(() => filterPartialMessages(panelMessages), [panelMessages])

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-3.5 border-b border-glass-border flex-shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-info-light flex items-center justify-center">
            <Bot className="h-4 w-4 text-info" />
          </div>
          <div>
            <span className="text-sm font-semibold leading-tight">{t("panelTitle")}</span>
            {isStreamingRef.current && (
              <span className="ml-2 inline-flex items-center gap-1 text-xs text-muted-foreground">
                <span className="w-1.5 h-1.5 rounded-full bg-info animate-pulse" />
              </span>
            )}
          </div>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            onClick={handleNewConversation}
            disabled={streamState.isStreaming}
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            aria-label={t("newChat", "New conversation")}
          >
            <Plus className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={onClose}
            className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground"
            aria-label={t("closePanel")}
          >
            {showMinimize ? <Minimize2 className="h-4 w-4" /> : <X className="h-4 w-4" />}
          </Button>
        </div>
      </div>

      {/* Messages */}
      <div
        ref={scrollContainerRef}
        className="flex-1 overflow-y-auto px-4 py-5 min-h-0"
      >
        {!llmBackendLoading && (!llmBackends || llmBackends.length === 0) ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 px-4">
              <div className="w-14 h-14 rounded-xl bg-muted flex items-center justify-center">
                <Settings className="h-7 w-7 text-muted-foreground" />
              </div>
              <h3 className="text-sm font-semibold mt-1">{t("notConfigured.title")}</h3>
              <p className="text-xs text-muted-foreground text-center leading-relaxed">
                {t("notConfigured.description")}
              </p>
              <Button
                variant="outline"
                size="sm"
                className="mt-2 gap-1.5"
                onClick={() => {
                  onClose()
                  onNavigateToSettings?.()
                }}
              >
                <Settings className="h-3.5 w-3.5" />
                {t("notConfigured.goToSettings")}
              </Button>
            </div>
          ) : isHistoryLoading ? (
            <div className="flex flex-col justify-end h-full">
              <div className="space-y-4">
                {/* Skeleton - assistant bubble */}
                <div className="flex gap-3 justify-start animate-pulse">
                  <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-muted" />
                  <div className="max-w-[80%]">
                    <div className="rounded-lg px-4 py-3 bg-muted">
                      <div className="space-y-2">
                        <div className="h-3.5 w-full bg-muted-foreground rounded" />
                        <div className="h-3.5 w-3/4 bg-muted-foreground rounded" />
                      </div>
                    </div>
                  </div>
                </div>
                {/* Skeleton - user bubble */}
                <div className="flex gap-3 justify-end animate-pulse">
                  <div className="max-w-[70%]">
                    <div className="rounded-lg px-4 py-2.5 bg-muted">
                      <div className="h-3.5 w-32 bg-muted-foreground rounded" />
                    </div>
                  </div>
                </div>
                {/* Skeleton - assistant bubble */}
                <div className="flex gap-3 justify-start animate-pulse">
                  <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-muted" />
                  <div className="max-w-[80%]">
                    <div className="rounded-lg px-4 py-3 bg-muted">
                      <div className="space-y-2">
                        <div className="h-3.5 w-full bg-muted-foreground rounded" />
                        <div className="h-3.5 w-2/3 bg-muted-foreground rounded" />
                        <div className="h-3.5 w-1/2 bg-muted-foreground rounded" />
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ) : filteredMessages.length === 0 && !streamState.isStreaming ? (
            <div className="flex flex-col items-center justify-center h-full gap-3">
              <div className="w-12 h-12 rounded-xl bg-muted flex items-center justify-center">
                <Bot className="h-6 w-6 text-foreground" />
              </div>
              <p className="text-sm text-muted-foreground text-center">{t("input.startNewConversation")}</p>
            </div>
          ) : (
            <MergedMessageList
              messages={filteredMessages}
              scrollElementRef={scrollContainerRef}
              isStreaming={streamState.isStreaming && !(currentStreamMessageId && filteredMessages.some(m => m.id === currentStreamMessageId))}
              streamingContent={streamState.streamingContent}
              streamingThinking={streamState.streamingThinking}
              streamingToolCalls={streamState.streamingToolCalls}
              executionPlan={streamState.executionPlan}
              planStepStates={streamState.planStepStates}
              roundContents={streamState.roundContents}
            />
          )}

          {/* Scroll anchor */}
          <div ref={messagesEndRef} />
      </div>

      {/* Input area */}
      <div className="bg-[var(--background)] backdrop-blur-xl px-4 pt-3 pb-6 flex-shrink-0">
        <div className="flex items-center gap-2">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault()
                  handleSend()
                }
                if (e.key === "Escape") onClose()
              }}
              placeholder={streamState.isStreaming ? t("status.wait", "请等待...") : t("input.placeholder")}
              rows={1}
              disabled={streamState.isStreaming}
              className="flex-1 px-4 py-2.5 rounded-lg resize-none text-sm border border-input bg-card text-foreground placeholder:text-muted-foreground focus-visible:outline-none transition-all duration-200 min-h-[44px] max-h-32 disabled:opacity-60"
              onInput={(e) => {
                const el = e.target as HTMLTextAreaElement
                el.style.height = "auto"
                el.style.height = Math.min(el.scrollHeight, 128) + "px"
              }}
            />
          <button
            onClick={handleSend}
            disabled={!input.trim() || streamState.isStreaming}
            className="h-[44px] w-[44px] rounded-lg flex-shrink-0 bg-primary hover:bg-primary-hover text-primary-foreground flex items-center justify-center transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Send className="h-5 w-5" />
          </button>
        </div>
      </div>
    </div>
  )
}
