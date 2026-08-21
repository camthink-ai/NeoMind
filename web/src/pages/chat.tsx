import { useEffect, useRef, useState, useCallback, useMemo } from "react"
import { createPortal } from "react-dom"
import { usePageSidebarSlot, PageSidebarColumn } from "@/components/layout/PageSidebarSlot"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { useParams, useNavigate, useSearchParams } from "react-router-dom"
import { generateId } from "@/lib/id"
import { Settings, Sparkles, MessageSquare, Loader2, RotateCcw, Plus } from "lucide-react"
import { Button } from "@/components/ui/button"
import { SessionSidebar } from "@/components/session/SessionSidebar"
import { WelcomeArea } from "@/components/chat/WelcomeArea"
import { ChatMessages } from "@/components/chat/ChatMessages"
import { ChatComposer } from "@/components/chat/ChatComposer"
import { ConnectionStatus } from "@/components/chat/ConnectionStatus"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { ws, type ConnectionState } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { Message, ServerMessage, ChatImage } from "@/types"
import { cn } from "@/lib/utils"
import { getPortalRoot } from "@/lib/portal"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useToast } from "@/hooks/use-toast"
import { useOnboarding } from "@/hooks/useOnboarding"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { LlmSetupGuide } from "@/components/llm/LlmSetupGuide"

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

export function ChatPage() {
  const { t } = useTranslation(['common', 'chat'])
  const { toast } = useToast()
  const { sessionId: urlSessionId } = useParams<{ sessionId?: string }>()
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const [searchParams, setSearchParams] = useSearchParams()
  const { handleError } = useErrorHandler()
  const llmBackends = useStore((state) => state.llmBackends)
  const llmBackendLoading = useStore((state) => state.llmBackendLoading)
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
    isLoadingSession
  } = useStore((s) => ({
    sessionId: s.sessionId,
    messages: s.messages,
    clearMessages: s.clearMessages,
    loadSessions: s.loadSessions,
    isLoadingSession: s.isLoadingSession,
  }), shallow)

  const addMessage = useStore((s) => s.addMessage)
  const createSession = useStore((s) => s.createSession)
  const switchSession = useStore((s) => s.switchSession)
  const user = useStore((s) => s.user)

  // Local state
  const [input, setInput] = useState("")
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  const [streamingToolCalls, setStreamingToolCalls] = useState<any[]>([])
  const [lastTokenUsage, setLastTokenUsage] = useState<{ promptTokens: number } | null>(null)
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const pageSidebarSlot = usePageSidebarSlot()
  // Track the ID of the last assistant message for tool call result updates
  const [lastAssistantMessageId, setLastAssistantMessageId] = useState<string | null>(null)

  // Pending stream recovery state (for reconnect)
  const [pendingStream, setPendingStream] = useState<{
    hasPending: boolean
    content: string
    thinking: string
    userMessage: string
  } | null>(null)

  // WebSocket connection state (for reconnection UI)
  const [connectionState, setConnectionState] = useState<ConnectionState>({ status: 'disconnected' })

  // Image upload state
  const [attachedImages, setAttachedImages] = useState<ChatImage[]>([])

  // Responsive
  const isDesktop = useIsDesktop()

  // Refs
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  // Captured streaming state for use in end event (state updates are async)
  const capturedStreamingRef = useRef({ content: "", thinking: "", toolCalls: [] as any[] })
  // Round tracking for multi-round tool calling
  const [roundContents, setRoundContents] = useState<Record<number, string>>({})
  const [streamingRoundThinking, setStreamingRoundThinking] = useState<Record<number, string>>({})
  // Active tool-loop round, derived from the last in-flight tool call — no
  // own state to reset (all streaming resets clear streamingToolCalls).
  const activeToolRound = streamingToolCalls.length > 0
    ? (streamingToolCalls[streamingToolCalls.length - 1].round ?? null)
    : null
  const currentRoundRef = useRef(1)
  const roundContentsAccumulatorRef = useRef<Record<number, string>>({})
  // Accumulate thinking across all rounds (interleaved thinking pattern)
  const thinkingAccumulatorRef = useRef("")
  // Per-round thinking for grouped rendering
  const roundThinkingAccumulatorRef = useRef<Record<number, string>>({})

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

  // Onboarding auto-detect: show getting-started dialog for new installations
  const [onboardingOpen, setOnboardingOpen] = useState(false)
  const hasShownOnboarding = useRef(false)
  const { status: onboardingStatus, dismiss: dismissOnboarding } = useOnboarding()

  useEffect(() => {
    if (hasShownOnboarding.current || !onboardingStatus) return
    // Auto-open onboarding if not dismissed and has incomplete steps
    if (!onboardingStatus.dismissed && (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)) {
      hasShownOnboarding.current = true
      const timer = setTimeout(() => setOnboardingOpen(true), 1000)
      return () => clearTimeout(timer)
    }
  }, [onboardingStatus])

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

  // Handle WebSocket events
  useEffect(() => {
    const handleMessage = (data: ServerMessage) => {
      switch (data.type) {
        case "Thinking":
          setIsStreaming(true)
          // Immediately update ref synchronously before setState
          capturedStreamingRef.current.thinking += (data.content || "")
          setStreamingThinking(prev => prev + (data.content || ""))
          break

        case "Content":
          setIsStreaming(true)
          // Immediately update ref synchronously before setState
          capturedStreamingRef.current.content += (data.content || "")
          setStreamingContent(prev => prev + (data.content || ""))
          break

        case "ToolCallStart": {
          const toolCall = {
            id: generateId(),
            name: data.tool,
            arguments: data.arguments,
            result: null,
            round: data.round ?? currentRoundRef.current
          }
          // Immediately update ref synchronously before setState
          capturedStreamingRef.current.toolCalls = [...capturedStreamingRef.current.toolCalls, toolCall]
          setStreamingToolCalls(prev => [...prev, toolCall])
          break
        }

        case "ToolCallEnd": {
          // Match FIRST unresolved tool call with same name (not all)
          const tcIdx = capturedStreamingRef.current.toolCalls.findIndex(
            tc => tc.name === data.tool && tc.result === null
          )
          if (tcIdx !== -1) {
            const updated = [...capturedStreamingRef.current.toolCalls]
            updated[tcIdx] = { ...updated[tcIdx], result: data.result }
            capturedStreamingRef.current.toolCalls = updated
          }
          setStreamingToolCalls(prev => {
            const idx = prev.findIndex(
              tc => tc.name === data.tool && tc.result === null
            )
            if (idx === -1) return prev
            const updated = [...prev]
            updated[idx] = { ...updated[idx], result: data.result }
            return updated
          })
          break
        }

        case "end": {
          // Capture token usage from backend
          if (data.tokenUsage?.promptTokens) {
            setLastTokenUsage({ promptTokens: data.tokenUsage.promptTokens })
          }
          const toolCalls = capturedStreamingRef.current.toolCalls
          // Accumulate thinking from current round into total
          // Store all raw data; PerRoundBlocks handles dedup during rendering
          if (capturedStreamingRef.current.thinking) {
            thinkingAccumulatorRef.current += capturedStreamingRef.current.thinking
            roundThinkingAccumulatorRef.current[currentRoundRef.current] = capturedStreamingRef.current.thinking
          }
          const thinking = thinkingAccumulatorRef.current
          // Last round's content becomes the final message content
          const lastRoundContent = capturedStreamingRef.current.content
          // NOTE: Do NOT save last round's content to roundContents — it is the
          // final message content and will be shown as the main response.
          // Only intermediate rounds' content (saved in IntermediateEnd) goes into round_contents.
          const hasRoundContents = Object.keys(roundContentsAccumulatorRef.current).length > 0
          const hasRoundThinking = Object.keys(roundThinkingAccumulatorRef.current).length > 0
          const messageContent = lastRoundContent
          if (messageContent || thinking || toolCalls.length > 0) {
            const messageId = streamingMessageIdRef.current || generateId()
            const completeMessage: Message = {
              id: messageId,
              role: "assistant",
              content: messageContent,
              timestamp: Date.now(),
              thinking: thinking || undefined,
              tool_calls: toolCalls.length > 0 ? toolCalls : undefined,
              round_contents: hasRoundContents ? roundContentsAccumulatorRef.current : undefined,
              round_thinking: hasRoundThinking ? roundThinkingAccumulatorRef.current : undefined,
            }
            addMessage(completeMessage)
            setLastAssistantMessageId(messageId)
          }
          setIsStreaming(false)
          setStreamingContent("")
          setStreamingThinking("")
          setStreamingToolCalls([])
          setRoundContents({})
          setStreamingRoundThinking({})
          // Reset captured ref
          capturedStreamingRef.current = { content: "", thinking: "", toolCalls: [] }
          streamingMessageIdRef.current = null
          currentRoundRef.current = 1
          roundContentsAccumulatorRef.current = {}
          thinkingAccumulatorRef.current = ""
          roundThinkingAccumulatorRef.current = {}
          break
        }

        case "IntermediateEnd":
        case "intermediate_end": {
          // Save current round's content to roundContents
          if (capturedStreamingRef.current.content) {
            roundContentsAccumulatorRef.current[currentRoundRef.current] = capturedStreamingRef.current.content
          }
          // Save per-round thinking for grouped rendering
          if (capturedStreamingRef.current.thinking) {
            thinkingAccumulatorRef.current += capturedStreamingRef.current.thinking
            roundThinkingAccumulatorRef.current[currentRoundRef.current] = capturedStreamingRef.current.thinking
          }
          // Reset captured content for next round
          // NOTE: Don't reset streamingThinking — keep showing all rounds' thinking continuously
          // streamingThinking already has all thinking via cumulative appends in "Thinking" handler
          capturedStreamingRef.current.content = ""
          capturedStreamingRef.current.thinking = ""
          currentRoundRef.current += 1
          setRoundContents({ ...roundContentsAccumulatorRef.current })
          setStreamingRoundThinking({ ...roundThinkingAccumulatorRef.current })
          setStreamingContent("")
          break
        }

        case "Error":
          // Don't immediately stop streaming — the backend may send a fallback
          // summary after the error (e.g., when tool calls failed). The End
          // event will finalize the streaming state.
          // Display error message to user with error styling
          {
            const errorMessage = data.message || "An error occurred during processing"
            const errorMsg: Message = {
              id: generateId(),
              role: "assistant",
              content: `❌ **Error**: ${errorMessage}`,
              timestamp: Date.now(),
            }
            addMessage(errorMsg)
          }
          break

        case "Warning":
          // Display warning message (non-blocking)
          const warningMessage = data.message || "Warning"
          const warningMsg: Message = {
            id: generateId(),
            role: "assistant",
            content: `⚠️ **Warning**: ${warningMessage}`,
            timestamp: Date.now(),
            isPartial: true,  // Mark as temporary/partial
          }
          addMessage(warningMsg)
          break

        case "session_created":
        case "session_switched":
          // Only switch if it's a different session to avoid unnecessary API calls
          if (data.sessionId && data.sessionId !== sessionId) {
            switchSession(data.sessionId)
          }
          break
      }
    }

    const unsubscribe = ws.onMessage(handleMessage)
    return () => { void unsubscribe() }
  }, [addMessage, switchSession, sessionId])

  // Check for pending stream after reconnection
  useEffect(() => {
    const unsubscribe = ws.onConnection((connected, isReconnect) => {
      if (connected && isReconnect && sessionId) {
        // Check if there's a pending stream from before disconnection
        api.getPendingStream(sessionId).then(result => {
          if (result?.hasPending) {
            setPendingStream({
              hasPending: true,
              content: result.content || "",
              thinking: result.thinking || "",
              userMessage: result.userMessage || "",
            })
            // Restore streaming state
            setStreamingContent(result.content || "")
            setStreamingThinking(result.thinking || "")
            setIsStreaming(true)
          }
        }).catch(() => {
          // Ignore errors checking pending stream
        })
      }
    })
    return () => { void unsubscribe() }
  }, [sessionId])

  // Subscribe to WebSocket connection state changes
  useEffect(() => {
    const unsubscribe = ws.onStateChange(setConnectionState)
    return () => { void unsubscribe() }
  }, [])

  // Send message - in welcome mode, create session and navigate
  const handleSend = async (e?: React.MouseEvent | React.KeyboardEvent) => {
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
    setIsStreaming(true)
    streamingMessageIdRef.current = generateId()
    setLastAssistantMessageId(null)
    // Reset round tracking
    currentRoundRef.current = 1
    roundContentsAccumulatorRef.current = {}
    thinkingAccumulatorRef.current = ""
    roundThinkingAccumulatorRef.current = {}
    setRoundContents({})

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

  // Handle pending stream recovery - restore
  const handleRestorePendingStream = () => {
    if (pendingStream) {
      // The streaming state is already restored, just clear the prompt
      setPendingStream(null)
    }
  }

  // Handle pending stream recovery - discard
  const handleDiscardPendingStream = async () => {
    if (sessionId && pendingStream) {
      // Clear pending stream from server
      await api.clearPendingStream(sessionId).catch(() => {})
      // Reset streaming state
      setIsStreaming(false)
      setStreamingContent("")
      setStreamingThinking("")
      setStreamingToolCalls([])
      capturedStreamingRef.current = { content: "", thinking: "", toolCalls: [] }
    }
    setPendingStream(null)
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

    // Reset streaming state
    setIsStreaming(false)
    setStreamingContent("")
    setStreamingThinking("")
    setStreamingToolCalls([])
    capturedStreamingRef.current = { content: "", thinking: "", toolCalls: [] }
    streamingMessageIdRef.current = null

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

  const getUserInitials = (username: string) => {
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

  // Context usage — real prompt tokens after a turn, chars/3 estimate otherwise
  const contextUsage = useMemo(() => {
    if (messages.length === 0 || isWelcomeMode) return null
    const activeBackend = llmBackends.find(b => b.id === activeBackendId)
    const maxContext = activeBackend?.capabilities?.max_context ?? 8192
    const promptTokens = lastTokenUsage?.promptTokens
    let used: number
    if (promptTokens != null && !isStreaming) {
      used = promptTokens
    } else {
      const msgChars = messages.reduce((sum, m) => sum + (m.content?.length ?? 0), 0)
      const streamChars = (streamingContent?.length ?? 0) + (streamingThinking?.length ?? 0)
        + streamingToolCalls.reduce((s, tc) => s + (tc.arguments?.length ?? 0) + (tc.result?.length ?? 0), 0)
      used = Math.ceil((msgChars + streamChars) / 3)
    }
    return { used, max: maxContext }
  }, [messages, isWelcomeMode, llmBackends, activeBackendId, lastTokenUsage, isStreaming, streamingContent, streamingThinking, streamingToolCalls])

  // Show LLM setup prompt if not configured (only after loading completes)
  if (!llmBackendLoading && (!llmBackends || llmBackends.length === 0)) {
    return <LlmSetupGuide />
  }

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
      {/* Pending stream recovery dialog */}
      {pendingStream?.hasPending && createPortal(
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg-80 backdrop-blur-sm">
          <div className="max-w-md w-full mx-4 bg-card border border-border rounded-lg shadow-lg p-6">
            <div className="flex items-center gap-3 mb-4">
              <div className="h-10 w-10 rounded-full bg-info-light flex items-center justify-center">
                <RotateCcw className="h-5 w-5 text-info" />
              </div>
              <div>
                <h3 className="font-semibold">{t('common:session.resumeResponseTitle')}</h3>
                <p className="text-sm text-muted-foreground">
                  {t('common:session.resumeResponseDesc')}
                </p>
              </div>
            </div>

            {(pendingStream.content || pendingStream.thinking) && (
              <div className="mb-4 p-3 bg-muted rounded-lg text-sm">
                {pendingStream.thinking && (
                  <div className="mb-2 text-muted-foreground italic">
                    {pendingStream.thinking.slice(0, 200)}
                    {pendingStream.thinking.length > 200 && "..."}
                  </div>
                )}
                {pendingStream.content && (
                  <div>
                    {pendingStream.content.slice(0, 200)}
                    {pendingStream.content.length > 200 && "..."}
                  </div>
                )}
              </div>
            )}

            <div className="flex gap-2">
              <Button
                variant="outline"
                className="flex-1"
                onClick={handleDiscardPendingStream}
              >
                {t('common:session.discard')}
              </Button>
              <Button
                className="flex-1"
                onClick={handleRestorePendingStream}
              >
                {t('common:session.resume')}
              </Button>
            </div>
          </div>
        </div>,
        getPortalRoot()
      )}

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
          title={(() => {
            const s = sessions.find((x) => x.sessionId === urlSessionId)
            return s?.title || t('chat:input.newChat')
          })()}
          leftExtra={
            !isDesktop && (sessions.length > 0 || !isWelcomeMode) ? (
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
            !isDesktop ? (
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
        {isWelcomeMode && !sessionsLoaded ? (
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
            <WelcomeArea className="min-h-full" onQuickAction={handleQuickAction} />
          </div>
        ) : isLoadingSession ? (
          /* Loading State - shown when switching sessions, with skeleton messages */
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
            className={cn("touch-scroll relative flex-1 min-h-0 overflow-y-auto px-2 sm:px-4 pt-6 pb-2 sm:pb-4 pb-4 md:pt-20")}
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
              currentRound={currentRoundRef.current}
              streamingMessageId={streamingMessageIdRef.current}
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
            conversation area above and the desktop input. */}
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
              maxHeight={isDesktop ? 160 : 100}
            />
          </div>
        </div>
      </div>
    </div>

    <OnboardingDialog
      open={onboardingOpen}
      onOpenChange={setOnboardingOpen}
      status={onboardingStatus}
      onDismiss={dismissOnboarding}
    />
    </>
  )
}
