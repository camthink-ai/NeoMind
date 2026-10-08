import { useEffect, useRef, useState, useCallback, useMemo } from "react"
import { createPortal } from "react-dom"
import { usePageSidebarSlot, PageSidebarColumn } from "@/components/layout/PageSidebarSlot"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { useParams, useNavigate, useSearchParams } from "react-router-dom"
import { generateId } from "@/lib/id"
import { Sparkles, MessageSquare, Loader2, Plus } from "lucide-react"
import { Button } from "@/components/ui/button"
import { SessionSidebar } from "@/components/session/SessionSidebar"
import { WelcomeArea } from "@/components/chat/WelcomeArea"
import { ChatMessages } from "@/components/chat/ChatMessages"
import { ChatComposer } from "@/components/chat/ChatComposer"
import { ConnectionStatus } from "@/components/chat/ConnectionStatus"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { ws, type ConnectionState } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { Message, ChatImage } from "@/types"
import { cn } from "@/lib/utils"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { LoadingState } from "@/components/shared"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useToast } from "@/hooks/use-toast"
import { LlmSetupGuide } from "@/components/llm/LlmSetupGuide"
import { useChatStream } from "@/hooks/useChatStream"
import { estimateTokens } from "@/lib/tokens"

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
  const { t } = useTranslation(['common', 'chat'])
  const { toast } = useToast()
  const { sessionId: urlSessionId } = useParams<{ sessionId?: string }>()
  const navigate = useNavigate()
  const _openSettings = useStore((s) => s.openSettings)
  const [searchParams, setSearchParams] = useSearchParams()
  const { handleError } = useErrorHandler()
  const llmBackends = useStore((state) => state.llmBackends)
  const llmBackendLoading = useStore((state) => state.llmBackendLoading)
  // True once a load has resolved with an empty list — distinguishes the
  // first load (skeleton) from refetches (keep the guide mounted).
  const everLoadedBackendsRef = useRef(false)
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
    roundContents,
    streamingRoundThinking,
    currentRound,
    beginTurn,
    hardReset,
    restore,
    restoreIfBehind,
  } = useChatStream({
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
    onEnd: (result) => {
      // Any end stops the detached-turn poll (a live end means the poll's
      // finalize branch would race a possibly-already-started NEXT turn).
      setResumedPendingStream(null)
      if (result.interrupted) {
        // The local accumulator was reset mid-turn (switch away/back,
        // detach/restore) — it holds only a tail. The server persisted the
        // full message; take history as the truth.
        void refreshCurrentSession()
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
      }
      addMessage(completeMessage)
      setLastAssistantMessageId(result.id)
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

  const handleScroll = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    // Consider "pinned" if within 80px of the bottom — covers minor sub-pixel
    // diffs and the gap inserted by smooth-scroll inertia.
    const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight
    isPinnedToBottomRef.current = distanceFromBottom < 80
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
      container.scrollTo({ top: container.scrollHeight, behavior: "smooth" })
    }
  }, [])

  // Debounced scroll to reduce excessive scrolling during streaming
  const debouncedScrollRef = useRef<ReturnType<typeof setTimeout> | null>(null)

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
    const unsubscribe = ws.onConnection((connected, isReconnect) => {
      // urlSessionId gate, same as path 2: on the WELCOME page (bare
      // /chat) the store session id still points at the session just
      // abandoned — resuming it here would inject its stream onto a page
      // showing no conversation (the leak path 2's gate exists for).
      if (connected && isReconnect && sessionId && urlSessionId) {
        api.getPendingStream(sessionId).then(result => {
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
    return () => { void unsubscribe() }
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
    api.getPendingStream(sessionId).then(result => {
      if (cancelled || !result?.hasPending) return
      resumePending(sessionId, result.content || "", result.thinking || "", {
              toolCalls: result.toolCalls,
              roundContents: result.roundContents,
            })
    }).catch(() => {})
    return () => { cancelled = true }
  }, [sessionId, urlSessionId, resumePending])

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
          // While the snapshot's LAST call has no result yet, a tool is
          // EXECUTING — legitimate silence up to the 300s tool ceiling. Use
          // a 10-minute stall cap there; 60s everywhere else.
          const calls = result.toolCalls ?? []
          const lastResultless = calls.length > 0 && calls[calls.length - 1].result == null
          const stallCap = lastResultless ? 200 : 20
          if (len > lastLen) { lastLen = len; stallTicks = 0 }
          else stallTicks += 1
          restoreIfBehind(result.content || "", result.thinking || "", {
            toolCalls: result.toolCalls,
            roundContents: result.roundContents,
            currentRound: result.currentRound,
          })
          if (stallTicks >= stallCap) {
            setResumedPendingStream(null)
            hardReset()
          }
        } else {
          setResumedPendingStream(null)
          hardReset()
          void refreshCurrentSession()
        }
      }).catch(() => { /* transient — keep polling */ })
    }, 3000)
    return () => { stopped = true; clearInterval(timer) }
  }, [resumedPendingStream, sessionId, restoreIfBehind, hardReset, refreshCurrentSession])

  // Subscribe to WebSocket connection state changes
  useEffect(() => {
    const unsubscribe = ws.onStateChange(setConnectionState)
    return () => { void unsubscribe() }
  }, [])

  // Send message - in welcome mode, create session and navigate
  const handleSend = async (_e?: React.MouseEvent | React.KeyboardEvent) => {
    const trimmedInput = input.trim()
    if ((!trimmedInput && attachedImages.length === 0) || isStreaming || isLoadingSession) return

    // Check if images are attached but current model doesn't support vision
    if (attachedImages.length > 0 && !supportsMultimodal) {
      toast({ title: t('chat:model.visionError'), variant: "destructive" })
      return
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
      // Navigate to the new session URL
      navigate(`/chat/${newSessionId}`)
    }

    // Prepare message content
    const messageContent = trimmedInput || (attachedImages.length > 0 ? "[Image]" : "")
    const userMessage: Message = {
      id: generateId(),
      role: "user",
      content: messageContent,
      timestamp: Date.now(),
      images: attachedImages.length > 0 ? [...attachedImages] : undefined,
    }
    addMessage(userMessage)

    // User just sent a message — re-pin to bottom so the new message and the
    // streaming response auto-scroll into view, even if they had scrolled up
    // to read history a moment ago.
    isPinnedToBottomRef.current = true

    setInput("")
    setAttachedImages([])

    // Reset textarea height to initial state
    if (inputRef.current) {
      inputRef.current.style.height = "40px"
    }

    // Set WebSocket session and send message
    if (!targetSessionId) {
      handleError(new Error('No valid session ID'), { operation: 'Send message', showToast: false })
      return
    }

    ws.setSessionId(targetSessionId)
    beginTurn()
    setLastAssistantMessageId(null)

    ws.sendMessage(trimmedInput, attachedImages.length > 0 ? attachedImages : undefined)

    requestAnimationFrame(() => {
      inputRef.current?.focus()
    })
  }

  // Toggle skill selection
  // Check if multimodal is supported
  const supportsMultimodal = getActiveBackendSupportsMultimodal(llmBackends, activeBackendId)

  // Handle quick action
  const handleQuickAction = (prompt: string) => {
    setInput(prompt)
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
      e.preventDefault()
      handleSend()
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
  const messageTokensEstimate = useMemo(
    () => estimateTokens(messages.map(m => m.content ?? '').join('\n')),
    [messages],
  )

  // Context usage — real prompt tokens after a turn, chars/3 estimate otherwise
  const contextUsage = useMemo(() => {
    if (messages.length === 0 || isWelcomeMode) return null
    const activeBackend = llmBackends.find(b => b.id === activeBackendId)
    // Backend-measured window (end event) is authoritative — capability
    // metadata can lag the live instance config.
    const maxContext = lastTokenUsage?.maxContextTokens
      ?? activeBackend?.capabilities?.max_context
      ?? 8192
    const promptTokens = lastTokenUsage?.promptTokens
    let used: number
    if (promptTokens != null && !isStreaming) {
      used = promptTokens
    } else {
      // Only the streaming delta is re-estimated per chunk — the settled
      // messages' total is memoized below, so each chunk costs O(delta)
      // instead of a full-history join + scan.
      const streamCorpus = (streamingContent ?? '') + (streamingThinking ?? '')
        + streamingToolCalls.map(tc => (tc.arguments ?? '') + (tc.result ?? '')).join('')
      used = messageTokensEstimate + estimateTokens(streamCorpus)
      // While streaming with a known real baseline, never dip below it — the
      // in-flight request's at least the last measured prompt.
      if (promptTokens != null) used = Math.max(used, promptTokens)
    }
    const system = lastTokenUsage?.systemPromptTokens
    const tools = lastTokenUsage?.toolTokens
    const history = system != null && tools != null && promptTokens != null
      ? Math.max(0, used - system - tools)
      : undefined
    const estimated = promptTokens == null || isStreaming
    return { used, max: maxContext, system, tools, history, estimated, messageCount: messages.length }
  }, [messages, isWelcomeMode, llmBackends, activeBackendId, lastTokenUsage, isStreaming, streamingContent, streamingThinking, streamingToolCalls])

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

  // Manual context compaction — force a summary now (backend bypasses the
  // auto threshold). Usage normalizes on the next turn.
  const handleCompact = async () => {
    if (!sessionId || isCompacting) return
    setIsCompacting(true)
    try {
      const r = await api.compactSession(sessionId)
      toast({
        description: r.summarizedMessages > 0
          ? t('chat:context.compactDone', { count: r.summarizedMessages })
          : t('chat:context.compactNothing'),
      })
    } catch (e) {
      toast({ title: t('chat:context.compactFailed'), variant: 'destructive' })
    } finally {
      setIsCompacting(false)
    }
  }

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

  // One composer element shared by its two hosts: centered in the welcome
  // screen (mainstream empty-state pattern — greeting, input, suggestions as
  // one centered group) and docked at the bottom during a conversation. A
  // React element is immutable, so only one instance is ever mounted.
  const composerEl = (
    <ChatComposer
      value={input}
      onChange={setInput}
      onSend={() => handleSend()}
      onKeyDown={handleKeyDown}
      textareaRef={inputRef}
      placeholder={t('chat:input.placeholder')}
      isStreaming={isStreaming}
      onCancel={handleCancelRequest}
      attachments={attachedImages}
      onAttachmentsChange={setAttachedImages}
      supportsMultimodal={supportsMultimodal}
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
    <div className="fixed left-0 right-0 flex flex-row overflow-hidden safe-top" style={{
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
      // No global top bar anymore (desktop chrome = sidebar rail) — the
      // safe-top class handles the mobile notch; desktop needs no top pad.
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

      {/* Mobile Sidebar - drawer */}
      {!isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
        <SessionSidebar
          open={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
          isDesktop={false}
        />
      )}

      {/* (Legacy mobile FAB removed — session toggle now lives in MobilePageHeader) */}

      {/* Main Content */}
      <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        {/* Mobile per-page header: hamburger (nav drawer) + sessions toggle + new session */}
        <MobilePageHeader
          title={noLlmContent
            ? t('common:llmGuide.title')
            : (() => {
              const s = sessions.find((x) => x.sessionId === urlSessionId)
              return s?.title || t('chat:input.newChat')
            })()}
          leftExtra={
            !isDesktop && !noLlmContent && (sessions.length > 0 || !isWelcomeMode) ? (
              <Button
                variant="ghost"
                size="icon"
                className="shrink-0"
                onClick={() => setSidebarOpen(true)}
                aria-label={t('common:session.history')}
              >
                <MessageSquare className="h-5 w-5" />
              </Button>
            ) : undefined
          }
          actions={
            // No "+" while the LLM setup guide owns the screen — a new session
            // can't be talked to yet; the guide's two CTAs are the actions.
            !isDesktop && !noLlmContent ? (
              <Button
                variant="ghost"
                size="icon"
                className="shrink-0"
                onClick={async () => {
                  const id = await createSession()
                  if (id) navigate(`/chat/${id}`)
                }}
                aria-label={t('chat:input.newChat')}
              >
                <Plus className="h-5 w-5" />
              </Button>
            ) : undefined
          }
        />
        {/* Chat Content Area */}
        <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
        {noLlmContent ? (
          /* LLM setup guide / first-load skeleton — replaces the chat area
             while the header (mobile nav) stays mounted above it. */
          noLlmContent
        ) : isWelcomeMode && !sessionsLoaded ? (
          /* Loading state while sessions are being loaded - prevents race condition */
          <div className="flex-1 min-h-0 flex items-center justify-center">
            <div className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="h-5 w-5 animate-spin" />
              <span className="text-sm">{t('common:loading')}</span>
            </div>
          </div>
        ) : isWelcomeMode ? (
          /* Welcome Area - shown on /chat (no sessionId), scrollable on mobile */
          <div
            className={cn("touch-scroll flex min-h-0 flex-1 flex-col overflow-y-auto px-4 sm:px-6 py-4 sm:py-6 pb-6", isDesktop && "pt-14")}
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return
              handleBackdropClick()
            }}
          >
            {/* Composer slots in as the middle of the centered group
                (greeting → input → suggestions), ChatGPT-style */}
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
          <div className={cn("flex-1 min-h-0 overflow-y-auto px-2 sm:px-4 py-2 sm:py-4", isDesktop && "pt-12")}>
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
          <div
            ref={scrollContainerRef}
            onScroll={handleScroll}
            className={cn("touch-scroll relative flex-1 min-h-0 overflow-y-auto px-2 sm:px-4 pt-6 pb-2 sm:pb-4 md:pt-20")}
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return
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
              onScrollToBottom={scrollToBottom}
              endRef={messagesEndRef}
            />
          </div>
        ) : (
          /* Empty chat - shown on /chat/:sessionId with no messages yet */
          <div
            className="flex-1 min-h-0 flex items-center justify-center px-4 py-4 sm:py-6"
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return
              handleBackdropClick()
            }}
          >
            <div className="text-center space-y-4 max-w-md">
              <div className="w-16 h-16 rounded-xl bg-muted flex items-center justify-center mx-auto">
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

        {/* Input Area - flex child of chat column. With
            `interactive-widget=resizes-content` in the viewport meta, the
            chat root's `height: 100dvh` shrinks naturally on keyboard open
            (iOS 16.4+ / Android Chrome), and `shrink-0` keeps the input
            pinned to the bottom of the visible area — no fixed-position
            hacks needed. No background: transparent to match the
            conversation area above and the desktop input.
            Welcome mode doesn't render this dock at all: its composer is
            centered inside WelcomeArea instead. The no-LLM setup state
            doesn't either — there is no backend to send to. */}
        {!isWelcomeMode && !noLlmContent && (
        <div
          className="shrink-0 px-2.5 sm:px-4 pt-3 pb-[calc(1.25rem+env(safe-area-inset-bottom,0px))] sm:pt-3 sm:pb-[calc(1.5rem+env(safe-area-inset-bottom,0px))] border-0"
          style={isDesktop ? undefined : { paddingBottom: 'max(1rem, env(safe-area-inset-bottom, 12px))' }}
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

            {composerEl}
          </div>
        </div>
        )}
      </div>
    </div>

    
    </>
  )
}
