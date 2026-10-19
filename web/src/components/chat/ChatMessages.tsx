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

import { useEffect, useMemo, useRef, useState, type RefObject } from "react"
import { useTranslation } from "react-i18next"
import { Loader2, Sparkles, Info } from "lucide-react"
import type { ChatImage, Message, UserInfo } from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { formatTimestamp, formatElapsedMs } from "@/lib/utils/format"
import { cleanToolCallJson, mergeMessagesForDisplay, dropInFlightTurn } from "@/lib/messageUtils"
import { getServerOrigin } from "@/lib/api"
import { isThinkingDuplicate } from "./toolCallUtils"
import { MarkdownMessage } from "./MarkdownMessage"
import { ThinkingBlock } from "./ThinkingBlock"
import { TurnTimeline } from "./TurnTimeline"
import { CopyMessageButton } from "./CopyMessageButton"
import { ZoomableImage } from "@/components/shared/ZoomableImage"
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
  endRef?: RefObject<HTMLDivElement>
  /** Older history exists beyond the loaded window — renders the
   *  "load earlier" affordance at the top of the list. */
  hasEarlierHistory?: boolean
  loadingEarlier?: boolean
  onLoadEarlier?: () => void
}

/** Image gallery for user messages */
function MessageImages({ images }: { images: ChatImage[] }) {
  const { t } = useTranslation("chat")
  if (!images || images.length === 0) return null

  return (
    <div className={images.length === 1 ? "mb-2" : "mb-2 grid grid-cols-2 gap-2"}>
      {images.map((img, idx) => {
        const src = img.data.startsWith('/api/images/')
          // Persisted reference form — prepend the server origin (the
          // webview origin differs from the API in Tauri / Vite dev).
          ? getServerOrigin() + img.data
          : img.data
        return (
          <ZoomableImage
            key={idx}
            src={src}
            alt={t("chat:image.alt", { index: idx + 1 })}
            className={images.length === 1 ? "my-0" : "my-0 block w-full"}
            imgClassName={images.length === 1 ? "max-h-64" : "max-h-48 w-full object-cover"}
          />
        )
      })}
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
  endRef,
  hasEarlierHistory,
  loadingEarlier,
  onLoadEarlier,
}: ChatMessagesProps) {
  const { t } = useTranslation("chat")
  const getUserInitials = (username: string) => username.slice(0, 2).toUpperCase()

  // Announce turn boundaries, not every streamed chunk. Screen readers get
  // "generating" once when a turn starts and "ready" when it ends; the log
  // itself is not a live region.
  const [announcement, setAnnouncement] = useState("")
  const wasStreamingRef = useRef(false)
  useEffect(() => {
    if (isStreaming) {
      setAnnouncement(t("a11y.generating"))
    } else if (wasStreamingRef.current) {
      setAnnouncement(t("a11y.answerReady"))
    }
    wasStreamingRef.current = isStreaming
  }, [isStreaming, t])

  const displayMessages = useMemo(() => {
    const merged = mergeMessagesForDisplay(messages)
    // While a turn streams (fresh, or restored after switching back), the
    // history already contains the in-flight turn's fragments (the history
    // endpoint serves the LIVE agent history). Rendering them alongside the
    // streaming bubble double-displays the same turn — an extra Thinking
    // block whose content then "restarts" in the bubble below. The bubble
    // owns the in-flight display; the completed turn replaces it on end.
    return isStreaming ? dropInFlightTurn(merged) : merged
  }, [messages, isStreaming])

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
    <div
      role="log"
      aria-busy={isStreaming}
      data-testid="messages-container"
      className="max-w-3xl mx-auto space-y-4 sm:space-y-6"
    >
      <div role="status" aria-live="polite" aria-atomic="true" className="sr-only">
        {announcement}
      </div>
      {hasEarlierHistory && onLoadEarlier && (
        <div className="flex justify-center">
          <button
            onClick={onLoadEarlier}
            disabled={loadingEarlier}
            className={cn(
              "inline-flex items-center gap-1.5 px-3 py-1 rounded-full",
              "border border-border bg-card text-muted-foreground",
              "hover:text-foreground hover:bg-muted-50 transition-colors",
              "disabled:opacity-60",
              textMini
            )}
          >
            {loadingEarlier ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
                {t("loadEarlierLoading")}
              </>
            ) : (
              t("loadEarlier")
            )}
          </button>
        </div>
      )}
      {allMessages.map((message, idx) => {
        const isCurrentlyStreaming = !!(message as any)._isStreaming
        // Copy the message as the user sees it: user messages verbatim;
        // assistant messages with tool calls strip embedded JSON.
        const copyContent = message.role === "user"
          ? (message.content || "")
          : (message.tool_calls && message.tool_calls.length > 0
              ? cleanToolCallJson(message.content || "")
              : (message.content || ""))
        // Reply stats stay available in the timestamp tooltip instead of
        // occupying the action row on every message.
        const statsTitle = message.role === "assistant" && !isCurrentlyStreaming && message.generationMs != null && message.generationMs > 0
          ? [
              formatElapsedMs(message.generationMs),
              message.content.length > 0
                ? `${Math.round(message.content.length / (message.generationMs / 1000)).toLocaleString()} ${t("message.charsPerSecond")}`
                : null,
            ].filter(Boolean).join(" · ")
          : undefined
        return (
          <div
            key={message.id || `msg-${idx}`}
            data-testid={message.role === "assistant" ? "assistant-message" : message.role === "user" ? "user-message" : undefined}
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

            {/* User bubbles hug their content (right-aligned). Assistant
                messages span the FULL column: the thinking/tool cards inside
                size to the widest sibling, so a content-hugging max-width
                made their width jump (short during early streaming, wide
                once long text/tables arrive) — a fixed full width keeps the
                card geometry stable through the whole turn. */}
            <div
              className={
                message.role === "user"
                  ? "order-1 max-w-[85%] sm:max-w-[80%]"
                  : "w-full"
              }
            >
              <div
                className={cn(
                  message.role === "user"
                    ? "rounded-xl px-3 py-2 sm:px-4 sm:py-3 bg-chat-user-bg text-chat-user-text"
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

                    // Round-based process: each round owns its thinking, tools,
                    // and intermediate output. The final answer is rendered
                    // separately below the timeline.
                    // 1. Process timeline (rounds of thinking / tools / output)
                    // 2. Final answer (bottom) - markdown content

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
                          <TurnTimeline
                            toolCalls={message.tool_calls!}
                            roundThinking={filteredRoundThinking}
                            roundContents={cleanedRoundContents}
                            thinking={!hasRoundThinking && !thinkingDupesContent ? message.thinking : undefined}
                            isStreaming={isCurrentlyStreaming}
                            currentRound={currentRound}
                            finalContent={displayContent}
                          />
                          {displayContent ? (
                            <MarkdownMessage content={displayContent} variant="assistant" className="px-3" isStreaming={isCurrentlyStreaming} />
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
                          <MarkdownMessage content={displayContent} variant="assistant" className="px-3" isStreaming={isCurrentlyStreaming} />
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
                  {/* Runtime guard notes (incomplete action, guard budget
                      spent): system callouts, clearly not model speech —
                      single-language, styled apart from the content. */}
                  {message.role === 'assistant' && message.systemNotes && message.systemNotes.length > 0 && (
                    <div className="mt-2 px-3 flex flex-col gap-1.5">
                      {message.systemNotes.map((note, i) => (
                        <div
                          key={i}
                          className="flex items-start gap-2 rounded-lg border border-dashed border-border bg-muted-20 px-3 py-2 text-xs leading-relaxed text-muted-foreground"
                        >
                          <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                          <span>{note}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>

              <div className="mt-1.5 flex items-center gap-2 px-3">
                <time
                  title={statsTitle}
                  className="text-xs text-muted-foreground tabular-nums"
                >
                  {formatTimestamp(message.timestamp, false)}
                </time>
                {!isCurrentlyStreaming && (
                  <div
                    className={cn(
                      "flex items-center gap-0.5 transition-opacity duration-fast",
                      "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 max-md:opacity-100",
                      "pointer-events-none group-hover:pointer-events-auto group-focus-within:pointer-events-auto max-md:pointer-events-auto"
                    )}
                  >
                    <CopyMessageButton content={copyContent} />
                  </div>
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
