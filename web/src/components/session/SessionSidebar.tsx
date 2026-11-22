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
import { sessionsApi } from '@/lib/api/sessions'
import { createPortal } from "react-dom"
import type { ChatSession } from "@/types"
import { cn } from "@/lib/utils"
import { getSessionDisplayTitle } from "@/lib/utils/sessionTitle"
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
  Loader2,
  ChevronRight,
  Cpu,
  Bot,
  LayoutDashboard,
  Bell,
} from "lucide-react"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
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
  const _storeSessionId = useStore((s) => s.sessionId)
  const sessionsHasMore = useStore((s) => s.sessionsHasMore)
  const sessionsLoading = useStore((s) => s.sessionsLoading)
  const createSession = useStore((s) => s.createSession)
  const _switchSession = useStore((s) => s.switchSession)
  const deleteSession = useStore((s) => s.deleteSession)
  const updateSessionTitle = useStore((s) => s.updateSessionTitle)
  const loadMoreSessions = useStore((s) => s.loadMoreSessions)
  const user = useStore((s) => s.user)

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
  const drawerRef = useRef<HTMLDivElement>(null)
  /** True while this drawer owns a history.pushState entry for system-back. */
  const historyPushedRef = useRef(false)

  // Close-time focus handoff: if focus is still inside the (now hidden)
  // drawer, return it to the opener (chat hamburger) — otherwise typed keys
  // keep landing in the invisible search field. Falls back to blur.
  useEffect(() => {
    if (open) return
    const root = drawerRef.current
    if (root && root.contains(document.activeElement)) {
      const opener = document.querySelector<HTMLElement>('[data-testid="chat-drawer-button"]')
      if (opener) opener.focus()
      else (document.activeElement as HTMLElement | null)?.blur()
    }
  }, [open])

  // Mobile system-back: opening pushes a history entry; Back closes the
  // panel instead of leaving the chat session. UI close pops that entry.
  useEffect(() => {
    if (isDesktop) return
    const onPopState = () => {
      if (!historyPushedRef.current) return
      historyPushedRef.current = false
      onClose()
    }
    window.addEventListener("popstate", onPopState)
    return () => window.removeEventListener("popstate", onPopState)
  }, [isDesktop, onClose])

  useEffect(() => {
    if (isDesktop) return
    if (open) {
      if (!historyPushedRef.current) {
        history.pushState({ nmSessionDrawer: true }, "")
        historyPushedRef.current = true
      }
      return
    }
    if (historyPushedRef.current) {
      historyPushedRef.current = false
      if ((history.state as { nmSessionDrawer?: boolean } | null)?.nmSessionDrawer) {
        history.back()
      }
    }
  }, [open, isDesktop])

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

  // Sessions with an in-flight generation (server truth — a session left
  // mid-turn keeps generating server-side; local streaming state dies on
  // navigation). Polled only while the sidebar is VISIBLE (desktop is always
  // mounted/visible; the mobile drawer polls while open — an always-mounted
  // closed drawer must not keep a permanent 5s background loop, e.g. in
  // welcome mode). Stale pending entries (>10 min, hard-killed servers) are
  // already filtered server-side.
  const [generatingIds, setGeneratingIds] = useState<Set<string>>(new Set())
  useEffect(() => {
    if (!(isDesktop || open)) return
    let stopped = false
    // Circuit breaker: an old backend predating the endpoint 404s forever
    // (the legacy router even matches /sessions/:id with id="pending").
    // Three consecutive failures = no endpoint there — stop polling until
    // the sidebar remounts/reopens; transient failures reset the count.
    let failures = 0
    const poll = () => {
      if (stopped || failures >= 3) return
      sessionsApi.listPendingSessions()
        .then(r => {
          if (stopped) return
          failures = 0
          setGeneratingIds(new Set(r.sessionIds ?? []))
        })
        .catch(() => {
          if (stopped) return
          failures += 1
        })
    }
    poll()
    const timer = setInterval(poll, 5000)
    return () => { stopped = true; clearInterval(timer) }
  }, [isDesktop, open])

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
    } catch (error) {
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

  const defaultTitle = t('session.defaultTitle')

  const renderSessionRow = (session: ChatSession, opts: { roomy: boolean }) => {
    const isActive = session.sessionId === currentSessionId
    const isDeleting = deletingId === session.sessionId
    const isEditing = editingId === session.sessionId
    const isGenerating = generatingIds.has(session.sessionId)
    const roomy = opts.roomy

    return (
      <div
        key={session.sessionId}
        onClick={() => !isEditing && handleSwitchSession(session.sessionId)}
        className={cn(
          "group relative flex cursor-pointer items-center gap-2 transition-colors focus-within:bg-muted-50",
          roomy ? "rounded-2xl px-3.5 py-3" : "rounded-lg px-2 py-2",
          isActive ? "bg-muted" : "hover:bg-muted-50",
          isEditing && "bg-muted",
        )}
      >
        {isEditing ? (
          <div className="flex min-w-0 flex-1 items-center gap-1" onClick={(e) => e.stopPropagation()}>
            <Input
              ref={editInputRef}
              value={editingTitle}
              onChange={(e) => setEditingTitle(e.target.value)}
              onKeyDown={(e) => handleEditKeyDown(e, session.sessionId)}
              className="h-8 min-w-0 flex-1 rounded-md text-sm"
              autoFocus
              disabled={isUpdating}
            />
            <button
              aria-label={t('common:confirm')}
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
              onClick={() => handleEditSave(session.sessionId)}
              disabled={isUpdating || !editingTitle.trim()}
            >
              <Check className="h-3.5 w-3.5" />
            </button>
            <button
              aria-label={t('common:cancel')}
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md hover:bg-muted transition-colors"
              onClick={handleEditCancel}
              disabled={isUpdating}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        ) : (
          <>
            <div className="flex min-w-0 flex-1 items-center gap-2">
              {isGenerating && (
                <Loader2
                  className="h-4 w-4 shrink-0 animate-spin text-primary motion-reduce:animate-none"
                  aria-label={t('session.generating')}
                />
              )}
              <div className="min-w-0 flex-1">
                <h4 className={cn(
                  "truncate",
                  roomy ? "text-[0.9375rem] leading-5" : "text-sm",
                  isActive ? "font-medium text-foreground" : "text-muted-foreground",
                )}>
                  {getSessionDisplayTitle(session, defaultTitle)}
                </h4>
                <div className={cn("mt-0.5 flex items-center gap-1.5 overflow-hidden text-muted-foreground", textNano)}>
                  <Clock className="h-2.5 w-2.5 shrink-0" />
                  <span className="truncate">
                    {session.updatedAt
                      ? formatTimestamp(session.updatedAt / 1000, false)
                      : formatTimestamp(session.createdAt / 1000, false)}
                  </span>
                  {session.messageCount ? (
                    <>
                      <span className="shrink-0">·</span>
                      <span className="truncate">{t('session.messages', { count: session.messageCount })}</span>
                    </>
                  ) : null}
                </div>
              </div>
            </div>

            {/* Desktop: hover-reveal. Mobile: single delete to keep rows clean. */}
            <div className={cn(
              "flex shrink-0 items-center gap-0.5 overflow-hidden transition-[width] duration-fast",
              isDesktop
                ? "w-8 group-hover:w-[4.125rem] group-focus-within:w-[4.125rem]"
                : "w-9",
            )}>
              {isDesktop && (
                <button
                  onClick={(e) => handleEditClick(e, session)}
                  aria-label={t('common:edit')}
                  className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  title={t('session.rename')}
                >
                  <Pencil className="h-3.5 w-3.5" />
                </button>
              )}
              <button
                onClick={(e) => handleDeleteClick(e, session.sessionId)}
                aria-label={t('common:delete')}
                disabled={isDeleting}
                className={cn(
                  "flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-error-light hover:text-error focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  isDeleting && "opacity-50",
                )}
                title={t('delete')}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          </>
        )}
      </div>
    )
  }

  const renderSessionList = (roomy: boolean) => (
    sortedSessions.length === 0 ? (
      <div className={cn("text-center", roomy ? "px-2 py-12" : "py-8")}>
        <MessageSquare className={cn("mx-auto mb-2 text-muted-foreground", roomy ? "h-9 w-9" : "h-8 w-8")} />
        <p className={cn("text-muted-foreground", roomy ? "text-sm" : "text-xs")}>
          {searchQuery ? t('session.noMatch') : t('session.noSessions')}
        </p>
      </div>
    ) : (
      <>
        {sortedSessions.map((session) => renderSessionRow(session, { roomy }))}
        <div ref={loadMoreTriggerRef} className="h-1" />
        {sessionsLoading && (
          <div className="flex items-center justify-center py-3">
            <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
          </div>
        )}
      </>
    )
  )

  const goWorkspace = (path: string) => {
    onClose()
    navigate(path)
  }

  // Original four quick entries with Lucide icons. Full destination list
  // lives on System hub — opened from the avatar header row above the tiles.
  const workspaceShortcuts = [
    { id: 'devices', path: '/devices', icon: Cpu, labelKey: 'nav.devices' },
    { id: 'agents', path: '/agents', icon: Bot, labelKey: 'nav.agents' },
    { id: 'dashboard', path: '/visual-dashboard', icon: LayoutDashboard, labelKey: 'nav.visual-dashboard' },
    { id: 'messages', path: '/messages', icon: Bell, labelKey: 'nav.messages' },
  ] as const

  const workspaceAvatarLabel = user?.username
    ? user.username.slice(0, 2).toUpperCase()
    : 'NM'

  // IMPORTANT: keep these as JSX values, not inner components. Defining
  // MobileDrawerContent/DesktopContent as nested function components remounts
  // the search input on every keystroke and breaks continuous typing/delete.
  const desktopContent = (
    <>
      <div
        className="relative z-[1] flex items-center justify-between px-3 py-2"
        onMouseDown={handleWindowDragMouseDown}
      >
        <h2 className="text-base font-semibold tracking-[-0.02em] text-foreground">
          {t('session.sessions')}
        </h2>
      </div>

      <div className="px-3 py-2">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={searchInputRef}
            type="text"
            placeholder={t('session.search')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-8 rounded-lg bg-muted-50 pl-9 text-sm"
          />
        </div>
      </div>

      <div className="px-3 pb-2">
        <Button
          onClick={handleNewSession}
          disabled={isCreating}
          variant="outline"
          className="h-8 w-full rounded-lg text-sm shadow-none"
        >
          <Plus className="mr-1.5 h-4 w-4" strokeWidth={2} />
          {t('session.newChat')}
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportRef={scrollViewportRef}>
        <div className="space-y-0.5 overflow-x-hidden px-2 pb-2">
          {renderSessionList(false)}
        </div>
      </ScrollArea>
    </>
  )

  // Mobile: close chrome → one workspace card (identity row + 4 tiles) → chats.
  const mobileDrawerContent = (
    <>
      <div className="flex items-center justify-between gap-3 px-5 pb-3 pt-1.5">
        <p className="min-w-0 truncate text-lg font-semibold tracking-[-0.03em] text-foreground">
          NeoMind
        </p>
        <Button
          variant="ghost"
          size="icon"
          onClick={onClose}
          className="h-10 w-10 shrink-0 rounded-full border border-glass-border bg-surface-glass shadow-sm backdrop-blur-xl hover:border-glass-border-hover hover:bg-muted sm:h-10 sm:w-10"
          aria-label={t('close')}
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      <div className="px-5 pb-5">
        <div className="overflow-hidden rounded-2xl border border-border bg-muted-50">
          <button
            type="button"
            data-testid="session-drawer-workspace"
            onClick={() => goWorkspace('/system')}
            className="flex min-h-14 w-full items-center gap-3 px-3.5 py-3 text-left transition-colors active:bg-muted"
            aria-label={`${t('system.pageTitle')} — ${t('system.moreEntries')}`}
          >
            <Avatar className="h-10 w-10 shrink-0 rounded-xl ring-1 ring-border">
              <AvatarFallback className="rounded-xl bg-background text-xs font-semibold tracking-wide text-foreground">
                {workspaceAvatarLabel}
              </AvatarFallback>
            </Avatar>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-semibold leading-5 text-foreground">
                {t('system.pageTitle')}
              </span>
              <span className="mt-0.5 block truncate text-xs leading-4 text-muted-foreground">
                {t('system.moreEntries')}
              </span>
            </span>
            <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          </button>

          <div className="mx-3.5 h-px bg-border" aria-hidden="true" />

          <div className="grid grid-cols-4 gap-0 px-1.5 pb-1.5 pt-1">
            {workspaceShortcuts.map(({ id, path, icon: Icon, labelKey }) => (
              <button
                key={id}
                type="button"
                data-testid={`session-drawer-shortcut-${id}`}
                onClick={() => goWorkspace(path)}
                className="flex min-h-[4.5rem] flex-col items-center justify-center gap-1.5 rounded-xl bg-transparent px-1 py-2.5 text-center transition-colors active:bg-muted"
              >
                <Icon className="h-5 w-5 text-foreground" strokeWidth={1.75} aria-hidden="true" />
                <span className="max-w-full truncate text-[0.6875rem] leading-tight text-muted-foreground">
                  {t(labelKey)}
                </span>
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="px-5 pb-2">
        <h3 className="text-sm font-medium text-muted-foreground">
          {t('session.recent')}
        </h3>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportRef={scrollViewportRef}>
        <div className="space-y-1 overflow-x-hidden px-3 pb-3">
          {renderSessionList(true)}
        </div>
      </ScrollArea>

      <div className="shrink-0 border-t border-border px-4 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))] pt-3">
        <div className="flex h-11 items-stretch gap-2">
          <div className="relative min-w-0 flex-1">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              ref={searchInputRef}
              type="text"
              placeholder={t('session.search')}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-11 rounded-2xl border-border bg-muted-50 pl-9 text-sm sm:h-11"
              data-testid="session-drawer-search"
            />
          </div>
          <Button
            type="button"
            onClick={handleNewSession}
            disabled={isCreating}
            size="icon"
            className="h-11 w-11 shrink-0 rounded-2xl sm:h-11 sm:w-11"
            aria-label={t('session.newChat')}
            data-testid="session-drawer-new-chat"
          >
            <Plus className="h-4 w-4" strokeWidth={2.25} />
          </Button>
        </div>
      </div>
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
          {desktopContent}
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

      {/* Fullscreen mobile workspace — covers the visible viewport (keyboard-
          aware via --app-height / --visual-viewport-offset-top). Desktop keeps
          the fixed sidebar above. `inert` when closed so focus cannot land in
          the offscreen search field. */}
      <div
        id="session-workspace-drawer"
        ref={drawerRef}
        className={cn(
          "fixed inset-x-0 z-50 flex w-full flex-col bg-background lg:hidden",
          "safe-top transform transition-transform duration-slow ease-out",
          open ? "translate-x-0" : "-translate-x-full",
        )}
        style={{
          top: "var(--visual-viewport-offset-top, 0px)",
          height: "var(--app-height, 100dvh)",
        }}
        data-testid="session-workspace-drawer"
        aria-hidden={!open}
        {...(!open ? ({ inert: '' } as Record<string, string>) : {})}
      >
        {mobileDrawerContent}
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
