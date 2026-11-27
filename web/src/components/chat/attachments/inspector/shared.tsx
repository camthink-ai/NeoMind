import { useState } from "react"
import { Check, Copy } from "lucide-react"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { resolveImageSrc } from "@/lib/imageUtils"
import { textMini } from "@/design-system/tokens/typography"

export function extractImageValue(v: unknown): string | null {
  if (typeof v !== "string") return null
  const s = v.trim()
  if (s.startsWith("/api/images/") || s.startsWith("data:image/")) {
    return resolveImageSrc(s)
  }
  return null
}

export function formatMetricValue(v: unknown): string {
  if (v === null || v === undefined || v === "") return "—"
  if (typeof v === "string") {
    if (v.startsWith("data:image/") || v.startsWith("/api/images/")) return "[Image]"
    return v.length > 48 ? `${v.slice(0, 45)}…` : v
  }
  if (typeof v === "number") {
    return Number.isInteger(v) ? String(v) : Number(v.toFixed(2)).toString()
  }
  if (typeof v === "boolean") return String(v)
  try {
    const s = JSON.stringify(v)
    return s.length > 48 ? `${s.slice(0, 45)}…` : s
  } catch {
    return String(v)
  }
}

export function formatLastSeen(raw: unknown): string | null {
  if (raw === null || raw === undefined || raw === "" || raw === 0 || raw === "0") {
    return null
  }
  if (typeof raw === "number") {
    if (raw <= 0) return null
    return formatTimestamp(raw, false)
  }
  if (typeof raw === "string") {
    const num = Number(raw)
    if (Number.isFinite(num) && num > 0) return formatTimestamp(num, false)
    const parsed = Date.parse(raw)
    if (Number.isFinite(parsed) && parsed > 0) return formatTimestamp(parsed, false)
  }
  return null
}

export function formatDurationMs(ms: number | undefined | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return "—"
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.round(ms / 1000)}s`
}

export function CopyIdPill({ id, isZh }: { id: string; isZh: boolean }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(id).catch(() => {})
    }
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1400)
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      title={isZh ? `点击复制 ID: ${id}` : `Copy ID: ${id}`}
      className="inline-flex h-6 min-h-0 min-w-0 max-w-[190px] items-center gap-1.5 rounded-md border border-border bg-muted-20 px-2 font-mono text-xs text-muted-foreground transition-colors hover:bg-muted-30 hover:text-foreground"
    >
      <span className="truncate">{copied ? (isZh ? "已复制" : "Copied") : id}</span>
      {copied ? (
        <Check className="h-3 w-3 shrink-0 text-success" aria-hidden="true" />
      ) : (
        <Copy className="h-3 w-3 shrink-0 opacity-70" aria-hidden="true" />
      )}
    </button>
  )
}

export interface SegmentedTabItem<T extends string> {
  id: T
  label: string
  count?: number
}

export function SegmentedTabs<T extends string>({
  items,
  value,
  onChange,
}: {
  items: SegmentedTabItem<T>[]
  value: T
  onChange: (next: T) => void
}) {
  return (
    <div
      role="tablist"
      className="grid gap-1 rounded-xl border border-border bg-muted-20 p-1"
      style={{ gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))` }}
    >
      {items.map((item) => {
        const active = item.id === value
        return (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(item.id)}
            className={cn(
              "inline-flex h-7 min-h-0 min-w-0 items-center justify-center gap-1.5 rounded-lg px-2 text-xs font-medium transition-colors",
              active
                ? "border border-border bg-card text-foreground shadow-sm"
                : "border border-transparent text-muted-foreground hover:text-foreground"
            )}
          >
            <span className="truncate">{item.label}</span>
            {item.count != null && (
              <span className="tabular-nums opacity-75">({item.count})</span>
            )}
          </button>
        )
      })}
    </div>
  )
}

export function InfoCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className={cn(textMini, "text-muted-foreground")}>{label}</div>
      <div className="mt-0.5 truncate text-sm font-medium text-foreground" title={value}>
        {value}
      </div>
    </div>
  )
}

export function SnapshotKV({ snapshot }: { snapshot: Record<string, unknown> }) {
  const entries = Object.entries(snapshot || {}).filter(
    ([, v]) => v !== null && v !== undefined && v !== ""
  )
  if (entries.length === 0) return null
  return (
    <div className="grid grid-cols-2 gap-2.5">
      {entries.map(([k, v]) => (
        <div key={k} className="min-w-0 rounded-xl border border-border bg-card px-3.5 py-2.5">
          <div className={cn(textMini, "truncate font-mono text-muted-foreground")}>{k}</div>
          <div className="mt-0.5 truncate text-sm font-medium text-foreground">
            {formatMetricValue(v)}
          </div>
        </div>
      ))}
    </div>
  )
}
