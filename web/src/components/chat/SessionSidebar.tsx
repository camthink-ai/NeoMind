import { useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { Plus, MessageSquare, X, Eraser } from "lucide-react"
import { SessionListItem } from "./SessionListItem"
import { SessionListItemIcon } from "./SessionListItemIcon"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { useState } from "react"
import { cn } from "@/lib/utils"
import { useErrorHandler } from "@/hooks/useErrorHandler"

interface SessionSidebarProps {
  onNewChat?: () => void
  onClose?: () => void
  mode?: 'full' | 'icon'
  onNewChatFromIcon?: () => void
}

export function SessionSidebar({ onNewChat, onClose, mode = 'full', onNewChatFromIcon }: SessionSidebarProps) {
  const { t } = useTranslation(['common', 'dashboard'])
  const navigate = useNavigate()
  const { handleError } = useErrorHandler()
  const {
    sessions,
    sessionId,
    switchSession,
    deleteSession,
    clearAllSessions,
    updateSessionTitle,
    createSession,
    loadSessions,
  } = useStore()
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [clearAllDialogOpen, setClearAllDialogOpen] = useState(false)
  const [renameDialogOpen, setRenameDialogOpen] = useState(false)
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null)
  const [sessionToRename, setSessionToRename] = useState<string | null>(null)
  const [newTitle, setNewTitle] = useState("")
  const [loading, setLoading] = useState(false)

  // Load sessions on mount (once)
  const hasLoadedSessions = useRef(false)
  useEffect(() => {
    if (!hasLoadedSessions.current) {
      hasLoadedSessions.current = true
      loadSessions()
    }
  }, [])

  const handleNewChat = async () => {
    setLoading(true)
    const newSessionId = await createSession()
    if (newSessionId) {
      navigate(`/chat/${newSessionId}`)
    }
    setLoading(false)
    onNewChat?.()
  }

  const handleSwitchSession = async (id: string) => {
    if (id === sessionId) return
    // Navigate first - the chat page will handle session loading
    // This prevents double-switching and eliminates UI flicker
    navigate(`/chat/${id}`)
  }

  const handleDeleteClick = (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    setSessionToDelete(id)
    setDeleteDialogOpen(true)
  }

  const handleDeleteConfirm = async () => {
    if (sessionToDelete) {
      setLoading(true)

      try {
        await deleteSession(sessionToDelete)
        // deleteSession already handles:
        // - Reloading the session list
        // - Switching to the first available session
        // - Creating a new session if needed
      } catch (error) {
        handleError(error, { operation: 'Delete session', showToast: false })
      }

      setLoading(false)
      setDeleteDialogOpen(false)
      setSessionToDelete(null)
    }
  }

  const handleRenameClick = (e: React.MouseEvent, id: string, currentTitle: string) => {
    e.stopPropagation()
    setSessionToRename(id)
    setNewTitle(currentTitle || "")
    setRenameDialogOpen(true)
  }

  const handleRenameConfirm = async () => {
    if (sessionToRename) {
      setLoading(true)
      await updateSessionTitle(sessionToRename, newTitle)
      setLoading(false)
      setRenameDialogOpen(false)
      setSessionToRename(null)
      setNewTitle("")
    }
  }

  const handleClearAllConfirm = async () => {
    setLoading(true)
    try {
      await clearAllSessions()
      setClearAllDialogOpen(false)
    } catch (error) {
      handleError(error, { operation: 'Clear all sessions', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  const getDisplayName = (session: { title?: string | null; preview?: string }) => {
    if (session.title) return session.title
    if (session.preview) return session.preview
    return t('defaultTitle')
  }

  return (
    <>
      {mode === 'icon' ? (
        // Icon mode - compact sidebar
        <div className="flex h-full w-full flex-col border-r bg-muted/10 overflow-hidden">
          {/* Header - New chat button */}
          <div className="flex items-center justify-center border-b p-2">
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={onNewChatFromIcon}
              disabled={loading}
              title={t('newChat')}
            >
              <Plus className="h-4 w-4" />
            </Button>
          </div>

          {/* Session List - icons only */}
          <ScrollArea className="flex-1">
            <div className="p-1.5 space-y-1 overflow-hidden">
              {sessions.map((session, index) => (
                <SessionListItemIcon
                  key={session.sessionId || `session-${index}`}
                  session={session}
                  isActive={sessionId === session.sessionId}
                  onSwitch={handleSwitchSession}
                  getDisplayName={getDisplayName}
                />
              ))}
            </div>
          </ScrollArea>
        </div>
      ) : (
        // Full mode - complete sidebar
        <div className="flex h-full w-full flex-col border-r bg-muted/10 overflow-hidden">
          {/* Header */}
          <div className="flex items-center justify-between border-b p-3">
            <div className="flex items-center gap-2">
              {/* Mobile close button */}
              {onClose && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 lg:hidden"
                  onClick={onClose}
                >
                  <X className="h-4 w-4" />
                </Button>
              )}
              <h2 className="text-sm font-semibold">{t('sessionList')}</h2>
            </div>
            <div className="flex items-center gap-1">
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleNewChat}
                disabled={loading}
                title={t('newChat')}
              >
                <Plus className="h-4 w-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-destructive"
                onClick={() => setClearAllDialogOpen(true)}
                disabled={loading || sessions.length === 0}
                title={t('clearHistory')}
              >
                <Eraser className="h-4 w-4" />
              </Button>
            </div>
          </div>

          {/* Session List */}
          <ScrollArea className="flex-1">
            <div className="p-2 space-y-1">
              {sessions.length === 0 ? (
                <div className="py-8 text-center text-sm text-muted-foreground">
                  <MessageSquare className="mx-auto mb-2 h-8 w-8 opacity-50" />
                  <p>{t('noSessions')}</p>
                  <p className="text-xs">{t('noSessionsDesc')}</p>
                </div>
              ) : (
                sessions.map((session, index) => (
                  <SessionListItem
                    key={session.sessionId || `session-${index}`}
                    session={session}
                    isActive={sessionId === session.sessionId}
                    onSwitch={handleSwitchSession}
                    onRename={handleRenameClick}
                    onDelete={handleDeleteClick}
                    getDisplayName={getDisplayName}
                  />
                ))
              )}
            </div>
          </ScrollArea>
        </div>
      )}

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent className="flex flex-col">
          <DialogHeader>
            <DialogTitle>{t('deleteSessionTitle')}</DialogTitle>
            <DialogDescription>
              {t('deleteDesc')}
            </DialogDescription>
          </DialogHeader>
          <DialogContentBody>
          </DialogContentBody>
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

      {/* Rename Dialog */}
      <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
        <DialogContent className="flex flex-col">
          <DialogHeader>
            <DialogTitle>{t('renameSession')}</DialogTitle>
            <DialogDescription>
              {t('renameDesc')}
            </DialogDescription>
          </DialogHeader>
          <DialogContentBody className="py-4">
            <Input
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              placeholder={t('renamePlaceholder')}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  handleRenameConfirm()
                }
              }}
            />
          </DialogContentBody>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenameDialogOpen(false)}>
              {t('cancel')}
            </Button>
            <Button onClick={handleRenameConfirm}>
              {t('save')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Clear All History Confirmation Dialog */}
      <Dialog open={clearAllDialogOpen} onOpenChange={setClearAllDialogOpen}>
        <DialogContent className="flex flex-col">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Eraser className="h-5 w-5 text-destructive" />
              {t('clearHistoryTitle', { ns: 'dashboard' })}
            </DialogTitle>
            <DialogDescription>
              {t('clearHistoryDesc', { ns: 'dashboard', count: sessions.length })}
            </DialogDescription>
          </DialogHeader>
          <DialogContentBody>
          </DialogContentBody>
          <DialogFooter>
            <Button variant="outline" onClick={() => setClearAllDialogOpen(false)} disabled={loading}>
              {t('cancel')}
            </Button>
            <Button variant="destructive" onClick={handleClearAllConfirm} disabled={loading}>
              {loading ? t('clearing', { ns: 'dashboard' }) : t('clearHistoryConfirm', { ns: 'dashboard' })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
