/**
 * MergedMessageList - Merges fragmented assistant messages at render time
 *
 * This component handles the case where the backend stores assistant responses
 * as multiple messages (thinking+tools first, then content separately).
 * It merges them for display without modifying the original data.
 */

import { type Message, type ChatImage } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolCallVisualization } from "./ToolCallVisualization"
import { QuickActions } from "./QuickActions"
import { MarkdownMessage } from "./MarkdownMessage"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { Sparkles } from "lucide-react"
import { useStore } from "@/store"

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
        />
      ))}
    </div>
  )
}

interface MergedMessageListProps {
  messages: Message[]
  isStreaming?: boolean
  streamingContent?: string
  streamingThinking?: string
  streamingToolCalls?: any[]
}

// Format timestamp to readable time
function formatTime(timestamp: number): string {
  const date = new Date(timestamp)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / (1000 * 60))

  if (diffMins < 1) return "刚刚"
  if (diffMins < 60) return `${diffMins}分钟前`

  const diffHours = Math.floor(diffMins / 60)
  if (diffHours < 24) return `${diffHours}小时前`

  const diffDays = Math.floor(diffHours / 24)
  if (diffDays < 7) return `${diffDays}天前`

  return date.toLocaleDateString()
}

/**
 * Check if two assistant messages should be merged.
 *
 * They should be merged if:
 * - First has thinking OR tools
 * - Second has content OR first is missing content (split response)
 * - They are consecutive assistant messages
 *
 * Backend pattern: [thinking+tools] + [content] or [thinking] + [tools+content]
 */
function shouldMergeMessages(first: Message, second: Message): boolean {
  const firstHasThinking = !!first.thinking && first.thinking.length > 0
  const firstHasTools = !!first.tool_calls && first.tool_calls.length > 0
  const firstHasContent = !!first.content && first.content.length > 0

  const secondHasThinking = !!second.thinking && second.thinking.length > 0
  const secondHasTools = !!second.tool_calls && second.tool_calls.length > 0
  const secondHasContent = !!second.content && second.content.length > 0

  // Always merge consecutive assistant messages where first has thinking or tools
  // This handles the backend pattern of splitting responses
  if (firstHasThinking || firstHasTools) {
    // Merge if second has content, OR if first is missing content (split response)
    return !firstHasContent || secondHasContent
  }

  // Also merge if second has thinking or tools and first only has content
  if ((secondHasThinking || secondHasTools) && firstHasContent) {
    return true
  }

  return false
}

/**
 * Merge fragmented assistant messages for display.
 *
 * Rules:
 * 1. User messages are kept as-is
 * 2. Consecutive assistant messages are merged if they should be:
 *    - Take thinking from the first one
 *    - Take tool_calls from the first one (or any that has them)
 *    - Concatenate all content
 *    - Use the earliest timestamp
 * 3. Other roles (system, tool) are filtered out
 */
function mergeMessagesForDisplay(messages: Message[]): Message[] {
  const result: Message[] = []

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i]

    // Skip tool messages (internal use)
    if ((msg as any).role === "tool") continue

    // User messages and system messages are kept as-is
    if (msg.role !== "assistant") {
      result.push(msg)
      continue
    }

    // Assistant messages - check if we should merge with following assistant messages
    const mergedAssistant: Message = { ...msg }
    const contentParts: string[] = []
    if (msg.content) {
      contentParts.push(msg.content)
    }

    // Look ahead for consecutive assistant messages to merge
    let j = i + 1
    while (j < messages.length && messages[j].role === "assistant") {
      const nextMsg = messages[j]

      // Only merge if they should be merged based on the same logic as sessionSlice
      if (!shouldMergeMessages(mergedAssistant, nextMsg)) {
        break
      }

      // Collect content
      if (nextMsg.content) {
        contentParts.push(nextMsg.content)
      }

      // Use thinking from first message that has it
      if (!mergedAssistant.thinking && nextMsg.thinking) {
        mergedAssistant.thinking = nextMsg.thinking
      }

      // Use tool_calls from first message that has them
      if (!mergedAssistant.tool_calls && nextMsg.tool_calls) {
        mergedAssistant.tool_calls = nextMsg.tool_calls
      }

      j++
    }

    // Set merged content
    mergedAssistant.content = contentParts.join("")

    // Only add if there's something to show
    if (mergedAssistant.content || mergedAssistant.thinking || mergedAssistant.tool_calls) {
      result.push(mergedAssistant)
    }

    // Skip the merged messages
    i = j - 1
  }

  return result
}

export function MergedMessageList({
  messages,
  isStreaming = false,
  streamingContent = "",
  streamingThinking = "",
  streamingToolCalls = [],
}: MergedMessageListProps) {
  const { user } = useStore()

  // Debug: log message structure
  console.log("[MergedMessageList] Input messages:", messages.map((m, i) => ({
    index: i,
    id: m.id,
    role: m.role,
    hasThinking: !!m.thinking,
    thinkingLen: m.thinking?.length || 0,
    hasTools: !!m.tool_calls?.length,
    toolsCount: m.tool_calls?.length || 0,
    contentLen: m.content?.length || 0,
    contentPreview: m.content?.substring(0, 50) || "(empty)",
  })))

  // Merge messages for display
  const displayMessages = mergeMessagesForDisplay(messages)

  console.log("[MergedMessageList] Output messages:", displayMessages.map((m, i) => ({
    index: i,
    role: m.role,
    hasThinking: !!m.thinking,
    hasTools: !!m.tool_calls?.length,
    contentLen: m.content?.length || 0,
  })))

  // Get user initials
  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  return (
    <>
      {/* Merged messages */}
      {displayMessages.map((message) => (
        <div
          key={message.id}
          className={`flex gap-3 ${message.role === "user" ? "justify-end" : "justify-start"}`}
        >
          {message.role === "assistant" && (
            <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center">
              <Sparkles className="h-4 w-4 text-white" />
            </div>
          )}

          <div className={`max-w-[80%] ${message.role === "user" ? "order-1" : ""}`}>
            <div
              className={`rounded-2xl px-4 py-3 ${
                message.role === "user"
                  ? "bg-[var(--msg-user-bg)] text-[var(--msg-user-text)]"
                  : "bg-[var(--msg-ai-bg)] text-[var(--msg-ai-text)]"
              }`}
            >
              {/* Images for user messages */}
              {message.role === "user" && message.images && message.images.length > 0 && (
                <MessageImages images={message.images} />
              )}

              {/* Thinking block */}
              {message.thinking && (
                <ThinkingBlock thinking={message.thinking} />
              )}

              {/* Tool calls */}
              {message.tool_calls && message.tool_calls.length > 0 && (
                <ToolCallVisualization
                  toolCalls={message.tool_calls}
                  isStreaming={false}
                />
              )}

              {/* Content */}
              {message.content && (
                <MarkdownMessage content={message.content} />
              )}
            </div>

            {/* Quick actions for assistant messages */}
            {message.role === "assistant" && (
              <QuickActions
                message={message}
                onActionClick={() => {}}
              />
            )}

            {/* Timestamp */}
            <p className="text-xs text-muted-foreground mt-1 px-1">
              {formatTime(message.timestamp)}
            </p>
          </div>

          {message.role === "user" && user && (
            <Avatar className="h-8 w-8 order-2">
              <AvatarFallback className="bg-blue-600 text-white text-xs">
                {getUserInitials(user.username)}
              </AvatarFallback>
            </Avatar>
          )}
        </div>
      ))}

      {/* Streaming message */}
      {isStreaming && (
        <div className="flex gap-3 justify-start">
          <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center">
            <Sparkles className="h-4 w-4 text-white animate-pulse" />
          </div>
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

              {/* Content */}
              {streamingContent && (
                <MarkdownMessage content={streamingContent} />
              )}

              {/* Loading indicator */}
              {!streamingContent && !streamingThinking && streamingToolCalls.length === 0 && (
                <div className="flex items-center gap-1">
                  <span key="dot-1" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "0ms" }} />
                  <span key="dot-2" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "150ms" }} />
                  <span key="dot-3" className="w-2 h-2 rounded-full bg-current animate-bounce" style={{ animationDelay: "300ms" }} />
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  )
}
