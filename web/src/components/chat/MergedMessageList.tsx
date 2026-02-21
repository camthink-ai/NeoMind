import { type Message } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolCallVisualization } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { MessageItem } from "./MessageItem"
import { useStore } from "@/store"
import { useMemo, useEffect, useState, useRef, useCallback } from "react"
import { mergeMessagesForDisplay } from "@/lib/messageUtils"
import { Loader2 } from "lucide-react"
import { selectSessionId } from "@/store/selectors"

interface MergedMessageListProps {
  messages: Message[]
  isStreaming?: boolean
  streamingContent?: string
  streamingThinking?: string
  streamingToolCalls?: any[]
}

// Performance optimization: Limit rendered messages to avoid DOM bloat
// Only render recent messages by default, load more on scroll to top
const MAX_RENDERED_MESSAGES = 100
const LOAD_MORE_THRESHOLD = 20 // Load more when 20 messages from top
const LOAD_MORE_COUNT = 50 // Number of messages to load each time

export function MergedMessageList({
  messages,
  isStreaming = false,
  streamingContent = "",
  streamingThinking = "",
  streamingToolCalls = [],
}: MergedMessageListProps) {
  const { user } = useStore()
  const sessionId = useStore(selectSessionId)

  // Track if we're in a valid session with data
  const [hasValidData, setHasValidData] = useState(false)

  // Windowed rendering state
  const [renderOffset, setRenderOffset] = useState(0)
  const [hiddenAboveCount, setHiddenAboveCount] = useState(0)

  // Ref for scroll detection
  const listRef = useRef<HTMLDivElement>(null)
  const topTriggerRef = useRef<HTMLDivElement>(null)

  // Memoize merged messages to avoid recalculation on every render
  const displayMessages = useMemo(() => mergeMessagesForDisplay(messages), [messages])

  // Calculate which messages to render
  const { visibleMessages, hiddenTopCount, hiddenBottomCount } = useMemo(() => {
    const totalMessages = displayMessages.length

    // If message count is under limit, render all
    if (totalMessages <= MAX_RENDERED_MESSAGES) {
      return {
        visibleMessages: displayMessages,
        hiddenTopCount: 0,
        hiddenBottomCount: 0,
      }
    }

    // Apply windowing: show most recent messages
    const visibleStart = Math.max(0, totalMessages - MAX_RENDERED_MESSAGES - renderOffset)
    const visibleMessages = displayMessages.slice(visibleStart)

    return {
      visibleMessages,
      hiddenTopCount: visibleStart,
      hiddenBottomCount: 0,
    }
  }, [displayMessages, renderOffset])

  // Load more messages when scrolling near top
  const loadMore = useCallback(() => {
    if (hiddenTopCount > 0) {
      const newOffset = Math.max(0, renderOffset - LOAD_MORE_COUNT)
      setRenderOffset(newOffset)

      // Preserve scroll position after content is added
      requestAnimationFrame(() => {
        if (listRef.current) {
          const scrollTop = listRef.current.scrollTop
          const triggerHeight = topTriggerRef.current?.offsetHeight || 0
          listRef.current.scrollTop = scrollTop + triggerHeight
        }
      })
    }
  }, [hiddenTopCount, renderOffset])

  // Intersection observer for lazy loading
  useEffect(() => {
    const trigger = topTriggerRef.current
    if (!trigger || hiddenTopCount === 0) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) {
          loadMore()
        }
      },
      { root: listRef.current, rootMargin: '100px' }
    )

    observer.observe(trigger)
    return () => observer.disconnect()
  }, [hiddenTopCount, loadMore])

  // Get user initials
  const getUserInitials = useCallback((username: string) => {
    return username.slice(0, 2).toUpperCase()
  }, [])

  // Handle session change with proper loading state
  useEffect(() => {
    // Reset loading state when session changes
    setHasValidData(false)
    setRenderOffset(0)
    setHiddenAboveCount(0)

    // Then mark that we have valid data after a short delay
    const timer = setTimeout(() => {
      setHasValidData(true)
    }, 150)

    return () => clearTimeout(timer)
  }, [sessionId])

  // Don't show content until we have valid data
  const shouldShowContent = hasValidData && displayMessages.length > 0

  return (
    <>
      {!shouldShowContent ? (
        // Loading state - show when session changes or no data
        <div className="flex items-center justify-center py-16">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          <span className="text-sm text-muted-foreground ml-2">加载中...</span>
        </div>
      ) : (
        // Actual messages with windowed rendering
        <div ref={listRef} className="message-list-container overflow-y-auto">
          {/* Hidden messages indicator at top */}
          {hiddenTopCount > 0 && (
            <div
              ref={topTriggerRef}
              className="flex items-center justify-center py-2 text-sm text-muted-foreground cursor-pointer hover:bg-muted/50 transition-colors"
              onClick={loadMore}
            >
              <Loader2 className="h-4 w-4 mr-2 animate-spin" />
              显示更早的消息 ({hiddenTopCount} 条历史消息)
            </div>
          )}

          {/* Visible messages */}
          {visibleMessages.map((message) => (
            <MessageItem
              key={message.id}
              message={message}
              user={user}
              getUserInitials={getUserInitials}
            />
          ))}

          {/* Streaming message - always visible */}
          {isStreaming && (
            <div className="flex gap-3 justify-start">
              <img src="/logo-square.png" alt="NeoMind" className="flex-shrink-0 w-8 h-8 rounded-lg animate-pulse" />
              <div className="max-w-[80%]">
                <div className="rounded-2xl px-4 py-3 bg-[var(--msg-ai-bg)] text-[var(--msg-ai-text)]">
                  {/* Thinking */}
                  {streamingThinking && (
                    <ThinkingBlock thinking={streamingThinking} />
                  )}

                  {/* Tool calls */}
                  {streamingToolCalls.length > 0 && (
                    <ToolCallVisualization
                      toolCalls={streamingToolCalls}
                      isStreaming={true}
                    />
                  )}

                  {/* Content with blinking cursor when streaming */}
                  {(streamingContent || streamingThinking || streamingToolCalls.length > 0) ? (
                    <>
                      {streamingThinking && (
                        <ThinkingBlock thinking={streamingThinking} />
                      )}

                      {streamingToolCalls.length > 0 && (
                        <ToolCallVisualization
                          toolCalls={streamingToolCalls}
                          isStreaming={true}
                        />
                      )}

                      {streamingContent && (
                        <div className="relative inline">
                          <MarkdownMessage content={streamingContent} isStreaming={true} />
                          {/* Blinking cursor at end of streaming content */}
                          <span className="inline-block w-0.5 h-4 ml-0.5 bg-current align-middle animate-pulse" />
                        </div>
                      )}
                    </>
                  ) : (
                    /* Loading indicator - shown when waiting for first response */
                    <div className="flex items-center gap-3 py-2">
                      <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                      <span className="text-sm text-muted-foreground">正在思考...</span>
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </>
  )
}
