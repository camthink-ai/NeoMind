import { useMemo, useState } from "react"
import { Activity, ChevronDown, Search, Terminal } from "lucide-react"
import type {
  CommandDefinition,
  Device,
  DeviceCurrentStateResponse,
  ParameterDataType,
} from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { DeviceStatusBadge } from "@/components/shared/DeviceStatusBadge"
import { ZoomableImage } from "@/components/shared/ZoomableImage"
import {
  CopyIdPill,
  InfoCell,
  SegmentedTabs,
  extractImageValue,
  formatLastSeen,
  formatMetricValue,
} from "./shared"

interface DeviceInspectorViewProps {
  device: Device
  deviceCurrent: DeviceCurrentStateResponse | null
  entityId?: string
  snapshotLastSeen?: unknown
  isZh: boolean
  onAttach?: () => void
}

function formatParamType(dt: ParameterDataType): string {
  if (typeof dt === "string") return dt
  if ("enum" in dt) return "enum"
  if ("array" in dt) return "array"
  return "any"
}

export function DeviceInspectorView({
  device,
  deviceCurrent,
  entityId,
  snapshotLastSeen,
  isZh,
  onAttach,
}: DeviceInspectorViewProps) {
  const [tab, setTab] = useState<"telemetry" | "commands">("telemetry")
  const [query, setQuery] = useState("")
  const [showEmptyMetrics, setShowEmptyMetrics] = useState(false)

  const commands: CommandDefinition[] = deviceCurrent?.commands ?? []

  const { imageMetrics, populatedMetrics, emptyMetrics } = useMemo(() => {
    const rawMetrics =
      deviceCurrent?.metrics ??
      (deviceCurrent as unknown as { current_values?: Record<string, unknown> })?.current_values ??
      {}
    const imgs: Array<{ key: string; label: string; src: string }> = []
    const populated: Array<{
      key: string
      label: string
      value: string
      unit?: string
      isVirtual: boolean
    }> = []
    const empty: Array<{ key: string; label: string }> = []

    for (const [k, raw] of Object.entries(rawMetrics)) {
      const isWrapped = raw && typeof raw === "object" && !Array.isArray(raw) && "value" in raw
      const label = isWrapped
        ? (raw as { display_name?: string }).display_name?.trim() || k
        : k
      const val = isWrapped ? (raw as { value?: unknown }).value : raw
      const unit = isWrapped ? (raw as { unit?: string }).unit?.trim() || undefined : undefined
      const isVirtual = isWrapped ? Boolean((raw as { is_virtual?: boolean }).is_virtual) : false

      const imgSrc = extractImageValue(val)
      if (imgSrc) {
        imgs.push({ key: k, label, src: imgSrc })
        continue
      }
      if (val === null || val === undefined || val === "") {
        empty.push({ key: k, label })
        continue
      }
      populated.push({
        key: k,
        label,
        value: formatMetricValue(val),
        unit,
        isVirtual,
      })
    }

    return { imageMetrics: imgs, populatedMetrics: populated, emptyMetrics: empty }
  }, [deviceCurrent])

  const filteredMetrics = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return populatedMetrics
    return populatedMetrics.filter(
      (m) => m.label.toLowerCase().includes(q) || m.key.toLowerCase().includes(q)
    )
  }, [populatedMetrics, query])

  const deviceForBadge = {
    online: deviceCurrent?.device?.online ?? device.online,
    transport_connected: device.transport_connected,
    last_seen: deviceCurrent?.device?.last_seen ?? device.last_seen,
  }
  const deviceLastSeen = formatLastSeen(
    deviceCurrent?.device?.last_seen ?? device.last_seen ?? snapshotLastSeen
  )
  const deviceAdapter =
    deviceCurrent?.device?.adapter_type ||
    device.adapter_type ||
    device.adapter_id ||
    "—"
  const activeTelemetryCount = imageMetrics.length + populatedMetrics.length
  const showTabs = commands.length > 0

  return (
    <div className="space-y-4">
      {/* Hero Summary */}
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <DeviceStatusBadge device={deviceForBadge} />
          {entityId && <CopyIdPill id={entityId} isZh={isZh} />}
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2.5 px-4 py-3 sm:grid-cols-3">
          <InfoCell label={isZh ? "设备类型" : "Type"} value={device.device_type || "—"} />
          <InfoCell label={isZh ? "接入方式" : "Adapter"} value={deviceAdapter} />
          <InfoCell
            label={isZh ? "最近上报" : "Last Seen"}
            value={deviceLastSeen || (isZh ? "尚未上报" : "Never")}
          />
        </div>
      </div>

      {showTabs && (
        <SegmentedTabs
          value={tab}
          onChange={setTab}
          items={[
            {
              id: "telemetry",
              label: isZh ? "实时数据" : "Telemetry",
              count: activeTelemetryCount,
            },
            {
              id: "commands",
              label: isZh ? "支持命令" : "Commands",
              count: commands.length,
            },
          ]}
        />
      )}

      {(!showTabs || tab === "telemetry") && (
        <div className="space-y-3">
          {imageMetrics.length > 0 && (
            <div className="space-y-2">
              {imageMetrics.slice(0, 2).map((img) => (
                <div key={img.key} className="overflow-hidden rounded-xl border border-border bg-card">
                  <ZoomableImage
                    src={img.src}
                    alt={img.label}
                    className="my-0 block w-full rounded-none border-0"
                    imgClassName="block max-h-56 w-full object-cover bg-muted-20"
                  />
                  <div
                    className={cn(
                      textMini,
                      "flex items-center justify-between border-t border-border px-3.5 py-2 text-muted-foreground"
                    )}
                  >
                    <span className="truncate font-medium text-foreground">{img.label}</span>
                    <span className="shrink-0 font-mono">{img.key}</span>
                  </div>
                </div>
              ))}
            </div>
          )}

          {populatedMetrics.length > 6 && (
            <div className="relative">
              <Search
                className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={isZh ? "筛选指标名称或键名..." : "Filter metrics..."}
                className="h-8 w-full rounded-lg border border-border bg-card pl-8 pr-3 text-xs text-foreground placeholder:text-muted-foreground focus:border-primary focus:outline-none"
              />
            </div>
          )}

          {filteredMetrics.length > 0 ? (
            <div className="grid grid-cols-2 gap-2.5">
              {filteredMetrics.map((m) => (
                <div
                  key={m.key}
                  className="min-w-0 rounded-xl border border-border bg-card px-3.5 py-2.5"
                >
                  <div className="flex items-center justify-between gap-1.5">
                    <span className={cn(textMini, "truncate text-muted-foreground")} title={m.key}>
                      {m.label}
                    </span>
                    {m.isVirtual && (
                      <span
                        className={cn(
                          textNano,
                          "shrink-0 rounded border border-border bg-muted-20 px-1.5 py-0.5 font-medium text-muted-foreground"
                        )}
                      >
                        {isZh ? "虚拟" : "Virtual"}
                      </span>
                    )}
                  </div>
                  <div className="mt-1 flex min-w-0 items-baseline gap-1">
                    <span
                      className="truncate text-heading font-semibold tabular-nums text-foreground"
                      title={m.unit ? `${m.value} ${m.unit}` : m.value}
                    >
                      {m.value}
                    </span>
                    {m.unit && (
                      <span className={cn(textMini, "shrink-0 text-muted-foreground")}>
                        {m.unit}
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          ) : populatedMetrics.length > 0 ? (
            <div className="rounded-xl border border-border bg-muted-20 px-4 py-5 text-center text-xs text-muted-foreground">
              {isZh ? "没有匹配的指标" : "No matching metrics"}
            </div>
          ) : imageMetrics.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-border bg-muted-20 px-4 py-6 text-center">
              <Activity className="h-5 w-5 text-muted-foreground opacity-60" aria-hidden="true" />
              <p className="text-sm font-medium text-foreground">
                {isZh ? "暂无实时上报数据" : "No live telemetry yet"}
              </p>
              <p className={cn(textMini, "text-muted-foreground")}>
                {isZh
                  ? "设备上线并上报数据后将在此显示最新读数"
                  : "Latest readings appear here once the device reports"}
              </p>
            </div>
          ) : null}

          {emptyMetrics.length > 0 && (
            <div className="pt-0.5">
              <button
                type="button"
                onClick={() => setShowEmptyMetrics((v) => !v)}
                className="inline-flex h-6 min-h-0 min-w-0 items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
              >
                <ChevronDown
                  className={cn("h-3.5 w-3.5 transition-transform", showEmptyMetrics && "rotate-180")}
                  aria-hidden="true"
                />
                {isZh
                  ? `${showEmptyMetrics ? "收起" : "查看"} ${emptyMetrics.length} 项未上报指标`
                  : `${showEmptyMetrics ? "Hide" : "Show"} ${emptyMetrics.length} metrics without data`}
              </button>
              {showEmptyMetrics && (
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {emptyMetrics.map((m) => (
                    <span
                      key={m.key}
                      title={m.key}
                      className={cn(textMini, "rounded-md bg-muted-30 px-2 py-1 text-muted-foreground")}
                    >
                      {m.label}
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {showTabs && tab === "commands" && (
        <div className="space-y-2.5">
          {commands.map((cmd) => {
            const params = cmd.parameters ?? []
            return (
              <div
                key={cmd.name}
                className="space-y-2 rounded-xl border border-border bg-card p-3.5"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <Terminal className="h-3.5 w-3.5 shrink-0 text-muted-foreground" strokeWidth={1.75} aria-hidden="true" />
                    <span className="truncate text-sm font-medium text-foreground">
                      {cmd.display_name || cmd.name}
                    </span>
                  </div>
                  <code className="shrink-0 rounded bg-muted-30 px-1.5 py-0.5 font-mono text-xs text-muted-foreground">
                    {cmd.name}
                  </code>
                </div>
                {cmd.llm_hints && (
                  <p className={cn(textMini, "leading-relaxed text-muted-foreground")}>
                    {cmd.llm_hints}
                  </p>
                )}
                <div className="flex items-center justify-between gap-2 pt-0.5">
                  <div className="flex flex-wrap gap-1">
                    {params.length > 0 ? (
                      params.map((p) => (
                        <span
                          key={p.name}
                          className={cn(
                            textNano,
                            "rounded border border-border bg-muted-20 px-1.5 py-0.5 font-mono text-muted-foreground"
                          )}
                        >
                          {p.name}: {formatParamType(p.data_type)}
                        </span>
                      ))
                    ) : (
                      <span className={cn(textMini, "text-muted-foreground")}>
                        {isZh ? "无参数" : "No parameters"}
                      </span>
                    )}
                  </div>
                  {onAttach && (
                    <button
                      type="button"
                      onClick={onAttach}
                      className="shrink-0 text-xs font-medium text-primary hover:underline"
                    >
                      {isZh ? "引用到聊天 →" : "Mention →"}
                    </button>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
