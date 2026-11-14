/**
 * Shared Transform test result presentation (1.3 iter D).
 * Metrics, unverified deps, and warnings are all visible; green success
 * only when deriveTransformTestStatus.allowGreenSuccess.
 */
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { AlertTriangle, CheckCircle2, ChevronDown, ChevronRight, HelpCircle, XCircle } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import {
  deriveTransformTestStatus,
  type TransformTestVerdict,
} from "@/lib/transformTestStatus"

export interface TransformTestMetricRow {
  device_id: string
  metric: string
  value: number
  timestamp: number
  quality: number | null
}

export interface TransformTestResultPanelProps {
  metrics: TransformTestMetricRow[]
  count: number
  warnings?: string[]
  unverified?: string[]
  /** From test-code path. */
  codeSuccess?: boolean
  /** Optional raw JSON output (builder panel). */
  outputText?: string | null
  className?: string
  compact?: boolean
}

function verdictTone(verdict: TransformTestVerdict): {
  icon: typeof CheckCircle2
  className: string
} {
  switch (verdict) {
    case "verified_success":
      return { icon: CheckCircle2, className: "text-success" }
    case "partial_success":
    case "unverified":
      return { icon: AlertTriangle, className: "text-warning" }
    case "no_output":
      return { icon: HelpCircle, className: "text-muted-foreground" }
    case "failed":
      return { icon: XCircle, className: "text-error" }
  }
}

function ExpandableList({
  title,
  items,
  toneClass,
  defaultOpen,
}: {
  title: string
  items: string[]
  toneClass: string
  defaultOpen: boolean
}) {
  const [open, setOpen] = useState(defaultOpen)
  if (items.length === 0) return null
  return (
    <div className="mt-3">
      <button
        type="button"
        className={cn("flex w-full items-center gap-1 text-left text-sm font-medium", toneClass)}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 shrink-0" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-4 w-4 shrink-0" aria-hidden="true" />
        )}
        {title}
        <Badge variant="outline" className="ml-1">
          {items.length}
        </Badge>
      </button>
      {open && (
        <ul className="mt-1 space-y-1 pl-5">
          {items.map((item, idx) => (
            <li key={idx} className={cn("text-sm flex items-start gap-2", toneClass)}>
              <AlertTriangle className="h-3.5 w-3.5 mt-0.5 shrink-0" aria-hidden="true" />
              <span className="break-words">{item}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

export function TransformTestResultPanel({
  metrics,
  count,
  warnings = [],
  unverified = [],
  codeSuccess,
  outputText,
  className,
  compact = false,
}: TransformTestResultPanelProps) {
  const { t } = useTranslation(["automation", "common"])
  const status = deriveTransformTestStatus({
    count,
    warnings,
    unverified,
    codeSuccess,
  })
  const tone = verdictTone(status.verdict)
  const Icon = tone.icon

  const verdictLabel = t(`automation:testVerdict.${status.verdict}`, {
    defaultValue: status.verdict,
  })

  return (
    <div className={cn("space-y-3", className)} data-transform-test-result data-verdict={status.verdict}>
      <div className="flex flex-wrap items-center gap-2">
        <Icon className={cn("h-5 w-5", tone.className)} aria-hidden="true" />
        <span className={cn("font-medium", tone.className)}>{verdictLabel}</span>
        <Badge variant="outline">
          {status.metricCount} {t("automation:metrics", { defaultValue: "metrics" })}
        </Badge>
        {status.unverifiedCount > 0 && (
          <Badge variant="outline" className="text-warning border-warning">
            {status.unverifiedCount} {t("automation:unverified", { defaultValue: "unverified" })}
          </Badge>
        )}
        {status.warningCount > 0 && (
          <Badge variant="outline" className="text-warning border-warning">
            {status.warningCount} {t("automation:warnings", { defaultValue: "warnings" })}
          </Badge>
        )}
      </div>

      {status.verdict === "unverified" && (
        <p className="text-xs text-muted-foreground" role="note">
          {t("automation:testVerdict.unverifiedHint", {
            defaultValue:
              "Dry-run blocked side-effecting calls. Computed metrics below are not a verified end-to-end success.",
          })}
        </p>
      )}

      {/* Metrics / output — always shown when present, even alongside unverified */}
      {metrics.length > 0 ? (
        <div>
          <h4 className="font-medium text-sm mb-1">
            {t("automation:generatedMetrics", { defaultValue: "Generated Metrics" })}
          </h4>
          <div className={cn("space-y-2", compact && "max-h-32 overflow-auto")}>
            {metrics.map((metric, idx) => (
              <div
                key={`${metric.metric}-${idx}`}
                className={cn(
                  "pl-3 border-l-2",
                  status.allowGreenSuccess ? "border-success" : "border-warning",
                )}
              >
                <div className="flex items-center gap-2 flex-wrap">
                  <Badge variant="outline">{metric.metric}</Badge>
                  <span className="font-mono text-sm">{metric.value}</span>
                  {metric.quality !== null && (
                    <span className="text-xs text-muted-foreground">
                      (q: {(metric.quality * 100).toFixed(0)}%)
                    </span>
                  )}
                </div>
                <div className="text-xs text-muted-foreground">
                  {metric.device_id} @ {formatTimestamp(metric.timestamp)}
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : outputText ? (
        <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all rounded-md bg-muted p-2 max-h-40 overflow-auto">
          {outputText}
        </pre>
      ) : (
        <p className="text-sm text-muted-foreground">
          {t("automation:noMetricsGenerated", {
            defaultValue: "No metrics were generated. Check your transform configuration.",
          })}
        </p>
      )}

      <ExpandableList
        title={t("automation:unverifiedDeps", { defaultValue: "Unverified dependencies" })}
        items={unverified}
        toneClass="text-warning"
        defaultOpen
      />
      <ExpandableList
        title={t("automation:warnings", { defaultValue: "Warnings" })}
        items={warnings}
        toneClass="text-warning"
        defaultOpen={status.verdict === "failed" || status.verdict === "partial_success"}
      />
    </div>
  )
}
