/**
 * SessionSidebar - Responsive session management panel
 * - Desktop (lg+): Fixed sidebar on left, collapsible
 * - Mobile: Slide-out drawer
 */

import { useState, useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import type { ChatSession } from "@/types"
import { cn } from "@/lib/utils"
import {
  X,
  Plus,
  Search,
  MessageSquare,
  Trash2,
  Clock,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"

interface SessionSidebarProps {
  /** Mobile drawer mode: open state */
  open: boolean
  /** Mobile drawer mode: close handler */
  onClose: () => void
  /** Desktop mode: collapsed state */
  collapsed?: boolean
  /** Desktop mode: toggle collapse */
  onToggleCollapse?: () => void
  /** Is desktop mode (fixed sidebar) */
  isDesktop?: boolean
}

// Format time ago with i18n
function useFormatTimeAgo() {
  const { t } = useTranslation('common')
  
  return (timestamp: number | undefined): string => {
    if (!timestamp) return ""
    const now = Date.now()
    const diff = now - timestamp

    if (diff < 60 * 1000) return t('timeAgo.justNow')
    if (diff < 60 * 60 * 1000) return t('timeAgo.minutesAgo', { count: Math.floor(diff / (60 * 1000)) })
    if (diff < 24 * 60 * 60 * 1000) return t('timeAgo.hoursAgo', { count: Math.floor(diff / (60 * 60 * 1000)) })
    if (diff < 7 * 24 * 60 * 60 * 1000) return t('timeAgo.daysAgo', { count: Math.floor(diff / (24 * 60 * 60 * 1000)) })
    return new Date(timestamp).toLocaleDateString()
  }
}

export function SessionSidebar({
  open,
  onClose,
  collapsed = false,
  onToggleCollapse,
  isDesktop = false
}: SessionSidebarProps) {
  const { t } = useTranslation('common')
  const formatTimeAgo = useFormatTimeAgo()
  const navigate = useNavigate()

  const {
    sessions,
    sessionId: currentSessionId,
    createSession,
    switchSession,
    deleteSession,
  } = useStore()

  const [searchQuery, setSearchQuery] = useState("")
  const [isCreating, setIsCreating] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)

  // Focus search when opened (mobile only)
  useEffect(() => {
    if (open && !isDesktop) {
      setTimeout(() => {
        searchInputRef.current?.focus()
      }, 200)
    }
  }, [open, isDesktop])

  // Filter sessions
  const filteredSessions = sessions.filter((session) => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return (
      (session.title || "").toLowerCase().includes(query) ||
      (session.preview || "").toLowerCase().includes(query)
    )
  })

  // Sort by update time
  const sortedSessions = [...filteredSessions].sort((a, b) => {
    const timeA = a.updatedAt || a.createdAt || 0
    const timeB = b.updatedAt || b.createdAt || 0
    return timeB - timeA
  })

  // Handle new session
  const handleNewSession = async () => {
    if (isCreating) return
    setIsCreating(true)
    try {
      const newSessionId = await createSession()
      // Navigate to the new session URL
      if (newSessionId) {
        navigate(`/chat/${newSessionId}`)
      } else {
        navigate('/chat')
      }
      if (!isDesktop) onClose()
    } finally {
      setIsCreating(false)
    }
  }

  // Handle switch session
  const handleSwitchSession = async (sessionId: string) => {
    if (sessionId === currentSessionId) {
      if (!isDesktop) onClose()
      return
    }
    await switchSession(sessionId)
    // Navigate to the session URL
    navigate(`/chat/${sessionId}`)
    if (!isDesktop) onClose()
  }

  // Handle delete session
  const handleDeleteSession = async (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation()
    if (deletingId) return
    
    setDeletingId(sessionId)
    try {
      await deleteSession(sessionId)
    } finally {
      setDeletingId(null)
    }
  }

  // Get session title
  const getSessionTitle = (session: ChatSession): string => {
    if (session.title && session.title !== "新对话" && session.title !== "New Chat") {
      return session.title
    }
    return t('session.defaultTitle')
  }

  // Sidebar content - shared between desktop and mobile
  const SidebarContent = ({ showHeader = true }: { showHeader?: boolean }) => (
    <>
      {/* Header */}
      {showHeader && (
        <div className="flex items-center justify-between p-3 border-b border-border/50">
          {!collapsed && <h2 className="text-sm font-semibold">{t('session.sessions')}</h2>}
          {isDesktop ? (
            <Button
              variant="ghost"
              size="icon"
              onClick={onToggleCollapse}
              className={cn("h-7 w-7 rounded-lg", collapsed && "mx-auto")}
            >
              {collapsed ? (
                <PanelLeftOpen className="h-4 w-4" />
              ) : (
                <PanelLeftClose className="h-4 w-4" />
              )}
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="icon"
              onClick={onClose}
              className="h-7 w-7 rounded-lg"
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      )}

      {/* Collapsed mode - only show icons */}
      {collapsed ? (
        <div className="flex flex-col items-center py-2 gap-1">
          <Button
            variant="ghost"
            size="icon"
            onClick={handleNewSession}
            disabled={isCreating}
            className="h-9 w-9 rounded-lg"
            title={t('session.newChat')}
          >
            <Plus className="h-4 w-4" />
          </Button>
          <div className="w-6 h-px bg-border/50 my-1" />
          <ScrollArea className="flex-1 w-full">
            <div className="flex flex-col items-center gap-1 py-1">
              {sortedSessions.map((session) => {
                const isActive = session.sessionId === currentSessionId
                return (
                  <Button
                    key={session.sessionId}
                    variant="ghost"
                    size="icon"
                    onClick={() => handleSwitchSession(session.sessionId)}
                    className={cn(
                      "h-9 w-9 rounded-lg",
                      isActive && "bg-muted"
                    )}
                    title={getSessionTitle(session)}
                  >
                    <MessageSquare className={cn(
                      "h-4 w-4",
                      isActive ? "text-foreground" : "text-muted-foreground"
                    )} />
                  </Button>
                )
              })}
            </div>
          </ScrollArea>
        </div>
      ) : (
        <>
          {/* Search */}
          <div className="px-3 py-2">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
              <Input
                ref={searchInputRef}
                type="text"
                placeholder={t('session.search')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 h-8 text-sm rounded-lg bg-muted/50 border-0"
              />
            </div>
          </div>

          {/* New Session Button */}
          <div className="px-3 pb-2">
            <Button
              onClick={handleNewSession}
              disabled={isCreating}
              variant="outline"
              className="w-full h-8 text-sm rounded-lg"
            >
              <Plus className="h-3.5 w-3.5 mr-1.5" />
              {t('session.newChat')}
            </Button>
          </div>

          {/* Session List */}
          <ScrollArea className="flex-1">
            <div className="px-2 pb-2 space-y-0.5">
              {sortedSessions.length === 0 ? (
                <div className="py-8 text-center">
                  <MessageSquare className="h-8 w-8 mx-auto text-muted-foreground/30 mb-2" />
                  <p className="text-xs text-muted-foreground">
                    {searchQuery ? t('session.noMatch') : t('session.noSessions')}
                  </p>
                </div>
              ) : (
                sortedSessions.map((session) => {
                  const isActive = session.sessionId === currentSessionId
                  const isDeleting = deletingId === session.sessionId

                  return (
                    <div
                      key={session.sessionId}
                      onClick={() => handleSwitchSession(session.sessionId)}
                      className={cn(
                        "group relative p-2 rounded-lg cursor-pointer transition-all",
                        isActive
                          ? "bg-muted"
                          : "hover:bg-muted/50"
                      )}
                    >
                      <div className="flex items-start gap-2">
                        <MessageSquare className={cn(
                          "h-3.5 w-3.5 mt-0.5 shrink-0",
                          isActive ? "text-foreground" : "text-muted-foreground"
                        )} />
                        <div className="flex-1 min-w-0">
                          <h4 className={cn(
                            "text-sm truncate",
                            isActive ? "text-foreground font-medium" : "text-foreground/80"
                          )}>
                            {getSessionTitle(session)}
                          </h4>
                          <div className="flex items-center gap-1.5 mt-0.5 text-[10px] text-muted-foreground">
                            <Clock className="h-2.5 w-2.5" />
                            <span>{formatTimeAgo(session.updatedAt || session.createdAt)}</span>
                            {session.messageCount ? (
                              <>
                                <span>·</span>
                                <span>{t('session.messages', { count: session.messageCount })}</span>
                              </>
                            ) : null}
                          </div>
                        </div>
                      </div>

                      {/* Delete button */}
                      <button
                        onClick={(e) => handleDeleteSession(e, session.sessionId)}
                        disabled={isDeleting}
                        className={cn(
                          "absolute right-1 top-1/2 -translate-y-1/2",
                          "p-1 rounded transition-all",
                          "text-muted-foreground hover:text-destructive hover:bg-destructive/10",
                          "opacity-0 group-hover:opacity-100",
                          isDeleting && "opacity-50"
                        )}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  )
                })
              )}
            </div>
          </ScrollArea>

          {/* Footer */}
          <div className="p-2 border-t border-border/50">
            <p className="text-[10px] text-muted-foreground text-center">
              {t('session.totalSessions', { count: sessions.length })}
            </p>
          </div>
        </>
      )}
    </>
  )

  // Desktop mode: fixed sidebar
  if (isDesktop) {
    return (
      <div
        className={cn(
          "h-full bg-background/50 border-r border-border/50 flex flex-col transition-all duration-200",
          collapsed ? "w-14" : "w-64"
        )}
      >
        <SidebarContent />
      </div>
    )
  }

  // Mobile mode: drawer
  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/30 backdrop-blur-sm z-40 transition-opacity lg:hidden"
          onClick={onClose}
        />
      )}

      {/* Sidebar */}
      <div
        className={cn(
          "fixed top-0 left-0 h-full w-72 z-50 lg:hidden",
          "bg-background shadow-xl flex flex-col",
          "transform transition-transform duration-300 ease-out",
          open ? "translate-x-0" : "-translate-x-full"
        )}
      >
        <SidebarContent />
      </div>
    </>
  )
}
