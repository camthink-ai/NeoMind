import { useEffect, useRef, useState, useCallback, useMemo } from "react"
import { createPortal } from "react-dom"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { useParams, useNavigate, useSearchParams } from "react-router-dom"
import { generateId } from "@/lib/id"
import { Settings, Send, Sparkles, PanelLeft, MessageSquare, Zap, ChevronDown, X, Image as ImageIcon, Loader2, Eye, Wrench, RotateCcw, Plus, Check, ArrowUp } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { SessionSidebar } from "@/components/session/SessionSidebar"
import { WelcomeArea } from "@/components/chat/WelcomeArea"
import { MarkdownMessage } from "@/components/chat/MarkdownMessage"
import { CopyMessageButton } from "@/components/chat/CopyMessageButton"
import { ThinkingBlock } from "@/components/chat/ThinkingBlock"
import { ConnectionStatus } from "@/components/chat/ConnectionStatus"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { ToolProcessBlock, isThinkingDuplicate } from "@/components/chat/ToolCallVisualization"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { ws, type ConnectionState } from "@/lib/websocket"
import { api } from "@/lib/api"
import type { Message, ServerMessage, ChatImage } from "@/types"
import { cn } from "@/lib/utils"
import { textNano, textMini, textMicro } from "@/design-system/tokens/typography"
import { getPortalRoot } from "@/lib/portal"
import { formatTimestamp } from "@/lib/utils/format"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useToast } from "@/hooks/use-toast"
import { mergeMessagesForDisplay, cleanToolCallJson } from "@/lib/messageUtils"
import { useOnboarding } from "@/hooks/useOnboarding"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"

/** Image gallery component for user messages */
function MessageImages({ images }: { images: ChatImage[] }) {
  if (!images || images.length === 0) return null

  return (
    <div className={images.length === 1 ? "mb-2" : "mb-2 grid grid-cols-2 gap-2"}>
      {images.map((img, idx) => (
        <img
          key={idx}
          src={img.data}
          alt={`Image ${idx + 1}`}
          className="rounded-lg max-w-full max-h-64 object-cover"
          loading="lazy"
        />
      ))}
    </div>
  )
}

// Hook to detect desktop breakpoint (lg: 1024px)
function useIsDesktop() {
  const [isDesktop, setIsDesktop] = useState(() => {
    if (typeof window === 'undefined') return false
    return window.innerWidth >= 1024
  })

  useEffect(() => {
    const checkIsDesktop = () => setIsDesktop(window.innerWidth >= 1024)
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
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

// Compress image to target size (default 2MB)
async function compressImage(file: File, maxSizeMB: number = 2): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = (e) => {
      const img = new Image()
      img.onload = () => {
        const canvas = document.createElement('canvas')
        let width = img.width
        let height = img.height

        // Calculate new dimensions to reduce file size
        const maxDimension = 2048
        if (width > maxDimension || height > maxDimension) {
          if (width > height) {
            height = (height / width) * maxDimension
            width = maxDimension
          } else {
            width = (width / height) * maxDimension
            height = maxDimension
          }
        }

        canvas.width = width
        canvas.height = height

        const ctx = canvas.getContext('2d')
        if (!ctx) {
          reject(new Error('Failed to get canvas context'))
          return
        }

        ctx.drawImage(img, 0, 0, width, height)

        // Try different quality levels to meet size target
        let quality = 0.9
        const tryCompress = () => {
          canvas.toBlob(
            (blob) => {
              if (!blob) {
                reject(new Error('Failed to compress image'))
                return
              }

              const sizeMB = blob.size / (1024 * 1024)

              // If size is acceptable or quality is too low, use this result
              if (sizeMB <= maxSizeMB || quality <= 0.5) {
                const reader = new FileReader()
                reader.onload = () => resolve(reader.result as string)
                reader.onerror = reject
                reader.readAsDataURL(blob)
              } else {
                // Try lower quality
                quality -= 0.1
                tryCompress()
              }
            },
            'image/jpeg',
            quality
          )
        }

        tryCompress()
      }
      img.onerror = () => reject(new Error('Failed to load image'))
      img.src = e.target?.result as string
    }
    reader.onerror = reject
    reader.readAsDataURL(file)
  })
}

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
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
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
  const [isUploadingImage, setIsUploadingImage] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

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
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()

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
  const [showScrollToBottom, setShowScrollToBottom] = useState(false)

  const handleScroll = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    // Consider "pinned" if within 80px of the bottom — covers minor sub-pixel
    // diffs and the gap inserted by smooth-scroll inertia.
    const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight
    isPinnedToBottomRef.current = distanceFromBottom < 80
    // Reveal the "scroll to bottom" button once the user scrolls a meaningful
    // amount up. setState with an unchanged boolean bails out (no re-render),
    // so this stays cheap even though scroll fires very frequently.
    setShowScrollToBottom(distanceFromBottom >= 240)
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
  // Handle image file selection
  const handleImageSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return

    setIsUploadingImage(true)
    try {
      const newImages: ChatImage[] = []
      for (let i = 0; i < files.length; i++) {
        const file = files[i]
        if (!file.type.startsWith('image/')) continue

        // Limit original file size to 10MB
        if (file.size > 10 * 1024 * 1024) {
          toast({ title: `Image ${file.name} is too large. Maximum size is 10MB.`, variant: "destructive" })
          continue
        }

        // Compress image to 2MB for better performance
        const dataUrl = await compressImage(file, 2)
        newImages.push({
          data: dataUrl,
          mimeType: 'image/jpeg', // Compressed images are always JPEG
        })
      }

      if (newImages.length > 0) {
        setAttachedImages(prev => [...prev, ...newImages])
      }
    } catch (error) {
      handleError(error, { operation: 'Process images', showToast: false })
      toast({ title: t('common:imageProcessFailed'), variant: "destructive" })
    } finally {
      setIsUploadingImage(false)
      // Reset file input
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    }
  }

  // Remove attached image
  const removeAttachedImage = (index: number) => {
    setAttachedImages(prev => prev.filter((_, i) => i !== index))
  }

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

  const displayMessages = useMemo(() =>
    mergeMessagesForDisplay(filteredMessages),
    [filteredMessages]
  )

  // Show chat area if there are messages or currently streaming
  const hasMessages = filteredMessages.length > 0 || isStreaming

  // Show LLM setup prompt if not configured (only after loading completes)
  if (!llmBackendLoading && (!llmBackends || llmBackends.length === 0)) {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <div className="text-center max-w-md px-6">
          <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-xl bg-muted">
            <Settings className="h-8 w-8 text-muted-foreground" />
          </div>
          <h2 className="mb-3 text-lg font-semibold">{t('chat:notConfigured.title')}</h2>
          <p className="text-sm text-muted-foreground mb-6">
            {t('chat:notConfigured.description')}
          </p>
          <Button
            onClick={() => openSettings()}
            className="gap-2"
            size="default"
          >
            <Settings className="h-4 w-4" />
            {t('chat:notConfigured.goToSettings')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <>
    <div className="fixed left-0 right-0 flex flex-row overflow-hidden safe-top" style={{
      // Offset past the desktop AppSidebar (0 on mobile, where the sidebar
      // is unmounted and the var resets to 0px).
      left: 'var(--app-sidebar-width, 0px)',
      // Anchor to the top of the VISIBLE area, not the layout viewport. iOS
      // PWA standalone doesn't honor `interactive-widget=resizes-content`, so
      // when the soft keyboard opens iOS scrolls the visualViewport
      // (visualViewport.offsetTop becomes > 0) instead of shrinking the
      // layout viewport. position:fixed `top:0` is relative to the LAYOUT
      // viewport, which would leave the chat container stranded above the
      // visible area. Use --visual-viewport-offset-top to follow the visible
      // area. Always 0 in Safari (where the layout viewport itself shrinks).
      top: 'var(--visual-viewport-offset-top, 0px)',
      // Desktop: TopNav height (set by useVisualViewport via --topnav-height).
      // Mobile: --topnav-height is 0 (no global nav); safe-top class handles
      // the notch via env(safe-area-inset-top). Don't combine them in a
      // `var()` fallback — when the variable is explicitly "0px" the fallback
      // is ignored and chat content ends up under the notch.
      paddingTop: 'var(--topnav-height, 0px)',
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

      {/* Desktop Sidebar - always show when there are sessions or in chat mode */}
      {isDesktop && (sessions.length > 0 || !isWelcomeMode) && (
        <div className="shrink-0 self-stretch">
          <SessionSidebar
            open={true}
            onClose={() => {}}
            collapsed={sidebarCollapsed}
            onToggleCollapse={() => setSidebarCollapsed(!sidebarCollapsed)}
            isDesktop={true}
          />
        </div>
      )}
      {/* Desktop sidebar skeleton while sessions are loading (only when sidebar isn't shown yet) */}
      {isDesktop && !sessionsLoaded && !(sessions.length > 0 || !isWelcomeMode) && (
        <div className="shrink-0 self-stretch w-64 border-r flex flex-col p-3 space-y-2">
          <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
          <div className="h-8 w-full bg-muted rounded-lg animate-pulse" />
          <div className="h-8 w-2/3 bg-muted rounded-lg animate-pulse" />
        </div>
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
            className="touch-scroll flex min-h-0 flex-1 flex-col overflow-y-auto px-4 sm:px-6 py-4 sm:py-6 pb-6"
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
          <div className="flex-1 min-h-0 overflow-y-auto px-2 sm:px-4 py-2 sm:py-4">
            <div className="max-w-3xl mx-auto space-y-4 sm:space-y-6">
              {/* Skeleton message - user */}
              <div className="flex gap-2 sm:gap-3 justify-end animate-pulse">
                <div className="max-w-[85%] sm:max-w-[80%]">
                  <div className="rounded-2xl px-3 py-2 sm:px-4 sm:py-3 bg-muted">
                    <div className="h-4 w-48 bg-muted rounded" />
                  </div>
                </div>
                <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-muted" />
              </div>
              {/* Skeleton message - assistant */}
              <div className="flex gap-2 sm:gap-3 justify-start animate-pulse">
                <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-muted" />
                <div className="max-w-[85%] sm:max-w-[80%]">
                  <div className="rounded-2xl px-3 py-2 sm:px-4 sm:py-3 bg-muted">
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
                  <div className="rounded-2xl px-3 py-2 sm:px-4 sm:py-3 bg-muted">
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
            className="touch-scroll relative flex-1 min-h-0 overflow-y-auto px-2 sm:px-4 py-2 sm:py-4 pb-4"
            onClick={(e) => {
              // If clicking outside interactive elements, dismiss keyboard
              if ((e.target as HTMLElement).closest('button, a, input, textarea, [role="button"]')) return
              handleBackdropClick()
            }}
          >
            <div className="max-w-3xl mx-auto space-y-4 sm:space-y-6">
              {(() => {
                // Build display list including streaming message (same loop = same React key = no flicker)
                const allMessages = [...displayMessages]
                if (isStreaming) {
                  // Build per-round thinking: completed rounds + current round
                  const mergedRoundThinking = { ...streamingRoundThinking }
                  const completedThinking = Object.values(streamingRoundThinking).join("")
                  const currentRoundThinking = streamingThinking.startsWith(completedThinking)
                    ? streamingThinking.slice(completedThinking.length)
                    : streamingThinking
                  if (currentRoundThinking) {
                    mergedRoundThinking[currentRoundRef.current] = currentRoundThinking
                  }
                  // Streaming message: same shape as persisted messages
                  // content = final answer (streams at bottom), tool_calls = process (above)
                  // Clean round_contents to remove JSON/markdown artifacts from small models
                  const cleanedStreamingRoundContents = Object.keys(roundContents).length > 0
                    ? Object.fromEntries(
                        Object.entries(roundContents).map(([k, v]) => [k, cleanToolCallJson(v)])
                      )
                    : undefined;
                  allMessages.push({
                    id: streamingMessageIdRef.current || '__streaming__',
                    role: 'assistant' as const,
                    content: streamingContent,
                    thinking: streamingThinking || undefined,
                    tool_calls: streamingToolCalls.length > 0 ? streamingToolCalls : undefined,
                    timestamp: Date.now(),
                    round_thinking: Object.keys(mergedRoundThinking).length > 0 ? mergedRoundThinking : undefined,
                    round_contents: cleanedStreamingRoundContents,
                    _isStreaming: true,
                  } as Message & { _isStreaming?: boolean })
                }
                return allMessages.map((message, idx) => {
                  const isCurrentlyStreaming = !!(message as any)._isStreaming
                  // Copy the message as the user sees it: user messages verbatim;
                  // assistant messages with tool calls strip embedded JSON.
                  const copyContent = message.role === "user"
                    ? (message.content || "")
                    : (message.tool_calls && message.tool_calls.length > 0
                        ? cleanToolCallJson(message.content || "")
                        : (message.content || ""))
                  return (
                <div
                  key={message.id || `msg-${idx}`}
                  className={`group flex gap-2 sm:gap-3 animate-fade-in-up ${message.role === "user" ? "justify-end" : "justify-start"}`}
                >
                  {message.role === "assistant" && (
                    <div className="flex-shrink-0 w-6 h-6 sm:w-8 sm:h-8 rounded-lg bg-foreground flex items-center justify-center">
                      <Sparkles className={cn(
                        "h-4 w-4 sm:h-4 sm:w-4 text-background",
                        isCurrentlyStreaming && "animate-pulse"
                      )} />
                    </div>
                  )}

                  <div className={`max-w-[85%] sm:max-w-[80%] ${message.role === "user" ? "order-1" : ""}`}>
                    <div
                      className={cn(
                        message.role === "user"
                          ? "rounded-2xl px-3 py-2 sm:px-4 sm:py-3 bg-[var(--msg-user-bg)] text-[var(--msg-user-text)]"
                          : ""
                      )}
                    >
                      <div className={message.role === "user" ? "message-bubble-user" : "message-bubble-assistant"}>
                      {/* Images for user messages */}
                      {message.role === "user" && message.images && message.images.length > 0 && (
                        <MessageImages images={message.images} />
                      )}
                      {/* User messages: just content */}
                      {message.role === "user" && message.content && (
                        <MarkdownMessage content={message.content} variant="user" />
                      )}
                      {/* Assistant messages: tool process + final content */}
                      {message.role === "assistant" && (() => {
                        const hasTools = message.tool_calls && message.tool_calls.length > 0
                        // Clean embedded tool call JSON from content for display
                        const displayContent = hasTools ? cleanToolCallJson(message.content || '') : (message.content || '')
                        // Clean round contents to remove any JSON/markdown artifacts from small models
                        const cleanedRoundContents = message.round_contents
                          ? Object.fromEntries(
                              Object.entries(message.round_contents).map(([k, v]) => [k, cleanToolCallJson(v)])
                            )
                          : undefined

                        // Three-layer design:
                        // 1. Thinking (top) - with per-round differentiation
                        // 2. Task Process (middle) - tool calls + round content
                        // 3. Final Answer (bottom) - markdown content

                        // Determine thinking to show
                        const hasRoundThinking = message.round_thinking && Object.keys(message.round_thinking).length > 0
                        const hasThinking = !!message.thinking
                        // Skip thinking if it duplicates final content (Phase 2 LLM echo)
                        const thinkingDupesContent = hasThinking && message.content
                          && isThinkingDuplicate(message.thinking, message.content)
                        // For round_thinking, dedup last round against content
                        let filteredRoundThinking = message.round_thinking
                        if (hasRoundThinking && message.content) {
                          const rounds = Object.entries(message.round_thinking!)
                            .map(([k, v]) => [Number(k), v] as [number, string])
                            .sort((a, b) => a[0] - b[0])
                          if (rounds.length > 0) {
                            const lastRound = rounds[rounds.length - 1]
                            if (isThinkingDuplicate(lastRound[1], message.content)) {
                              // Remove last round if it duplicates content
                              filteredRoundThinking = { ...message.round_thinking! }
                              delete filteredRoundThinking[lastRound[0]]
                              if (Object.keys(filteredRoundThinking).length === 0) {
                                filteredRoundThinking = undefined
                              }
                            }
                          }
                        }

                        const showThinking = (hasRoundThinking && !!filteredRoundThinking) || (hasThinking && !thinkingDupesContent && !hasRoundThinking)

                        if (hasTools) {
                          return (
                            <>
                              {showThinking && (
                                <ThinkingBlock
                                  thinking={!hasRoundThinking ? message.thinking : undefined}
                                  roundThinking={filteredRoundThinking}
                                  isStreaming={isCurrentlyStreaming}
                                  defaultExpanded={false}
                                />
                              )}
                              <ToolProcessBlock
                                toolCalls={message.tool_calls!}
                                roundContents={cleanedRoundContents}
                                isStreaming={isCurrentlyStreaming}
                              />
                              {displayContent ? (
                                <MarkdownMessage content={displayContent} variant="assistant" className="px-3" />
                              ) : isCurrentlyStreaming ? (
                                <div className="flex items-center gap-1 px-3 py-1">
                                  <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '0ms' }} />
                                  <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '150ms' }} />
                                  <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '300ms' }} />
                                </div>
                              ) : null}
                            </>
                          )
                        }

                        // Path B: simple response → Thinking + Content
                        return (
                          <>
                            {showThinking && (
                              <ThinkingBlock
                                thinking={message.thinking}
                                roundThinking={filteredRoundThinking}
                                isStreaming={isCurrentlyStreaming}
                              />
                            )}
                            {displayContent ? (
                              <MarkdownMessage content={displayContent} variant="assistant" className="px-3" />
                            ) : isCurrentlyStreaming ? (
                              <div className="flex items-center gap-1 px-3 py-1">
                                <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '0ms' }} />
                                <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '150ms' }} />
                                <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground opacity-40 animate-bounce" style={{ animationDelay: '300ms' }} />
                              </div>
                            ) : null}
                          </>
                        )
                      })()}
                      </div>
                    </div>

                    <div className="flex items-center gap-1 mt-1.5 px-3">
                      <p className="text-xs text-muted-foreground">
                        {formatTimestamp(message.timestamp, false)}
                      </p>
                      {!isCurrentlyStreaming && (
                        <CopyMessageButton content={copyContent} />
                      )}
                    </div>
                  </div>

                  {message.role === "user" && user && (
                    <Avatar className="h-6 w-6 sm:h-8 sm:w-8 order-2">
                      <AvatarFallback className={cn("bg-muted text-muted-foreground", textNano, "sm:text-xs")}>
                        {getUserInitials(user.username)}
                      </AvatarFallback>
                    </Avatar>
                  )}
                </div>
                )})
              })()}

              <div ref={messagesEndRef} />
            </div>
            {showScrollToBottom && (
              <button
                type="button"
                onClick={scrollToBottom}
                className="absolute bottom-4 right-4 z-10 flex h-9 w-9 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-md transition-all hover:bg-muted active:scale-95"
                aria-label={t("chat:scrollToBottom", "回到底部")}
              >
                <ChevronDown className="h-4 w-4" />
              </button>
            )}
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
              <div className="w-16 h-16 rounded-2xl bg-muted flex items-center justify-center mx-auto">
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
          className="shrink-0 px-2.5 sm:px-4 pt-3 pb-5 sm:pt-3 sm:pb-6 safe-bottom border-0"
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

            {/* Image previews */}
            {attachedImages.length > 0 && (
              <div className="flex flex-wrap gap-1.5 mb-1">
                {attachedImages.map((image, index) => (
                  <div key={index} className="relative group">
                    <img
                      src={image.data}
                      alt={`Attached ${index + 1}`}
                      className="h-8 w-8 sm:h-9 sm:w-9 object-cover rounded-md border border-border"
                    />
                    <button
                      type="button"
                      className="absolute -top-1 -right-1 h-2.5 w-2.5 rounded-full bg-destructive text-destructive-foreground flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity p-0"
                      onClick={() => removeAttachedImage(index)}
                    >
                      <X className="h-2 w-2" />
                    </button>
                  </div>
                ))}
              </div>
            )}

            {/* Single unified input box — everything inside one container */}
            <div className="rounded-2xl border border-input bg-card shadow-sm transition-colors">
              {/* Textarea — fills the top, borderless */}
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t('chat:input.placeholder')}
                rows={1}
                className={cn(
                  "w-full block px-4 pt-3 pb-1 resize-none text-sm leading-5 bg-transparent",
                  "placeholder:text-muted-foreground",
                  "focus:outline-none",
                  "max-h-[100px] lg:max-h-40 scroll-mb-32"
                )}
                style={{ minHeight: "44px" }}
                onInput={(e) => {
                  const target = e.target as HTMLTextAreaElement
                  target.style.height = "auto"
                  const maxHeight = isDesktop ? 160 : 100
                  target.style.height = Math.max(44, Math.min(target.scrollHeight, maxHeight)) + "px"
                }}
              />

              {/* Bottom toolbar — left: image + model + context, right: send */}
              <div className="flex items-center gap-1 px-2 pb-2">
                {/* Image upload */}
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  multiple
                  className="hidden"
                  onChange={handleImageSelect}
                  disabled={isStreaming || !supportsMultimodal}
                />
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => fileInputRef.current?.click()}
                  disabled={isStreaming || !supportsMultimodal}
                  className={cn(
                    "h-8 w-8 rounded-lg flex-shrink-0 text-muted-foreground hover:text-foreground",
                    !supportsMultimodal && "opacity-50"
                  )}
                  title={supportsMultimodal ? t('chat:model.addImage') : t('chat:model.notSupportImage')}
                >
                  {isUploadingImage ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : attachedImages.length > 0 ? (
                    <div className="relative">
                      <ImageIcon className="h-4 w-4" />
                      <span className="absolute -top-1 -right-1 bg-primary text-primary-foreground text-nano rounded-full h-4 w-4 flex items-center justify-center font-semibold tabular-nums">
                        {attachedImages.length}
                      </span>
                    </div>
                  ) : (
                    <ImageIcon className="h-4 w-4" />
                  )}
                </Button>

                {/* Model selector */}
                {llmBackends.length > 0 && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-8 px-2 rounded-lg text-muted-foreground hover:text-foreground text-xs gap-1 max-w-[120px] sm:max-w-[140px]"
                      >
                        <span className="truncate">
                          {llmBackends.find(b => b.id === activeBackendId)?.name ||
                           llmBackends.find(b => b.id === activeBackendId)?.model ||
                           t('chat:input.selectModel')}
                        </span>
                        <ChevronDown className="h-3.5 w-3.5 shrink-0" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="start" className="w-72 max-h-[50vh] overflow-y-auto scrollbar-none p-0">
                      <div className="sticky top-0 z-10 bg-popover px-3 py-2 border-b border-border">
                        <span className="font-semibold text-sm">{t('chat:input.selectLLMModel')}</span>
                      </div>
                      <div className="p-1">
                      {llmBackends.map((backend) => {
                        const isActive = backend.id === activeBackendId
                        return (
                        <DropdownMenuItem
                          key={backend.id}
                          onClick={() => activateBackend(backend.id)}
                          className="gap-2.5 py-2"
                        >
                          {/* Left slot: active → Check, else health dot (symmetrical) */}
                          <div className="w-4 flex items-center justify-center shrink-0">
                            {isActive ? (
                              <Check className="h-4 w-4 text-primary" />
                            ) : (
                              <span className={cn(
                                "w-1.5 h-1.5 rounded-full",
                                backend.healthy ? "bg-success" : "bg-muted-foreground"
                              )} />
                            )}
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-1.5">
                              <p className={cn("text-sm truncate", isActive && "font-medium")}>
                                {backend.name || backend.model}
                              </p>
                              <div className="flex items-center gap-1 shrink-0">
                                {backend.capabilities?.supports_multimodal && (
                                  <span title={t('chat:model.supportsVision')} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-info-light text-info", textMicro)}>{t('chat:capability.vision', { defaultValue: 'Vision' })}</span>
                                )}
                                {backend.capabilities?.supports_tools && (
                                  <span title={t('chat:model.supportsTools')} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-accent-orange-light text-accent-orange", textMicro)}>{t('chat:capability.tools', { defaultValue: 'Tools' })}</span>
                                )}
                                {backend.capabilities?.supports_thinking && (
                                  <span title={t('chat:model.supportsThinking')} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-accent-purple-light text-accent-purple", textMicro)}>{t('chat:capability.thinking', { defaultValue: 'Thinking' })}</span>
                                )}
                              </div>
                            </div>
                            <p className={cn(textNano, "text-muted-foreground truncate mt-0.5")}>
                              {backend.backend_type} · {backend.model}
                            </p>
                          </div>
                        </DropdownMenuItem>
                        )
                      })}
                      </div>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}

                {/* Context usage indicator */}
                {(() => {
                  const activeBackend = llmBackends.find(b => b.id === activeBackendId)
                  const maxContext = activeBackend?.capabilities?.max_context ?? 8192
                  const promptTokens = lastTokenUsage?.promptTokens
                  let displayTokens: number
                  let ratio: number
                  if (promptTokens != null && !isStreaming) {
                    displayTokens = promptTokens
                    ratio = promptTokens / maxContext
                  } else {
                    const msgChars = messages.reduce((sum, m) => sum + (m.content?.length ?? 0), 0)
                    const streamChars = (streamingContent?.length ?? 0) + (streamingThinking?.length ?? 0)
                      + streamingToolCalls.reduce((s, tc) => s + (tc.arguments?.length ?? 0) + (tc.result?.length ?? 0), 0)
                    displayTokens = Math.ceil((msgChars + streamChars) / 3)
                    ratio = displayTokens / maxContext
                  }
                  if (messages.length === 0 || isWelcomeMode) return null
                  return (
                    <span className={cn(
                      textMini, "shrink-0 transition-colors tabular-nums",
                      ratio > 0.9 ? "text-error" : ratio > 0.7 ? "text-warning" : "text-muted-foreground"
                    )}>
                      Context {(displayTokens / 1000).toFixed(1)}K / {(maxContext / 1000).toFixed(0)}K
                    </span>
                  )
                })()}

                <div className="flex-1" />

                {/* Send or Cancel button */}
                {isStreaming ? (
                  <Button
                    type="button"
                    onClick={handleCancelRequest}
                    variant="outline"
                    className="h-8 w-8 rounded-full flex-shrink-0 p-0 border-destructive text-destructive hover:bg-destructive-light"
                    title="Cancel request"
                  >
                    <Loader2 className="h-4 w-4 animate-spin" />
                  </Button>
                ) : (
                  <Button
                    type="button"
                    onClick={handleSend}
                    disabled={!input.trim() && attachedImages.length === 0}
                    className={cn(
                      "h-8 w-8 rounded-full flex-shrink-0 p-0 transition-all",
                      (!input.trim() && attachedImages.length === 0)
                        ? "bg-muted text-muted-foreground"
                        : "bg-primary hover:bg-primary-hover text-primary-foreground"
                    )}
                  >
                    <ArrowUp className="h-4 w-4" />
                  </Button>
                )}
              </div>
            </div>
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
