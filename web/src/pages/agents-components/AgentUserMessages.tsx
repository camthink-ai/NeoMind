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
import { Textarea } from "@/components/ui/textarea"
import {
  Loader2,
  MessageSquare,
  Send,
  Trash2,
  
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
  const bottomRef = useRef<HTMLDivElement>(null)

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
    if (messages.length > 0) {
      bottomRef.current?.scrollIntoView({ block: 'nearest' })
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
    <div className="space-y-3">
      {/* Messages List */}
      <div>
        <div>
          <div className="space-y-3">
            {loading ? (
              <div className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                {t('common:loading')}
              </div>
            ) : messages.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-10 text-center">
                <MessageSquare className="h-8 w-8 text-muted-foreground/40" />
                <p className="text-sm text-muted-foreground">{t('agents:userMessages.empty')}</p>
                <p className="text-xs text-muted-foreground/70">{t('agents:userMessages.emptyHint')}</p>
              </div>
            ) : (
              <>
                {messages.map((message) => (
                  <MessageBubble
                    key={message.id}
                    message={message}
                    onDelete={() => handleDeleteMessage(message.id)}
                  />
                ))}
                <div ref={bottomRef} />
              </>
            )}
          </div>
        </div>
      </div>

      {/* Input Area */}
      <div className="rounded-lg border border-border bg-card p-3">
        <Textarea
          ref={textareaRef}
          value={newMessage}
          onChange={(e) => setNewMessage(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t('agents:userMessages.placeholder')}
          className="resize-none min-h-[64px] max-h-[140px] border-0 bg-transparent p-0 shadow-none focus-visible:ring-0"
          disabled={sending}
        />
        <div className="mt-2 flex items-center justify-between gap-2 border-t border-border pt-2">
          <span className="text-xs text-muted-foreground">
            {t('agents:userMessages.hint')} ⌘⏎ / Ctrl+Enter
          </span>
          <Button
            size="sm"
            onClick={handleSendMessage}
            disabled={!newMessage.trim() || sending}
            className="shrink-0"
          >
            {sending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Send className="h-4 w-4" />}
          </Button>
        </div>
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
    <div className="group relative rounded-lg border border-border bg-card px-3 py-2 transition-colors hover:border-foreground/20">
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

      <p className="pr-5 text-sm leading-relaxed whitespace-pre-wrap break-words">
        {message.content}
      </p>
      <p className="mt-1 text-xs tabular-nums text-muted-foreground">
        {formatTimestamp(message.timestamp, false)}
        {message.message_type ? ` · ${message.message_type}` : ''}
      </p>
    </div>
  )
}
