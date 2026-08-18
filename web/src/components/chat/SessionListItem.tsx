import React from "react"
import { MessageSquare, Trash2, Edit2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"

interface SessionListItemProps {
  session: {
    sessionId?: string | null
    title?: string | null
    preview?: string
    createdAt: number
  }
  isActive: boolean
  onSwitch: (id: string) => void
  onRename: (e: React.MouseEvent, id: string, title: string) => void
  onDelete: (e: React.MouseEvent, id: string) => void
  getDisplayName: (session: { title?: string | null; preview?: string }) => string
}

/**
 * Memoized session list item component.
 * Only re-renders when session.id, title, preview, createdAt, or isActive changes.
 */
export const SessionListItem = React.memo<SessionListItemProps>(
  ({ session, isActive, onSwitch, onRename, onDelete, getDisplayName }) => {
    const handleClick = () => {
      if (session.sessionId) {
        onSwitch(session.sessionId)
      }
    }

    const handleTouchEnd = (e: React.TouchEvent) => {
      e.preventDefault()
      handleClick()
    }

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if ((e.key === 'Enter' || e.key === ' ') && session.sessionId) {
        e.preventDefault()
        onSwitch(session.sessionId)
      }
    }

    return (
      <div
        onClick={handleClick}
        onTouchEnd={handleTouchEnd}
        onKeyDown={handleKeyDown}
        role="button"
        tabIndex={0}
        className={cn(
          "group relative w-full flex items-center gap-2 rounded-md px-2 py-2 text-sm cursor-pointer transition-colors text-left overflow-hidden min-w-0",
          isActive
            ? "bg-primary text-primary-foreground"
            : "hover:bg-muted"
        )}
        style={{ touchAction: 'manipulation' }}
      >
        <MessageSquare className="h-4 w-4 shrink-0" />
        {/* Title with flex shrink */}
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">
            {getDisplayName(session)}
          </div>
          <div className={cn(
            "text-xs truncate",
            isActive
              ? "text-white/70"
              : "text-muted-foreground"
          )}>
            {formatTimestamp(session.createdAt / 1000, false)}
          </div>
        </div>

        {/* Action buttons - always occupy space */}
        {session.sessionId && (
          <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
            {/* Rename button */}
            <Button
              variant="ghost"
              size="icon"
              className={cn(" shrink-0",
                isActive ? "hover:bg-white/20" : ""
              )}
              onClick={(e) => onRename(e, session.sessionId!, session.title || session.preview || "")}
            >
              <Edit2 className="h-4 w-4" />
            </Button>
            {/* Delete button */}
            <Button
              variant="ghost"
              size="icon"
              className={cn(" shrink-0",
                isActive ? "hover:bg-white/20" : ""
              )}
              onClick={(e) => onDelete(e, session.sessionId!)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        )}
      </div>
    )
  },
  (prev, next) => {
    // Custom comparison: only re-render if these specific props change
    return (
      prev.session.sessionId === next.session.sessionId &&
      prev.session.title === next.session.title &&
      prev.session.preview === next.session.preview &&
      prev.session.createdAt === next.session.createdAt &&
      prev.isActive === next.isActive
    )
  }
)

SessionListItem.displayName = "SessionListItem"
