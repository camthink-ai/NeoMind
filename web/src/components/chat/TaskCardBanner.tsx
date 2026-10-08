/**
 * TaskCardBanner — session-level display of the current task card.
 *
 * Data flow (§3.2): GET /api/sessions/:id/task-card on mount + on the
 * "taskCard" progress event (the update_task_card tool fires it; the
 * chat page's onRawEvent collector triggers the refetch).
 *
 * Placement: docked INSIDE the input area, directly above the composer —
 * a flex sibling, never an overlay. The task card is working context for
 * what the user is about to type, so it belongs next to the input rather
 * than pinned over the conversation (the v1 sticky-top banner covered
 * message history — round-92 redesign). Collapsed to one line; expands
 * to all five slots. Empty card renders nothing.
 *
 * Refresh semantics: a refreshToken bump (mid-stream card update) keeps
 * the previous card on screen until the fetch resolves — no flicker.
 * Stale fetch responses are dropped via a monotonic generation counter.
 */

import { useState, useCallback, useEffect, useRef } from "react"
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
  // Monotonic generation for stale-response detection: rapid taskCard
  // events trigger overlapping fetches; only the LATEST response is applied.
  const fetchGen = useRef(0)
  const prevSessionRef = useRef(sessionId)
  const [expanded, setExpanded] = useState(false)

  const fetchCard = useCallback(async () => {
    if (!sessionId) return
    const gen = ++fetchGen.current
    try {
      const resp = await sessionsApi.getTaskCard(sessionId)
      if (gen !== fetchGen.current) return // a newer fetch superseded this one
      setCard(resp)
    } catch {
      // Non-fatal: the card is an enhancement, not a requirement.
      if (gen === fetchGen.current) setCard(null)
    }
  }, [sessionId])

  // Fetch on mount, on refresh token bump, and on session switch. The card
  // is cleared IMMEDIATELY on session change — without this, the old
  // session's card is visible during the async fetch for the new session's
  // card (stale data flash). A refreshToken bump does NOT clear: mid-stream
  // updates keep the previous card visible until the fresh one lands.
  useEffect(() => {
    if (prevSessionRef.current !== sessionId) {
      setCard(null)
      prevSessionRef.current = sessionId
    }
    fetchCard().catch(() => {})
  }, [fetchCard, sessionId, refreshToken])

  if (!card || card.isEmpty) return null

  const summary = card.goal || card.params || card.step || ""
  if (!summary) return null

  return (
    <div
      className="mb-2 overflow-hidden rounded-lg border border-border/60 bg-muted/30"
      data-testid="task-card-banner"
      role="region"
      aria-label={t("taskCard.title")}
    >
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left"
        aria-expanded={expanded}
      >
        <Target className="size-3.5 shrink-0 text-primary" aria-hidden />
        <span className={cn(textMini, "shrink-0 font-medium text-primary")}>
          {t("taskCard.title")}
        </span>
        <span className={cn(textMini, "min-w-0 truncate text-muted-foreground")}>
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
        <div className="space-y-1 border-t border-border/60 px-2.5 py-2">
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
          "min-w-0 whitespace-pre-wrap",
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
