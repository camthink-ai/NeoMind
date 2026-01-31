import React from "react"
import { type Message, type UserInfo } from "@/types"
import { ThinkingBlock } from "./ThinkingBlock"
import { ToolCallVisualization } from "./ToolCallVisualization"
import { QuickActions } from "./QuickActions"
import { MarkdownMessage } from "./MarkdownMessage"
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
    return (
      <div
        className={`flex gap-3 ${message.role === "user" ? "justify-end" : "justify-start"}`}
      >
        {message.role === "assistant" && (
          <div className="flex-shrink-0 w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center">
            <Sparkles className="h-4 w-4 text-white" />
          </div>
        )}

        <div className={`max-w-[80%] ${message.role === "user" ? "order-1" : ""}`}>
          <div
            className={`message-bubble-${message.role} rounded-2xl px-4 py-3 ${
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
              <MarkdownMessage content={message.content} variant={message.role as 'user' | 'assistant'} />
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
            {formatTimestamp(message.timestamp, false)}
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
