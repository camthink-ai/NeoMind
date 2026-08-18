import React from "react"
import { type Message, type UserInfo } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolProcessBlock } from "./ToolCallVisualization"
import { MarkdownMessage } from "./MarkdownMessage"
import { CopyMessageButton } from "./CopyMessageButton"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { Sparkles } from "lucide-react"
import { formatTimestamp } from "@/lib/utils/format"

interface MessageItemProps {
  message: Message
  user: UserInfo | null
  getUserInitials: (username: string) => string
}

/**
 * Memoized message item component.
 * Only re-renders when message.id, content, thinking, tool_calls, or role changes.
 */
export const MessageItem = React.memo<MessageItemProps>(
  ({ message, user, getUserInitials }) => {
    const isAssistant = message.role === "assistant"

    return (
      <div
        className={`group flex gap-3 items-start animate-fade-in-up ${
          isAssistant ? "justify-start" : "justify-end"
        }`}
      >
        {isAssistant && (
          <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-foreground flex items-center justify-center">
            <Sparkles className="h-4 w-4 text-background" />
          </div>
        )}

        {isAssistant ? (
          <div className="flex-1 min-w-0">
            {/* Thinking block */}
            {message.thinking && (
              <ThinkingBlock thinking={message.thinking} />
            )}

            {/* Tool calls */}
            {message.tool_calls && message.tool_calls.length > 0 && (
              <ToolProcessBlock
                toolCalls={message.tool_calls}
                roundContents={message.round_contents}
                isStreaming={false}
              />
            )}

            {/* Content */}
            {message.content && (
              <MarkdownMessage content={message.content} variant="assistant" className="px-3" />
            )}

            {/* Timestamp */}
            <div className="flex items-center gap-1 mt-1.5 px-3">
              <p className="text-xs text-muted-foreground">
                {formatTimestamp(message.timestamp, false)}
              </p>
              <CopyMessageButton content={message.content || ""} />
            </div>
          </div>
        ) : (
          /* User: keep bubble with natural width */
          <div className="max-w-[80%] order-1">
            <div className="rounded-lg px-4 py-3 overflow-hidden bg-[var(--msg-user-bg)] text-[var(--msg-user-text)]">
              {message.images && message.images.length > 0 && (
                <MessageImages images={message.images} />
              )}
              {message.content && (
                <MarkdownMessage content={message.content} variant="user" />
              )}
            </div>
            <div className="flex items-center gap-1 mt-1 px-1">
              <p className="text-xs text-muted-foreground">
                {formatTimestamp(message.timestamp, false)}
              </p>
              <CopyMessageButton content={message.content || ""} />
            </div>
          </div>
        )}

        {message.role === "user" && user && (
          <Avatar className="h-8 w-8 order-2">
            <AvatarFallback className="bg-primary text-primary-foreground text-xs">
              {getUserInitials(user.username)}
            </AvatarFallback>
          </Avatar>
        )}
      </div>
    )
  },
  (prev, next) => {
    // Custom comparison: only re-render if these specific props change
    return (
      prev.message.id === next.message.id &&
      prev.message.content === next.message.content &&
      prev.message.thinking === next.message.thinking &&
      prev.message.tool_calls === next.message.tool_calls &&
      prev.message.role === next.message.role &&
      prev.message.images === next.message.images
    )
  }
)

MessageItem.displayName = "MessageItem"

/** Image gallery component for user messages */
function MessageImages({ images }: { images: any[] }) {
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
