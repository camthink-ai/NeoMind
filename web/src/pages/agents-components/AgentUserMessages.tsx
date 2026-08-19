/**
 * Agent User Messages Panel
 *
 * Allows users to send messages to agents between executions.
 * These messages provide additional context or corrections that the agent
 * will consider in its next execution.
 */

import { useState, useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import {
  MessageSquare,
  Send,
  Trash2,
  X,
  Clock,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api } from "@/lib/api"
import { formatTimestamp } from "@/lib/utils/format"
import type { UserMessage } from "@/types"

interface AgentUserMessagesProps {
  agentId: string
  onMessageAdded?: () => void
}

export function AgentUserMessages({ agentId, onMessageAdded }: AgentUserMessagesProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const [messages, setMessages] = useState<UserMessage[]>([])
  const [loading, setLoading] = useState(false)
  const [sending, setSending] = useState(false)
  const [newMessage, setNewMessage] = useState("")
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const scrollAreaRef = useRef<HTMLDivElement>(null)

  // Load messages
  const loadMessages = async () => {
    setLoading(true)
    try {
      const data = await api.getAgentUserMessages(agentId)
      setMessages(data)
    } catch (error) {
      handleError(error, { operation: 'Load user messages', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadMessages()
  }, [agentId])

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    if (scrollAreaRef.current && messages.length > 0) {
      const scrollContainer = scrollAreaRef.current.querySelector('[data-radix-scroll-area-viewport]')
      if (scrollContainer) {
        scrollContainer.scrollTop = scrollContainer.scrollHeight
      }
    }
  }, [messages])

  const handleSendMessage = async () => {
    if (!newMessage.trim() || sending) return

    const content = newMessage.trim()
    setNewMessage("")
    setSending(true)

    try {
      const message = await api.addAgentUserMessage(agentId, content)
      setMessages(prev => [...prev, message])
      onMessageAdded?.()
    } catch (error) {
      handleError(error, { operation: 'Send message to agent', showToast: false })
      // Restore message on error
      setNewMessage(content)
    } finally {
      setSending(false)
      textareaRef.current?.focus()
    }
  }

  const handleDeleteMessage = async (messageId: string) => {
    try {
      await api.deleteAgentUserMessage(agentId, messageId)
      setMessages(prev => prev.filter(m => m.id !== messageId))
    } catch (error) {
      handleError(error, { operation: 'Delete user message', showToast: false })
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      handleSendMessage()
    }
  }

  return (
    <div className="flex flex-col h-full">
      {/* Messages List */}
      <div className="flex-1 min-h-0">
        <ScrollArea className="h-full" ref={scrollAreaRef}>
          <div className="p-4 space-y-3">
            {loading ? (
              <div className="text-center py-8 text-muted-foreground text-sm">
                {t('common:loading')}...
              </div>
            ) : messages.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                <MessageSquare className="h-10 w-10 mx-auto mb-2 opacity-20" />
                <p className="text-sm">{t('agents:userMessages.empty')}</p>
                <p className="text-xs mt-1">{t('agents:userMessages.emptyHint')}</p>
              </div>
            ) : (
              messages.map((message) => (
                <MessageBubble
                  key={message.id}
                  message={message}
                  onDelete={() => handleDeleteMessage(message.id)}
                />
              ))
            )}
          </div>
        </ScrollArea>
      </div>

      {/* Input Area */}
      <div className="border-t p-3 bg-muted-20">
        <div className="flex gap-2">
          <Textarea
            ref={textareaRef}
            value={newMessage}
            onChange={(e) => setNewMessage(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t('agents:userMessages.placeholder')}
            className="resize-none min-h-[60px] max-h-[120px]"
            disabled={sending}
          />
          <div className="flex flex-col gap-2">
            <Button
              size="icon"
              onClick={handleSendMessage}
              disabled={!newMessage.trim() || sending}
              className="h-full shrink-0"
            >
              <Send className="h-4 w-4" />
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground mt-2">
          {t('agents:userMessages.hint')} ⌘⏎ / Ctrl+Enter
        </p>
      </div>
    </div>
  )
}

// ============================================================================
// Sub Components
// ============================================================================

interface MessageBubbleProps {
  message: UserMessage
  onDelete: () => void
}

function MessageBubble({ message, onDelete }: MessageBubbleProps) {
  const { t } = useTranslation(['common', 'agents'])

  return (
    <Card className="p-3 relative group">
      <button
        onClick={onDelete}
        aria-label={t('common:delete')}
        className={cn(
          "absolute top-2 right-2 opacity-0 group-hover:opacity-100",
          "transition-opacity p-1 rounded hover:bg-muted",
          "text-muted-foreground hover:text-error"
        )}
        title={t('common:delete')}
      >
        <Trash2 className="h-4 w-4" />
      </button>

      <div className="pr-6">
        <div className="flex items-center gap-2 mb-1.5">
          <MessageSquare className="h-4 w-4 text-muted-foreground" />
          <span className="text-xs text-muted-foreground flex items-center gap-1">
            <Clock className="h-4 w-4" />
            {formatTimestamp(message.timestamp, false)}
          </span>
          {message.message_type && (
            <Badge variant="secondary" className="text-xs h-5">
              {message.message_type}
            </Badge>
          )}
        </div>
        <p className="text-sm whitespace-pre-wrap break-words">
          {message.content}
        </p>
      </div>
    </Card>
  )
}
