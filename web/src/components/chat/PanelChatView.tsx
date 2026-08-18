/**
 * PanelChatView - Independent chat interface for the global side panel
 *
 * Has its own session and messages, completely independent from the main chat page.
 * Handles WebSocket streaming, message rendering, and input.
 * Renders through the shared ChatMessages + ChatComposer — same design as the
 * chat page (model selector and image upload included; no skill selector or
 * session history).
 */

import { useState, useRef, useEffect, useCallback, useReducer, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { generateId } from "@/lib/id"
import { ws } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { ServerMessage, Message } from "@/types"
import type { StreamProgress as StreamProgressType } from "@/types"
import { filterPartialMessages, mergeMessagesForDisplay as mergeAssistantMessages } from "@/lib/messageUtils"
import {
  selectLlmBackendState,
  selectChatActions,
} from "@/store/selectors"
import { useToast } from "@/hooks/use-toast"
import type { SkillSummary } from "@/types/skill"
import { pickPageAssistant, panelSessionKey } from "./pageAssistant"
import { useLocation } from "react-router-dom"
import { ChatMessages } from "./ChatMessages"
import { ChatComposer } from "./ChatComposer"
import { X, Minimize2, Bot, Plus, Settings } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { ChatImage } from "@/types"

/** Pin at most this many page-matched skills per send (prompt-budget guard). */
const MAX_PINNED_SKILLS = 5

/** Match installed skills against a page's domain keywords (name/category/keywords). */
function matchSkills(skills: SkillSummary[], keywords: string[]): string[] {
  if (keywords.length === 0) return []
  const kw = keywords.map((k) => k.toLowerCase())
  return skills
    .filter((s) => {
      const hay = [s.name, s.category, ...(s.keywords || [])].join(" ").toLowerCase()
      return kw.some((k) => hay.includes(k))
    })
    .slice(0, MAX_PINNED_SKILLS)
    .map((s) => s.id)
}

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
  // Per-round thinking for grouped rendering (completed rounds, keyed by round)
  streamingRoundThinking: Record<number, string>
  streamingToolCalls: any[]
  streamProgress: StreamProgressType
  currentPlanStep: string
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
  | { type: 'ROUND_END' }
  | { type: 'END_STREAM' }
  | { type: 'ERROR' }
  | { type: 'RESET' }

const initialStreamState: StreamState = {
  isStreaming: false,
  streamingContent: "",
  streamingThinking: "",
  streamingRoundThinking: {},
  streamingToolCalls: [],
  streamProgress: {
    elapsed: 0,
    stage: 'thinking',
    warnings: [],
    remainingTime: 300,
  },
  currentPlanStep: "",
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
        streamingRoundThinking: {
          ...state.streamingRoundThinking,
          [state.currentRound]: (state.streamingRoundThinking[state.currentRound] || "") + action.content,
        },
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
  const { toast } = useToast()

  // Only read LLM backend state from global store (read-only, never affects chat page)
  const { llmBackends, llmBackendLoading } = useStore(selectLlmBackendState)
  const { loadBackends } = useStore(selectChatActions)
  const activeBackendId = useStore((s) => s.activeBackendId)
  const activateBackend = useStore((s) => s.activateBackend)
  const user = useStore((s) => s.user)

  // Independent panel state — does NOT touch global messages/sessionId
  const [panelMessages, setPanelMessages] = useState<Message[]>([])
  const [isHistoryLoading, setIsHistoryLoading] = useState(true)
  const [attachedImages, setAttachedImages] = useState<ChatImage[]>([])
  const panelSessionIdRef = useRef<string | null>(null)

  // Streaming state
  const [streamState, dispatch] = useReducer(streamReducer, initialStreamState)
  const [currentStreamMessageId, setCurrentStreamMessageId] = useState<string | null>(null)
  const currentStreamMessageIdRef = useRef<string | null>(null)
  const [input, setInput] = useState("")
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Page context — reactive, only read when sending first message.
  // The page-scoped assistant specializes the panel per route: its
  // systemPromptSuffix is baked into the session's REAL system prompt at
  // creation (sessionConfig), its tool list becomes the session allowlist,
  // and matching skills get pinned on every send.
  const location = useLocation()
  const { i18n } = useTranslation()
  const assistant = useMemo(
    () => pickPageAssistant(location.pathname, i18n.language),
    [location.pathname, i18n.language]
  )
  // Current page bucket ('devices' | … | 'default') — drives the per-page
  // session key. Kept in a ref for stable callbacks.
  const currentPageKeyRef = useRef(assistant?.key ?? "default")
  // Skill ids matched for the current page (refreshed on page change)
  const matchedSkillIdsRef = useRef<string[]>([])

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

  // Create a new panel session for the current page. The page profile
  // (system-prompt suffix + tool allowlist) rides the creation request —
  // the backend honors sessionConfig only at this moment.
  const createPanelSession = useCallback(async () => {
    try {
      const cfg = assistant
        ? { systemPromptSuffix: assistant.systemPromptSuffix, allowedTools: assistant.tools }
        : undefined
      const result = await api.createSession(cfg)
      if (result?.sessionId) {
        panelSessionIdRef.current = result.sessionId
        localStorage.setItem(panelSessionKey(currentPageKeyRef.current), result.sessionId)
        ws.setSessionId(result.sessionId)
        setPanelMessages([])
      }
    } catch { /* ignore — panel just won't work until backend is available */ }
    setIsHistoryLoading(false)
  }, [assistant])

  // Initialize/re-init the panel for the current page. Runs on mount AND on
  // route change — each page bucket has its own session (own system prompt
  // and tools), so switching pages switches the conversation.
  useEffect(() => {
    loadBackends()

    const pageKey = assistant?.key ?? "default"
    currentPageKeyRef.current = pageKey
    matchedSkillIdsRef.current = []

    // Reset transient state from the previous page's conversation
    panelSessionIdRef.current = null
    setPanelMessages([])
    setIsHistoryLoading(true)
    dispatch({ type: 'RESET' })

    const storageKey = panelSessionKey(pageKey)
    const persistedId = localStorage.getItem(storageKey)
    if (persistedId) {
      // Load history for this page's persisted session
      api.getSessionHistory(persistedId, { skipErrorToast: true }).then(result => {
        panelSessionIdRef.current = persistedId
        ws.setSessionId(persistedId)
        const merged = mergeAssistantMessages(result.messages || [])
        setPanelMessages(merged)
        setIsHistoryLoading(false)
      }).catch(() => {
        // Session no longer exists — clear the bucket; a fresh one (with the
        // page profile) is created lazily on first send.
        localStorage.removeItem(storageKey)
        setIsHistoryLoading(false)
      })
    } else {
      // Lazy creation on first send — the profile must be fresh at that point
      setIsHistoryLoading(false)
    }

    // Best-effort skill matching for this page's domain
    if (assistant && assistant.skillKeywords.length > 0) {
      api.listSkills(1, 100).then(res => {
        matchedSkillIdsRef.current = matchSkills(res.skills || [], assistant.skillKeywords)
      }).catch(() => { /* skills stay unpinned */ })
    }
  }, [location.pathname]) // eslint-disable-line react-hooks/exhaustive-deps

  // New conversation handler — resets the CURRENT page's bucket
  const handleNewConversation = useCallback(async () => {
    if (streamState.isStreaming) return
    localStorage.removeItem(panelSessionKey(currentPageKeyRef.current))
    panelSessionIdRef.current = null
    setPanelMessages([])
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

  // Multimodal gate — mirrors the chat page's composer input
  const activeBackend = llmBackends.find(b => b.id === activeBackendId)
  const supportsMultimodal = activeBackend?.capabilities?.supports_multimodal ?? false

  // Send message — ensure session is ready before sending
  const handleSend = useCallback(async () => {
    const text = input.trim()
    if ((!text && attachedImages.length === 0) || streamState.isStreaming) return

    // Images need a vision-capable backend
    if (attachedImages.length > 0 && !supportsMultimodal) {
      toast({ title: t("model.visionError"), variant: "destructive" })
      return
    }

    // Ensure we have a session before sending
    if (!panelSessionIdRef.current) {
      await createPanelSession()
    }

    addPanelMessage({
      id: generateId(),
      role: "user",
      content: text || "[Image]",
      timestamp: Math.floor(Date.now() / 1000),
      images: attachedImages.length > 0 ? [...attachedImages] : undefined,
    })

    const sentImages = attachedImages.length > 0 ? [...attachedImages] : undefined
    setAttachedImages([])
    setInput("")
    if (inputRef.current) inputRef.current.style.height = "auto"
    dispatch({ type: 'START_STREAM' })
    const streamMsgId = generateId()
    setCurrentStreamMessageId(streamMsgId)
    currentStreamMessageIdRef.current = streamMsgId
    // Page-matched skills ride every send (backend pins them per message);
    // the page's system focus already lives in the session's system prompt.
    const skillIds = matchedSkillIdsRef.current.length > 0 ? [...matchedSkillIdsRef.current] : undefined
    ws.sendMessage(text, sentImages, skillIds, undefined)
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [input, attachedImages, streamState.isStreaming, addPanelMessage, createPanelSession, supportsMultimodal, toast, t])

  const filteredMessages = useMemo(() => filterPartialMessages(panelMessages), [panelMessages])

  // Context estimate — mirrors the chat page's composer input
  const contextUsage = useMemo(() => {
    if (filteredMessages.length === 0) return null
    const maxContext = activeBackend?.capabilities?.max_context ?? 8192
    const msgChars = panelMessages.reduce((sum, m) => sum + (m.content?.length ?? 0), 0)
    const streamChars = (streamState.streamingContent?.length ?? 0) + (streamState.streamingThinking?.length ?? 0)
      + streamState.streamingToolCalls.reduce((s, tc) => s + (tc.arguments?.length ?? 0) + (tc.result?.length ?? 0), 0)
    return { used: Math.ceil((msgChars + streamChars) / 3), max: maxContext }
  }, [filteredMessages.length, panelMessages, activeBackend, streamState.streamingContent, streamState.streamingThinking, streamState.streamingToolCalls])

  // Cancel the in-flight request (same channel the chat page uses)
  const handleCancelRequest = useCallback(() => {
    if (!streamState.isStreaming) return
    ws.sendMessage("__CANCEL__", undefined)
    dispatch({ type: 'ERROR' })
    setCurrentStreamMessageId(null)
    currentStreamMessageIdRef.current = null
    addPanelMessage({
      id: generateId(),
      role: "assistant",
      content: "⚠️ Request cancelled by user",
      timestamp: Math.floor(Date.now() / 1000),
    })
  }, [streamState.isStreaming, addPanelMessage])

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
            <div className="flex flex-col items-center justify-center h-full gap-3 px-2">
              <div className="w-12 h-12 rounded-xl bg-muted flex items-center justify-center">
                <Bot className="h-6 w-6 text-foreground" />
              </div>
              <p className="text-sm text-muted-foreground text-center">
                {assistant?.greeting || t("input.startNewConversation")}
              </p>
              {assistant && assistant.quickActions.length > 0 && (
                <div className="flex flex-col items-stretch gap-1.5 w-full max-w-[260px]">
                  {assistant.quickActions.map((qa) => (
                    <button
                      key={qa.label}
                      type="button"
                      onClick={() => {
                        setInput(qa.prompt)
                        requestAnimationFrame(() => inputRef.current?.focus())
                      }}
                      className="text-left text-xs rounded-lg border border-border bg-card px-3 py-2 text-muted-foreground transition-colors hover:text-foreground hover:bg-muted-50"
                    >
                      {qa.label}
                    </button>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <ChatMessages
              messages={filteredMessages}
              user={user}
              isStreaming={streamState.isStreaming && !(currentStreamMessageId && filteredMessages.some(m => m.id === currentStreamMessageId))}
              streamingContent={streamState.streamingContent}
              streamingThinking={streamState.streamingThinking}
              streamingRoundThinking={streamState.streamingRoundThinking}
              streamingToolCalls={streamState.streamingToolCalls}
              roundContents={streamState.roundContents}
              currentRound={streamState.currentRound}
              streamingMessageId={currentStreamMessageId}
              onScrollToBottom={() => {
                const el = scrollContainerRef.current
                if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" })
              }}
              endRef={messagesEndRef}
            />
          )}

          {/* Scroll anchor */}
          <div ref={messagesEndRef} />
      </div>

      {/* Input area — the shared ChatComposer, same design as the chat page */}
      <div className="px-3 pt-3 pb-4 flex-shrink-0">
        <ChatComposer
          value={input}
          onChange={setInput}
          onSend={handleSend}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault()
              handleSend()
            }
            if (e.key === "Escape") onClose()
          }}
          textareaRef={inputRef}
          placeholder={t("input.placeholder")}
          isStreaming={streamState.isStreaming}
          onCancel={handleCancelRequest}
          attachments={attachedImages}
          onAttachmentsChange={setAttachedImages}
          supportsMultimodal={supportsMultimodal}
          backends={llmBackends}
          activeBackendId={activeBackendId}
          onActivateBackend={activateBackend}
          contextUsage={contextUsage}
          maxHeight={128}
        />
      </div>
    </div>
  )
}
