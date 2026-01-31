/**
 * MergedMessageList - Merges fragmented assistant messages at render time
 *
 * This component handles the case where the backend stores assistant responses
 * as multiple messages (thinking+tools first, then content separately).
 * It merges them for display without modifying the original data.
 */

import { type Message } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolCallVisualization } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { Sparkles } from "lucide-react"
import { useStore } from "@/store"
import { MessageItem } from "./MessageItem"
import { useMemo } from "react"

interface MergedMessageListProps {
  messages: Message[]
  isStreaming?: boolean
  streamingContent?: string
  streamingThinking?: string
  streamingToolCalls?: any[]
}

/**
 * Combine two content strings without duplicating when backend sent the same content twice.
 */
function dedupeContentConcat(a: string, b: string): string {
  const x = (a || '').trim()
  const y = (b || '').trim()
  if (!y) return a || ''
  if (!x) return b || ''
  if (x === y) return a
  if (x.endsWith(y)) return a
  if (y.startsWith(x)) return b
  if (x.includes(y)) return a
  return (a || '') + (b || '')
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

      // Collect content (will dedupe when joining)
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

    // Set merged content, deduplicating so the same text is not shown twice
    mergedAssistant.content = contentParts.reduce(
      (acc, part) => dedupeContentConcat(acc, part),
      ""
    )

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

  // Memoize merged messages to avoid recalculation on every render
  // Only recompute when messages array reference changes
  const displayMessages = useMemo(() => mergeMessagesForDisplay(messages), [messages])

  // Get user initials
  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  return (
    <>
      {/* Merged messages */}
      {displayMessages.map((message) => (
        <MessageItem
          key={message.id}
          message={message}
          user={user}
          getUserInitials={getUserInitials}
        />
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
                  <span key="dot-1" className="w-2 h-2 rounded-full bg-current animate-bounce delay-0" />
                  <span key="dot-2" className="w-2 h-2 rounded-full bg-current animate-bounce delay-150" />
                  <span key="dot-3" className="w-2 h-2 rounded-full bg-current animate-bounce delay-300" />
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  )
}
