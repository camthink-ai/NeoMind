import { useEffect, useMemo, useRef, useState } from "react"
import { LayoutDashboard } from "lucide-react"
import type { DashboardResponse } from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { Badge } from "@/components/ui/badge"
import { ErrorBoundary } from "@/components/shared/ErrorBoundary"
import { DashboardGrid } from "@/components/dashboard/DashboardGrid"
import { renderDashboardComponent } from "@/pages/dashboard-components/Renderers"
import { fromDashboardDTO, type DashboardDTO } from "@/store/persistence/types"
import { useIsMobile } from "@/hooks/useMobile"
import { useStore } from "@/store"
import { CopyIdPill, InfoCell, SegmentedTabs, formatLastSeen } from "./shared"

const DESKTOP_PREVIEW_CANVAS_WIDTH = 1280

interface DashboardInspectorViewProps {
  dashboard: DashboardResponse
  entityId?: string
  isZh: boolean
  onAttach?: () => void
}

const WIDGET_TYPE_LABELS: Record<string, { zh: string; en: string }> = {
  "value-card": { zh: "指标卡", en: "KPI Card" },
  "line-chart": { zh: "折线图", en: "Line Chart" },
  "area-chart": { zh: "面积图", en: "Area Chart" },
  "bar-chart": { zh: "柱状图", en: "Bar Chart" },
  "pie-chart": { zh: "饼图", en: "Pie Chart" },
  gauge: { zh: "仪表盘", en: "Gauge" },
  "led-indicator": { zh: "状态灯", en: "LED Status" },
  sparkline: { zh: "迷你趋势", en: "Sparkline" },
  "progress-bar": { zh: "进度条", en: "Progress" },
  "image-display": { zh: "实时画面", en: "Image" },
  "image-history": { zh: "历史图片", en: "Image History" },
  "video-display": { zh: "视频流", en: "Video" },
  "markdown-display": { zh: "文档卡片", en: "Markdown" },
  "web-display": { zh: "网页嵌入", en: "Web View" },
  "agent-monitor-card": { zh: "代理监控", en: "Agent Monitor" },
  "agent-status-card": { zh: "代理状态", en: "Agent Status" },
  "toggle-switch": { zh: "开关控制", en: "Toggle" },
  "map-display": { zh: "地图", en: "Map" },
  "custom-layer": { zh: "自定义图层", en: "Custom Layer" },
}

function getWidgetTypeLabel(type: string, isZh: boolean): string {
  const entry = WIDGET_TYPE_LABELS[type]
  if (entry) return isZh ? entry.zh : entry.en
  return type
}

function summarizeWidgetDataSource(
  comp: DashboardResponse["components"][number],
  isZh: boolean
): { text: string; unbound: boolean } {
  const staticTypes = new Set(["markdown-display", "web-display", "custom-layer"])
  const raw =
    comp.data_source ?? (comp as unknown as { dataSource?: unknown }).dataSource
  const list = Array.isArray(raw) ? raw : raw ? [raw] : []
  const valid = list.filter(
    (d): d is Record<string, unknown> => !!d && typeof d === "object"
  )
  if (valid.length === 0) {
    if (staticTypes.has(comp.type)) {
      return { text: isZh ? "静态 / 展示内容" : "Static content", unbound: false }
    }
    return { text: isZh ? "未绑定数据源" : "No data source bound", unbound: true }
  }
  const first = valid[0]
  const srcType = typeof first.type === "string" ? first.type : ""
  const target =
    (first.sourceId as string | undefined) ||
    (first.extensionId as string | undefined) ||
    (first.agentId as string | undefined) ||
    (first.transformId as string | undefined) ||
    (first.systemMetric as string | undefined) ||
    ""
  const field =
    (first.property as string | undefined) ||
    (first.metricId as string | undefined) ||
    (first.extensionMetric as string | undefined) ||
    (first.command as string | undefined) ||
    (first.infoProperty as string | undefined) ||
    ""
  const parts = [srcType, target, field].filter(Boolean)
  const base = parts.length > 0 ? parts.join(" · ") : isZh ? "已绑定数据源" : "Bound"
  return {
    text: valid.length > 1 ? `${base} (+${valid.length - 1})` : base,
    unbound: false,
  }
}

export function DashboardInspectorView({
  dashboard,
  entityId,
  isZh,
  onAttach,
}: DashboardInspectorViewProps) {
  const isMobile = useIsMobile()
  const [tab, setTab] = useState<"preview" | "list">("preview")
  const [onlyUnbound, setOnlyUnbound] = useState(false)

  const previewViewportRef = useRef<HTMLDivElement>(null)
  const previewCanvasRef = useRef<HTMLDivElement>(null)
  const [viewportWidth, setViewportWidth] = useState(380)
  const [canvasHeight, setCanvasHeight] = useState(720)

  const fetchDevices = useStore((s) => s.fetchDevices)
  const fetchDevicesCurrentBatch = useStore((s) => s.fetchDevicesCurrentBatch)

  useEffect(() => {
    if (isMobile || tab !== "preview") return
    const vp = previewViewportRef.current
    const cv = previewCanvasRef.current
    if (!vp) return

    const sync = () => {
      if (vp.clientWidth > 0) setViewportWidth(vp.clientWidth)
      if (cv && cv.scrollHeight > 0) setCanvasHeight(cv.scrollHeight)
    }
    sync()

    const ro = new ResizeObserver(sync)
    ro.observe(vp)
    if (cv) ro.observe(cv)
    return () => ro.disconnect()
  }, [isMobile, tab, dashboard.id, dashboard.components.length])

  // Ensure device metadata & latest telemetry are hydrated so live widgets render values immediately.
  useEffect(() => {
    let cancelled = false
    const hydrate = async () => {
      try {
        if (useStore.getState().devices.length === 0) {
          await fetchDevices()
        }
        if (cancelled) return
        const devices = useStore.getState().devices
        if (devices.length > 0) {
          await fetchDevicesCurrentBatch(devices.map((d) => d.id))
        }
      } catch {
        // Non-blocking best-effort hydration for preview widgets
      }
    }
    void hydrate()
    return () => {
      cancelled = true
    }
  }, [dashboard.id, fetchDevices, fetchDevicesCurrentBatch])

  const gridCols = Math.max(1, dashboard.layout?.columns ?? 12)

  const items = useMemo(
    () =>
      dashboard.components.map((comp) => ({
        comp,
        ds: summarizeWidgetDataSource(comp, isZh),
        typeLabel: getWidgetTypeLabel(comp.type, isZh),
        w: comp.position?.w ?? 4,
        h: comp.position?.h ?? 2,
      })),
    [dashboard.components, isZh]
  )

  const parsedDashboard = useMemo(() => {
    try {
      return fromDashboardDTO(dashboard as unknown as DashboardDTO)
    } catch {
      return null
    }
  }, [dashboard])

  const gridComponents = useMemo(() => {
    if (!parsedDashboard) return []
    return parsedDashboard.components.map((comp) => ({
      id: comp.id,
      position: comp.position,
      children: (
        <ErrorBoundary>
          <div className="relative flex h-full w-full flex-col overflow-hidden">
            {renderDashboardComponent(comp, false)}
          </div>
        </ErrorBoundary>
      ),
    }))
  }, [parsedDashboard])

  const unboundCount = items.filter((it) => it.ds.unbound).length
  const boundCount = items.length - unboundCount
  const listItems = onlyUnbound ? items.filter((it) => it.ds.unbound) : items

  return (
    <div className="space-y-4">
      {/* Hero Summary */}
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-2.5">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="inline-flex items-center rounded-md border border-border bg-muted-20 px-2 py-0.5 text-xs font-medium tabular-nums text-foreground">
              {items.length > 0
                ? isZh
                  ? `${boundCount}/${items.length} 已绑定`
                  : `${boundCount}/${items.length} bound`
                : isZh
                  ? "0 个组件"
                  : "0 widgets"}
            </span>
            {unboundCount > 0 && (
              <span className="inline-flex items-center rounded-md border border-warning bg-card px-2 py-0.5 text-xs font-medium tabular-nums text-warning">
                {isZh ? `${unboundCount} 未绑定` : `${unboundCount} unbound`}
              </span>
            )}
            {dashboard.is_default && (
              <Badge variant="secondary" className={textNano}>
                {isZh ? "默认" : "Default"}
              </Badge>
            )}
          </div>
          {entityId && <CopyIdPill id={entityId} isZh={isZh} />}
        </div>
        <div className="grid grid-cols-3 gap-3 px-4 py-3">
          <InfoCell
            label={isZh ? "网格布局" : "Grid"}
            value={`${gridCols} ${isZh ? "列" : "cols"}`}
          />
          <InfoCell label={isZh ? "组件总数" : "Widgets"} value={String(items.length)} />
          <InfoCell
            label={isZh ? "最近更新" : "Updated"}
            value={formatLastSeen(dashboard.updated_at) || "—"}
          />
        </div>
        {dashboard.description && (
          <div className="border-t border-border px-4 py-2.5 text-xs leading-relaxed text-muted-foreground">
            {dashboard.description}
          </div>
        )}
      </div>

      {items.length === 0 ? (
        <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-border bg-muted-20 px-4 py-6 text-center">
          <LayoutDashboard className="h-5 w-5 text-muted-foreground opacity-60" aria-hidden="true" />
          <p className="text-sm font-medium text-foreground">
            {isZh ? "暂无组件" : "No widgets yet"}
          </p>
          <p className={cn(textMini, "text-muted-foreground")}>
            {isZh
              ? "可以引用该仪表盘让 AI 继续添加图表或指标卡"
              : "Mention this dashboard to ask AI to add charts or KPI cards"}
          </p>
        </div>
      ) : (
        <>
          <SegmentedTabs
            value={tab}
            onChange={setTab}
            items={[
              { id: "preview", label: isZh ? "实时预览" : "Live Preview" },
              {
                id: "list",
                label: isZh ? "组件清单" : "Widgets",
                count: items.length,
              },
            ]}
          />

          {tab === "preview" ? (
            isMobile ? (
              <div className="rounded-xl border border-border bg-muted-20 p-2">
                <DashboardGrid
                  components={gridComponents}
                  editMode={false}
                  rowHeight={56}
                  margin={[8, 8]}
                  containerPadding={[0, 0]}
                />
              </div>
            ) : (() => {
              const scale = Math.min(1, Math.max(0.2, viewportWidth / DESKTOP_PREVIEW_CANVAS_WIDTH))
              const scaledHeight = Math.ceil(canvasHeight * scale)
              return (
                <div
                  ref={previewViewportRef}
                  className="h-[340px] w-full overflow-y-auto overflow-x-hidden rounded-xl border border-border bg-muted-20"
                >
                  <div className="relative w-full" style={{ height: scaledHeight }}>
                    <div
                      ref={previewCanvasRef}
                      style={{
                        width: DESKTOP_PREVIEW_CANVAS_WIDTH,
                        transform: `scale(${scale})`,
                        transformOrigin: "top left",
                      }}
                    >
                      <DashboardGrid
                        components={gridComponents}
                        editMode={false}
                        cols={{ lg: gridCols, md: gridCols, sm: gridCols, xs: gridCols }}
                      />
                    </div>
                  </div>
                </div>
              )
            })()
          ) : (
            <div className="space-y-2.5">
              {unboundCount > 0 && (
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted-foreground">
                    {isZh ? `显示 ${listItems.length} 个组件` : `Showing ${listItems.length}`}
                  </span>
                  <button
                    type="button"
                    onClick={() => setOnlyUnbound((v) => !v)}
                    className={cn(
                      "inline-flex h-6 min-h-0 items-center rounded-md border px-2 text-xs transition-colors",
                      onlyUnbound
                        ? "border-primary bg-primary text-primary-foreground"
                        : "border-border bg-card text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {onlyUnbound
                      ? isZh
                        ? "显示全部"
                        : "Show all"
                      : isZh
                        ? `仅看未绑定 (${unboundCount})`
                        : `Unbound only (${unboundCount})`}
                  </button>
                </div>
              )}

              <div className="space-y-2">
                {listItems.map(({ comp, ds, typeLabel, w, h }) => (
                  <div
                    key={comp.id}
                    className={cn(
                      "space-y-1 rounded-xl border bg-card px-3.5 py-2.5",
                      ds.unbound ? "border-warning" : "border-border"
                    )}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span
                        className="truncate text-sm font-medium text-foreground"
                        title={comp.title || comp.id}
                      >
                        {comp.title || typeLabel}
                      </span>
                      <div className="flex shrink-0 items-center gap-1.5">
                        <span
                          className={cn(
                            textNano,
                            "rounded bg-muted-30 px-1.5 py-0.5 text-muted-foreground"
                          )}
                        >
                          {typeLabel}
                        </span>
                        <span className={cn(textNano, "font-mono tabular-nums text-muted-foreground")}>
                          {w}×{h}
                        </span>
                      </div>
                    </div>
                    <div className="flex items-center justify-between gap-2">
                      <span
                        className={cn(
                          textMini,
                          "truncate font-mono",
                          ds.unbound ? "text-warning" : "text-muted-foreground"
                        )}
                        title={ds.text}
                      >
                        {ds.text}
                      </span>
                      {ds.unbound && onAttach && (
                        <button
                          type="button"
                          onClick={onAttach}
                          className="shrink-0 text-xs font-medium text-primary hover:underline"
                        >
                          {isZh ? "让 AI 补绑" : "Bind"}
                        </button>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}
