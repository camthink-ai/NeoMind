import { type Message, type ExecutionPlan } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolProcessBlock } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { MessageItem } from "./MessageItem"
import { ExecutionPlanPanel } from "./ExecutionPlanPanel"
import { useStore } from "@/store"
import { useMemo, useCallback, useRef, type RefObject } from "react"
import { useTranslation } from "react-i18next"
import { useVirtualizer } from "@tanstack/react-virtual"
import { mergeMessagesForDisplay } from "@/lib/messageUtils"
import { Loader2, Bot } from "lucide-react"

interface MergedMessageListProps {
  messages: Message[]
  scrollElementRef: RefObject<HTMLDivElement | null>
  isStreaming?: boolean
  streamingContent?: string
  streamingThinking?: string
  streamingToolCalls?: any[]
  executionPlan?: ExecutionPlan | null
  planStepStates?: Record<number, 'pending' | 'running' | 'completed' | 'failed'>
  roundContents?: Record<number, string>
}

export function MergedMessageList({
  messages,
  scrollElementRef,
  isStreaming = false,
  streamingContent = "",
  streamingThinking = "",
  streamingToolCalls = [],
  executionPlan,
  planStepStates,
  roundContents = {},
}: MergedMessageListProps) {
  const user = useStore((s) => s.user)
  const { t } = useTranslation("chat")

  // Memoize merged messages to avoid recalculation on every render
  const displayMessages = useMemo(() => mergeMessagesForDisplay(messages), [messages])

  // Pin last 2 messages outside virtualizer for smooth streaming transition.
  // This prevents layout shift when the streaming block is replaced by the saved message.
  const PIN_COUNT = 2
  const virtualMessages = useMemo(() => {
    return displayMessages.length > PIN_COUNT
      ? displayMessages.slice(0, -PIN_COUNT)
      : []
  }, [displayMessages])

  const pinnedMessages = useMemo(() => {
    return displayMessages.length > PIN_COUNT
      ? displayMessages.slice(-PIN_COUNT)
      : displayMessages
  }, [displayMessages])

  // Get user initials
  const getUserInitials = useCallback((username: string) => {
    return username.slice(0, 2).toUpperCase()
  }, [])

  // Virtual scrolling — uses the parent scroll container (ChatContainer's overflow-y-auto div)
  // to avoid nested scrolling issues
  const virtualizer = useVirtualizer({
    count: virtualMessages.length,
    getScrollElement: () => scrollElementRef.current,
    estimateSize: () => 120,
    overscan: 5,
  })

  // Show content when we have messages, or when streaming (even if messages are empty)
  const hasContent = displayMessages.length > 0 || isStreaming

  return (
    <>
      {!hasContent ? null : (
        <div className="space-y-6">
          {/* Virtual list for existing messages */}
          {virtualMessages.length > 0 && (
            <div
              style={{
                height: virtualizer.getTotalSize(),
                width: '100%',
                position: 'relative',
              }}
            >
              {virtualizer.getVirtualItems().map((virtualItem) => {
                const message = virtualMessages[virtualItem.index]
                return (
                  <div
                    key={virtualItem.key}
                    data-index={virtualItem.index}
                    ref={virtualizer.measureElement}
                    style={{
                      position: 'absolute',
                      top: 0,
                      left: 0,
                      width: '100%',
                      transform: `translateY(${virtualItem.start}px)`,
                    }}
                  >
                    <MessageItem
                      message={message}
                      user={user}
                      getUserInitials={getUserInitials}
                    />
                  </div>
                )
              })}
            </div>
          )}

          {/* Pinned recent messages — rendered directly (no virtualizer) for smooth transition
              from streaming block to saved message without layout shift */}
          {pinnedMessages.map((message) => (
            <MessageItem
              key={message.id}
              message={message}
              user={user}
              getUserInitials={getUserInitials}
            />
          ))}

          {/* Streaming message — always rendered outside the virtualizer */}
          {isStreaming && (
            <div className="flex gap-3 items-start justify-start">
              <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-muted flex items-center justify-center">
                <Bot className="h-4 w-4 text-foreground" />
              </div>
              <div className="flex-1 min-w-0">
                {/* Execution plan */}
                {executionPlan && (
                  <ExecutionPlanPanel
                    plan={executionPlan}
                    stepStates={planStepStates ?? {}}
                  />
                )}

                {/* Thinking */}
                {streamingThinking && (
                  <ThinkingBlock thinking={streamingThinking} isStreaming={true} />
                )}

                {/* Tool calls with per-round intermediate text */}
                {streamingToolCalls.length > 0 && (
                  <ToolProcessBlock
                    toolCalls={streamingToolCalls}
                    isStreaming={true}
                    roundContents={roundContents}
                  />
                )}

                {/* Streaming content with blinking cursor */}
                {streamingContent ? (
                  <div>
                    <MarkdownMessage content={streamingContent} className="px-3" />
                    <span className="inline-block w-0.5 h-[1.1em] ml-0.5 bg-current align-text-bottom animate-pulse" />
                  </div>
                ) : !streamingThinking && streamingToolCalls.length === 0 && (
                  <div className="flex items-center gap-3 py-2">
                    <Loader2 className="h-4 w-4 animate-spin opacity-60" />
                    <span className="text-sm opacity-60">{t("thinking.thinking")}</span>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </>
  )
}
