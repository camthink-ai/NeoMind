/**
 * SessionSidebar - Responsive session management panel
 * - Desktop (lg+): Fixed sidebar on left, collapsible
 * - Mobile: Slide-out drawer (rendered via Portal to avoid stacking context issues)
 */

import { getPortalRoot } from '@/lib/portal'
import { handleWindowDragMouseDown } from "@/lib/windowDrag"
import { useState, useEffect, useRef, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate, useParams } from "react-router-dom"
import { useStore } from "@/store"
import { createPortal } from "react-dom"
import type { ChatSession } from "@/types"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
import {
  X,
  Plus,
  Search,
  MessageSquare,
  Trash2,
  Clock,
  Pencil,
  Check,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { useToast } from "@/hooks/use-toast"
import { showErrorToast } from "@/lib/error-messages"

interface SessionSidebarProps {
  /** Mobile drawer mode: open state */
  open: boolean
  /** Mobile drawer mode: close handler */
  onClose: () => void
  /** Is desktop mode (fixed sidebar) */
  isDesktop?: boolean
}

export function SessionSidebar({
  open,
  onClose,
  isDesktop = false
}: SessionSidebarProps) {
  const { t } = useTranslation('common')
  const navigate = useNavigate()
  const { toast } = useToast()
  const { sessionId: urlSessionId } = useParams<{ sessionId?: string }>()

  const sessions = useStore((s) => s.sessions)
  const storeSessionId = useStore((s) => s.sessionId)
  const sessionsHasMore = useStore((s) => s.sessionsHasMore)
  const sessionsLoading = useStore((s) => s.sessionsLoading)
  const createSession = useStore((s) => s.createSession)
  const switchSession = useStore((s) => s.switchSession)
  const deleteSession = useStore((s) => s.deleteSession)
  const updateSessionTitle = useStore((s) => s.updateSessionTitle)
  const loadMoreSessions = useStore((s) => s.loadMoreSessions)

  // Use URL sessionId for active highlight, not store sessionId.
  // This ensures no session is highlighted in welcome mode (/chat without sessionId).
  const currentSessionId = urlSessionId || null

  const [searchQuery, setSearchQuery] = useState("")
  const [isCreating, setIsCreating] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingTitle, setEditingTitle] = useState("")
  const [isUpdating, setIsUpdating] = useState(false)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const loadMoreTriggerRef = useRef<HTMLDivElement>(null)
  const editInputRef = useRef<HTMLInputElement>(null)
  const scrollViewportRef = useRef<HTMLDivElement>(null)
  const savedScrollTop = useRef<number>(0)

  // Save scroll position before re-render caused by session switch
  useEffect(() => {
    const viewport = scrollViewportRef.current
    if (!viewport) return

    const handleScroll = () => {
      savedScrollTop.current = viewport.scrollTop
    }

    viewport.addEventListener('scroll', handleScroll, { passive: true })
    return () => viewport.removeEventListener('scroll', handleScroll)
  }, [])

  // Focus search when opened (mobile only)
  useEffect(() => {
    if (open && !isDesktop) {
      const raf = requestAnimationFrame(() => {
        searchInputRef.current?.focus()
      })
      return () => cancelAnimationFrame(raf)
    }
  }, [open, isDesktop])

  // Infinite scroll using Intersection Observer
  useEffect(() => {
    const trigger = loadMoreTriggerRef.current
    if (!trigger || !sessionsHasMore || sessionsLoading || searchQuery) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && sessionsHasMore && !sessionsLoading && !searchQuery) {
          loadMoreSessions()
        }
      },
      { threshold: 0.1, rootMargin: '100px' }
    )

    observer.observe(trigger)
    return () => observer.disconnect()
  }, [sessionsHasMore, sessionsLoading, searchQuery, loadMoreSessions])

  // Filter and sort sessions - memoized to prevent unnecessary re-renders
  const sortedSessions = useMemo(() => {
    const filtered = searchQuery
      ? sessions.filter((session) => {
          const query = searchQuery.toLowerCase()
          return (
            (session.title || "").toLowerCase().includes(query) ||
            (session.preview || "").toLowerCase().includes(query)
          )
        })
      : sessions

    return [...filtered].sort((a, b) => {
      const timeA = a.updatedAt || a.createdAt || 0
      const timeB = b.updatedAt || b.createdAt || 0
      return timeB - timeA
    })
  }, [sessions, searchQuery])

  // Restore scroll position after sessions list changes
  useEffect(() => {
    const viewport = scrollViewportRef.current
    if (!viewport || savedScrollTop.current === 0) return

    requestAnimationFrame(() => {
      viewport.scrollTop = savedScrollTop.current
    })
  }, [sortedSessions])

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
    // Mobile: close drawer immediately for better UX
    if (!isDesktop) {
      onClose()
    }

    // Always navigate - React Router is a no-op for same-URL navigation.
    // This fixes the case where the store's sessionId matches but the URL
    // is /chat (no sessionId), which makes the session unclickable.
    // The chat page handles calling switchSession via its URL-based useEffect,
    // which guards against redundant loading.
    navigate(`/chat/${sessionId}`)
  }

  // Handle delete session - show confirmation first
  const handleDeleteClick = (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation()
    setSessionToDelete(sessionId)
    setDeleteDialogOpen(true)
  }

  // Confirm and execute delete
  const handleConfirmDelete = async () => {
    if (!sessionToDelete || deletingId) return

    const wasCurrentSession = sessionToDelete === currentSessionId
    setDeletingId(sessionToDelete)
    setDeleteDialogOpen(false)

    try {
      await deleteSession(sessionToDelete)

      // If we deleted the current session, navigate to the new current session
      // deleteSession already switches the session in the store, so we need to
      // get the new sessionId from the store and update the URL
      if (wasCurrentSession) {
        const { sessionId: newSessionId, sessions: updatedSessions } = useStore.getState()
        if (newSessionId) {
          navigate(`/chat/${newSessionId}`)
        } else if (updatedSessions.length === 0) {
          // No sessions left, go to welcome page
          navigate('/chat')
        }
      }

      toast({
        title: t('session.sessionDeleted'),
      })
    } catch (error: any) {
      showErrorToast(toast, error, t('error'))
    } finally {
      setDeletingId(null)
      setSessionToDelete(null)
    }
  }

  // Cancel delete
  const handleCancelDelete = () => {
    setDeleteDialogOpen(false)
    setSessionToDelete(null)
  }

  // Handle edit click
  const handleEditClick = (e: React.MouseEvent, session: ChatSession) => {
    e.stopPropagation()
    setEditingId(session.sessionId)
    setEditingTitle(session.title || "")
    // Focus input after ensuring it's rendered
    requestAnimationFrame(() => {
      editInputRef.current?.focus()
      editInputRef.current?.select()
    })
  }

  // Handle edit cancel
  const handleEditCancel = () => {
    setEditingId(null)
    setEditingTitle("")
  }

  // Handle edit save
  const handleEditSave = async (sessionId: string) => {
    const trimmedTitle = editingTitle.trim()
    if (!trimmedTitle || isUpdating) return

    setIsUpdating(true)
    try {
      await updateSessionTitle(sessionId, trimmedTitle)
      setEditingId(null)
      setEditingTitle("")
    } catch (error) {
      console.error('Failed to update session title:', error)
    } finally {
      setIsUpdating(false)
    }
  }

  // Handle edit key down
  const handleEditKeyDown = (e: React.KeyboardEvent, sessionId: string) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      handleEditSave(sessionId)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      handleEditCancel()
    }
  }

  // Get session title
  const getSessionTitle = (session: ChatSession): string => {
    if (session.title && session.title !== "新对话" && session.title !== "New Chat") {
      return session.title
    }
    if (session.preview) {
      return session.preview.length > 30 ? session.preview.slice(0, 30) + '...' : session.preview
    }
    return t('session.defaultTitle')
  }

  // Sidebar content - shared between desktop and mobile
  const SidebarContent = ({ showHeader = true }: { showHeader?: boolean }) => (
    <>
      {/* Header */}
      {showHeader && (
        <div className="relative z-[1] flex items-center justify-between px-3 py-2" onMouseDown={handleWindowDragMouseDown}>
          <h2 className="text-base font-semibold">{t('session.sessions')}</h2>
          {!isDesktop && (
            <Button
              variant="ghost"
              size="icon"
              onClick={onClose}
              className="h-6 w-6 rounded-lg"
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      )}

      {/* Search */}
      <div className="px-3 py-2">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            ref={searchInputRef}
            type="text"
            placeholder={t('session.search')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 h-8 text-sm rounded-lg bg-muted-50 border-0"
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
              <Plus className="h-4 w-4 mr-1.5" />
              {t('session.newChat')}
            </Button>
          </div>

          {/* Session List */}
          <ScrollArea className="flex-1 min-h-0" viewportRef={scrollViewportRef}>
            <div className="px-2 pb-2 space-y-0.5 overflow-x-hidden">
              {sortedSessions.length === 0 ? (
                <div className="py-8 text-center">
                  <MessageSquare className="h-8 w-8 mx-auto text-muted-foreground mb-2" />
                  <p className="text-xs text-muted-foreground">
                    {searchQuery ? t('session.noMatch') : t('session.noSessions')}
                  </p>
                </div>
              ) : (
                <>
                  {sortedSessions.map((session) => {
                    const isActive = session.sessionId === currentSessionId
                    const isDeleting = deletingId === session.sessionId
                    const isEditing = editingId === session.sessionId

                    return (
                      <div
                        key={session.sessionId}
                        onClick={() => !isEditing && handleSwitchSession(session.sessionId)}
                        className={cn(
                          // Flat list: selected = plain muted block (no
                          // raised card / shadow — the 3D look was overkill)
                          "group relative flex items-start gap-2 p-2 rounded-lg cursor-pointer transition-colors",
                          isActive
                            ? "bg-muted"
                            : "hover:bg-muted-50",
                          isEditing && "bg-muted"
                        )}
                      >
                        {isEditing ? (
                          // Edit mode
                          <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                            <Input
                              ref={editInputRef}
                              value={editingTitle}
                              onChange={(e) => setEditingTitle(e.target.value)}
                              onKeyDown={(e) => handleEditKeyDown(e, session.sessionId)}
                              className="h-7 text-sm flex-1 rounded-md"
                              autoFocus
                              disabled={isUpdating}
                            />
                            <button
                              aria-label={t('common:confirm')}
                              className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
                              onClick={() => handleEditSave(session.sessionId)}
                              disabled={isUpdating || !editingTitle.trim()}
                            >
                              <Check className="h-3.5 w-3.5" />
                            </button>
                            <button
                              aria-label={t('common:cancel')}
                              className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
                              onClick={handleEditCancel}
                              disabled={isUpdating}
                            >
                              <X className="h-3.5 w-3.5" />
                            </button>
                          </div>
                        ) : (
                          // Normal mode
                          <>
                            <div className="flex items-center gap-2 min-w-0 flex-1">
                              <MessageSquare className={cn(
                                "h-4 w-4 shrink-0",
                                isActive ? "text-foreground" : "text-muted-foreground"
                              )} />
                              <div className="min-w-0 flex-1">
                                <h4 className={cn(
                                  "text-sm truncate",
                                  isActive ? "text-foreground font-medium" : "text-muted-foreground"
                                )}>
                                  {getSessionTitle(session)}
                                </h4>
                                <div className={cn("flex items-center gap-1.5 mt-0.5 overflow-hidden", textNano, "text-muted-foreground")}>
                                  <Clock className="h-2.5 w-2.5 shrink-0" />
                                  <span className="truncate">{session.updatedAt ? formatTimestamp(session.updatedAt / 1000, false) : formatTimestamp(session.createdAt / 1000, false)}</span>
                                  {session.messageCount ? (
                                    <>
                                      <span className="shrink-0">·</span>
                                      <span className="truncate">{t('session.messages', { count: session.messageCount })}</span>
                                    </>
                                  ) : null}
                                </div>
                              </div>
                            </div>

                            {/* Action buttons */}
                            <div className="absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-0.5 opacity-0 translate-x-1 transition-all duration-fast group-hover:translate-x-0 group-hover:opacity-100">
                              <button
                                onClick={(e) => handleEditClick(e, session)}
                                aria-label={t('common:edit')}
                                className="h-6 w-6 flex items-center justify-center rounded hover:bg-muted transition-colors"
                                title={t('session.rename')}
                              >
                                <Pencil className="h-3 w-3" />
                              </button>
                              <button
                                onClick={(e) => handleDeleteClick(e, session.sessionId)}
                                aria-label={t('common:delete')}
                                disabled={isDeleting}
                                className={cn(
                                  "h-6 w-6 flex items-center justify-center rounded hover:bg-error-light text-muted-foreground hover:text-error transition-colors",
                                  isDeleting && "opacity-50"
                                )}
                                title={t('delete')}
                              >
                                <Trash2 className="h-3 w-3" />
                              </button>
                            </div>
                          </>
                        )}
                      </div>
                    )
                  })}
                  {/* Load more trigger */}
                  <div ref={loadMoreTriggerRef} className="h-1" />
                  {/* Loading indicator */}
                  {sessionsLoading && (
                    <div className="flex items-center justify-center py-3">
                      <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
                    </div>
                  )}
                </>
              )}
            </div>
          </ScrollArea>
    </>
  )

  // Desktop mode: fixed sidebar
  if (isDesktop) {
    return (
      <>
      <div
        className={cn(
          // Chrome panel — one step brighter than the rail in BOTH themes
          // (light: white vs #F8F9FA rail; dark: 0.20 vs 0.115 rail), so
          // the page sidebar reads as a separate surface. Right border
          // separates it from the white content area. Fixed width.
          "relative h-full w-64 bg-background border-r border-border flex flex-col overflow-hidden"
        )}
        style={{
          // Safe top clearance: the drawer starts below the window chrome /
          // traffic-light strip instead of touching the window edge
          paddingTop: "calc(env(safe-area-inset-top, 0px) + 1rem)",
        }}
      >
          {/* Window drag strip — covers the top clearance zone + header row
              behind it; the header content sits above (z-[1]) so its
              buttons stay clickable */}
          <div
            className="absolute inset-x-0 top-0 z-0"
            style={{ height: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px) + 2.75rem)" }}
            onMouseDown={handleWindowDragMouseDown}
          />
          {SidebarContent({})}
        </div>

        {/* Delete Confirmation Dialog */}
        <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{t('session.deleteTitle')}</AlertDialogTitle>
              <AlertDialogDescription>
                {t('session.deleteDescription')}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel onClick={handleCancelDelete}>
                {t('cancel')}
              </AlertDialogCancel>
              <AlertDialogAction
                onClick={handleConfirmDelete}
                className="bg-destructive text-destructive-foreground hover:bg-destructive-hover"
              >
                {t('delete')}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </>
    )
  }

  // Mobile mode: drawer - use Portal to render at document root for proper z-index stacking
  return createPortal(
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-overlay-light backdrop-blur-sm z-50 transition-opacity lg:hidden"
          onClick={onClose}
        />
      )}

      {/* Sidebar */}
      <div
        className={cn(
          "fixed top-0 left-0 h-full w-72 z-50 lg:hidden safe-top",
          // bg-popover matches the desktop persistent sidebar and every
          // other drawer / popup in the app. Previously bg-background,
          // which is /97% alpha in dark mode and let the page + backdrop
          // bleed through, making the drawer read darker than the chrome
          // layer it slides out from.
          "bg-popover shadow-xl flex flex-col",
          "transform transition-transform duration-slow ease-out",
          open ? "translate-x-0" : "-translate-x-full"
        )}
      >
        {SidebarContent({})}
      </div>

      {/* Delete Confirmation Dialog */}
      <AlertDialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('session.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('session.deleteDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={handleCancelDelete}>
              {t('cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={handleConfirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive-hover"
            >
              {t('delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>, getPortalRoot()
  )
}
