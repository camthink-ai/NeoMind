import { contextUsageSnapshot } from '@/lib/contextUsage'
import { useEffect, useRef, useState, useCallback, useMemo } from "react"
import { createPortal } from "react-dom"
import { usePageSidebarSlot, PageSidebarColumn } from "@/components/layout/PageSidebarSlot"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { useParams, useNavigate, useSearchParams } from "react-router-dom"
import { generateId } from "@/lib/id"
import { Loader2, Menu, Plus } from "lucide-react"
import { Button } from "@/components/ui/button"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { SessionSidebar } from "@/components/session/SessionSidebar"
import { WelcomeArea } from "@/components/chat/WelcomeArea"
import { ChatMessages } from "@/components/chat/ChatMessages"
import { ChatComposer } from "@/components/chat/ChatComposer"
import { ChatScrollPill } from "@/components/chat/ChatScrollPill"
import { ConnectionStatus } from "@/components/chat/ConnectionStatus"
import { ws, type ConnectionState } from "@/lib/websocket"
import { api } from "@/lib/api"
import { mentionKey, resolveMentions } from "@/lib/api/mentions"
import type {
  AuthorizeNewOperationRequest,
  ChatContentNode,
  ChatImage,
  ChatReference,
  Message,
} from "@/types"
import {
  contentNodesToComposerFields,
  loadSessionDraft,
  markDraftForRetry,
  normalizeContentNodes,
  pruneSessionDrafts,
  saveSessionDraft,
  canHydrateDraftIntoEditor,
  composerReplaceTextOnly,
  sessionUnchangedAfterAwait,
  settlePendingDraft,
  stashPendingDraft,
  validateSendContent,
} from "@/lib/chat/contentProtocol"
import {
  resolveAuthorizeNewOperationForSend,
  toAuthorizeRequest,
  type AuthorizeNewOperationConfirm,
} from "@/lib/chat/authorizeNewOperation"
import { AuthorizeConfirmBar } from "@/components/chat/AuthorizeConfirmBar"
import { cn } from "@/lib/utils"
import { getSessionDisplayTitle } from "@/lib/utils/sessionTitle"
import { normalizeTimestampMs } from "@/lib/utils/format"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { LoadingState } from "@/components/shared"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useToast } from "@/hooks/use-toast"
import { LlmSetupGuide } from "@/components/llm/LlmSetupGuide"
import { useChatStream } from "@/hooks/useChatStream"

// Hook to detect desktop breakpoint — md: 768px, matching the app-wide
// breakpoint (useIsMobile < 768). The old 1024 left the 768–1024 band in a
// hybrid state.
function useIsDesktop() {
  const [isDesktop, setIsDesktop] = useState(() => {
    if (typeof window === 'undefined') return false
    return window.innerWidth >= 768
  })

  useEffect(() => {
    const checkIsDesktop = () => setIsDesktop(window.innerWidth >= 768)
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

/** Welcome-mode stream binding: matches no real session id. */
const WELCOME_DETACHED = "\u0000__welcome_detached__"

export function ChatPage() {
  const { t, i18n } = useTranslation(['common', 'chat'])
  const { toast } = useToast()
  const { sessionId: urlSessionId } = useParams<{ sessionId?: string }>()
  /** Live URL session for ACK callbacks — never close over a stale send-time id. */
  const urlSessionIdRef = useRef(urlSessionId)
  urlSessionIdRef.current = urlSessionId
  const navigate = useNavigate()
  const _openSettings = useStore((s) => s.openSettings)
  const [searchParams, setSearchParams] = useSearchParams()
  const { handleError } = useErrorHandler()
  const llmBackends = useStore((state) => state.llmBackends)
  const llmBackendLoading = useStore((state) => state.llmBackendLoading)
  // True once a load has resolved with an empty list — distinguishes the
  // first load (skeleton) from refetches (keep the guide mounted).
  const everLoadedBackendsRef = useRef(false)
  // One context-truncation notice per session (a long turn truncates every
  // round; toasting each time would be noise).
  const historyTruncatedSessionRef = useRef<string | null>(null)
  const activeBackendId = useStore((state) => state.activeBackendId)
  const activateBackend = useStore((state) => state.activateBackend)
  const loadBackends = useStore((state) => state.loadBackends)
  const hasLoadedBackends = useRef(false)
  const [sessionsLoaded, setSessionsLoaded] = useState(false)

  // Chat state from store - use shallow to prevent re-renders on unrelated state changes
  const {
    sessionId,
    messages,
    clearMessages,
    loadSessions,
    isLoadingSession,
    refreshCurrentSession
  } = useStore((s) => ({
    sessionId: s.sessionId,
    messages: s.messages,
    clearMessages: s.clearMessages,
    loadSessions: s.loadSessions,
    isLoadingSession: s.isLoadingSession,
    refreshCurrentSession: s.refreshCurrentSession,
  }), shallow)

  const addMessage = useStore((s) => s.addMessage)
  const createSession = useStore((s) => s.createSession)
  const switchSession = useStore((s) => s.switchSession)
  const hasEarlierHistory = useStore((s) => s.hasEarlierHistory)
  const loadEarlierHistory = useStore((s) => s.loadEarlierHistory)
  const user = useStore((s) => s.user)
  const [loadingEarlier, setLoadingEarlier] = useState(false)

  // Local state
  const [input, setInput] = useState("")
  const [lastTokenUsage, setLastTokenUsage] = useState<{ promptTokens: number; systemPromptTokens?: number; toolTokens?: number; maxContextTokens?: number } | null>(null)
  const [isCompacting, setIsCompacting] = useState(false)
  const [contextAction, setContextAction] = useState<{
    phase: "running" | "done" | "failed"
    summarizedCount?: number
  } | null>(null)

  // Auto-dismiss compact "done" chip so the thread stays quiet.
  useEffect(() => {
    if (contextAction?.phase !== "done" && contextAction?.phase !== "failed") return
    const t = window.setTimeout(() => setContextAction(null), 3200)
    return () => window.clearTimeout(t)
  }, [contextAction])

  // Token usage survives reloads/session switches — the context it measured
  // is unchanged until the next reply, so a restored session shows real
  // numbers instead of falling back to the character estimate.
  const tokenUsageKey = (sid: string) => `neomind:tokenUsage:${sid}`
  const restoreTokenUsage = (sid: string | undefined) => {
    if (!sid) { setLastTokenUsage(null); return }
    try {
      const raw = localStorage.getItem(tokenUsageKey(sid))
      setLastTokenUsage(raw ? JSON.parse(raw) : null)
    } catch { setLastTokenUsage(null) }
  }
  const persistTokenUsage = (sid: string | undefined, usage: { promptTokens: number; systemPromptTokens?: number; toolTokens?: number; maxContextTokens?: number } | null) => {
    if (!sid) return
    try {
      if (usage) localStorage.setItem(tokenUsageKey(sid), JSON.stringify(usage))
      else localStorage.removeItem(tokenUsageKey(sid))
    } catch { /* storage unavailable */ }
  }
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const pageSidebarSlot = usePageSidebarSlot()
  // Track the ID of the last assistant message for tool call result updates
  const [_lastAssistantMessageId, setLastAssistantMessageId] = useState<string | null>(null)
  /** Session id whose detached turn we resumed — drives the 3s poll.
   *  Null = no poll. Cleared by any live end event, by finalize, and on
   *  session switch (the poll must never display another session's
   *  content on the screen the user is looking at). */
  const [resumedPendingStream, setResumedPendingStream] = useState<string | null>(null)

  // WebSocket connection state (for reconnection UI)
  const [connectionState, setConnectionState] = useState<ConnectionState>({ status: 'disconnected' })

  // Image upload state
  const [attachedImages, setAttachedImages] = useState<ChatImage[]>([])
  // Structured @-references for the next message (ids authoritative).
  const [chatReferences, setChatReferences] = useState<ChatReference[]>([])
  const [chatContentNodes, setChatContentNodes] = useState<ChatContentNode[]>([])
  const editorHostRef = useRef<HTMLElement | null>(null)
  /** Preflight failures: `type:id` → detail. Cleared on successful resolve / chip edit. */
  const [invalidRefs, setInvalidRefs] = useState<Record<string, string>>({})

  // Responsive
  const isDesktop = useIsDesktop()

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Shared stream state machine (also drives the side panel) — view-specific
  // behaviors ride the callbacks below. See hooks/useChatStream.ts.
  const {
    isStreaming,
    streamingMessageId,
    streamingContent,
    streamingThinking,
    streamingToolCalls,
    pendingAuthorizeConfirms,
    roundContents,
    streamingRoundThinking,
    currentRound,
    beginTurn,
    hardReset,
    restore,
    restoreIfBehind,
    dismissAuthorizeConfirm,
  } = useChatStream({
    onContextCompacted: (count, _sid) => {
      // tokenUsage (if any) already applied via onTokenUsage from the WS
      // ContextCompacted frame — do not clear the meter to pending.
      // In-thread status instead of a toast popup.
      setContextAction({ phase: "done", summarizedCount: count })
    },
    // Drop events from other sessions — the side panel shares this socket.
    // Welcome mode binds a sentinel matching NO session: navigating to bare
    // /chat keeps the store's sessionId (sidebar entries don't clear it), so
    // binding the stale id would keep accepting the left session's stream —
    // its isStreaming then spun the WELCOME composer with no conversation
    // visible. The sentinel change also trips the hook's session-change
    // hard-reset, clearing the spinner. The welcome-mode send is safe: the
    // store session is created BEFORE ws.sendMessage, so real events only
    // flow after this prop is the new session's id.
    sessionId: urlSessionId ? sessionId : WELCOME_DETACHED,
    onCancelled: () => {
      // Cross-tab cancel: another view killed this turn; stop the
      // detached-poll before its next 3s tick resurrects a ghost stream.
      setResumedPendingStream(null)
    },
    onStalled: () => {
      // No terminal frame arrived (server restart mid-turn / lost end frame).
      // Settle on history instead of leaving the bubble spinning until the
      // user switches sessions.
      setResumedPendingStream(null)
      void refreshCurrentSession()
    },
    onEnd: (result) => {
      // Any end stops the detached-turn poll (a live end means the poll's
      // finalize branch would race a possibly-already-started NEXT turn).
      setResumedPendingStream(null)
      const emptyStatusOnly =
        !result.content &&
        !result.thinking &&
        result.toolCalls.length === 0 &&
        (result.goalStatus != null || result.endReason != null)
      if (result.interrupted || emptyStatusOnly) {
        // Interrupted OR no-body End (goalStatus/endReason only): local
        // assembly is incomplete — reload the list from server history.
        void (async () => {
          // Turn wall-time stats are LOCAL knowledge — the refreshed
          // server history carries no generationMs. Derive them from the
          // user message that started the turn (captured BEFORE the
          // refresh replaces the local view) and re-attach them to the
          // last assistant message after the refresh lands, so the
          // "x.xs · N chars/s" footnote survives the welcome-page remount.
          const turnStart = useStore
            .getState()
            .messages.filter(m => m.role === "user")
            .pop()?.timestamp
          await refreshCurrentSession()
          if (turnStart == null) return
          // turnStart comes from a store message — server history carries
          // Unix SECONDS, local messages carry ms. Normalize before the
          // subtraction: raw seconds against Date.now() produced a
          // 497,289-hour generation time (2026-10-15).
          const turnStartMs = normalizeTimestampMs(turnStart)
          if (turnStartMs == null) return
          const generationMs = Date.now() - turnStartMs
          useStore.setState(s => {
            const messages = [...s.messages]
            for (let i = messages.length - 1; i >= 0; i--) {
              if (messages[i].role === "assistant") {
                messages[i] = { ...messages[i], generationMs }
                break
              }
            }
            return { messages }
          })
        })()
        return
      }
      const completeMessage: Message = {
        id: result.id,
        role: "assistant",
        content: result.content,
        timestamp: Date.now(),
        generationMs: result.generationMs,
        thinking: result.thinking,
        tool_calls: result.toolCalls.length > 0 ? result.toolCalls : undefined,
        round_contents: result.roundContents,
        round_thinking: result.roundThinking,
        systemNotes: result.systemNotes,
        goalStatus: result.goalStatus,
        endReason: result.endReason,
      }
      addMessage(completeMessage)
      setLastAssistantMessageId(result.id)
      if (result.historyTruncated && historyTruncatedSessionRef.current !== sessionId) {
        historyTruncatedSessionRef.current = sessionId
        toast({ title: t('chat:context.historyTruncated') })
      }
    },
    onError: (message) => {
      // Render immediately — the backend may still send a fallback summary
      // before `end` finalizes the turn.
      addMessage({
        id: generateId(),
        role: "assistant",
        content: `❌ **Error**: ${message}`,
        timestamp: Date.now(),
      })
    },
    onWarning: (message) => {
      addMessage({
        id: generateId(),
        role: "assistant",
        content: `⚠️ **Warning**: ${message}`,
        timestamp: Date.now(),
        isPartial: true, // Mark as temporary/partial
      })
    },
    onTokenUsage: (usage, sid) => {
      setLastTokenUsage(usage)
      persistTokenUsage(sid || urlSessionId, usage)
    },
    onRawEvent: (data) => {
      // Only switch if it's a different session to avoid unnecessary API calls
      if (
        (data.type === "session_created" || data.type === "session_switched") &&
        data.sessionId && data.sessionId !== sessionId
      ) {
        switchSession(data.sessionId)
      }
    },
  })

  /** Auto-resume a detached turn (live testing: a "resume?" dialog made the
   *  user click before anything showed, and restoring its ≤2s-lagged
   *  snapshot visibly rewound content that had streamed beneath it). The
   *  restore happens on session enter / reconnect, before the user has seen
   *  anything in this visit — no rewind perception, no click. */
  const resumePending = useCallback((
    sid: string,
    content: string,
    thinking: string,
    extras?: import("@/hooks/useChatStream").RestoreExtras & { currentRound?: number },
  ) => {
    hardReset()
    restore(content, thinking, extras)
    setResumedPendingStream(sid)
  }, [hardReset, restore])

  // Active tool-loop round, derived from the last in-flight tool call — no
  // own state to reset (all streaming resets clear streamingToolCalls).
  const activeToolRound = streamingToolCalls.length > 0
    ? (streamingToolCalls[streamingToolCalls.length - 1].round ?? null)
    : null

  // Load LLM backends and sessions on mount
  useEffect(() => {
    if (!hasLoadedBackends.current) {
      hasLoadedBackends.current = true
      loadBackends()
      loadSessions().then(() => setSessionsLoaded(true))
    }
  }, [loadBackends, loadSessions])

  // Cleanup on unmount: blur any still-focused input so the soft keyboard is
  // fully dismissed before the next page mounts. The document-scroll resets
  // that used to live here are no longer needed now that `html { overflow:
  // hidden }` prevents iOS PWA from scrolling the root scroller (see
  // index.css) and the chat root tracks `--visual-viewport-offset-top` (see
  // useVisualViewport.ts).
  useEffect(() => {
    return () => {
      if (document.activeElement instanceof HTMLElement) {
        document.activeElement.blur()
      }
    }
  }, [])

  // Refresh backends when window gains focus (e.g., returning from settings page)
  useEffect(() => {
    const handleFocus = () => {
      loadBackends()
    }
    window.addEventListener('focus', handleFocus)
    return () => window.removeEventListener('focus', handleFocus)
  }, [loadBackends])

  // Get sessions from store for navigation logic
  const sessions = useStore((state) => state.sessions)

  // Prune per-session token-usage keys whose session no longer exists —
  // deleting a session never removed its key, so entries accumulated for
  // the life of the browser profile.
  useEffect(() => {
    if (sessions.length === 0) return
    const live = new Set(sessions.map(s => s.sessionId))
    try {
      const stale: string[] = []
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i)
        if (key?.startsWith('neomind:tokenUsage:')) {
          const sid = key.slice('neomind:tokenUsage:'.length)
          if (!live.has(sid)) stale.push(key)
        }
      }
      stale.forEach(k => localStorage.removeItem(k))
    } catch { /* storage unavailable */ }
  }, [sessions])

  // Load session from URL parameter (only when on /chat/:sessionId)
  // This effect handles all session switches triggered by URL changes:
  // - Initial page load with sessionId in URL
  // - Browser back/forward navigation
  // - Click events in SessionSidebar (which navigate to the URL)
  useEffect(() => {
    if (urlSessionId) {
      // Reset pin state on session switch — new session should show latest
      // messages regardless of where the user scrolled in the previous session.
      isPinnedToBottomRef.current = true
      switchSession(urlSessionId).catch((err) => {
        handleError(err, { operation: 'Load session from URL', showToast: false })
      })
    } else {
      // Navigated to /chat (welcome mode) — clear stale messages from previous session
      clearMessages()
      setLastTokenUsage(null)
    }
    // Restore the persisted per-session usage (falls back to estimate if absent)
    restoreTokenUsage(urlSessionId)
  }, [urlSessionId, switchSession, handleError, clearMessages])

  // Draft lifecycle (E15): per-conversation drafts. On SWITCHING away, the
  // current composer content is parked under the OLD session's key and the
  // composer is cleared — session A's half-typed text can never be sent to
  // session B. On entering, the target session's draft (refresh / failed
  // send / previous switch) is restored unless a ?q= onboarding prefill is
  // present (prefill wins). A successful send clears the key (handleSend),
  // so a sent draft cannot resurrect.
  const composerDraftRef = useRef({
    input: "",
    references: [] as ChatReference[],
    nodes: [] as ChatContentNode[],
    images: [] as ChatImage[],
  })
  composerDraftRef.current = {
    input,
    references: chatReferences,
    nodes: chatContentNodes,
    images: attachedImages,
  }
  const prevDraftSessionRef = useRef<string | null>(null)
  useEffect(() => {
    // One-time growth bound: nothing deletes drafts of removed sessions.
    pruneSessionDrafts()
  }, [])
  useEffect(() => {
    const target = urlSessionId ?? "new"
    const prev = prevDraftSessionRef.current
    prevDraftSessionRef.current = target
    if (prev === target) return
    if (prev !== null) {
      // Switching conversations: park the old draft, never inherit it.
      const cur = composerDraftRef.current
      if (cur.input !== "" || cur.references.length > 0 || cur.nodes.length > 0) {
        saveSessionDraft({
          sessionId: prev,
          message: cur.input,
          references: cur.references,
          contentNodes: cur.nodes.length > 0 ? cur.nodes : undefined,
          updatedAt: Date.now(),
        })
      }
      setInput("")
      setChatReferences([])
      setChatContentNodes([])
    }
    // ?q= onboarding prefill wins over a restored draft.
    if (searchParams.get("q")) return
    const draft = loadSessionDraft(target)
    if (!draft || (!draft.message && draft.references.length === 0)) return
    if (prev !== null) {
      // Switch path: composer was just cleared — always restore.
      setInput(draft.message)
      setChatReferences(draft.references)
      setChatContentNodes(draft.contentNodes ?? [])
    } else {
      // Initial mount: never clobber in-progress typing (HMR / late mount).
      const cur = composerDraftRef.current
      if (cur.input !== "" || cur.references.length > 0 || cur.nodes.length > 0) return
      setInput(draft.message)
      setChatReferences(draft.references)
      setChatContentNodes(draft.contentNodes ?? [])
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [urlSessionId])
  // Remount across /chat ↔ /chat/:id resets prevDraftSessionRef, so park on
  // unmount — same storage keys the switch effect uses.
  useEffect(() => {
    return () => {
      const cur = composerDraftRef.current
      if (cur.input === "" && cur.references.length === 0 && cur.nodes.length === 0) {
        return
      }
      const key = prevDraftSessionRef.current ?? urlSessionId ?? "new"
      saveSessionDraft({
        sessionId: key,
        message: cur.input,
        references: cur.references,
        contentNodes: cur.nodes.length > 0 ? cur.nodes : undefined,
        updatedAt: Date.now(),
      })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Handle deleted session redirects and root path
  useEffect(() => {
    if (!sessionsLoaded) return

    const currentPath = window.location.pathname

    // If current sessionId in URL is not in sessions list (session was deleted)
    // redirect to /chat (welcome mode)
    if (urlSessionId && sessions.length > 0 && !sessions.find(s => s.sessionId === urlSessionId)) {
      navigate('/chat', { replace: true })
      return
    }

    // If sessions become empty, redirect to /chat
    if (urlSessionId && sessions.length === 0) {
      navigate('/chat', { replace: true })
      return
    }

    // Redirect root path to /chat
    if (currentPath === '/') {
      navigate('/chat', { replace: true })
    }
  }, [urlSessionId, sessions, navigate, sessionsLoaded])

  // Sync WebSocket sessionId when store sessionId changes
  useEffect(() => {
    if (sessionId) {
      ws.setSessionId(sessionId)
    }
  }, [sessionId])

  // Sync active backend ID to WebSocket so messages are routed to the correct LLM
  useEffect(() => {
    ws.setActiveBackend(activeBackendId)
  }, [activeBackendId])

  // Determine mode: welcome mode (no sessionId in URL) or chat mode (has sessionId in URL)
  // While sessions are loading, treat as welcome mode but show loading instead of welcome content
  const isWelcomeMode = !urlSessionId

  // Ref for the scrollable message container
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // Track whether the user is currently "pinned" to the bottom of the message
  // list. We only auto-scroll on new content while pinned. If the user has
  // scrolled up to read history, auto-scrolling would yank them back down —
  // extremely annoying when waiting for a long response while reviewing context.
  const isPinnedToBottomRef = useRef(true)
  const [showScrollPill, setShowScrollPill] = useState(false)

  const handleScroll = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    // Consider "pinned" if within 80px of the bottom — covers minor sub-pixel
    // diffs and the gap inserted by smooth-scroll inertia.
    const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight
    isPinnedToBottomRef.current = distanceFromBottom < 80
    setShowScrollPill(distanceFromBottom > 120)
  }, [])

  // Auto-scroll to bottom by directly setting scrollTop on the scroll container
  // Using scrollIntoView is unreliable when sibling elements (like sidebar) have CSS transitions,
  // as it scrolls based on viewport position which shifts during layout reflow.
  const scrollToBottom = useCallback(() => {
    const container = scrollContainerRef.current
    if (container) {
      // Sending a new message or opening a session — force-pin so subsequent
      // streaming tokens auto-scroll.
      isPinnedToBottomRef.current = true
      setShowScrollPill(false)
      container.scrollTo({ top: container.scrollHeight, behavior: "smooth" })
    }
  }, [])

  // Debounced scroll to reduce excessive scrolling during streaming
  const debouncedScrollRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Entering or switching a session starts pinned to the newest message.
  // Without this, a user who had scrolled up in the previous session could
  // land mid-history in the new one.
  useEffect(() => {
    isPinnedToBottomRef.current = true
    setShowScrollPill(false)
  }, [urlSessionId])

  useEffect(() => {
    // Don't auto-scroll if the user has scrolled away from the bottom to read
    // history. They can scroll down manually when ready.
    if (!isPinnedToBottomRef.current) return

    // Clear existing timeout
    if (debouncedScrollRef.current) {
      clearTimeout(debouncedScrollRef.current)
    }

    // Set new timeout for debounced scroll
    debouncedScrollRef.current = setTimeout(() => {
      scrollToBottom()
    }, 100) // 100ms debounce

    // Cleanup on unmount
    return () => {
      if (debouncedScrollRef.current) {
        clearTimeout(debouncedScrollRef.current)
      }
    }
  }, [messages, streamingContent, scrollToBottom])


  // Pending-stream recovery, path 1: reconnect while viewing a session.
  useEffect(() => {
    // Cancel flag, same as path 2: the reconnect fires a fetch whose RTT
    // can outlive a session switch — a late restore() would paint the OLD
    // session's stream onto the NEW view.
    let cancelled = false
    const unsubscribe = ws.onConnection((connected, isReconnect) => {
      // urlSessionId gate, same as path 2: on the WELCOME page (bare
      // /chat) the store session id still points at the session just
      // abandoned — resuming it here would inject its stream onto a page
      // showing no conversation (the leak path 2's gate exists for).
      if (connected && isReconnect && sessionId && urlSessionId) {
        api.getPendingStream(sessionId).then(result => {
          if (cancelled) return
          if (result?.hasPending) resumePending(sessionId, result.content || "", result.thinking || "", {
              toolCalls: result.toolCalls,
              roundContents: result.roundContents,
              currentRound: result.currentRound,
            })
        }).catch(() => {
          // Ignore errors checking pending stream
        })
      }
    })
    return () => { cancelled = true; void unsubscribe() }
  }, [sessionId, urlSessionId, resumePending])

  // Pending-stream recovery, path 2: session ENTER (mount, switch, return
  // from another page). The reconnect-only check missed the most common
  // case — a hard refresh opens a FRESH socket whose first connect is not a
  // "reconnect", so an in-flight answer was invisible until it silently
  // landed in history. Switching back to a still-generating session had no
  // check at all. Auto-resume: the user sees the generating answer as a
  // normal streaming bubble — no dialog, no click.
  useEffect(() => {
    // Leaving this session stops its poll — a poll keyed to the PREVIOUS
    // session re-injected that session's content here every 3s (the
    // live-test cross-session bleed). Includes leaving to the WELCOME page
    // (bare /chat): the store session id survives that navigation, so
    // without the urlSessionId gate this effect would auto-resume the
    // abandoned session's stream onto a page that shows no conversation.
    setResumedPendingStream(null)
    if (!sessionId || !urlSessionId) return
    let cancelled = false
    // Welcome-send remount: the hint set at send time says a turn is
    // already in flight on THIS page — show the streaming spinner now
    // instead of an idle composer; the pending fetch (a beat later) or the
    // first live event replaces this optimistic state.
    const optimisticTurn = useStore.getState().pendingTurnHint === sessionId
    if (optimisticTurn) {
      useStore.getState().setPendingTurnHint(null)
      restore("", "", {})
    }
    // An optimistic turn (just sent from the welcome page) with no snapshot
    // yet is usually a RACE, not a finished turn: the frame can still be
    // queued on a reconnecting socket, and the server writes the snapshot at
    // frame receipt — the pre-stream setup (backend reconfig, decision
    // pipeline, first LLM request) follows it, it doesn't precede it.
    // Killing the optimistic spinner on the first false left the whole
    // model-load window showing an empty reply area (2026-10-19:
    // "ai回复UI为空需要刷新页面才看得到ai正在回复中"). Retry briefly before
    // declaring the turn dead; a live event or the snapshot takes over, and
    // the exhausted path still settles to history.
    const OPTIMISTIC_PENDING_RETRIES = 5
    let attempts = 0
    let retryTimer: ReturnType<typeof setTimeout> | null = null
    const settleDeadTurn = () => {
      // The turn finished BEFORE this page mounted — its end event had no
      // live listener. Clear the optimistic spinner (if any) and take the
      // server's completed answer.
      if (optimisticTurn) {
        hardReset()
        void refreshCurrentSession()
      }
    }
    const checkPending = () => {
      api.getPendingStream(sessionId).then(result => {
        if (cancelled) return
        if (result?.hasPending) {
          resumePending(sessionId, result.content || "", result.thinking || "", {
            toolCalls: result.toolCalls,
            roundContents: result.roundContents,
            currentRound: result.currentRound,
          })
          return
        }
        if (optimisticTurn && attempts < OPTIMISTIC_PENDING_RETRIES) {
          attempts += 1
          retryTimer = setTimeout(checkPending, 1000)
          return
        }
        settleDeadTurn()
      }).catch(() => {
        if (cancelled) return
        if (optimisticTurn && attempts < OPTIMISTIC_PENDING_RETRIES) {
          attempts += 1
          retryTimer = setTimeout(checkPending, 1000)
          return
        }
        settleDeadTurn()
      })
    }
    checkPending()
    return () => {
      cancelled = true
      if (retryTimer) clearTimeout(retryTimer)
    }
  }, [sessionId, urlSessionId, resumePending, restore, hardReset, refreshCurrentSession])

  // Detached-turn poll: after a resume, the generating turn's events may be
  // bound to a DEAD connection (hard refresh) — the server's pending
  // snapshot (saved every ≤2s) is then the only progress source. Adopt it
  // whenever it is ahead of local; when pending clears, the turn finished
  // server-side: reset the partial display and take history.
  // Guarded to the VIEWED session (see path 2) and stalled-guarded: a
  // snapshot that stops growing for 60s means the server died mid-turn —
  // stop pretending and clear the spinner instead of polling forever.
  useEffect(() => {
    if (!resumedPendingStream || resumedPendingStream !== sessionId) return
    let stopped = false
    let lastLen = -1
    let stallTicks = 0
    const timer = setInterval(() => {
      if (stopped) return
      api.getPendingStream(resumedPendingStream).then(result => {
        if (stopped) return
        if (result?.hasPending) {
          // Tool-call COUNT is part of the stall signature and of "ahead":
          // during a slow tool execution (shell, extension inference) text
          // does not grow for minutes — a new call/round is the only
          // aliveness signal a detached page can see.
          const len = (result.content || "").length + (result.thinking || "").length
            + (result.toolCalls?.length ?? 0)
            + Object.values(result.roundContents || {}).reduce((a, b) => a + b.length, 0)
          // Stall = 60s with no growth across SUCCESSFUL polls. (A previous
          // "resultless last call ⇒ 10-minute cap" branch was dead code: the
          // server emits a tool batch's Start/End back-to-back after
          // execution completes, so an EXECUTING tool never appears
          // resultless in a snapshot.)
          if (len > lastLen) { lastLen = len; stallTicks = 0 }
          else stallTicks += 1
          restoreIfBehind(result.content || "", result.thinking || "", {
            toolCalls: result.toolCalls,
            roundContents: result.roundContents,
            currentRound: result.currentRound,
          })
          if (stallTicks >= 20) {
            setResumedPendingStream(null)
            hardReset()
            // A stalled snapshot means the turn died server-side — settle
            // the view to whatever history has instead of leaving a
            // vanished bubble over a stale composer state.
            void refreshCurrentSession()
          }
        } else {
          setResumedPendingStream(null)
          hardReset()
          void refreshCurrentSession()
        }
      }).catch(() => {
        // Transient blip OR a dead server — both count toward the stall
        // cap (a dead server rejects every poll; the old catch-branch
        // polled forever). A blip resets on the next successful poll.
        if (stopped) return
        stallTicks += 1
        if (stallTicks >= 20) {
          setResumedPendingStream(null)
          hardReset()
          void refreshCurrentSession()
        }
      })
    }, 3000)
    return () => { stopped = true; clearInterval(timer) }
  }, [resumedPendingStream, sessionId, restoreIfBehind, hardReset, refreshCurrentSession])

  // Subscribe to WebSocket connection state changes
  useEffect(() => {
    const unsubscribe = ws.onStateChange(setConnectionState)
    return () => { void unsubscribe() }
  }, [])

  // Mid-turn WS drop (server restart, network blip, proxy idle-kill): the
  // live event feed is gone but the turn may well still be running or its
  // pending snapshot recoverable. Instead of leaving the local bubble
  // frozen (events between drop and reconnect are lost either way), hand
  // the display to the pending-poll path IMMEDIATELY — it rebuilds from the
  // server's snapshot, keeps updating while the turn lives, and settles to
  // history when it ends or stalls. Previously the frozen bubble sat until
  // the user refreshed or switched sessions (2026-10-15: "AI 回复的消息就
  // 消失了，要刷新页面才能看到").
  useEffect(() => {
    if (connectionState.status === 'connected') return
    if (!isStreaming || !sessionId || !urlSessionId) return
    setResumedPendingStream(sessionId)
  }, [connectionState.status, isStreaming, sessionId, urlSessionId])

  // Send message - in welcome mode, create session and navigate
  const handleSend = async (
    _e?: React.MouseEvent | React.KeyboardEvent,
    overrides?: {
      message?: string
      authorizeNewOperation?: AuthorizeNewOperationRequest
    },
  ) => {
    const trimmedInput = (overrides?.message ?? input).trim()
    if ((!trimmedInput && attachedImages.length === 0) || isStreaming || isLoadingSession) return

    // Authorize-confirm sends (E16) ride ONLY the phrase text + the
    // authorizeNewOperation payload: no staged images / references / inline
    // nodes are attached, and the user's in-progress draft is never cleared.
    const isOverrideSend = overrides?.message !== undefined
    // Draft key of THIS conversation (welcome mode has no session id yet) —
    // save / restore / clear all use the same derivation (E15).
    const draftSession = urlSessionId ?? "new"

    // Check if images are attached but current model doesn't support vision
    if (!isOverrideSend && attachedImages.length > 0 && !imagesAllowed) {
      toast({ title: t('chat:model.visionError'), variant: "destructive" })
      return
    }

    // Client mirror of the server's content-node caps (E14): an over-cap
    // draft (e.g. 21 total inline mentions of one entity — in-sentence
    // duplicates are legal, so the unique-count insertion guard does not
    // catch them) must fail HERE with the composer intact, not as a 400
    // after the composer was already cleared.
    if (!isOverrideSend && chatContentNodes.some((n) => n.kind === "reference")) {
      const capCode = validateSendContent(chatContentNodes)
      if (capCode) {
        // Keep editable draft (nodes + chips) for fix-and-resend.
        saveSessionDraft({
          sessionId: draftSession,
          message: input,
          references: chatReferences,
          contentNodes: chatContentNodes.length > 0 ? chatContentNodes : undefined,
          updatedAt: Date.now(),
        })
        toast({ title: t('chat:mention.contentOverLimit'), variant: "destructive" })
        return
      }
    }

    // Preflight invalid/stale references — keep draft, mark chips, do not send.
    // Confirm sends carry no references, so they skip the preflight and keep
    // any invalid-ref marks on the untouched draft.
    if (!isOverrideSend && chatReferences.length > 0) {
      try {
        const resolved = await resolveMentions(chatReferences)
        // After await: user may have switched sessions — do not write draft /
        // optimistic bubbles into the wrong conversation.
        if (!sessionUnchangedAfterAwait(draftSession, urlSessionIdRef.current)) {
          return
        }
        if (!resolved.ok) {
          const next: Record<string, string> = {}
          for (const r of resolved.resolutions) {
            if (!r.ok) next[mentionKey(r.type, r.id)] = r.detail
          }
          setInvalidRefs(next)
          // Keep editable draft (nodes + chips) for fix-and-resend after refresh.
          saveSessionDraft({
            sessionId: draftSession,
            message: input,
            references: chatReferences,
            contentNodes: chatContentNodes.length > 0 ? chatContentNodes : undefined,
            updatedAt: Date.now(),
          })
          toast({
            title: t('chat:mention.invalidSend', '部分引用无效，请修正后重发'),
            variant: "destructive",
          })
          return
        }
        setInvalidRefs({})
      } catch {
        if (!sessionUnchangedAfterAwait(draftSession, urlSessionIdRef.current)) {
          return
        }
        // Network failure on preflight must not block send — server still
        // re-resolves and spells NOT FOUND into the model context.
        setInvalidRefs({})
      }
    } else if (!isOverrideSend) {
      setInvalidRefs({})
    }

    // In welcome mode, create session first, then send message
    let targetSessionId = sessionId
    if (isWelcomeMode) {
      const newSessionId = await createSession()
      if (!newSessionId) {
        handleError(new Error('Failed to create session'), { operation: 'Create session', showToast: false })
        return
      }
      targetSessionId = newSessionId
      // The remount hint: /chat → /chat/:id is a route CHANGE (full
      // remount) — without it the fresh page shows an idle composer until
      // the pending fetch or first live event lands.
      useStore.getState().setPendingTurnHint(newSessionId)
      // Navigate to the new session URL
      navigate(`/chat/${newSessionId}`)
    }

    // Prepare message content
    const messageContent = trimmedInput || (attachedImages.length > 0 ? "[Image]" : "")
    const sentImages = !isOverrideSend && attachedImages.length > 0 ? [...attachedImages] : undefined
    const sentReferences = !isOverrideSend && chatReferences.length > 0 ? [...chatReferences] : undefined
    const hasInlineRefs = !isOverrideSend && chatContentNodes.some((n) => n.kind === "reference")
    const sentContentNodes = hasInlineRefs ? [...chatContentNodes] : undefined
    const userMessage: Message = {
      id: generateId(),
      role: "user",
      content: messageContent,
      timestamp: Date.now(),
      images: sentImages,
      contentNodes: sentContentNodes,
    }
    addMessage(userMessage)

    // User just sent a message — re-pin to bottom so the new message and the
    // streaming response auto-scroll into view, even if they had scrolled up
    // to read history a moment ago.
    isPinnedToBottomRef.current = true
    setShowScrollPill(false)

    // Stash the outgoing composer payload BEFORE clearing UI so a failed
    // wire send can restore it. Authorize-confirm (isOverrideSend) never
    // touches the user's draft.
    const draftStash = !isOverrideSend
      ? {
          message: input,
          references: chatReferences,
          contentNodes: chatContentNodes.length > 0 ? chatContentNodes : undefined,
          images: attachedImages,
        }
      : null

    if (!isOverrideSend) {
      setInput("")
      setAttachedImages([])
      setChatReferences([])
      setChatContentNodes([])
      setInvalidRefs({})

      // Reset textarea height when the legacy host is active.
      if (inputRef.current) {
        inputRef.current.style.height = "40px"
      }
    }

    // Set WebSocket session and send message
    if (!targetSessionId) {
      if (draftStash) {
        setInput(draftStash.message)
        setAttachedImages(draftStash.images)
        setChatReferences(draftStash.references)
        setChatContentNodes(draftStash.contentNodes ?? [])
        markDraftForRetry(draftSession, {
          message: draftStash.message,
          references: draftStash.references,
          contentNodes: draftStash.contentNodes,
        })
      }
      handleError(new Error('No valid session ID'), { operation: 'Send message', showToast: false })
      return
    }

    ws.setSessionId(targetSessionId)
    beginTurn()
    setLastAssistantMessageId(null)

    const authorizeNewOperation =
      overrides?.authorizeNewOperation ??
      resolveAuthorizeNewOperationForSend(trimmedInput, pendingAuthorizeConfirms) ??
      undefined
    const { wireOk, requestId } = ws.sendMessage(
      trimmedInput,
      sentImages,
      undefined,
      undefined,
      sentReferences,
      sentContentNodes,
      authorizeNewOperation,
    )

    if (!isOverrideSend && draftStash) {
      // Keep draft by request id until server MessageReceived — local
      // ws.send / queue success is NOT a receive-ack.
      stashPendingDraft(requestId, draftSession, draftStash)
      if (!wireOk) {
        settlePendingDraft(requestId, "failed")
        // beginTurn already armed the stream machine — reach a terminal
        // state even when no Error+end will arrive (queue/wire fail).
        hardReset()
        setInput(draftStash.message)
        setAttachedImages(draftStash.images)
        setChatReferences(draftStash.references)
        setChatContentNodes(draftStash.contentNodes ?? [])
        toast({ title: t('chat:errors.sendMessage'), variant: "destructive" })
      } else {
        void ws.waitForReceiveAck(requestId).then((ack) => {
          if (ack.status === "accepted") {
            settlePendingDraft(requestId, "accepted")
            return
          }
          // rejected / timeout / queue_drop: always terminal-reset the stream
          // machine (server may omit Error+end on some reject paths).
          hardReset()
          // Persist under the original session always; hydrate the editor
          // only if we are still on that session and it is unmodified.
          // Drop any still-queued copy — no server clientRequestId dedupe yet.
          ws.dropPendingByRequestId(requestId)
          const restored = settlePendingDraft(requestId, "failed")
          if (restored) {
            const live = composerDraftRef.current
            // Read the LIVE route session — a send-time closed-over
            // urlSessionId would hydrate the wrong composer after a switch.
            const currentSession = urlSessionIdRef.current ?? "new"
            if (
              canHydrateDraftIntoEditor(restored.sessionId, currentSession, {
                message: live.input,
                references: live.references,
                contentNodes: live.nodes,
                images: live.images,
              })
            ) {
              setInput(restored.message)
              setAttachedImages(restored.images ?? [])
              setChatReferences(restored.references)
              setChatContentNodes(restored.contentNodes ?? [])
            }
          }
          toast({ title: t('chat:errors.sendMessage'), variant: "destructive" })
        })
      }
    }

    requestAnimationFrame(() => {
      const pm = editorHostRef.current?.querySelector<HTMLElement>(".ProseMirror")
      if (pm) pm.focus()
      else inputRef.current?.focus()
    })
  }

  const handleConfirmAuthorize = (confirm: AuthorizeNewOperationConfirm) => {
    const phrase = i18n.language?.startsWith('zh') ? '确认重跑' : 'confirm re-run'
    dismissAuthorizeConfirm(confirm.requestId)
    void handleSend(undefined, {
      message: phrase,
      authorizeNewOperation: toAuthorizeRequest(confirm),
    })
  }

  // Toggle skill selection
  // Check if multimodal is supported
  const supportsMultimodal = getActiveBackendSupportsMultimodal(llmBackends, activeBackendId)

  // Vision companion: a running local VLM lets a text-only active model
  // take images (the session layer delegates the descriptions to it).
  // 8s + refresh-on-focus is enough: the check matters ONLY while the
  // active model is text-only (otherwise the gate passes on its own), and
  // route changes remount this page with an immediate check anyway.
  const needCompanionPoll = !supportsMultimodal
  const [companionRunning, setCompanionRunning] = useState(false)
  useEffect(() => {
    let alive = true
    const check = () =>
      api
        .getBuiltinLlmStatus()
        .then((s) => {
          if (alive) setCompanionRunning(s?.companion?.running === true)
        })
        .catch(() => {})
    check()
    if (!needCompanionPoll) return () => { alive = false }
    const timer = window.setInterval(check, 8000)
    window.addEventListener('focus', check)
    return () => {
      alive = false
      window.clearInterval(timer)
      window.removeEventListener('focus', check)
    }
  }, [needCompanionPoll])
  const imagesAllowed = supportsMultimodal || companionRunning
  const visionDelegate = !supportsMultimodal && companionRunning

  const handleQuickAction = (prompt: string) => {
    // Text replace must drop stale mention/reference nodes from a prior draft.
    const next = composerReplaceTextOnly(prompt)
    setInput(next.message)
    setChatReferences(next.references)
    setChatContentNodes(next.contentNodes)
    setInvalidRefs({})
    inputRef.current?.focus()
  }

  // Pre-fill input from ?q= URL param (e.g. onboarding prompt navigation).
  // Do NOT auto-send — user reviews and presses Enter.
  useEffect(() => {
    const q = searchParams.get("q")
    if (q) {
      setInput(q)
      setSearchParams({}, { replace: true })
      inputRef.current?.focus()
    }
  }, [searchParams, setSearchParams])

  // Load the next older history page. The scroll container is anchored: the
  // user's viewport stays on the same messages — prepending without this
  // yanks them to the "new" top of the list.
  const handleLoadEarlier = async () => {
    if (loadingEarlier) return
    setLoadingEarlier(true)
    const container = scrollContainerRef.current
    const prevHeight = container?.scrollHeight ?? 0
    const prevTop = container?.scrollTop ?? 0
    try {
      const loaded = await loadEarlierHistory()
      if (loaded && container) {
        requestAnimationFrame(() => {
          container.scrollTop = prevTop + (container.scrollHeight - prevHeight)
        })
      }
    } finally {
      setLoadingEarlier(false)
    }
  }

  // Handle manual reconnect
  const handleManualReconnect = () => {
    ws.manualReconnect()
  }

  // Handle cancel request
  const handleCancelRequest = () => {
    if (!isStreaming) return

    // Send cancel message to backend
    ws.sendMessage("__CANCEL__", undefined)

    // Reset streaming state (local path — the server's `cancelled` ack
    // covers cross-tab cancels via the hook)
    hardReset()

    // Kill the detached-turn poll too: the server deletes the pending
    // snapshot on __CANCEL__, but the poll key must drop locally or the
    // next 3s tick would resurrect whatever snapshot state remains.
    setResumedPendingStream(null)

    // Add a message to indicate cancellation
    const cancelMsg: Message = {
      id: generateId(),
      role: "assistant",
      content: "⚠️ Request cancelled by user",
      timestamp: Date.now(),
    }
    addMessage(cancelMsg)
  }

  // Handle keyboard shortcuts
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      // IME confirmation must not send (Chinese/Japanese Enter / keyCode 229).
      if (
        e.nativeEvent.isComposing ||
        (e.nativeEvent as KeyboardEvent).keyCode === 229
      ) {
        return
      }
      e.preventDefault()
      void handleSend()
    }
  }

  // Handle tap outside to dismiss keyboard (mobile)
  const handleBackdropClick = () => {
    forceViewportReset()
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  }

  const _getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  // Filter out partial messages and merge fragmented assistant messages
  // Use useMemo to cache the result and avoid recalculating on every render
  const filteredMessages = useMemo(() =>
    messages.filter(msg => !msg.isPartial),
    [messages]
  )

  // Show chat area if there are messages or currently streaming
  const hasMessages = filteredMessages.length > 0 || isStreaming

  // Settled-messages token estimate (backend CJK-aware weights) — memoized
  // separately from the streaming delta so chunks don't resc an the history.
  // Only server request snapshots describe context; UI history includes
  // compressed/omitted rounds and must not be added into the meter.
  const contextUsage = useMemo(() => {
    if (messages.length === 0 || isWelcomeMode) return null
    const activeBackend = llmBackends.find(b => b.id === activeBackendId)
    return contextUsageSnapshot(lastTokenUsage, activeBackend?.capabilities?.max_context, messages.length)
  }, [messages, isWelcomeMode, llmBackends, activeBackendId, lastTokenUsage])

  // Show LLM setup prompt if not configured. First-ever load shows a
  // skeleton; a REFETCH (focus/click re-triggers loadBackends, which flips
  // llmBackendLoading true) keeps the guide mounted — the old gate fell
  // through to the real chat UI for a frame on every refetch, flashing the
  // conversation beneath the guide.
  // Rendered INSIDE the page layout, not via an early return: the early
  // return skipped MobilePageHeader, and on phones that header is the ONLY
  // navigation entry point — users landed on the setup guide with no way to
  // reach settings or any other page.
  const noLlmContent = (() => {
    if (llmBackends && llmBackends.length > 0) return null
    // An install that HAS configured backends never shows the guide while a
    // load is in flight — including first paint, where llmBackendLoading is
    // still false (the mount effect flips it a tick later). The marker is
    // set/cleared by loadBackends on every completed fetch.
    const knownConfigured = localStorage.getItem('neomind_has_llm_backends') === '1'
    if (knownConfigured && (llmBackendLoading || !everLoadedBackendsRef.current)) {
      everLoadedBackendsRef.current = true
      return <LoadingState variant="page" />
    }
    if (llmBackendLoading && !everLoadedBackendsRef.current) {
      return <LoadingState variant="page" />
    }
    everLoadedBackendsRef.current = true
    return <LlmSetupGuide />
  })()

  const mobileChatTitle = noLlmContent
    ? t('common:llmGuide.title')
    : getSessionDisplayTitle(
        sessions.find((session) => session.sessionId === urlSessionId),
        t('chat:input.newChat'),
      )

  // Manual context compaction — force a summary now (backend bypasses the
  // auto threshold). Prefer the response's post-compact tokenUsage so the
  // ring updates immediately. If the API omits tokenUsage, KEEP the previous
  // meter (clearing it paints an empty pending ring that looks broken).
  const handleCompact = async () => {
    if (!sessionId || isCompacting) return
    setIsCompacting(true)
    setContextAction({ phase: "running" })
    try {
      const r = await api.compactSession(sessionId)
      if (r.summarizedMessages > 0) {
        if (r.tokenUsage && Number.isFinite(r.tokenUsage.promptTokens)) {
          setLastTokenUsage(r.tokenUsage)
          persistTokenUsage(sessionId, r.tokenUsage)
        }
        // else: keep lastTokenUsage as-is — next ContextUsage will refresh.
      }
      setContextAction({
        phase: "done",
        summarizedCount: r.summarizedMessages,
      })
    } catch (e) {
      setContextAction({ phase: "failed" })
    } finally {
      setIsCompacting(false)
    }
  }

  /** History “fill composer” — restores ordered contentNodes for resend. */
  const handleReuseUserMessage = useCallback((message: Message) => {
    const nodes =
      normalizeContentNodes(message.contentNodes) ??
      (message.content ? [{ kind: "text" as const, text: message.content }] : [])
    const fields = contentNodesToComposerFields(nodes)
    setInput(fields.message)
    setChatReferences(fields.references)
    setChatContentNodes(fields.contentNodes)
    setInvalidRefs({})
    const draftSession = sessionId || "new"
    saveSessionDraft({
      sessionId: draftSession,
      message: fields.message,
      references: fields.references,
      contentNodes: fields.contentNodes,
      updatedAt: Date.now(),
    })
    // Focus TipTap ProseMirror when present; fall back to legacy textarea.
    requestAnimationFrame(() => {
      const pm = editorHostRef.current?.querySelector<HTMLElement>(".ProseMirror")
      if (pm) pm.focus()
      else inputRef.current?.focus()
    })
  }, [sessionId])

  // Clear this session's history (server + local view), keep the session.
  const handleClearContext = async () => {
    if (!sessionId) return
    try {
      await api.clearSessionHistory(sessionId)
      clearMessages()
      setLastTokenUsage(null)
      persistTokenUsage(sessionId, null)
      toast({ description: t('chat:context.clearDone') })
    } catch (e) {
      toast({ title: t('chat:context.clearFailed'), variant: 'destructive' })
    }
  }

  // One composer element shared by its two hosts: below the welcome title on
  // an empty chat and docked at the bottom during a conversation. A
  // React element is immutable, so only one instance is ever mounted.
  const composerEl = (
    <ChatComposer
      value={input}
      onChange={setInput}
      onSend={() => handleSend()}
      onKeyDown={handleKeyDown}
      textareaRef={inputRef}
      editorHostRef={editorHostRef}
      placeholder={t('chat:input.placeholder')}
      isStreaming={isStreaming}
      onCancel={handleCancelRequest}
      attachments={attachedImages}
      onAttachmentsChange={setAttachedImages}
      references={chatReferences}
      contentNodes={chatContentNodes}
      onReferencesChange={(refs) => {
        setChatReferences(refs)
        // Drop invalid marks for removed entities; keep marks that still apply.
        setInvalidRefs((prev) => {
          const keep: Record<string, string> = {}
          for (const r of refs) {
            const k = mentionKey(r.type, r.id)
            if (prev[k]) keep[k] = prev[k]
          }
          return keep
        })
      }}
      onContentNodesChange={setChatContentNodes}
      invalidRefs={invalidRefs}
      supportsMultimodal={imagesAllowed}
      visionDelegate={visionDelegate}
      backends={llmBackends}
      activeBackendId={activeBackendId}
      onActivateBackend={activateBackend}
      contextUsage={contextUsage}
      onCompact={sessionId ? handleCompact : undefined}
      onClearContext={sessionId ? handleClearContext : undefined}
      compacting={isCompacting}
      maxHeight={isDesktop ? 160 : 100}
      // One resting height everywhere — welcome vs conversation used to
      // differ (72/56 vs 44), and the box visibly jumped the moment the
      // first reply landed (the context ring appearing at the same time
      // made it look icon-caused).
      minHeight={44}
    />
  )

  return (
    <>
    <div className={cn(
      "chat-mobile-shell fixed left-0 right-0 flex flex-row overflow-hidden bg-background",
      // Desktop: shell owns the macOS titlebar inset. Mobile: the chat header
      // owns safe-top (same contract as MobilePageHeader) so the notch strip
      // is painted by the header token, not a second transparent pad.
      isDesktop && "safe-top",
    )} style={{
      // Offset past BOTH the desktop AppSidebar and the page sidebar column
      // (sessions list, hoisted to the shell slot). Both are 0 on mobile.
      left: 'calc(var(--app-sidebar-width, 0px) + var(--page-sidebar-width, 0px))',
      // Anchor to the top of the VISIBLE area, not the layout viewport. iOS
      // PWA standalone doesn't honor `interactive-widget=resizes-content`, so
      // when the soft keyboard opens iOS scrolls the visualViewport
      // (visualViewport.offsetTop becomes > 0) instead of shrinking the
      // layout viewport. position:fixed `top:0` is relative to the LAYOUT
      // viewport, which would leave the chat container stranded above the
      // visible area. Use --visual-viewport-offset-top to follow the visible
      // area. Always 0 in Safari (where the layout viewport itself shrinks).
      top: 'var(--visual-viewport-offset-top, 0px)',
      // Drive height from `--app-height` (visualViewport.height) so the chat
      // page shrinks with the soft keyboard on iOS PWA standalone, where
      // `interactive-widget=resizes-content` is NOT honored and 100dvh stays
      // full-screen. Without this, focusing the chat input lets iOS PWA
      // scroll/transform the whole document and the header ends up under the
      // notch. Falls back to 100dvh when --app-height is unset.
      height: 'var(--app-height, 100dvh)',
    }}>
      {/* Desktop Sidebar - always show when there are sessions or in chat mode.
          Hoisted to the shell's full-height slot (left of the content) so it
          sits level with the AppSidebar; falls back to in-flow if the slot
          is unavailable. Fixed width — never collapses. */}
      {isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
        pageSidebarSlot ? createPortal(
          <PageSidebarColumn>
            <SessionSidebar
              open={true}
              onClose={() => {}}
              isDesktop={true}
            />
          </PageSidebarColumn>,
          pageSidebarSlot
        ) : (
        <div className="shrink-0 self-stretch">
          <SessionSidebar
            open={true}
            onClose={() => {}}
            isDesktop={true}
          />
        </div>
        )
      )}
      {/* Desktop sidebar skeleton while sessions are loading (only when sidebar isn't shown yet) */}
      {isDesktop && !sessionsLoaded && !(sessions.length > 0 || !isWelcomeMode) && (
        pageSidebarSlot ? createPortal(
          <PageSidebarColumn>
            <div className="w-64 h-full border-r flex flex-col p-3 space-y-2">
              <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
              <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
              <div className="h-8 w-2/3 bg-muted rounded-lg animate-pulse" />
            </div>
          </PageSidebarColumn>,
          pageSidebarSlot
        ) : (
        <div className="shrink-0 self-stretch w-64 border-r flex flex-col p-3 space-y-2">
          <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
          <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
          <div className="h-8 w-2/3 bg-muted rounded-lg animate-pulse" />
        </div>
        )
      )}

      {/* Mobile workspace drawer — always mounted so welcome/empty chat can
          still reach sessions + the System hub without a second top control. */}
      {!isDesktop && (
        <SessionSidebar
          open={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
          isDesktop={false}
        />
      )}

      {/* Main Content — mobile header overlays the conversation (transparent);
          scroll area is full-height; content padding clears the controls. */}
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {/* Mobile chrome: [☰ workspace drawer] [session title] [+].
            Left opens the unified drawer (chats + workspace exit); title is
            display-only so it no longer competes with the list affordance. */}
        {!isDesktop && (
          <div className="pointer-events-none absolute inset-x-0 top-0 z-30">
            <div className="pointer-events-auto">
              <MobilePageHeader
                titleAlign="center"
                scrim
                leftExtra={
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-10 w-10 shrink-0 rounded-full border border-glass-border bg-surface-glass shadow-sm backdrop-blur-xl hover:border-glass-border-hover hover:bg-muted sm:h-10 sm:w-10"
                    onClick={() => setSidebarOpen(true)}
                    aria-label={t('common:session.openDrawer')}
                    aria-expanded={sidebarOpen}
                    aria-controls="session-workspace-drawer"
                    data-testid="chat-drawer-button"
                  >
                    <Menu className="h-5 w-5" />
                  </Button>
                }
                title={
                  <span
                    data-testid="chat-session-title"
                    className="mx-auto block min-w-0 max-w-full truncate text-center"
                  >
                    {mobileChatTitle}
                  </span>
                }
                actions={
                  !noLlmContent ? (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-10 w-10 sm:h-10 sm:w-10"
                      onClick={async () => {
                        const id = await createSession()
                        if (id) navigate(`/chat/${id}`)
                      }}
                      aria-label={t('chat:input.newChat')}
                      data-testid="chat-new-session"
                    >
                      <Plus className="h-5 w-5" strokeWidth={2} />
                    </Button>
                  ) : undefined
                }
              />
            </div>
          </div>
        )}
        {/* Full-height content column — do NOT pad the flex shell (that
            shrinks the scroll viewport). Mobile clearance lives on the
            scroll/content children as padding-top instead. */}
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {noLlmContent ? (
          /* LLM setup guide / first-load skeleton — replaces the chat area
             while the header (mobile nav) stays mounted above it. */
          noLlmContent
        ) : isWelcomeMode && !sessionsLoaded ? (
          /* Loading state while sessions are being loaded - prevents race condition */
          <div className="flex min-h-0 flex-1 items-center justify-center">
            <div className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="h-5 w-5 animate-spin" />
              <span className="text-sm">{t('common:loading')}</span>
            </div>
          </div>
        ) : isWelcomeMode ? (
          /* Welcome Area - shown on /chat (no sessionId), scrollable on mobile */
          <div
            className={cn(
              "touch-scroll flex min-h-0 flex-1 flex-col overflow-y-auto px-4 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))] sm:px-6 sm:py-6",
              isDesktop
                ? "pt-14"
                : "pt-[calc(4rem+env(safe-area-inset-top,0px)+var(--titlebar-inset,0px)+0.5rem)]",
            )}
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"], [data-chat-composer]')) return
              handleBackdropClick()
            }}
          >
            <WelcomeArea className="min-h-full" onQuickAction={handleQuickAction}>
              {composerEl}
            </WelcomeArea>
          </div>
        ) : isLoadingSession || (urlSessionId && sessionId !== urlSessionId) ? (
          /* Loading State - shown while a session loads: during an explicit
             switch AND on first entry with a sessionId in the URL, where the
             store's sessionId is still null for the first frame — without
             the second condition the "Empty chat" default flashes for one
             frame before the real messages arrive. */
          <div className={cn(
            "min-h-0 flex-1 overflow-y-auto px-4 py-2 sm:py-4",
            isDesktop
              ? "pt-12"
              : "pt-[calc(4rem+env(safe-area-inset-top,0px)+var(--titlebar-inset,0px)+0.5rem)] pb-[calc(5.5rem+env(safe-area-inset-bottom,0px))]",
          )}>
            <div className="max-w-3xl mx-auto space-y-4 sm:space-y-6">
              {/* Skeleton message - user */}
              <div className="flex gap-2 sm:gap-3 justify-end animate-pulse">
                <div className="max-w-[85%] sm:max-w-[80%]">
                  <div className="rounded-lg px-3 py-2 sm:px-4 sm:py-3 bg-muted">
                    <div className="h-4 w-48 bg-muted rounded" />
                  </div>
                </div>
                <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-muted" />
              </div>
              {/* Skeleton message - assistant */}
              <div className="flex gap-2 sm:gap-3 justify-start animate-pulse">
                <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-muted" />
                <div className="max-w-[85%] sm:max-w-[80%]">
                  <div className="rounded-lg px-3 py-2 sm:px-4 sm:py-3 bg-muted">
                    <div className="space-y-2">
                      <div className="h-4 w-full bg-muted rounded" />
                      <div className="h-4 w-3/4 bg-muted rounded" />
                      <div className="h-4 w-1/2 bg-muted rounded" />
                    </div>
                  </div>
                </div>
              </div>
              {/* Another skeleton message - user */}
              <div className="flex gap-2 sm:gap-3 justify-end animate-pulse">
                <div className="max-w-[85%] sm:max-w-[80%]">
                  <div className="rounded-lg px-3 py-2 sm:px-4 sm:py-3 bg-muted">
                    <div className="h-4 w-32 bg-muted rounded" />
                  </div>
                </div>
                <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-muted" />
              </div>
            </div>
          </div>
        ) : hasMessages ? (
          /* Chat Messages - shown on /chat/:sessionId with messages */
          <div className="relative min-h-0 flex-1">
            <div
              ref={scrollContainerRef}
              onScroll={handleScroll}
              className={cn(
                "touch-scroll h-full overflow-y-auto px-4 pb-2 sm:pb-4",
                isDesktop
                  ? "pt-6 md:pt-20"
                  // Bottom pad clears the overlay composer so the list still
                  // paints under the transparent dock and fills the column.
                  : "pt-[calc(4rem+env(safe-area-inset-top,0px)+var(--titlebar-inset,0px)+0.5rem)] pb-[calc(5.5rem+env(safe-area-inset-bottom,0px))]",
              )}
              onClick={(e) => {
                // If clicking outside interactive elements, dismiss keyboard
                if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"], [data-chat-composer]')) return
                handleBackdropClick()
              }}
            >
              <ChatMessages

                messages={filteredMessages}
                user={user}
                isStreaming={isStreaming}
                streamingContent={streamingContent}
                streamingThinking={streamingThinking}
                streamingRoundThinking={streamingRoundThinking}
                streamingToolCalls={streamingToolCalls}
                roundContents={roundContents}
                currentRound={currentRound}
                streamingMessageId={streamingMessageId}
                hasEarlierHistory={hasEarlierHistory}
                loadingEarlier={loadingEarlier}
                onLoadEarlier={handleLoadEarlier}
                endRef={messagesEndRef}
                onReuseUserMessage={handleReuseUserMessage}
                contextAction={contextAction}
              />
            </div>
            <ChatScrollPill
              visible={showScrollPill}
              hasNewContent={isStreaming}
              onClick={scrollToBottom}
              className={!isDesktop ? "bottom-[calc(5.5rem+env(safe-area-inset-bottom,0px))]" : undefined}
            />
          </div>
        ) : (
          /* Empty chat - shown on /chat/:sessionId with no messages yet */
          <div
            className={cn(
              "flex min-h-0 flex-1 items-center justify-center px-4 py-4 sm:py-6",
              !isDesktop && "pt-[calc(4rem+env(safe-area-inset-top,0px)+var(--titlebar-inset,0px))] pb-[calc(5.5rem+env(safe-area-inset-bottom,0px))]",
            )}
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"], [data-chat-composer]')) return
              handleBackdropClick()
            }}
          >
            <div className="max-w-md space-y-2 text-center">
              <div>
                <h3 className="text-xl font-semibold tracking-tight">{t('chat:input.newChat')}</h3>
                <p className="text-sm text-muted-foreground mt-1">
                  {t('chat:input.startNewConversation')}
                </p>
              </div>
            </div>
          </div>
        )}
        </div>

        {/* Input Area — mobile overlays the conversation with a bottom→top
            scrim (see .chat-composer-dock); desktop stays in-flow.
            Welcome / no-LLM skip the dock. */}
        {!isWelcomeMode && !noLlmContent && (
        <div
          className={cn(
            "chat-composer-dock overflow-visible border-0 px-4 pt-3 sm:pb-[calc(1.5rem+env(safe-area-inset-bottom,0px))] sm:pt-3",
            isDesktop ? "relative shrink-0" : "absolute inset-x-0 bottom-0 z-20",
          )}
        >
          <div className="max-w-3xl mx-auto">
            {/* Connection status — only surfaces when the WebSocket is not
                connected, so a healthy connection adds zero visual noise.
                Reuses the already-subscribed connectionState and the
                previously-dead handleManualReconnect. */}
            {(connectionState.status === 'error' || connectionState.status === 'disconnected') && (
              <div className="flex justify-center mb-2">
                <ConnectionStatus
                  state={connectionState}
                  onManualReconnect={handleManualReconnect}
                />
              </div>
            )}

            {/* Tool-loop progress — surfaces the round count while the agent
                works through multi-round tool calling. On slow local models a
                legitimate loop can run minutes; without this it reads as a
                hang (0.9.18 plan item: the eval data showed single cases
                running 30 rounds). Zero noise when not tool-calling. */}
            {isStreaming && activeToolRound !== null && (
              <div className="flex justify-center mb-2">
                <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                  <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
                  {t('chat:toolLoopProgress', { round: activeToolRound })}
                </span>
              </div>
            )}

            <AuthorizeConfirmBar
              pending={pendingAuthorizeConfirms}
              disabled={isStreaming || isLoadingSession}
              onConfirm={handleConfirmAuthorize}
            />

            {composerEl}
          </div>
        </div>
        )}
      </div>
    </div>

    
    </>
  )
}
