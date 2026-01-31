/**
 * Session Drawer - Slide-out sidebar for chat history
 * Simplified session management with auto-naming and auto-cleanup
 */

import { useState, useEffect, useRef, memo } from "react"
import { useTranslation } from "react-i18next"
import {
  X,
  Search,
  Plus,
  MessageSquare,
  Clock,
  Trash2,
  Loader2
} from "lucide-react"
import { useStore } from "@/store"
import type { ChatSession } from "@/types"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { confirm } from "@/hooks/use-confirm"

interface SessionDrawerProps {
  open: boolean
  onClose: () => void
  onNewSession: () => void
  onSelectSession: (sessionId: string) => void
  currentSessionId: string | null
}

interface GroupedSessions {
  today: ChatSession[]
  yesterday: ChatSession[]
  week: ChatSession[]
  older: ChatSession[]
}

function groupSessionsByTime(sessions: ChatSession[]): GroupedSessions {
  const now = Date.now()
  const todayStart = new Date().setHours(0, 0, 0, 0)
  const yesterdayStart = todayStart - 24 * 60 * 60 * 1000
  const weekStart = todayStart - 7 * 24 * 60 * 60 * 1000

  return sessions.reduce((groups, session) => {
    const time = session.updatedAt || session.createdAt || now
    if (time >= todayStart) {
      groups.today.push(session)
    } else if (time >= yesterdayStart) {
      groups.yesterday.push(session)
    } else if (time >= weekStart) {
      groups.week.push(session)
    } else {
      groups.older.push(session)
    }
    return groups
  }, { today: [], yesterday: [], week: [], older: [] } as GroupedSessions)
}

function SessionItem({
  session,
  isActive,
  onClick,
  onDelete,
  t
}: {
  session: ChatSession
  isActive: boolean
  onClick: () => void
  onDelete: (e: React.MouseEvent) => void
  t: (key: string, params?: any) => string
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left p-3 rounded-lg transition-all duration-200 group relative",
        "hover:bg-[var(--session-item-hover)]",
        isActive && "bg-[var(--session-item-active)] border border-[var(--border)]"
      )}
    >
      <div className="flex items-start gap-3">
        <div className={cn(
          "mt-0.5 flex-shrink-0",
          isActive ? "text-blue-500" : "text-muted-foreground"
        )}>
          <MessageSquare className="h-4 w-4" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between gap-2">
            <h4 className={cn(
              "text-sm font-medium truncate",
              isActive ? "text-foreground" : "text-muted-foreground"
            )}>
              {session.title || t('defaultTitle')}
            </h4>
            <span className={cn(
              "flex items-center gap-1 text-xs flex-shrink-0",
              isActive ? "text-foreground/70" : "text-muted-foreground"
            )}>
              <Clock className="h-3 w-3" />
              {session.updatedAt ? formatTimestamp(session.updatedAt / 1000, false) : '-'}
            </span>
          </div>
          {session.preview && (
            <p className="text-xs text-muted-foreground truncate mt-1">
              {session.preview}
            </p>
          )}
          <div className="flex items-center gap-2 mt-1.5">
            <span className="text-xs text-muted-foreground">
              {session.messageCount || 0} {t('messages')}
            </span>
          </div>
        </div>
      </div>

      {/* Delete button - shows on hover */}
      <button
        onClick={onDelete}
        className={cn(
          "absolute right-2 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100",
          "p-1.5 rounded-md transition-all",
          "hover:bg-destructive/10 hover:text-destructive",
          "text-muted-foreground"
        )}
        title={t('deleteSession')}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </button>
  )
}

// Performance optimization: Memoize SessionDrawer to prevent unnecessary re-renders
// Only re-renders when open, currentSessionId, or sessions.length changes
const SessionDrawerMemo = memo(function SessionDrawer({
  open,
  onClose,
  onNewSession,
  onSelectSession,
  currentSessionId
}: SessionDrawerProps) {
  const { t } = useTranslation('dashboard')
  const { sessions, deleteSession } = useStore()
  const [searchQuery, setSearchQuery] = useState("")
  const [isDeleting, setIsDeleting] = useState<string | null>(null)
  const [isCreating, setIsCreating] = useState(false)
  const searchInputRef = useRef<HTMLInputElement>(null)

  // Focus search input when drawer opens
  useEffect(() => {
    if (open) {
      const timer = setTimeout(() => {
        searchInputRef.current?.focus()
      }, 100)
      return () => clearTimeout(timer)
    }
  }, [open])

  // Filter sessions by search query
  const filteredSessions = sessions.filter(session => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return (
      (session.title || "").toLowerCase().includes(query) ||
      (session.preview || "").toLowerCase().includes(query)
    )
  })

  const grouped = groupSessionsByTime(filteredSessions)

  const handleNewSession = async () => {
    if (isCreating) return
    setIsCreating(true)
    await onNewSession()
    setIsCreating(false)
  }

  const handleDelete = async (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation()
    if (isDeleting) return

    const confirmed = await confirm({
      title: t('deleteSessionTitle'),
      description: t('deleteDesc'),
      confirmText: t('confirmDelete'),
      cancelText: t('cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    setIsDeleting(sessionId)
    try {
      await deleteSession(sessionId)
      // If deleted current session, close drawer
      if (sessionId === currentSessionId) {
        onClose()
      }
    } finally {
      setIsDeleting(null)
    }
  }

  const totalSessions = filteredSessions.length

  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/50 z-40 animate-in fade-in duration-200"
          onClick={onClose}
        />
      )}

      {/* Drawer */}
      <div
        className={cn(
          "fixed top-0 left-0 h-full w-80 z-50",
          "bg-[var(--session-drawer-bg)] border-r border-[var(--session-drawer-border)]",
          "transition-transform duration-300 ease-out",
          open ? "translate-x-0" : "-translate-x-full"
        )}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[var(--session-drawer-border)]">
          <h2 className="text-lg font-semibold">{t('conversationHistory')}</h2>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-[var(--session-item-hover)] transition-colors"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Search */}
        <div className="p-4">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <input
              ref={searchInputRef}
              type="text"
              placeholder={t('searchPlaceholder')}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className={cn(
                "w-full pl-10 pr-4 py-2.5 rounded-lg",
                "bg-[var(--card-hover-bg)] border border-[var(--border)]",
                "text-sm text-foreground placeholder:text-muted-foreground",
                "focus:outline-none focus:ring-2 focus:ring-blue-500/50"
              )}
            />
          </div>
        </div>

        {/* New Session Button */}
        <div className="px-4 pb-4">
          <button
            onClick={handleNewSession}
            disabled={isCreating}
            className={cn(
              "w-full flex items-center justify-center gap-2",
              "px-4 py-2.5 rounded-lg",
              "bg-blue-600 hover:bg-blue-700 text-white",
              "transition-colors duration-200",
              "disabled:opacity-50 disabled:cursor-not-allowed"
            )}
          >
            {isCreating ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Plus className="h-4 w-4" />
            )}
            {t('newChat')}
          </button>
        </div>

        {/* Session List */}
        <div className="flex-1 overflow-y-auto px-4 pb-4">
          {totalSessions === 0 ? (
            <div className="text-center py-12">
              <MessageSquare className="h-12 w-12 mx-auto text-muted-foreground/50 mb-3" />
              <p className="text-sm text-muted-foreground">
                {searchQuery ? t('noMatchingSessions') : t('noSessionsYet')}
              </p>
            </div>
          ) : (
            <div className="space-y-4">
              {/* Today */}
              {grouped.today.length > 0 && (
                <div className="space-y-2">
                  <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                    {t('today')}
                  </h3>
                  {grouped.today.map(session => (
                    <SessionItem
                      key={session.sessionId}
                      session={session}
                      isActive={session.sessionId === currentSessionId}
                      onClick={() => {
                        onSelectSession(session.sessionId)
                        onClose()
                      }}
                      onDelete={(e) => handleDelete(e, session.sessionId)}
                      t={t}
                    />
                  ))}
                </div>
              )}

              {/* Yesterday */}
              {grouped.yesterday.length > 0 && (
                <div className="space-y-2">
                  <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                    {t('yesterday')}
                  </h3>
                  {grouped.yesterday.map(session => (
                    <SessionItem
                      key={session.sessionId}
                      session={session}
                      isActive={session.sessionId === currentSessionId}
                      onClick={() => {
                        onSelectSession(session.sessionId)
                        onClose()
                      }}
                      onDelete={(e) => handleDelete(e, session.sessionId)}
                      t={t}
                    />
                  ))}
                </div>
              )}

              {/* This Week */}
              {grouped.week.length > 0 && (
                <div className="space-y-2">
                  <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                    {t('thisWeek')}
                  </h3>
                  {grouped.week.map(session => (
                    <SessionItem
                      key={session.sessionId}
                      session={session}
                      isActive={session.sessionId === currentSessionId}
                      onClick={() => {
                        onSelectSession(session.sessionId)
                        onClose()
                      }}
                      onDelete={(e) => handleDelete(e, session.sessionId)}
                      t={t}
                    />
                  ))}
                </div>
              )}

              {/* Older */}
              {grouped.older.length > 0 && (
                <div className="space-y-2">
                  <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                    {t('older')}
                  </h3>
                  {grouped.older.map(session => (
                    <SessionItem
                      key={session.sessionId}
                      session={session}
                      isActive={session.sessionId === currentSessionId}
                      onClick={() => {
                        onSelectSession(session.sessionId)
                        onClose()
                      }}
                      onDelete={(e) => handleDelete(e, session.sessionId)}
                      t={t}
                    />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="p-4 border-t border-[var(--session-drawer-border)]">
          <p className="text-xs text-muted-foreground text-center">
            共 {totalSessions} 个会话
            <span className="mx-2">•</span>
            自动清理30天前的会话
          </p>
        </div>
      </div>
    </>
  )
})

export { SessionDrawerMemo as SessionDrawer }
