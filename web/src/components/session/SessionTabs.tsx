/**
 * SessionTabs - Inline horizontal session tabs
 * Compact tab-based session switching
 */

import { useState, useRef, useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import type { ChatSession } from "@/types"
import { cn } from "@/lib/utils"
import { Plus, X, ChevronLeft, ChevronRight, MoreHorizontal, MessageSquare } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useTranslation } from "react-i18next"

interface SessionTabsProps {
  className?: string
  onSessionChange?: (sessionId: string) => void
}

export function SessionTabs({ className, onSessionChange }: SessionTabsProps) {
  const { t } = useTranslation(["common", "dashboard"])
  const navigate = useNavigate()
  const {
    sessions,
    sessionId: currentSessionId,
    createSession,
    switchSession,
    deleteSession,
  } = useStore()

  const scrollRef = useRef<HTMLDivElement>(null)
  const [showLeftScroll, setShowLeftScroll] = useState(false)
  const [showRightScroll, setShowRightScroll] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null)

  // Check scroll buttons visibility
  const checkScroll = () => {
    const el = scrollRef.current
    if (!el) return

    setShowLeftScroll(el.scrollLeft > 0)
    setShowRightScroll(el.scrollLeft < el.scrollWidth - el.clientWidth - 1)
  }

  useEffect(() => {
    checkScroll()
    const el = scrollRef.current
    if (el) {
      el.addEventListener("scroll", checkScroll)
      window.addEventListener("resize", checkScroll)
      return () => {
        el.removeEventListener("scroll", checkScroll)
        window.removeEventListener("resize", checkScroll)
      }
    }
  }, [sessions])

  // Scroll functions
  const scrollLeft = () => {
    scrollRef.current?.scrollBy({ left: -200, behavior: "smooth" })
  }

  const scrollRight = () => {
    scrollRef.current?.scrollBy({ left: 200, behavior: "smooth" })
  }

  // Handle new session
  const handleNewSession = async () => {
    if (isCreating) return
    setIsCreating(true)
    try {
      const newSessionId = await createSession()
      if (newSessionId) {
        navigate(`/chat/${newSessionId}`)
      }
    } finally {
      setIsCreating(false)
    }
  }

  // Handle session switch
  const handleSwitchSession = async (sessionId: string) => {
    if (sessionId === currentSessionId) return
    await switchSession(sessionId)
    navigate(`/chat/${sessionId}`)
    onSessionChange?.(sessionId)
  }

  // Handle delete session - show confirmation first
  const handleDeleteSession = (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation()
    setSessionToDelete(sessionId)
    setDeleteDialogOpen(true)
  }

  const handleDeleteConfirm = async () => {
    if (sessionToDelete) {
      await deleteSession(sessionToDelete)
      setDeleteDialogOpen(false)
      setSessionToDelete(null)
    }
  }

  // Format session title
  const defaultTitle = t('dashboard.defaultTitle')
  const getSessionTitle = (session: ChatSession): string => {
    if (session.title && session.title !== defaultTitle) {
      return session.title.length > 16
        ? session.title.slice(0, 16) + "..."
        : session.title
    }
    return defaultTitle
  }

  // Sort sessions by update time (newest first)
  const sortedSessions = [...sessions].sort((a, b) => {
    const timeA = a.updatedAt || a.createdAt || 0
    const timeB = b.updatedAt || b.createdAt || 0
    return timeB - timeA
  })

  // Visible sessions (limit for performance)
  const visibleSessions = sortedSessions.slice(0, 20)
  const hasMore = sortedSessions.length > 20
  const hasSessions = sessions.length > 0

  return (
    <div className={cn(
      "h-11 flex items-center px-2 sm:px-3 gap-1",
      "bg-background/50 backdrop-blur-sm",
      className
    )}>
      {/* New session button */}
      <Button
        variant="ghost"
        size="sm"
        onClick={handleNewSession}
        disabled={isCreating}
        className={cn(
          "h-8 gap-1.5 flex-shrink-0 rounded-lg",
          "text-muted-foreground hover:text-foreground hover:bg-muted/50",
          "transition-all"
        )}
      >
        <Plus className="h-4 w-4" />
        <span className="text-xs hidden sm:inline">{t('dashboard.newChat')}</span>
      </Button>

      {/* Separator */}
      {hasSessions && (
        <div className="h-5 w-px bg-muted mx-1" />
      )}

      {/* Left scroll button */}
      {showLeftScroll && (
        <Button
          variant="ghost"
          size="sm"
          onClick={scrollLeft}
          className="h-7 w-7 p-0 flex-shrink-0 text-muted-foreground hover:text-foreground rounded-lg"
        >
          <ChevronLeft className="h-4 w-4" />
        </Button>
      )}

      {/* Tabs scroll container */}
      <div
        ref={scrollRef}
        className="flex-1 flex items-center gap-1 overflow-x-auto scrollbar-none"
        style={{ scrollbarWidth: "none", msOverflowStyle: "none" }}
      >
        {visibleSessions.map((session) => {
          const isActive = session.sessionId === currentSessionId

          return (
            <button
              key={session.sessionId}
              onClick={() => handleSwitchSession(session.sessionId)}
              className={cn(
                "group flex items-center gap-1.5 min-w-0",
                "px-2.5 py-1.5 rounded-lg text-sm whitespace-nowrap",
                "transition-all duration-150",
                isActive
                  ? "bg-muted text-foreground font-medium"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/50"
              )}
            >
              <MessageSquare className="h-3.5 w-3.5 flex-shrink-0 opacity-60" />
              <span className="truncate max-w-[120px] sm:max-w-[140px]">
                {getSessionTitle(session)}
              </span>

              {/* Close button */}
              <span
                onClick={(e) => handleDeleteSession(e, session.sessionId)}
                className={cn(
                  "p-0.5 rounded-md transition-all ml-0.5",
                  "hover:bg-foreground/10",
                  isActive 
                    ? "opacity-50 hover:opacity-100" 
                    : "opacity-0 group-hover:opacity-50 hover:!opacity-100"
                )}
              >
                <X className="h-3 w-3" />
              </span>
            </button>
          )
        })}
      </div>

      {/* Right scroll button */}
      {showRightScroll && (
        <Button
          variant="ghost"
          size="sm"
          onClick={scrollRight}
          className="h-7 w-7 p-0 flex-shrink-0 text-muted-foreground hover:text-foreground rounded-lg"
        >
          <ChevronRight className="h-4 w-4" />
        </Button>
      )}

      {/* More sessions dropdown */}
      {hasMore && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 flex-shrink-0 text-muted-foreground hover:text-foreground rounded-lg"
            >
              <MoreHorizontal className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-56">
            {sortedSessions.slice(20).map((session) => (
              <DropdownMenuItem
                key={session.sessionId}
                onClick={() => handleSwitchSession(session.sessionId)}
                className="flex items-center justify-between gap-2"
              >
                <div className="flex items-center gap-2 min-w-0">
                  <MessageSquare className="h-3.5 w-3.5 flex-shrink-0 opacity-60" />
                  <span className="truncate">{getSessionTitle(session)}</span>
                </div>
                <span
                  onClick={(e) => handleDeleteSession(e, session.sessionId)}
                  className="p-1 rounded hover:bg-muted flex-shrink-0"
                >
                  <X className="h-3 w-3" />
                </span>
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem className="text-xs text-muted-foreground justify-center">
              {t('session.totalSessions', { count: sortedSessions.length })}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('session.deleteTitle')}</DialogTitle>
            <DialogDescription>
              {t('session.deleteDescription')}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteDialogOpen(false)}>
              {t('cancel')}
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              {t('delete')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
