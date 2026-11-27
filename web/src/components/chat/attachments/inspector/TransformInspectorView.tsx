import { useState } from "react"
import { ArrowRight, Check, Copy, Puzzle } from "lucide-react"
import type { Automation } from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { CopyIdPill } from "./shared"

interface TransformInspectorViewProps {
  automation: Automation
  definition: Record<string, unknown> | null
  snapshotScope?: unknown
  entityId?: string
  isZh: boolean
}

function formatTransformScope(scope: unknown, isZh: boolean): string {
  if (!scope) return isZh ? "全局" : "Global"
  if (typeof scope === "string") {
    return scope === "global" ? (isZh ? "全局" : "Global") : scope
  }
  if (typeof scope === "object" && !Array.isArray(scope)) {
    const obj = scope as Record<string, unknown>
    if (typeof obj.device_type === "string" && obj.device_type) {
      return `${isZh ? "设备类型" : "Type"}: ${obj.device_type}`
    }
    if (typeof obj.device === "string" && obj.device) {
      return `${isZh ? "设备" : "Device"}: ${obj.device}`
    }
    if (typeof obj.device_id === "string" && obj.device_id) {
      return `${isZh ? "设备" : "Device"}: ${obj.device_id}`
    }
    const entries = Object.entries(obj).filter(([, v]) => v != null && v !== "")
    if (entries.length > 0) {
      return entries.map(([k, v]) => `${k}: ${String(v)}`).join(", ")
    }
  }
  return isZh ? "全局" : "Global"
}

function extractOutputMetrics(definition: Record<string, unknown> | null): string[] {
  if (!definition) return []
  const raw = definition.output_metrics ?? definition.metrics
  if (!Array.isArray(raw)) return []
  return raw
    .map((item) => {
      if (typeof item === "string") return item
      if (item && typeof item === "object") {
        const obj = item as Record<string, unknown>
        if (typeof obj.name === "string") return obj.name
        if (typeof obj.metric === "string") return obj.metric
      }
      return ""
    })
    .filter(Boolean)
}

export function TransformInspectorView({
  automation,
  definition,
  snapshotScope,
  entityId,
  isZh,
}: TransformInspectorViewProps) {
  const [copiedCode, setCopiedCode] = useState(false)

  const scopeLabel = formatTransformScope(definition?.scope ?? snapshotScope, isZh)
  const outputPrefix =
    typeof definition?.output_prefix === "string" && definition.output_prefix.trim()
      ? definition.output_prefix.trim()
      : null
  const outputMetrics = extractOutputMetrics(definition)

  const transformCode =
    (definition?.js_code as string | undefined) ??
    (definition?.code as string | undefined) ??
    (definition?.expression as string | undefined) ??
    null

  const extOp =
    definition?.extension_operation && typeof definition.extension_operation === "object"
      ? (definition.extension_operation as Record<string, unknown>)
      : definition?.extension_id
        ? {
            extension_id: definition.extension_id,
            operation: definition.operation ?? definition.command,
            params: definition.params ?? definition.parameters,
          }
        : null

  const codeLines = transformCode ? transformCode.split("\n") : []

  const handleCopyCode = () => {
    if (!transformCode) return
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(transformCode).catch(() => {})
    }
    setCopiedCode(true)
    window.setTimeout(() => setCopiedCode(false), 1400)
  }

  return (
    <div className="space-y-4">
      {/* Hero Summary */}
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <span
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium",
              automation.enabled ? "badge-success" : "bg-muted-50 text-muted-foreground"
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                automation.enabled ? "bg-success" : "bg-muted-foreground"
              )}
            />
            {automation.enabled ? (isZh ? "已启用" : "Enabled") : isZh ? "已停用" : "Disabled"}
          </span>
          {entityId && <CopyIdPill id={entityId} isZh={isZh} />}
        </div>
        {automation.description && (
          <div className="px-4 py-3 text-xs leading-relaxed text-muted-foreground">
            {automation.description}
          </div>
        )}
      </div>

      {/* Data Flow Pipeline */}
      <div className="space-y-2.5 rounded-xl border border-border bg-card p-3.5">
        <span className={cn(textMini, "block font-medium uppercase tracking-wider text-muted-foreground")}>
          {isZh ? "数据流向" : "Data Pipeline"}
        </span>
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="min-w-0 rounded-lg border border-border bg-muted-20 p-2.5">
            <div className={cn(textMini, "text-muted-foreground")}>
              {isZh ? "输入作用域" : "Input Scope"}
            </div>
            <div className="mt-0.5 truncate text-xs font-medium text-foreground" title={scopeLabel}>
              {scopeLabel}
            </div>
          </div>
          <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
          <div className="min-w-0 rounded-lg border border-border bg-muted-20 p-2.5">
            <div className={cn(textMini, "text-muted-foreground")}>
              {isZh ? "输出前缀" : "Output Prefix"}
            </div>
            <div
              className={cn(
                "mt-0.5 truncate font-mono text-xs font-medium",
                outputPrefix ? "text-accent-purple" : "text-muted-foreground"
              )}
              title={outputPrefix || undefined}
            >
              {outputPrefix || (isZh ? "无前缀" : "None")}
            </div>
          </div>
        </div>
        {outputMetrics.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
            <span className={cn(textMini, "text-muted-foreground")}>
              {isZh ? "产出指标：" : "Outputs:"}
            </span>
            {outputMetrics.map((m) => (
              <code
                key={m}
                className="rounded border border-border bg-muted-20 px-2 py-0.5 font-mono text-xs text-foreground"
              >
                {outputPrefix && !m.startsWith(outputPrefix) ? `${outputPrefix}${m}` : m}
              </code>
            ))}
          </div>
        )}
      </div>

      {/* JS Code Block */}
      {transformCode && (
        <div className="overflow-hidden rounded-xl border border-border bg-card">
          <div className="flex items-center justify-between border-b border-border bg-muted-20 px-3.5 py-2 text-xs">
            <span className="font-mono text-muted-foreground">
              JavaScript · {codeLines.length} {isZh ? "行" : "lines"}
            </span>
            <button
              type="button"
              onClick={handleCopyCode}
              className="inline-flex h-6 min-h-0 min-w-0 items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
            >
              {copiedCode ? (
                <Check className="h-3.5 w-3.5 text-success" aria-hidden="true" />
              ) : (
                <Copy className="h-3.5 w-3.5" aria-hidden="true" />
              )}
              <span>{copiedCode ? (isZh ? "已复制" : "Copied") : isZh ? "复制代码" : "Copy"}</span>
            </button>
          </div>
          <div className="max-h-72 overflow-auto p-3 font-mono text-xs leading-relaxed text-foreground">
            {codeLines.map((line, i) => (
              <div key={i} className="flex gap-3">
                <span className="w-6 shrink-0 select-none text-right tabular-nums text-muted-foreground opacity-55">
                  {i + 1}
                </span>
                <span className="whitespace-pre-wrap break-words">{line || " "}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Extension Operation (non-JS transform) */}
      {!transformCode && extOp && (
        <div className="space-y-2.5 rounded-xl border border-border bg-card p-3.5">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-1.5 text-xs font-medium text-foreground">
              <Puzzle className="h-3.5 w-3.5 text-accent-purple" aria-hidden="true" />
              <span>{isZh ? "扩展算子转换" : "Extension Operation"}</span>
            </div>
            <span
              className={cn(
                textNano,
                "rounded bg-accent-purple-light px-1.5 py-0.5 font-mono text-accent-purple"
              )}
            >
              extension_operation
            </span>
          </div>
          <div className="grid grid-cols-2 gap-2.5 text-xs">
            <div className="min-w-0 rounded-lg border border-border bg-muted-20 p-2.5">
              <div className={cn(textMini, "text-muted-foreground")}>
                {isZh ? "扩展 ID" : "Extension ID"}
              </div>
              <div className="mt-0.5 truncate font-mono font-medium text-foreground">
                {String(extOp.extension_id || "—")}
              </div>
            </div>
            <div className="min-w-0 rounded-lg border border-border bg-muted-20 p-2.5">
              <div className={cn(textMini, "text-muted-foreground")}>
                {isZh ? "算子操作" : "Operation"}
              </div>
              <div className="mt-0.5 truncate font-mono font-medium text-foreground">
                {String(extOp.operation || "—")}
              </div>
            </div>
          </div>
          {extOp.params != null && (
            <pre className="max-h-40 overflow-auto rounded-lg border border-border bg-muted-20 p-2.5 font-mono text-xs text-muted-foreground">
              {JSON.stringify(extOp.params, null, 2)}
            </pre>
          )}
        </div>
      )}
    </div>
  )
}
