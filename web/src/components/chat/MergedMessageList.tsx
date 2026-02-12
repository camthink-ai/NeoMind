import { type Message } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolCallVisualization } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { useStore } from "@/store"
import { useMemo, useEffect, useState } from "react"
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

  // Memoize merged messages to avoid recalculation on every render
  const displayMessages = useMemo(() => mergeMessagesForDisplay(messages), [messages])

  // Get user initials
  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  // Handle session change with proper loading state
  useEffect(() => {
    // Reset loading state when session changes
    setHasValidData(false)

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
        // Actual messages
        <div className="message-list-container">
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
