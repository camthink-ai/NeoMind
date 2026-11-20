/**
 * ThinkingBlock - Displays AI reasoning process
 *
 * Supports two modes:
 * 1. Per-round thinking (roundThinking provided): shows thinking grouped by round
 * 2. Simple thinking (only `thinking` provided): shows as a single block
 *
 * Inline mode (inside TurnTimeline): one-line `思考 · N 字`, default collapsed.
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { ChevronDown, Loader2 } from "lucide-react"
import { cn } from "@/lib/utils"
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
          : "mb-3 min-w-0"
      )}
    >
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className={cn(
            "w-full flex items-center gap-2 text-left transition-colors hover:text-foreground focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring focus-visible:-outline-offset-2",
            inline ? "items-center gap-2 py-2" : "py-2.5 gap-3"
          )}
        >
          {inline ? (
            <span className="min-w-0 truncate text-base leading-5 text-muted-foreground">
              {t("thinking.inlineTitle")}
              {!isStreaming && (
                <>
                  {" · "}
                  <span className="tabular-nums">
                    {t("chat:thinking.characters", { count: totalLength })}
                  </span>
                </>
              )}
            </span>
          ) : (
            <>
              <span className="text-base font-medium">
                {t("thinking.title")}
              </span>

              {roundCount > 1 && (
                <span className="text-base text-muted-foreground">
                  {t("chat:thinking.rounds", { count: roundCount })}
                </span>
              )}

              <div className="flex-1" />

              {isStreaming ? (
                <span className="inline-flex items-center gap-1.5 text-base text-muted-foreground">
                  <Loader2 className="h-3 w-3 motion-safe:animate-spin" aria-hidden="true" />
                  {t("thinking.thinking")}
                </span>
              ) : (
                <span className="text-base tabular-nums text-muted-foreground">
                  {t("chat:thinking.characters", { count: totalLength })}
                </span>
              )}
            </>
          )}

          {inline && <div className="flex-1" />}

          {inline && isStreaming && (
            <span className="inline-flex shrink-0 items-center gap-1.5 text-base leading-5 text-muted-foreground">
              <Loader2 className="h-4 w-4 motion-safe:animate-spin" aria-hidden="true" />
              {t("thinking.thinking")}
            </span>
          )}

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
        <div className={inline ? "pb-2" : "pb-3"}>
          {hasRoundThinking ? (
            <PerRoundThinking rounds={roundThinking!} isStreaming={isStreaming} />
          ) : (
            <div className="text-base leading-relaxed text-foreground whitespace-pre-wrap break-words">
              {thinking}
              {isStreaming && (
                <span className="inline-block w-1 h-3.5 bg-current motion-safe:animate-pulse ml-0.5 align-middle" />
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
      <div className="leading-relaxed text-foreground whitespace-pre-wrap break-words text-base">
        {text}
        {isStreaming && (
          <span className="bg-current motion-safe:animate-pulse align-middle ml-0.5 inline-block w-1 h-3.5" />
        )}
      </div>
    )
  }

  // Multiple rounds — stacked paragraphs, no R1/Rn chrome.
  return (
    <div className="space-y-2">
      {sortedRounds.map(([round, text], idx) => (
        <div
          key={round}
          className="leading-relaxed text-foreground whitespace-pre-wrap break-words text-base"
        >
          {text}
          {idx === sortedRounds.length - 1 && isStreaming && (
            <span className="bg-current motion-safe:animate-pulse align-middle ml-0.5 inline-block w-1 h-3.5" />
          )}
        </div>
      ))}
    </div>
  )
}
