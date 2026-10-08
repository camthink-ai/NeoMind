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

/** The explore tool's result payload (snake_case, from the Rust tool). */
export interface ExploreResult {
  summary?: string
  rounds_used?: number
  tool_calls_made?: number
  wall_clock_ms?: number
  budget_exhausted?: boolean
  rounds?: Array<{ tool: string; note: string }>
}

/**
 * Normalize the tool-call result into the structured shape.
 *
 * Two arrival paths, two shapes: the live-stream wire (ToolCallEnd) carries
 * the result as a display STRING — the JSON text of the payload — while
 * persisted history (loaded from the session store) carries the parsed
 * OBJECT. Without this, the live path renders an empty body (string.summary
 * is undefined), which is exactly the "explore 没有任何内容" report. A
 * non-JSON string (sanitizer-truncated or legacy) degrades to a summary
 * line rather than an empty panel.
 */
function normalizeExploreResult(raw: unknown): ExploreResult | null {
  if (raw == null) return null
  if (typeof raw === "string") {
    const trimmed = raw.trim()
    if (!trimmed) return null
    try {
      const v = JSON.parse(trimmed)
      if (v && typeof v === "object" && !Array.isArray(v)) return v as ExploreResult
    } catch {
      // not JSON — treat the whole string as the summary
    }
    return { summary: trimmed }
  }
  if (typeof raw === "object" && !Array.isArray(raw)) return raw as ExploreResult
  return null
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

  // Completed state: render from the tool result (F3) — normalized across
  // the live-wire string shape and the persisted object shape.
  const result = normalizeExploreResult(toolCall.result)

  // Done when: not streaming (definitively over), OR the result has landed
  // (the tool finished; the stream may still be running its next round —
  // the panel should show the completed state immediately, not vanish in
  // the gap between "last progress event" and "stream end").
  const isDone = result !== null || (!isStreaming && liveDetail === null)

  // ── Completed: from the tool result ──
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
          <div className="space-y-1 border-t border-border px-2 py-1.5">
            {/* The summary is the payload the main agent consumed — always
                show it first; the round notes are supporting detail. */}
            {result.summary && (
              <p className={cn(textMini, "text-foreground")}>{result.summary}</p>
            )}
            {rounds.length > 0 && (
              <ul className="space-y-0.5">
                {rounds.map((r, i) => (
                  <li key={i} className={cn(textMini, "text-muted-foreground")}>
                    <span className="font-mono text-[11px]">{r.tool}</span>
                    {r.note ? ` — ${r.note}` : ""}
                  </li>
                ))}
              </ul>
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
