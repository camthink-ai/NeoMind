/**
 * SubagentExplorePanel — live progress display for the explore subagent.
 *
 * Data flow (§2.0d/F1/F2): the backend emits `Progress` events with
 * `stage: "explore"` and a camelCase `detail` payload; chat.tsx's
 * onRawEvent collects them into `exploreProgress` state and passes the
 * LATEST event down. Matching is by stage alone — single-flight on the
 * backend guarantees at most one explore per stream, so no toolCallId is
 * needed (F1). After the stream ends, the completed state renders from
 * the tool call's persisted result (F3: wall clock + per-round notes live
 * in the result, so refresh doesn't lose the summary).
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { ChevronDown, Search } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini } from "@/design-system/tokens/typography"
import type { ToolCall } from "@/types"

/** The `detail` payload from the backend Progress event (camelCase wire). */
export interface ExploreDetail {
  toolCallId?: string | null
  round: number
  maxRounds: number
  currentTool: string
  note?: string
  toolCallsSoFar: number
  wallClockMs: number
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  const rem = s % 60
  return `${m}m${rem.toString().padStart(2, "0")}s`
}

interface SubagentExplorePanelProps {
  /** The explore tool call (with result when completed). */
  toolCall: ToolCall
  /** Latest live progress event during streaming; null after/without. */
  liveDetail: ExploreDetail | null
  isStreaming?: boolean
}

export function SubagentExplorePanel({
  toolCall,
  liveDetail,
  isStreaming = false,
}: SubagentExplorePanelProps) {
  const { t } = useTranslation("chat")
  const [expanded, setExpanded] = useState(true)

  // Completed state: render from the persisted tool result (F3).
  const result = toolCall.result as
    | {
        summary?: string
        rounds_used?: number
        tool_calls_made?: number
        wall_clock_ms?: number
        budget_exhausted?: boolean
        rounds?: Array<{ tool: string; note: string }>
      }
    | undefined

  const isDone = !isStreaming && (result !== undefined || liveDetail === null)

  // ── Completed: from the persisted result ──
  if (isDone && result) {
    const rounds = result.rounds ?? []
    return (
      <div
        className="mt-1 rounded-md border border-border bg-muted/30"
        data-testid="explore-panel-done"
      >
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="flex w-full items-center gap-2 px-2 py-1.5 text-left"
          aria-expanded={expanded}
        >
          <Search className="size-3.5 shrink-0 text-primary" aria-hidden />
          <span className={cn(textMini, "text-muted-foreground")}>
            {t("explore.completed", {
              rounds: result.rounds_used ?? 0,
              calls: result.tool_calls_made ?? 0,
              duration: formatDuration(result.wall_clock_ms ?? 0),
            })}
            {result.budget_exhausted ? ` · ${t("explore.budgetExhausted")}` : ""}
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
          <div className="border-t border-border px-2 py-1.5">
            {rounds.length > 0 ? (
              <ul className="space-y-0.5">
                {rounds.map((r, i) => (
                  <li key={i} className={cn(textMini, "text-muted-foreground")}>
                    <span className="font-mono text-[11px]">{r.tool}</span>
                    {r.note ? ` — ${r.note}` : ""}
                  </li>
                ))}
              </ul>
            ) : (
              <p className={cn(textMini, "text-muted-foreground")}>
                {result.summary}
              </p>
            )}
          </div>
        )}
      </div>
    )
  }

  // ── In-progress: from the live detail ──
  if (liveDetail) {
    return (
      <div
        className="mt-1 rounded-md border border-border bg-muted/30 px-2 py-1.5"
        data-testid="explore-panel-live"
        role="status"
        aria-label={t("explore.inProgress", {
          round: liveDetail.round,
          max: liveDetail.maxRounds,
        })}
      >
        <div className="flex items-center gap-2">
          <span
            className="inline-block size-2 animate-pulse rounded-full bg-primary motion-reduce:animate-none"
            aria-hidden
          />
          <span className={cn(textMini, "text-muted-foreground")}>
            {t("explore.inProgress", {
              round: liveDetail.round,
              max: liveDetail.maxRounds,
            })}
          </span>
          <span className={cn(textMini, "ml-auto text-muted-foreground/70")}>
            {formatDuration(liveDetail.wallClockMs)} ·{" "}
            {liveDetail.toolCallsSoFar} {t("explore.toolCalls")}
          </span>
        </div>
        {liveDetail.currentTool && (
          <p
            className={cn(
              textMini,
              "mt-0.5 truncate font-mono text-[11px] text-muted-foreground/80",
            )}
          >
            {liveDetail.currentTool}
            {liveDetail.note ? ` — ${liveDetail.note}` : ""}
          </p>
        )}
      </div>
    )
  }

  // ── Degraded: no detail (legacy event or pre-relay backend) — render
  // nothing; the ordinary tool-call row above already conveys "explore ran".
  return null
}
