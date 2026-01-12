import { useEffect, useRef } from "react"
import { useStore } from "@/store"
import { Plus, MessageSquare, Trash2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
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

function formatTimeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000)
  if (seconds < 60) return "刚刚"
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`
  if (seconds < 604800) return `${Math.floor(seconds / 86400)} 天前`
  return new Date(timestamp).toLocaleDateString()
}

export function SessionSidebar({ onNewChat }: SessionSidebarProps) {
  const {
    sessions,
    sessionId,
    switchSession,
    deleteSession,
    createSession,
    loadSessions,
  } = useStore()
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null)
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

  return (
    <>
      <div className="flex h-full w-64 flex-col border-r bg-muted/10">
        {/* Header */}
        <div className="flex items-center justify-between border-b p-3">
          <h2 className="text-sm font-semibold">会话列表</h2>
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
                <p>暂无会话</p>
                <p className="text-xs">点击 + 创建新会话</p>
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
                  <div className="min-w-0 flex-1 truncate">
                    <div className="truncate font-medium">
                      {session.preview || "新对话"}
                    </div>
                    <div className={`text-xs ${
                      sessionId === session.sessionId
                        ? "text-primary-foreground/70"
                        : "text-muted-foreground"
                    }`}>
                      {formatTimeAgo(session.createdAt)}
                    </div>
                  </div>

                  {/* Delete button */}
                  {session.sessionId && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className={`h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 ${
                        sessionId === session.sessionId ? "hover:bg-primary-foreground/20" : ""
                      }`}
                      onClick={(e) => handleDeleteClick(e, session.sessionId)}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
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
            <DialogTitle>删除会话</DialogTitle>
            <DialogDescription>
              确定要删除这个会话吗？此操作无法撤销。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteDialogOpen(false)}>
              取消
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
