/**
 * PanelChatView - Independent chat interface for the global side panel
 *
 * Has its own session and messages, completely independent from the main chat page.
 * Handles WebSocket streaming, message rendering, and input.
 * Renders through the shared ChatMessages + ChatComposer — same design as the
 * chat page (model selector and image upload included; no skill selector or
 * session history).
 */

import { useState, useRef, useEffect, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { BuiltinModelWizard } from "@/components/llm/BuiltinModelWizard"
import { generateId } from "@/lib/id"
import { ws } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { Message } from "@/types"
import { filterPartialMessages, mergeMessagesForDisplay as mergeAssistantMessages } from "@/lib/messageUtils"
import { useChatStream } from "@/hooks/useChatStream"
import { estimateTokens } from "@/lib/tokens"
import {
  selectLlmBackendState,
  selectChatActions,
} from "@/store/selectors"
import { useToast } from "@/hooks/use-toast"
import type { SkillSummary } from "@/types/skill"
import { pickPageAssistant, panelSessionKey, profileFingerprint, readStoredPanelSession, writeStoredPanelSession } from "./pageAssistant"
import { useLocation } from "react-router-dom"
import { ChatMessages } from "./ChatMessages"
import { ChatComposer } from "./ChatComposer"
import { X, Minimize2, Bot, Plus, Settings, Cpu, Download } from "lucide-react"
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

export function PanelChatView({ onClose, onStreamingChange, showMinimize, onNavigateToSettings }: PanelChatViewProps) {
  const { t } = useTranslation(["chat", "common"])
  const { toast } = useToast()

  // Only read LLM backend state from global store (read-only, never affects chat page)
  const { llmBackends, llmBackendLoading } = useStore(selectLlmBackendState)
  const { loadBackends } = useStore(selectChatActions)
  const activeBackendId = useStore((s) => s.activeBackendId)
  const activateBackend = useStore((s) => s.activateBackend)
  const [panelWizardOpen, setPanelWizardOpen] = useState(false)
  const user = useStore((s) => s.user)

  // Sync the active backend to the WS singleton — the same effect the chat
  // page runs. Without it, switching models from the panel updates the store
  // and server, but outgoing frames keep the stale ws backendId and the next
  // message still runs on the old model (the server only reconfigures the
  // session agent when an explicit backendId rides the request).
  useEffect(() => {
    ws.setActiveBackend(activeBackendId)
  }, [activeBackendId])

  // Independent panel state — does NOT touch global messages/sessionId
  const [panelMessages, setPanelMessages] = useState<Message[]>([])
  const [isHistoryLoading, setIsHistoryLoading] = useState(true)
  const [attachedImages, setAttachedImages] = useState<ChatImage[]>([])
  const panelSessionIdRef = useRef<string | null>(null)
  // Session-filter binding for useChatStream. Unlike panelSessionIdRef this
  // is NEVER nulled on page change: while a page switch finds the next
  // session, the PREVIOUS page's turn may still be streaming on the shared
  // socket — binding to the stale id keeps those events filtered out
  // (null would mean "accept everything" and drive a phantom bubble).
  const filterSessionIdRef = useRef<string | null>(null)

  // Shared stream state machine (the same one the chat page uses) — the
  // panel's view-specific behavior rides the callbacks: local-message
  // persistence (seconds-precision timestamps), reset-on-error, and the
  // post-end reconciliation against the server's canonical history.
  const {
    isStreaming,
    isStreamingRef,
    streamingMessageId,
    streamingContent,
    streamingThinking,
    streamingToolCalls,
    roundContents,
    streamingRoundThinking,
    currentRound,
    beginTurn,
    hardReset,
  } = useChatStream({
    // Drop events from other sessions — the chat page shares this socket.
    // See filterSessionIdRef: intentionally never cleared on page change.
    sessionId: filterSessionIdRef.current,
    onEnd: (result) => {
      addPanelMessage({
        id: result.id,
        role: "assistant",
        content: result.content,
        timestamp: Math.floor(Date.now() / 1000),
        thinking: result.thinking,
        tool_calls: result.toolCalls.length > 0 ? result.toolCalls : undefined,
        round_contents: result.roundContents,
        round_thinking: result.roundThinking,
      })
      // Reconcile with the server's canonical history. The live assembly
      // above can diverge from what the server persisted (interleaved
      // turns, replayed round events, mid-stream remounts) — reloading
      // converges the panel to exactly what /chat renders.
      const sid = panelSessionIdRef.current
      if (sid) {
        api.getSessionHistory(sid, undefined, { skipErrorToast: true }).then(r => {
          // A new stream started while the fetch was in flight — keep
          // the live state; the next end reconciles again.
          if (isStreamingRef.current) return
          const merged = mergeAssistantMessages(r.messages || [])
          setPanelMessages(prev =>
            // Same turn count → the live assembly is already correct AND
            // holds stable local ids. Adopting server ids here would
            // change every React key and remount the whole list (the
            // visible "snap to one message" at completion). Only take
            // server truth when it actually diverges.
            merged.length === prev.length ? prev : merged
          )
        }).catch(() => { /* keep the live assembly */ })
      }
    },
    onError: (message) => {
      addPanelMessage({
        id: generateId(),
        role: "assistant",
        content: `**${t("errors.llmError")}**\n\n${message}`,
        timestamp: Math.floor(Date.now() / 1000),
      })
      hardReset()
    },
  })

  const [input, setInput] = useState("")
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Page context — reactive, only read when sending first message.
  // The page-scoped assistant specializes the panel per route: its
  // systemPromptSuffix is baked into the session's REAL system prompt at
  // creation (sessionConfig), its tool list becomes the session allowlist,
  // and matching skills get pinned on every send.
  const location = useLocation()
  const { i18n } = useTranslation()
  const dashboards = useStore((s) => s.dashboards)

  // Base profile — derived from URL + language ONLY. This is the fingerprint
  // source: the fingerprint decides whether a stored session is still valid,
  // so anything store-derived must stay out of it. (The dashboard component
  // snapshot below used to ride the fingerprint — editing the dashboard via
  // the panel, or the dashboards store not having loaded yet at restore
  // time, both flipped it and readStoredPanelSession DELETED the pointer.
  // Conversations on dashboard pages were lost on every refresh.)
  const baseAssistant = useMemo(
    () => pickPageAssistant(location.pathname, i18n.language),
    [location.pathname, i18n.language]
  )
  const sessionFingerprint = useMemo(() => profileFingerprint(baseAssistant), [baseAssistant])

  const assistant = useMemo(() => {
    if (!baseAssistant) return null
    // Dashboard-aware bucket: /visual-dashboard/:id gets its OWN session key
    // derived from the ROUTE PARAM (store-independent), plus a suffix naming
    // the open dashboard + a component snapshot when it's loaded. The
    // snapshot is a creation-time bonus baked into the session prompt — the
    // suffix also tells the agent to fetch live truth via `dashboard get`.
    // It is NEVER part of the validity fingerprint.
    const m = location.pathname.match(/^\/visual-dashboard\/([^/]+)/)
    if (baseAssistant.key === "visual-dashboard" && m) {
      const dash = dashboards.find((d) => d.id === m[1])
      if (!dash) {
        // Dashboards still loading — same per-dashboard bucket (URL-derived),
        // base profile only.
        return { ...baseAssistant, key: `visual-dashboard:${m[1]}` }
      }
      const comps = dash.components
        .slice(0, 12)
        .map((c) => `- ${c.id}: ${c.title || "(untitled)"} (${c.type})`)
        .join("\n")
      return {
        ...baseAssistant,
        key: `visual-dashboard:${dash.id}`,
        systemPromptSuffix:
          baseAssistant.systemPromptSuffix +
          `\n\n## 当前打开的看板\n「${dash.name}」(id: ${dash.id}),${dash.components.length} 个组件:\n${comps}\n组件清单是会话建立时的快照——操作前先用 \`neomind dashboard get ${dash.id}\` 获取实时状态。用户说「这个看板/这里」时指的就是它。`,
      }
    }
    return baseAssistant
  }, [baseAssistant, location.pathname, dashboards])
  // Current page bucket ('devices' | … | 'default') — drives the per-page
  // session key. Kept in a ref for stable callbacks.
  const currentPageKeyRef = useRef(assistant?.key ?? "default")
  // Skill ids matched for the current page (refreshed on page change)
  const matchedSkillIdsRef = useRef<string[]>([])

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const onStreamingChangeRef = useRef(onStreamingChange)
  useEffect(() => { onStreamingChangeRef.current = onStreamingChange }, [onStreamingChange])

  // Sync streaming state to parent
  useEffect(() => {
    onStreamingChangeRef.current(isStreaming)
  }, [isStreaming])

  // Auto-scroll
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({
      behavior: isStreaming ? "smooth" : "instant",
    })
  }, [panelMessages, streamingContent, isStreaming])

  // Add message to local panel state (NOT global store)
  const addPanelMessage = useCallback((msg: Message) => {
    setPanelMessages(prev => [...prev, msg])
  }, [])

  // Create a new panel session for the current page. The page profile
  // (system-prompt suffix + tool allowlist) rides the creation request —
  // the backend honors sessionConfig only at this moment. The stored
  // fingerprint is the BASE profile's (never the dashboard snapshot) — see
  // the baseAssistant comment.
  const createPanelSession = useCallback(async () => {
    try {
      const cfg = assistant
        ? { systemPromptSuffix: assistant.systemPromptSuffix, allowedTools: assistant.tools }
        : undefined
      const result = await api.createSession(cfg)
      if (result?.sessionId) {
        panelSessionIdRef.current = result.sessionId
        filterSessionIdRef.current = result.sessionId
        writeStoredPanelSession(currentPageKeyRef.current, result.sessionId, profileFingerprint(baseAssistant))
        ws.setSessionId(result.sessionId)
        setPanelMessages([])
      }
    } catch { /* ignore — panel just won't work until backend is available */ }
    setIsHistoryLoading(false)
  }, [assistant, baseAssistant])

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
    hardReset()

    // Fingerprint-aware: a session whose creation-time profile (prompt
    // suffix / tool allowlist / language) no longer matches is dropped —
    // reusing it would silently keep the OLD specialization forever.
    // sessionFingerprint comes from the URL/i18n-derived base profile, so
    // neither dashboard edits nor store load timing can flip it.
    const persistedId = readStoredPanelSession(pageKey, sessionFingerprint)
    if (persistedId) {
      // Load history for this page's persisted session
      api.getSessionHistory(persistedId, undefined, { skipErrorToast: true }).then(result => {
        panelSessionIdRef.current = persistedId
        filterSessionIdRef.current = persistedId
        ws.setSessionId(persistedId)
        const merged = mergeAssistantMessages(result.messages || [])
        setPanelMessages(merged)
        setIsHistoryLoading(false)
      }).catch(() => {
        // Session no longer exists — clear the bucket; a fresh one (with the
        // page profile) is created lazily on first send.
        localStorage.removeItem(panelSessionKey(pageKey))
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
    if (isStreaming) return
    localStorage.removeItem(panelSessionKey(currentPageKeyRef.current))
    panelSessionIdRef.current = null
    setPanelMessages([])
    hardReset()
    await createPanelSession()
  }, [isStreaming, createPanelSession, hardReset])

  // Surface connection failures that would otherwise leave the streaming
  // bubble spinning forever. With server-side detached delivery the turn
  // keeps running and lands in history — say that, don't imply the answer
  // is lost. Two guards against FALSE kills:
  // - the subscription's immediate callback replays STALE state (a remount
  //   while a long-dead error lingers) — skip the first invocation;
  // - a 2-second network blip mid-reconnect must not end the stream UI
  //   while the message is safely queued and the turn still runs — require
  //   the bad state to PERSIST for 3s (cleared the moment we reconnect).
  useEffect(() => {
    let firstInvocation = true
    let killTimer: ReturnType<typeof setTimeout> | null = null
    const clearKillTimer = () => {
      if (killTimer) { clearTimeout(killTimer); killTimer = null }
    }
    const unsubscribe = ws.onStateChange((state) => {
      if (firstInvocation) {
        firstInvocation = false
        return
      }
      if (!isStreamingRef.current) return
      const bad = state.status === 'error' || state.status === 'disconnected' || state.status === 'reconnecting'
      if (!bad) {
        clearKillTimer()
        return
      }
      if (killTimer) return // already counting down
      killTimer = setTimeout(() => {
        killTimer = null
        if (!isStreamingRef.current) return
        hardReset()
        const content = state.errorMessage
          ? `**${t("chat.connection.authFailed")}**\n\n${state.errorMessage}`
          : `⚠️ ${t("chat.connection.interruptedSaved")}`
        addPanelMessage({
          id: generateId(),
          role: "assistant",
          content,
          timestamp: Math.floor(Date.now() / 1000),
        })
      }, 3000)
    })
    return () => {
      clearKillTimer()
      void unsubscribe()
    }
  }, [addPanelMessage, t, hardReset])

  // Multimodal gate — mirrors the chat page's composer input
  const activeBackend = llmBackends.find(b => b.id === activeBackendId)
  const supportsMultimodal = activeBackend?.capabilities?.supports_multimodal ?? false

  // Send message — ensure session is ready before sending
  const handleSend = useCallback(async () => {
    const text = input.trim()
    if ((!text && attachedImages.length === 0) || isStreaming) return

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
    beginTurn()
    // Page-matched skills ride every send (backend pins them per message);
    // the page's system focus already lives in the session's system prompt.
    const skillIds = matchedSkillIdsRef.current.length > 0 ? [...matchedSkillIdsRef.current] : undefined
    ws.sendMessage(text, sentImages, skillIds, undefined)
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [input, attachedImages, isStreaming, addPanelMessage, createPanelSession, supportsMultimodal, toast, t, beginTurn])

  const filteredMessages = useMemo(() => filterPartialMessages(panelMessages), [panelMessages])

  // Context estimate — the shared CJK-weighted estimator (the old chars/3
  // underestimated Chinese ~5x). The panel has no measured token usage, so
  // this is always an estimate, computed cheaply per streaming delta.
  const contextUsage = useMemo(() => {
    if (filteredMessages.length === 0) return null
    const maxContext = activeBackend?.capabilities?.max_context ?? 8192
    const msgTokens = panelMessages.reduce((sum, m) => sum + estimateTokens(m.content ?? ""), 0)
    const streamCorpus = (streamingContent ?? '') + (streamingThinking ?? '')
      + streamingToolCalls.map(tc => String(tc.arguments ?? '') + String(tc.result ?? '')).join('')
    return { used: msgTokens + estimateTokens(streamCorpus), max: maxContext }
  }, [filteredMessages.length, panelMessages, activeBackend, streamingContent, streamingThinking, streamingToolCalls])

  // Cancel the in-flight request (same channel the chat page uses)
  const handleCancelRequest = useCallback(() => {
    if (!isStreaming) return
    ws.sendMessage("__CANCEL__", undefined)
    hardReset()
    addPanelMessage({
      id: generateId(),
      role: "assistant",
      content: "⚠️ Request cancelled by user",
      timestamp: Math.floor(Date.now() / 1000),
    })
  }, [isStreaming, addPanelMessage, hardReset])

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
            {isStreaming && (
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
            disabled={isStreaming}
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
              <div className="w-14 h-14 rounded-xl bg-primary-light text-primary flex items-center justify-center">
                <Cpu className="h-7 w-7" />
              </div>
              <h3 className="text-sm font-semibold mt-1">{t("notConfigured.title")}</h3>
              <p className="text-xs text-muted-foreground text-center leading-relaxed">
                {t("notConfigured.description")}
              </p>
              <div className="w-full max-w-xs space-y-2">
                <Button
                  size="sm"
                  className="w-full gap-1.5"
                  onClick={() => setPanelWizardOpen(true)}
                >
                  <Download className="h-3.5 w-3.5" />
                  {t("common:llmGuide.builtinShort")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="w-full gap-1.5"
                  onClick={() => {
                    onClose()
                    onNavigateToSettings?.()
                  }}
                >
                  <Settings className="h-3.5 w-3.5" />
                  {t("common:llmGuide.ownShort")}
                </Button>
              </div>
              <BuiltinModelWizard
                open={panelWizardOpen}
                onOpenChange={setPanelWizardOpen}
                onActivated={() => { setPanelWizardOpen(false); loadBackends() }}
              />
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
          ) : filteredMessages.length === 0 && !isStreaming ? (
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
              isStreaming={isStreaming && !(streamingMessageId && filteredMessages.some(m => m.id === streamingMessageId))}
              streamingContent={streamingContent}
              streamingThinking={streamingThinking}
              streamingRoundThinking={streamingRoundThinking}
              streamingToolCalls={streamingToolCalls}
              roundContents={roundContents}
              currentRound={currentRound}
              streamingMessageId={streamingMessageId}
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
          isStreaming={isStreaming}
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
