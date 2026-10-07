/**
 * TaskCardBanner — sticky session-level display of the current task card.
 *
 * Data flow (§3.2): GET /api/sessions/:id/task-card on mount + on the
 * "taskCard" progress event (the update_task_card tool fires it; the
 * chat page's onRawEvent collector triggers the refetch). Renders as a
 * sticky banner above the message list — collapsed to one line on mobile,
 * expandable to show all five slots. Empty card renders nothing.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { ChevronDown, Target } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini } from "@/design-system/tokens/typography"
import { sessionsApi } from "@/lib/api/sessions"

/** The GET /task-card response shape (all slots nullable). */
export interface TaskCardData {
  goal?: string | null
  params?: string | null
  step?: string | null
  open?: string | null
  findings?: string | null
  isEmpty?: boolean
}

interface TaskCardBannerProps {
  sessionId: string
  /** Bumped by the parent when a taskCard progress event fires (refetch). */
  refreshToken: number
}

export function TaskCardBanner({ sessionId, refreshToken }: TaskCardBannerProps) {
  const { t } = useTranslation("chat")
  const [card, setCard] = useState<TaskCardData | null>(null)
  const [expanded, setExpanded] = useState(false)
  const [loading, setLoading] = useState(false)

  const fetchCard = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    try {
      const resp = await sessionsApi.getTaskCard(sessionId)
      setCard(resp)
    } catch {
      // Non-fatal: the card is an enhancement, not a requirement.
      setCard(null)
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  // Fetch on mount, on refresh token bump, and on session switch.
  // The card is cleared IMMEDIATELY on session change — without this,
  // the old session's card is visible during the async fetch for the
  // new session's card (stale data flash).
  useEffect(() => {
    setCard(null) // clear stale card from previous session
    void fetchCard()
  }, [fetchCard, refreshToken])

  if (!card || card.isEmpty || loading) return null

  const summary = card.goal || card.params || card.step || ""
  if (!summary) return null

  return (
    <div
      className="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
      data-testid="task-card-banner"
      role="region"
      aria-label={t("taskCard.title")}
    >
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left"
        aria-expanded={expanded}
      >
        <Target className="size-3.5 shrink-0 text-primary" aria-hidden />
        <span className={cn(textMini, "truncate text-muted-foreground")}>
          {summary}
        </span>
        <ChevronDown
          className={cn(
            "ml-auto size-3 shrink-0 text-muted-foreground transition-transform",
            expanded ? "" : "-rotate-90",
          )}
          aria-hidden
        />
      </button>
      {expanded && (
        <div className="space-y-1 border-t border-border px-3 py-2">
          {card.goal && (
            <CardSlot label={t("taskCard.goal")} value={card.goal} />
          )}
          {card.params && (
            <CardSlot
              label={t("taskCard.params")}
              value={card.params}
              highlight
            />
          )}
          {card.step && (
            <CardSlot label={t("taskCard.step")} value={card.step} />
          )}
          {card.open && (
            <CardSlot label={t("taskCard.open")} value={card.open} />
          )}
          {card.findings && (
            <CardSlot label={t("taskCard.findings")} value={card.findings} />
          )}
        </div>
      )}
    </div>
  )
}

function CardSlot({
  label,
  value,
  highlight = false,
}: {
  label: string
  value: string
  highlight?: boolean
}) {
  return (
    <div className="flex gap-2">
      <span className={cn(textMini, "shrink-0 font-medium text-muted-foreground")}>
        {label}
      </span>
      <span
        className={cn(
          textMini,
          highlight
            ? "rounded bg-primary/10 px-1 text-foreground"
            : "text-muted-foreground",
        )}
      >
        {value}
      </span>
    </div>
  )
}
