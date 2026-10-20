/**
 * Tool call sub-components for the round-based execution timeline.
 *
 * TurnTimeline owns the outer process card and round grouping; this file
 * provides the reusable intermediate-output row (RoundContent) and the
 * single tool-call row (ToolCallItem).
 */

import { useId, useState } from "react"
import { useTranslation } from "react-i18next"
import {
  ChevronDown,
  CheckCircle2,
  AlertCircle,
  Loader2,
  Code,
  FileText,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini, textCode, textNano } from "@/design-system/tokens/typography"
import { formatElapsedMs } from "@/lib/utils/format"
import type { ToolCall } from "@/types"
import { getToolDisplayName, isToolError } from "./toolCallUtils"

// ─── Helpers ───

type ToolStatus = "pending" | "running" | "completed"

function getToolStatus(hasResult: boolean, isStreaming: boolean): ToolStatus {
  if (hasResult) return "completed"
  if (isStreaming) return "running"
  return "pending"
}

function formatJson(data: unknown): string {
  if (typeof data === "string") {
    try {
      const parsed = JSON.parse(data)
      return JSON.stringify(parsed, null, 2)
    } catch {
      return data
    }
  }
  return JSON.stringify(data, null, 2)
}

// ─── RoundContent: intermediate output row inside a round ───

export function RoundContent({ content }: { content: string }) {
  const [isExpanded, setIsExpanded] = useState(false)
  const contentId = useId()
  const { t } = useTranslation("chat")

  if (!content || content.trim().length === 0) return null

  return (
    <div>
      <button
        type="button"
        onClick={() => setIsExpanded(!isExpanded)}
        aria-expanded={isExpanded}
        aria-controls={contentId}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-muted-30"
      >
        <FileText className="h-4 w-4 text-muted-foreground shrink-0" aria-hidden="true" />
        {!isExpanded && (
          <span className="text-sm text-muted-foreground truncate">
            {content.slice(0, 80)}{content.length > 80 && "..."}
          </span>
        )}
        {isExpanded && (
          <span className="text-sm text-muted-foreground">
            {t("toolCall.hideDetails")}
          </span>
        )}
        <div className="flex-1" />
        <ChevronDown
          className={cn(
            "h-4 w-4 text-muted-foreground transition-transform duration-normal",
            isExpanded && "rotate-180"
          )}
          aria-hidden="true"
        />
      </button>
      {isExpanded && (
        <div id={contentId} className="px-3 pb-2">
          <div className="text-sm font-mono text-muted-foreground whitespace-pre-wrap break-words leading-relaxed">
            {content}
          </div>
        </div>
      )}
    </div>
  )
}

// ─── ToolCallItem: single tool call with expandable args/result ───

export function ToolCallItem({
  toolCall,
  isStreaming,
  t,
}: {
  toolCall: ToolCall
  isStreaming: boolean
  t: (key: string, options?: Record<string, unknown>) => string
}) {
  const [isExpanded, setIsExpanded] = useState(false)
  const detailsId = useId()
  const status = getToolStatus(
    toolCall.result !== undefined && toolCall.result !== null,
    isStreaming
  )
  const hasArguments = toolCall.arguments !== undefined && toolCall.arguments !== null
  const hasResult = toolCall.result !== undefined && toolCall.result !== null
  const hasDetails = hasArguments || hasResult
  const failed = isToolError(toolCall)

  const statusLabels = {
    pending: t("toolCall.status.pending"),
    running: t("toolCall.status.running"),
    completed: t("toolCall.status.completed"),
    failed: t("toolCall.status.failed"),
  }

  return (
    <div className="px-3 py-1">
      <button
        type="button"
        onClick={() => { if (hasDetails) setIsExpanded(!isExpanded) }}
        aria-expanded={hasDetails ? isExpanded : undefined}
        aria-controls={hasDetails ? detailsId : undefined}
        className={cn(
          "w-full flex items-center gap-2 rounded text-left transition-colors",
          hasDetails && "hover:bg-muted-30"
        )}
        aria-label={hasDetails ? t("toolCall.toggleDetails") : undefined}
      >
        <div className={cn(
          "h-4 w-4 rounded-full flex items-center justify-center shrink-0",
          failed && "text-error",
          !failed && status === "completed" && "text-muted-foreground",
          status === "running" && "text-warning",
          status === "pending" && "text-muted-foreground"
        )}>
          {status === "running" ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
          ) : failed ? (
            <AlertCircle className="h-4 w-4" aria-hidden="true" />
          ) : status === "completed" ? (
            <CheckCircle2 className="h-4 w-4" aria-hidden="true" />
          ) : (
            <div className="h-1.5 w-1.5 rounded-full bg-muted-foreground" />
          )}
        </div>
        <span className={cn("text-sm", "truncate", failed ? "text-error" : "text-muted-foreground")}>
          {getToolDisplayName(toolCall.name, toolCall.arguments)}
        </span>
        {!isStreaming && toolCall.durationMs != null && toolCall.durationMs > 0 && (
          <span className={cn(textNano, "text-muted-foreground tabular-nums shrink-0")}>
            {formatElapsedMs(toolCall.durationMs)}
          </span>
        )}
        {status === "running" && (
          <span className={cn(textMini, "px-1.5 py-0.5 rounded bg-warning-light text-warning shrink-0")}>
            {statusLabels[status]}
          </span>
        )}
        {failed && (
          <span className={cn(textMini, "px-1.5 py-0.5 rounded bg-error-light text-error shrink-0")}>
            {statusLabels.failed}
          </span>
        )}
        <div className="flex-1" />
        {hasDetails && (
          <ChevronDown className={cn(
            "h-4 w-4 text-muted-foreground transition-transform duration-normal shrink-0",
            isExpanded && "rotate-180"
          )} aria-hidden="true" />
        )}
      </button>
      {isExpanded && hasDetails && (
        <div id={detailsId} className="ml-5.5 mt-0.5 mb-1 space-y-1">
          {hasArguments && (
            <div className="rounded bg-muted-30 p-1.5">
              <div className={cn(textMini, "text-muted-foreground mb-0.5 flex items-center gap-1")}>
                <Code className="h-3 w-3" aria-hidden="true" />{t("toolCall.arguments")}
              </div>
              <pre className={cn(textCode, "font-mono text-muted-foreground whitespace-pre-wrap [overflow-wrap:anywhere] max-h-40 overflow-y-auto overscroll-contain leading-relaxed")}>
                {formatJson(toolCall.arguments)}
              </pre>
            </div>
          )}
          {hasResult && (
            <div className="rounded bg-muted-30 p-1.5">
              <div className={cn(textMini, "text-muted-foreground mb-0.5 flex items-center gap-1")}>
                {failed ? <AlertCircle className="h-3 w-3" aria-hidden="true" /> : <CheckCircle2 className="h-3 w-3" aria-hidden="true" />}
                {t("toolCall.result")}
              </div>
              <pre className={cn(textCode, "font-mono text-muted-foreground whitespace-pre-wrap [overflow-wrap:anywhere] max-h-40 overflow-y-auto overscroll-contain leading-relaxed")}>
                {formatJson(toolCall.result)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

// ─── Legacy exports (keep for backward compat if other files import them) ───

export function PerRoundBlocks(_props: any) {
  return null // Deprecated: use TurnTimeline + MarkdownMessage instead
}

export function ToolCallVisualization(_props: any) {
  return null // Deprecated: use TurnTimeline instead
}
