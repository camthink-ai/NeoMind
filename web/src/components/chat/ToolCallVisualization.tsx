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
  AlertCircle,
  Loader2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { formatElapsedMs } from "@/lib/utils/format"
import type { ToolCall } from "@/types"
import {
  extractToolDelivery,
  getToolDisplayName,
  getToolFailureReason,
  isToolError,
} from "./toolCallUtils"
import {
  deriveCommandDeliveryLadder,
  type SendCommandDeliveryResult,
} from "@/lib/commandDeliveryLadder"

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

function deliverySubtitleKey(delivery: SendCommandDeliveryResult): string | null {
  const ladder = deriveCommandDeliveryLadder(delivery)
  if (!ladder.dispatched && !ladder.reachedKind) return null
  if (ladder.allowVerifiedSuccess) return "commandDelivery.summaryVerified"
  if (ladder.reachedKind === "adapter_accepted") return "commandDelivery.summaryAdapter"
  if (ladder.reachedKind === "extension_accepted") return "commandDelivery.summaryExtension"
  if (ladder.reachedKind === "device_acknowledged") return "commandDelivery.summaryDeviceAck"
  if (ladder.dispatched) return "commandDelivery.summaryUnknown"
  return null
}

// ─── RoundContent: inter-round text shown directly (muted, secondary) ───

export function RoundContent({ content }: { content: string }) {
  if (!content || content.trim().length === 0) return null

  return (
    <div
      data-testid="round-intermediate"
      className="py-2 text-base leading-5 text-muted-foreground whitespace-pre-wrap [overflow-wrap:anywhere]"
    >
      {content}
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
  const { t: tDevices } = useTranslation("devices")
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
  const failureReason = failed ? getToolFailureReason(toolCall) : null
  const delivery = !failed ? extractToolDelivery(toolCall) : null
  const deliveryKey = delivery ? deliverySubtitleKey(delivery) : null
  const displayName = getToolDisplayName(toolCall.name, toolCall.arguments)

  return (
    <div>
      <button
        type="button"
        onClick={() => { if (hasDetails) setIsExpanded(!isExpanded) }}
        aria-expanded={hasDetails ? isExpanded : undefined}
        aria-controls={hasDetails ? detailsId : undefined}
        className={cn(
          "flex w-full items-center gap-2 rounded py-2 text-left transition-colors",
          hasDetails && "hover:text-foreground focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring focus-visible:-outline-offset-2"
        )}
        aria-label={hasDetails ? t("toolCall.toggleDetails") : undefined}
      >
        <span className={cn("min-w-0 truncate text-base leading-5", failed ? "text-error" : "text-foreground")}>
          <span className="mr-2 text-muted-foreground">{t("toolCall.label")}</span>
          {displayName}
        </span>
        {!isStreaming && toolCall.durationMs != null && toolCall.durationMs > 0 && (
          <span className="shrink-0 text-base tabular-nums text-muted-foreground">
            {formatElapsedMs(toolCall.durationMs)}
          </span>
        )}
        {status === "running" && (
          <span className="shrink-0 text-base text-warning">
            {t("toolCall.status.running")}
          </span>
        )}
        {failed && (
          <span className="max-w-[40%] shrink-0 truncate text-base text-error" title={failureReason ?? undefined}>
            {failureReason || t("toolCall.status.failed")}
          </span>
        )}
        <div className="flex-1" />
        {/* Status icons share the trailing cluster with the chevron (vertically centered). */}
        {status === "running" && (
          <Loader2 className="h-4 w-4 shrink-0 self-center text-warning motion-safe:animate-spin" aria-hidden="true" />
        )}
        {failed && <AlertCircle className="h-4 w-4 shrink-0 self-center text-error" aria-hidden="true" />}
        {hasDetails && (
          <ChevronDown
            className={cn(
              "h-4 w-4 shrink-0 self-center text-current transition-transform duration-normal",
              isExpanded && "rotate-180"
            )}
            aria-hidden="true"
          />
        )}
      </button>
      {deliveryKey && (
        <div className="pl-[2.5rem] text-base text-muted-foreground" data-testid="tool-delivery-subtitle">
          {tDevices(`devices:${deliveryKey}`)}
        </div>
      )}
      {isExpanded && hasDetails && (
        <div id={detailsId} className="mb-1 mt-0.5 space-y-1">
          {hasArguments && (
            <div className="py-1">
              <div className="mb-1 flex items-center gap-1.5 text-sm text-muted-foreground">
                {t("toolCall.arguments")}
              </div>
              <pre className="max-h-40 overflow-y-auto overscroll-contain whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground [overflow-wrap:anywhere]">
                {formatJson(toolCall.arguments)}
              </pre>
            </div>
          )}
          {hasResult && (
            <div className="py-1">
              <div className="mb-1 flex items-center gap-1.5 text-sm text-muted-foreground">
                {t("toolCall.result")}
              </div>
              <pre className="max-h-40 overflow-y-auto overscroll-contain whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground [overflow-wrap:anywhere]">
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
