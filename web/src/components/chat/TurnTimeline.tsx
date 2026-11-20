/**
 * TurnTimeline — round-based view of an assistant turn's process.
 *
 * Expanded layout is flat and scannable: compact R1/R2 prefixes beside the
 * first row of each round (not a lone label line), collapsed thinking
 * (`思考 · N 字`), one tool row per call, and inter-round text shown
 * directly in muted body type. The final answer stays outside, rendered by
 * ChatMessages.
 *
 * Only used when a message has tool calls; a simple answer keeps the plain
 * ThinkingBlock + markdown path.
 */

import { useState, type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import {
  AlertCircle,
  ChevronDown,
  Loader2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import type { ToolCall } from "@/types"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { ThinkingBlock } from "./ThinkingBlock"
import { RoundContent, ToolCallItem } from "./ToolCallVisualization"
import { isToolError } from "./toolCallUtils"

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

/** Compact round marker: prefix on the first row only; later rows share the gutter.
 *  Match ThinkingBlock / tool / note: text-base + leading-5 + py-2 so the
 *  label and first content line share one top edge (no mt offset, no h-box). */
function RoundRow({
  round,
  showLabel,
  isActive,
  children,
}: {
  round: number
  showLabel: boolean
  isActive?: boolean
  children: ReactNode
}) {
  return (
    <div className="flex items-start gap-2">
      <span
        className="w-7 shrink-0 py-2 text-base font-medium leading-5 tabular-nums text-muted-foreground"
        aria-hidden={!showLabel}
        data-testid={showLabel ? `round-label-${round}` : undefined}
      >
        {showLabel ? (
          <span className="inline-flex items-center gap-1 leading-5">
            R{round}
            {isActive && (
              <Loader2 className="h-3.5 w-3.5 text-warning motion-safe:animate-spin" aria-hidden="true" />
            )}
          </span>
        ) : null}
      </span>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  )
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
  const rounds = buildRounds({
    toolCalls,
    roundThinking,
    roundContents,
    thinking,
    finalContent,
  })

  // A turn that streamed open must not snap shut when isStreaming flips
  // false. The user keeps their choice until they toggle it.
  const [isExpanded, setIsExpanded] = useState(() => isStreaming)

  if (rounds.length === 0) return null

  const totalTools = toolCalls.length
  const hasError = toolCalls.some(isToolError)
  const activeRound = isStreaming ? (currentRound ?? 1) : undefined
  const counts = [
    t("chat:toolCall.roundsCount", { count: rounds.length }),
    ...(totalTools > 0 ? [t("chat:toolCall.callsCount", { count: totalTools })] : []),
  ].join(" · ")
  const duration = toolCalls.reduce((sum, call) => sum + (call.durationMs ?? 0), 0)

  return (
    <Collapsible
      open={isExpanded}
      onOpenChange={setIsExpanded}
      data-testid="turn-timeline"
      className="mb-4 min-w-0 overflow-hidden"
    >
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex min-h-10 w-full items-center gap-2 rounded-md py-1.5 text-left text-muted-foreground transition-colors hover:text-foreground focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring focus-visible:-outline-offset-2"
        >
          <span className="min-w-0 truncate text-base font-medium">
            {t("chat:timeline.title")} · {counts}
            {!isStreaming && duration > 0 ? ` · ${Math.round(duration / 100) / 10} s` : ""}
          </span>
          <div className="flex-1" />
          {isStreaming ? (
            <Loader2 className="h-3.5 w-3.5 shrink-0 motion-safe:animate-spin text-muted-foreground" aria-hidden="true" />
          ) : hasError ? (
            <AlertCircle className="h-4 w-4 shrink-0 text-error" aria-hidden="true" />
          ) : null}
          <ChevronDown
            className={cn(
              "h-4 w-4 shrink-0 text-current transition-transform duration-normal",
              isExpanded && "rotate-180"
            )}
            aria-hidden="true"
          />
        </button>
      </CollapsibleTrigger>

      <CollapsibleContent className="overflow-hidden motion-safe:data-[state=open]:animate-collapsible-down">
        <div className="mb-1 space-y-2">
          {rounds.map(group => {
            const isActive = activeRound === group.round
            const rows: ReactNode[] = []

            if (group.thinking?.trim()) {
              rows.push(
                <ThinkingBlock
                  key={`think-${group.round}`}
                  thinking={group.thinking}
                  inline
                  defaultExpanded={false}
                />
              )
            }
            for (let index = 0; index < group.calls.length; index++) {
              const call = group.calls[index]
              rows.push(
                <ToolCallItem
                  key={`call-${group.round}-${index}`}
                  toolCall={call}
                  isStreaming={!!isActive && call.result == null}
                  t={t}
                />
              )
            }
            if (group.content?.trim()) {
              rows.push(
                <RoundContent key={`content-${group.round}`} content={group.content} />
              )
            }

            if (rows.length === 0) return null

            return (
              <div
                key={group.round}
                data-testid={`turn-round-${group.round}`}
                className="space-y-0"
              >
                {rows.map((row, idx) => (
                  <RoundRow
                    key={idx}
                    round={group.round}
                    showLabel={idx === 0}
                    isActive={isActive && idx === 0}
                  >
                    {row}
                  </RoundRow>
                ))}
              </div>
            )
          })}
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}
