import { useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Plus, MessageSquare, Trash2, Edit2 } from "lucide-react"
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
} from "@/components/ui/dialog"
import { useState } from "react"

interface SessionSidebarProps {
  onNewChat?: () => void
}

function formatTimeAgo(timestamp: number, t: (key: string, params?: any) => string): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000)
  if (seconds < 60) return t('timeAgo.justNow')
  if (seconds < 3600) return t('timeAgo.minutesAgo', { count: Math.floor(seconds / 60) })
  if (seconds < 86400) return t('timeAgo.hoursAgo', { count: Math.floor(seconds / 3600) })
  if (seconds < 604800) return t('timeAgo.daysAgo', { count: Math.floor(seconds / 86400) })
  return new Date(timestamp).toLocaleDateString()
}

export function SessionSidebar({ onNewChat }: SessionSidebarProps) {
  const { t } = useTranslation(['common', 'dashboard'])
  const {
    sessions,
    sessionId,
    switchSession,
    deleteSession,
    updateSessionTitle,
    createSession,
    loadSessions,
  } = useStore()
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
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
    await createSession()
    setLoading(false)
    onNewChat?.()
  }

  const handleSwitchSession = async (id: string) => {
    if (id === sessionId) return
    setLoading(true)
    await switchSession(id)
    setLoading(false)
  }

  const handleDeleteClick = (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    setSessionToDelete(id)
    setDeleteDialogOpen(true)
  }

  const handleDeleteConfirm = async () => {
    if (sessionToDelete) {
      setLoading(true)
      await deleteSession(sessionToDelete)
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

  const getDisplayName = (session: { title?: string | null; preview?: string }) => {
    if (session.title) return session.title
    if (session.preview) return session.preview
    return t('defaultTitle')
  }

  return (
    <>
      <div className="flex h-full w-64 flex-col border-r bg-muted/10">
        {/* Header */}
        <div className="flex items-center justify-between border-b p-3">
          <h2 className="text-sm font-semibold">{t('sessionList')}</h2>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={handleNewChat}
            disabled={loading}
          >
            <Plus className="h-4 w-4" />
          </Button>
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
                <button
                  key={session.sessionId || `session-${index}`}
                  onClick={() => session.sessionId && handleSwitchSession(session.sessionId)}
                  className={`group relative flex items-center gap-2 rounded-md px-3 py-2 text-sm cursor-pointer transition-colors w-full text-left ${
                    sessionId === session.sessionId
                      ? "bg-primary text-primary-foreground"
                      : "hover:bg-muted"
                  }`}
                >
                  <MessageSquare className="h-4 w-4 shrink-0" />
                  <div className="min-w-0 flex-1 truncate text-left">
                    <div className="truncate font-medium">
                      {getDisplayName(session)}
                    </div>
                    <div className={`text-xs ${
                      sessionId === session.sessionId
                        ? "text-primary-foreground/70"
                        : "text-muted-foreground"
                    }`}>
                      {formatTimeAgo(session.createdAt, t)}
                    </div>
                  </div>

                  {/* Action buttons */}
                  {session.sessionId && (
                    <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100">
                      {/* Rename button */}
                      <Button
                        variant="ghost"
                        size="icon"
                        className={`h-6 w-6 shrink-0 ${
                          sessionId === session.sessionId ? "hover:bg-primary-foreground/20" : ""
                        }`}
                        onClick={(e) => handleRenameClick(e, session.sessionId, session.title || session.preview || "")}
                      >
                        <Edit2 className="h-3 w-3" />
                      </Button>
                      {/* Delete button */}
                      <Button
                        variant="ghost"
                        size="icon"
                        className={`h-6 w-6 shrink-0 ${
                          sessionId === session.sessionId ? "hover:bg-primary-foreground/20" : ""
                        }`}
                        onClick={(e) => handleDeleteClick(e, session.sessionId)}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  )}
                </button>
              ))
            )}
          </div>
        </ScrollArea>
      </div>

      {/* Delete Confirmation Dialog */}
      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('deleteSessionTitle')}</DialogTitle>
            <DialogDescription>
              {t('deleteDesc')}
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

      {/* Rename Dialog */}
      <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('renameSession')}</DialogTitle>
            <DialogDescription>
              {t('renameDesc')}
            </DialogDescription>
          </DialogHeader>
          <div className="py-4">
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
          </div>
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
    </>
  )
}
