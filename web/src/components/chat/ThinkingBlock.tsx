/**
 * ThinkingBlock - Displays AI reasoning process
 *
 * Supports two modes:
 * 1. Per-round thinking (roundThinking provided): shows thinking grouped by round
 *    with visual labels, differentiated styling per round
 * 2. Simple thinking (only `thinking` provided): shows as a single block
 *
 * The block is a real surface (warm tint + border) rather than a transparent
 * row, so the three-layer reply (thinking / tools / answer) reads as a stack.
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Brain, ChevronDown, Loader2 } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"

interface ThinkingBlockProps {
  thinking?: string
  /** Per-round thinking for grouped rendering with visual differentiation */
  roundThinking?: Record<number, string>
  isStreaming?: boolean
  defaultExpanded?: boolean
  /** Inline mode: no outer card chrome or internal divider, for embedding
   *  inside a round of the execution timeline. */
  inline?: boolean
}

export function ThinkingBlock({
  thinking,
  roundThinking,
  isStreaming = false,
  defaultExpanded = false,
  inline = false,
}: ThinkingBlockProps) {
  const { t } = useTranslation("chat")
  const [isExpanded, setIsExpanded] = useState(defaultExpanded)

  // Determine what to render
  const hasRoundThinking = roundThinking && Object.keys(roundThinking).length > 0
  const hasSimpleThinking = !!thinking

  if (!hasRoundThinking && !hasSimpleThinking) return null

  // Calculate total thinking length for summary
  const totalLength = hasRoundThinking
    ? Object.values(roundThinking!).reduce((sum, s) => sum + s.length, 0)
    : (thinking?.length ?? 0)

  const roundCount = hasRoundThinking ? Object.keys(roundThinking!).length : 0

  return (
    <Collapsible
      open={isExpanded}
      onOpenChange={setIsExpanded}
      className={cn(
        inline
          ? ""
          : "mb-3 min-w-0 overflow-hidden rounded-xl border border-chat-card-border bg-muted-20"
      )}
    >
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className={cn(
            "w-full flex items-center gap-2 text-left transition-colors hover:bg-muted-30",
            inline ? "px-3 py-1.5 gap-2" : "px-3 py-2.5 gap-3"
          )}
        >
          <Brain
            className={cn(
              "h-4 w-4 shrink-0",
              isStreaming ? "text-info" : "text-muted-foreground"
            )}
            aria-hidden="true"
          />

          <span className={cn("text-sm font-medium", inline && "text-body sm:text-sm")}>
            {t("thinking.title")}
          </span>

          {roundCount > 1 && !inline && (
            <span className={cn(textMini, "text-muted-foreground")}>
              {t("chat:thinking.rounds", { count: roundCount })}
            </span>
          )}

          <div className="flex-1" />

          {isStreaming ? (
            <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
              {t("thinking.thinking")}
            </span>
          ) : (
            <span className={cn(textNano, "text-muted-foreground tabular-nums")}>
              {t("chat:thinking.characters", { count: totalLength })}
            </span>
          )}

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
        <div className={cn("px-3", inline ? "pb-2" : "pb-3")}>
          {hasRoundThinking ? (
            <PerRoundThinking rounds={roundThinking!} isStreaming={isStreaming} />
          ) : (
            <div className="text-body sm:text-sm leading-relaxed text-chat-thinking-text whitespace-pre-wrap break-words">
              {thinking}
              {isStreaming && (
                <span className="inline-block w-1 h-3.5 bg-current animate-blink ml-0.5 align-middle" />
              )}
            </div>
          )}
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

/** Per-round thinking with visual differentiation */
function PerRoundThinking({
  rounds,
  isStreaming,
}: {
  rounds: Record<number, string>
  isStreaming: boolean
}) {
  const sortedRounds = Object.entries(rounds)
    .map(([k, v]) => [Number(k), v] as [number, string])
    .sort((a, b) => a[0] - b[0])

  if (sortedRounds.length === 0) return null

  // If only one round, render without label
  if (sortedRounds.length === 1) {
    const [, text] = sortedRounds[0]
    return (
      <div className="leading-relaxed text-chat-thinking-text whitespace-pre-wrap break-words text-body sm:text-sm">
        {text}
        {isStreaming && (
          <span className="bg-current animate-blink align-middle ml-0.5 inline-block w-1 h-3.5" />
        )}
      </div>
    )
  }

  // Multiple rounds - show with round labels
  return (
    <div className="space-y-2">
      {sortedRounds.map(([round, text], idx) => (
        <div key={round}>
          {/* Round label */}
          <div className="flex items-center gap-1.5 mb-1">
            <span
              className={cn(
                "inline-flex items-center justify-center rounded-full font-medium",
                "h-5 px-2", textMini,
                getRoundColor(round)
              )}
            >
              R{round}
            </span>
          </div>
          {/* Round thinking text */}
          <div className="leading-relaxed text-chat-thinking-text whitespace-pre-wrap break-words pl-6 text-body sm:text-sm">
            {text}
            {idx === sortedRounds.length - 1 && isStreaming && (
              <span className="bg-current animate-blink align-middle ml-0.5 inline-block w-1 h-3.5" />
            )}
          </div>
        </div>
      ))}
    </div>
  )
}

/** Get round badge color - cycles through a subtle palette */
function getRoundColor(round: number): string {
  const colors = [
    "bg-info-light text-info",
    "bg-accent-emerald-light text-accent-emerald",
    "bg-warning-light text-warning",
    "bg-accent-purple-light text-accent-purple",
    "bg-muted text-foreground",
  ]
  return colors[(round - 1) % colors.length]
}
