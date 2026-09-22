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
  /** Bump to refetch (the sibling composer increments it after sending). */
  refreshToken?: number
}

interface AgentUserMessagesComposerProps {
  agentId: string
  /** Fired after a successful send so the list can refetch/scroll. */
  onSent?: (message: UserMessage) => void
}

/** The message list. The composer that belongs to it renders separately
 *  (pinned to the detail page's bottom bar) — the list here only reads. */
export function AgentUserMessages({ agentId, onMessageAdded, refreshToken = 0 }: AgentUserMessagesProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const [messages, setMessages] = useState<UserMessage[]>([])
  const [loading, setLoading] = useState(false)
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
    // refreshToken: the composer lives outside this component; a send bumps
    // the token so the list picks the new note up.
  }, [agentId, refreshToken])

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    if (messages.length > 0) {
      bottomRef.current?.scrollIntoView({ block: 'nearest' })
    }
  }, [messages])

  const handleDeleteMessage = async (messageId: string) => {
    try {
      await api.deleteAgentUserMessage(agentId, messageId)
      setMessages(prev => prev.filter(m => m.id !== messageId))
    } catch (error) {
      handleError(error, { operation: 'Delete user message', showToast: false })
    }
  }

  return (
    <div className="space-y-3">
            {loading ? (
              <div className="flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                {t('common:loading')}
              </div>
            ) : messages.length === 0 ? (
              <div className="flex min-h-[280px] flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border text-center">
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
  )
}

/**
 * The composer for agent notes. Rendered by the detail page's bottom bar
 * (pinned, chat-style) rather than inside the list, so it stays put while
 * the section scrolls.
 */
export function AgentUserMessagesComposer({ agentId, onSent }: AgentUserMessagesComposerProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const [newMessage, setNewMessage] = useState("")
  const [sending, setSending] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const handleSend = async () => {
    if (!newMessage.trim() || sending) return

    const content = newMessage.trim()
    setNewMessage("")
    setSending(true)

    try {
      const message = await api.addAgentUserMessage(agentId, content)
      onSent?.(message)
    } catch (error) {
      handleError(error, { operation: 'Send message to agent', showToast: false })
      setNewMessage(content) // restore on error
    } finally {
      setSending(false)
      textareaRef.current?.focus()
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Enter sends; Shift+Enter inserts a newline.
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      void handleSend()
    }
  }

  return (
    <div className="flex items-center gap-1.5 rounded-lg border border-input bg-card py-1.5 pl-3 pr-1.5">
      <Textarea
        ref={textareaRef}
        value={newMessage}
        onChange={(e) => setNewMessage(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={t('agents:userMessages.placeholder')}
        rows={1}
        className="min-h-0 max-h-[120px] flex-1 resize-none border-0 bg-transparent p-0 text-sm shadow-none focus-visible:ring-0"
        disabled={sending}
      />
      <Button
        size="icon"
        onClick={() => void handleSend()}
        disabled={!newMessage.trim() || sending}
        className="h-7 w-7 shrink-0 rounded-md"
        aria-label={t('common:send')}
      >
        {sending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Send className="h-3.5 w-3.5" />}
      </Button>
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
    <div className="group relative rounded-lg bg-muted px-3 py-2">
      <button
        onClick={onDelete}
        aria-label={t('common:delete')}
        className={cn(
          "absolute right-1.5 top-1.5 rounded p-1 opacity-0 transition-opacity group-hover:opacity-100",
          "text-muted-foreground hover:bg-card hover:text-error"
        )}
        title={t('common:delete')}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>

      <p className="pr-6 text-sm leading-relaxed whitespace-pre-wrap break-words">
        {message.content}
      </p>
      <p className="mt-1 text-[11px] tabular-nums text-muted-foreground">
        {formatTimestamp(message.timestamp, false)}
        {message.message_type ? ` · ${message.message_type}` : ''}
      </p>
    </div>
  )
}
