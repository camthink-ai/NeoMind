/**
 * ChatMessages — shared message rendering for the chat page and the global
 * chat panel. One source of truth for the three-layer assistant layout
 * (thinking / tool process / final answer), user bubbles, and the per-message
 * action row (timestamp + copy + scroll-to-bottom).
 *
 * The parent owns the scroll container and the streaming state; this
 * component renders the final messages plus the in-flight synthetic message
 * in the same loop so the streaming→saved transition keeps its React key
 * (no flicker).
 */

import { useMemo, type RefObject } from "react"
import { useTranslation } from "react-i18next"
import { ChevronDown, Sparkles } from "lucide-react"
import type { ChatImage, Message, UserInfo } from "@/types"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
import { cleanToolCallJson, mergeMessagesForDisplay } from "@/lib/messageUtils"
import { isThinkingDuplicate } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolProcessBlock } from "./ToolCallVisualization"
import { CopyMessageButton } from "./CopyMessageButton"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"

interface ChatMessagesProps {
  /** Raw (filtered) messages — merged here for display. */
  messages: Message[]
  user: UserInfo | null
  // Streaming state — when isStreaming is true a synthetic assistant message
  // is appended to the same render loop.
  isStreaming: boolean
  streamingContent: string
  streamingThinking: string
  /** Completed rounds' thinking, keyed by round number. */
  streamingRoundThinking: Record<number, string>
  streamingToolCalls: any[]
  roundContents: Record<number, string>
  currentRound: number
  /** Id the streaming message will keep once persisted (smooth transition). */
  streamingMessageId?: string | null
  onScrollToBottom?: () => void
  endRef?: RefObject<HTMLDivElement>
}

/** Image gallery for user messages */
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

export function ChatMessages({
  messages,
  user,
  isStreaming,
  streamingContent,
  streamingThinking,
  streamingRoundThinking,
  streamingToolCalls,
  roundContents,
  currentRound,
  streamingMessageId,
  onScrollToBottom,
  endRef,
}: ChatMessagesProps) {
  const { t } = useTranslation("chat")
  const getUserInitials = (username: string) => username.slice(0, 2).toUpperCase()

  const displayMessages = useMemo(() => mergeMessagesForDisplay(messages), [messages])

  // Build display list including the streaming message (same loop = same
  // React key = no flicker between streaming and saved states).
  const allMessages = useMemo(() => {
    const list = [...displayMessages]
    if (isStreaming) {
      // Build per-round thinking: completed rounds + current round
      const mergedRoundThinking = { ...streamingRoundThinking }
      const completedThinking = Object.values(streamingRoundThinking).join("")
      const currentRoundThinking = streamingThinking.startsWith(completedThinking)
        ? streamingThinking.slice(completedThinking.length)
        : streamingThinking
      if (currentRoundThinking) {
        mergedRoundThinking[currentRound] = currentRoundThinking
      }
      // Streaming message: same shape as persisted messages.
      // content = final answer (streams at bottom), tool_calls = process (above).
      // Clean round_contents to remove JSON/markdown artifacts from small models
      const cleanedStreamingRoundContents = Object.keys(roundContents).length > 0
        ? Object.fromEntries(
            Object.entries(roundContents).map(([k, v]) => [k, cleanToolCallJson(v)])
          )
        : undefined
      list.push({
        id: streamingMessageId || "__streaming__",
        role: "assistant" as const,
        content: streamingContent,
        thinking: streamingThinking || undefined,
        tool_calls: streamingToolCalls.length > 0 ? streamingToolCalls : undefined,
        timestamp: Date.now(),
        round_thinking: Object.keys(mergedRoundThinking).length > 0 ? mergedRoundThinking : undefined,
        round_contents: cleanedStreamingRoundContents,
        _isStreaming: true,
      } as Message & { _isStreaming?: boolean })
    }
    return list
  }, [displayMessages, isStreaming, streamingContent, streamingThinking, streamingRoundThinking, streamingToolCalls, roundContents, currentRound, streamingMessageId])

  return (
    <div className="max-w-3xl mx-auto space-y-4 sm:space-y-6">
      {allMessages.map((message, idx) => {
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
                    ? "rounded-lg px-3 py-2 sm:px-4 sm:py-3 bg-[var(--msg-user-bg)] text-[var(--msg-user-text)]"
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
                    // Clean round contents to remove any JSON/markdown artifacts
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
                {message.role === 'assistant' && !isCurrentlyStreaming && message.generationMs != null && message.generationMs > 0 && (
                  <span className="text-xs text-muted-foreground/80 tabular-nums">
                    {(message.generationMs / 1000).toFixed(1)}s {Math.round(message.content.length / (message.generationMs / 1000))} chars/s
                  </span>
                )}
                {!isCurrentlyStreaming && (
                  <CopyMessageButton content={copyContent} />
                )}
                {/* Always-visible scroll-to-bottom, next to the copy
                    button on assistant replies */}
                {message.role === "assistant" && !isCurrentlyStreaming && onScrollToBottom && (
                  <button
                    type="button"
                    onClick={onScrollToBottom}
                    className="inline-flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    aria-label={t("scrollToBottom", "回到底部")}
                    title={t("scrollToBottom", "回到底部")}
                  >
                    <ChevronDown className="h-3.5 w-3.5" />
                  </button>
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
        )
      })}

      <div ref={endRef} />
    </div>
  )
}
