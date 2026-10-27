/**
 * TurnTimeline — round-based view of an assistant turn's process.
 *
 * The old layout grouped by category: all thinking in one card, all tool
 * calls in another. That made the causal chain across rounds hard to read.
 * This component groups by ROUND instead: each round owns its thinking, its
 * intermediate output, and the tools it called. The final answer stays
 * outside the timeline, rendered by ChatMessages.
 *
 * Only used when a message has tool calls; a simple answer keeps the plain
 * ThinkingBlock + markdown path.
 */

import { useId, useState } from "react"
import { useTranslation } from "react-i18next"
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Circle,
  Loader2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import type { ToolCall } from "@/types"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { ThinkingBlock } from "./ThinkingBlock"
import { RoundContent, ToolCallItem } from "./ToolCallVisualization"
import { getToolDisplayName, isToolError } from "./toolCallUtils"

interface TurnTimelineProps {
  toolCalls: ToolCall[]
  /** Per-round thinking, keyed by round number. */
  roundThinking?: Record<number, string>
  /** Per-round intermediate output, keyed by round number. */
  roundContents?: Record<number, string>
  /** Legacy non-round thinking; assigned to the first tool round. */
  thinking?: string
  isStreaming?: boolean
  /** Current round while streaming (1-based). */
  currentRound?: number
  /** Cleaned final answer — used to avoid echoing it as an intermediate output. */
  finalContent?: string
}

interface RoundGroup {
  round: number
  thinking?: string
  content?: string
  calls: ToolCall[]
}

function buildRounds({
  toolCalls,
  roundThinking,
  roundContents,
  thinking,
  finalContent,
}: TurnTimelineProps): RoundGroup[] {
  const groups = new Map<number, RoundGroup>()
  const ensure = (round: number): RoundGroup => {
    let group = groups.get(round)
    if (!group) {
      group = { round, calls: [] }
      groups.set(round, group)
    }
    return group
  }

  for (const [key, text] of Object.entries(roundThinking ?? {})) {
    if (!text?.trim()) continue
    ensure(Number(key)).thinking = text
  }

  const final = finalContent?.trim()
  for (const [key, text] of Object.entries(roundContents ?? {})) {
    if (!text?.trim()) continue
    if (final && text.trim() === final) continue
    ensure(Number(key)).content = text
  }

  for (const call of toolCalls) {
    ensure(call.round ?? 1).calls.push(call)
  }

  // Legacy thinking has no round; attach it to the first tool round rather
  // than inventing an R1 that never existed.
  const hasRoundThinking = Object.keys(roundThinking ?? {}).length > 0
  if (thinking?.trim() && !hasRoundThinking) {
    const firstRound = toolCalls.length > 0
      ? Math.min(...toolCalls.map(call => call.round ?? 1))
      : 1
    ensure(firstRound).thinking = thinking
  }

  return [...groups.values()].sort((a, b) => a.round - b.round)
}

function roundPreview(group: RoundGroup): string {
  const text = group.thinking?.trim() || group.content?.trim()
  if (text) {
    const firstLine = text.split("\n").find(line => line.trim()) ?? text
    const cleaned = firstLine.replace(/^[#>\-*\s]+/, "").replace(/\*\*/g, "")
    return cleaned.length > 90 ? `${cleaned.slice(0, 87)}…` : cleaned
  }
  return group.calls
    .map(call => getToolDisplayName(call.name, call.arguments))
    .join(" · ")
}

export function TurnTimeline({
  toolCalls,
  roundThinking,
  roundContents,
  thinking,
  isStreaming = false,
  currentRound,
  finalContent,
}: TurnTimelineProps) {
  const { t } = useTranslation("chat")
  const idPrefix = useId()
  const rounds = buildRounds({
    toolCalls,
    roundThinking,
    roundContents,
    thinking,
    finalContent,
  })

  // Expansion is decided once at mount: a turn that streamed open must not
  // snap shut when isStreaming flips false. The user keeps their choice until
  // they toggle it.
  const [isExpanded, setIsExpanded] = useState(() => isStreaming || toolCalls.length <= 4)
  const [openRounds, setOpenRounds] = useState<Record<number, boolean>>({})

  if (rounds.length === 0) return null

  const totalTools = toolCalls.length
  const hasError = toolCalls.some(isToolError)
  const allComplete = totalTools > 0 && toolCalls.every(tc => tc.result != null) && !isStreaming
  const activeRound = isStreaming ? (currentRound ?? 1) : undefined
  const counts = [
    t("chat:toolCall.roundsCount", { count: rounds.length }),
    ...(totalTools > 0 ? [t("chat:toolCall.callsCount", { count: totalTools })] : []),
  ].join(" · ")

  return (
    <Collapsible
      open={isExpanded}
      onOpenChange={setIsExpanded}
      data-testid="turn-timeline"
      className="mb-4 min-w-0 overflow-hidden rounded-xl border border-chat-card-border bg-muted-20"
    >
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="w-full flex items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-muted-30"
        >
          <div
            className={cn(
              "h-4 w-4 rounded flex items-center justify-center shrink-0",
              hasError && !isStreaming && "bg-error-light text-error",
              !hasError && allComplete && "bg-accent-emerald-light text-accent-emerald",
              isStreaming && "bg-warning-light text-warning",
              !allComplete && !isStreaming && !hasError && "bg-muted text-muted-foreground"
            )}
          >
            {isStreaming ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            ) : hasError ? (
              <AlertCircle className="h-4 w-4" aria-hidden="true" />
            ) : allComplete ? (
              <CheckCircle2 className="h-4 w-4" aria-hidden="true" />
            ) : (
              <Circle className="h-3.5 w-3.5" aria-hidden="true" />
            )}
          </div>
          <span className="text-body sm:text-sm font-medium text-foreground truncate">
            {t("chat:timeline.title")} · {counts}
          </span>
          <div className="flex-1" />
          <ChevronDown
            className={cn(
              "h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-normal",
              isExpanded && "rotate-180"
            )}
            aria-hidden="true"
          />
        </button>
      </CollapsibleTrigger>

      <CollapsibleContent className="overflow-hidden data-[state=open]:animate-collapsible-down">
        <div className="border-t border-border">
          {rounds.map(group => {
            const isOpen = openRounds[group.round] ?? true
            const isActive = activeRound === group.round
            const failed = group.calls.some(isToolError)
            const complete = group.calls.length > 0
              && group.calls.every(call => call.result != null)
              && !isActive
            const preview = roundPreview(group)
            const bodyId = `${idPrefix}-round-${group.round}`

            return (
              <div key={group.round} className="border-t border-border first:border-t-0">
                <button
                  type="button"
                  onClick={() =>
                    setOpenRounds(prev => ({ ...prev, [group.round]: !isOpen }))
                  }
                  aria-expanded={isOpen}
                  aria-controls={bodyId}
                  className={cn(
                    "w-full flex items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-muted-30",
                    isActive && "bg-muted-30"
                  )}
                >
                  <span
                    className={cn(
                      "inline-flex h-6 shrink-0 items-center justify-center rounded-md px-2.5 font-semibold",
                      "border border-border bg-muted text-foreground text-xs"
                    )}
                  >
                    R{group.round}
                  </span>
                  <span
                    className={cn(
                      "min-w-0 flex-1 truncate text-body sm:text-sm",
                      preview ? "text-foreground font-medium" : "text-muted-foreground"
                    )}
                  >
                    {preview || t("chat:timeline.roundFallback", { round: group.round })}
                  </span>
                  {isActive && (
                    <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-warning" aria-hidden="true" />
                  )}
                  {failed && !isActive && (
                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-error" aria-hidden="true" />
                  )}
                  {/* Completed rounds only need a summary marker when collapsed;
                      the tool rows below own the expanded state. Reserve green
                      for the outer, whole-process summary. */}
                  {!isOpen && complete && !failed && (
                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                  )}
                  {group.calls.length > 0 && (
                    <span className={cn(textNano, "shrink-0 whitespace-nowrap text-muted-foreground tabular-nums")}>
                      {t("chat:toolCall.countShort", { count: group.calls.length })}
                    </span>
                  )}
                  <ChevronDown
                    className={cn(
                      "h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-normal",
                      isOpen && "rotate-180"
                    )}
                    aria-hidden="true"
                  />
                </button>

                {isOpen && (
                  <div id={bodyId}>
                    {group.thinking && group.thinking.trim() && (
                      <ThinkingBlock thinking={group.thinking} inline defaultExpanded={false} />
                    )}
                    {group.content && group.content.trim() && (
                      <RoundContent content={group.content} />
                    )}
                    {group.calls.map((call, index) => (
                      <ToolCallItem
                        key={`${group.round}-${index}`}
                        toolCall={call}
                        isStreaming={isActive && call.result == null}
                        t={t}
                      />
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}
