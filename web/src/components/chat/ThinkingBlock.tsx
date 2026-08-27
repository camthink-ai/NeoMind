/**
 * ThinkingBlock - Displays AI reasoning process
 *
 * Supports two modes:
 * 1. Per-round thinking (roundThinking provided): shows thinking grouped by round
 *    with visual labels, differentiated styling per round
 * 2. Simple thinking (only `thinking` provided): shows as single block
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Brain, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"

interface ThinkingBlockProps {
  thinking?: string
  /** Per-round thinking for grouped rendering with visual differentiation */
  roundThinking?: Record<number, string>
  isStreaming?: boolean
  defaultExpanded?: boolean
  /** Inline mode: no outer card border, for embedding inside ToolProcessBlock */
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

  if (inline) {
    return (
      <div className="border-b border-border">
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-muted-30 transition-colors"
        >
          <Brain className={cn(
            "h-4 w-4",
            isStreaming ? "text-info" : "text-muted-foreground"
          )} />
          <span className={cn("text-body sm:text-sm", "font-medium")}>{t("thinking.title")}</span>
          <div className="flex-1" />
          <ChevronDown className={cn(
            "h-4 w-4 text-muted-foreground transition-transform duration-normal",
            isExpanded && "rotate-180"
          )} />
        </button>
        {isExpanded && (
          <div className="px-3 pb-2">
            {hasRoundThinking ? (
              <PerRoundThinking rounds={roundThinking!} isStreaming={isStreaming} compact />
            ) : (
              <div className={cn("text-sm", "leading-relaxed text-muted-foreground whitespace-pre-wrap break-words")}>
                {thinking}
                {isStreaming && <span className="inline-block w-1 h-3.5 bg-muted-foreground animate-pulse ml-0.5 align-middle" />}
              </div>
            )}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="mb-3 overflow-hidden rounded-lg bg-muted/40">
      {/* Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-muted-50 transition-colors"
      >
        <Brain className={cn(
          "h-4 w-4",
          isStreaming ? "text-info" : "text-muted-foreground"
        )} />

        <span className="text-sm font-medium">{t("thinking.title")}</span>

        {roundCount > 1 && (
          <span className="text-xs text-muted-foreground">
            {roundCount} {t("thinking.rounds", "rounds")}
          </span>
        )}

        <div className="flex-1" />

        <span className="text-xs text-muted-foreground mr-2 tabular-nums min-w-[4rem] text-right inline-block">
          {totalLength} {t("thinking.characters")}
        </span>

        <ChevronDown className={cn(
          "h-4 w-4 text-muted-foreground transition-transform duration-normal",
          isExpanded && "rotate-180"
        )} />
      </button>

      {/* Content */}
      {isExpanded && (
        <div className="px-3 pb-3">
          {hasRoundThinking ? (
            <PerRoundThinking rounds={roundThinking!} isStreaming={isStreaming} />
          ) : (
            <div className="text-sm leading-relaxed text-muted-foreground whitespace-pre-wrap break-words">
              {thinking}
              {isStreaming && <span className="inline-block w-1 h-4 bg-muted-foreground animate-pulse ml-1 align-middle" />}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

/** Per-round thinking with visual differentiation */
function PerRoundThinking({
  rounds,
  isStreaming,
  compact = false,
}: {
  rounds: Record<number, string>
  isStreaming: boolean
  compact?: boolean
}) {
  const sortedRounds = Object.entries(rounds)
    .map(([k, v]) => [Number(k), v] as [number, string])
    .sort((a, b) => a[0] - b[0])

  if (sortedRounds.length === 0) return null

  // If only one round, render without label
  if (sortedRounds.length === 1) {
    const [, text] = sortedRounds[0]
    return (
      <div className={cn(
        "leading-relaxed text-muted-foreground whitespace-pre-wrap break-words", "text-body sm:text-sm",
      )}>
        {text}
        {isStreaming && <span className={cn(
          "bg-muted-foreground animate-pulse align-middle ml-0.5 inline-block w-1 h-3.5"
        )} />}
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
            <span className={cn(
              "inline-flex items-center justify-center rounded-full font-medium",
              "h-5 px-2", textMini,
              getRoundColor(round)
            )}>
              R{round}
            </span>
          </div>
          {/* Round thinking text */}
          <div className={cn(
            "leading-relaxed text-muted-foreground whitespace-pre-wrap break-words pl-6", "text-body sm:text-sm",
          )}>
            {text}
            {idx === sortedRounds.length - 1 && isStreaming && (
              <span className={cn(
                "bg-muted-foreground animate-pulse align-middle ml-0.5 inline-block w-1 h-3.5"
              )} />
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
