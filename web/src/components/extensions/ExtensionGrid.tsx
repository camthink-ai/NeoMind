import { useMemo } from "react"
import { useTranslation } from "react-i18next"
import { ExtensionCard } from "./ExtensionCard"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/shared/EmptyState"
import {
  Search,
  X,
  Brain,
  Wrench,
  Plug,
  Camera,
  Eye,
  Factory,
  Home,
  MonitorPlay,
  ScanText,
  UserCheck,
  Wifi,
  CloudSun,
} from "lucide-react"
import type { Extension } from "@/types"

// Extension showcase data for the marquee (matches real NeoMind-Extensions)
interface ShowcaseExtension {
  name: string
  desc: string
  icon: React.ReactNode
  category: string
}

const showcaseExtensions: ShowcaseExtension[] = [
  { name: "YOLO Video", desc: "Real-time video object detection with ROI analytics", icon: <Eye className="h-5 w-5" />, category: "AI Vision" },
  { name: "Face Recognition", desc: "ArcFace embeddings with face gallery and identity matching", icon: <UserCheck className="h-5 w-5" />, category: "AI Vision" },
  { name: "BACnet Bridge", desc: "Discover building automation devices and read sensors", icon: <Factory className="h-5 w-5" />, category: "Industrial" },
  { name: "Modbus Bridge", desc: "Connect PLCs, power meters, and industrial devices", icon: <Factory className="h-5 w-5" />, category: "Industrial" },
  { name: "LoRaWAN Bridge", desc: "Connect ChirpStack/TTN sensors with auto-discovery", icon: <Wifi className="h-5 w-5" />, category: "IoT" },
  { name: "ONVIF Bridge", desc: "Discover IP cameras, get RTSP streams, PTZ control", icon: <Camera className="h-5 w-5" />, category: "Camera" },
  { name: "Home Assistant", desc: "Import 3000+ HA entity integrations as devices", icon: <Home className="h-5 w-5" />, category: "Smart Home" },
  { name: "Stream Player", desc: "RTSP/RTMP/HLS video playback with FFmpeg transcoding", icon: <MonitorPlay className="h-5 w-5" />, category: "Media" },
  { name: "Weather Forecast", desc: "Real-time multi-city weather with OpenWeatherMap", icon: <CloudSun className="h-5 w-5" />, category: "Data" },
  { name: "OCR Inference", desc: "Automatic OCR text recognition on device image streams", icon: <ScanText className="h-5 w-5" />, category: "AI Vision" },
  { name: "OPC-UA Bridge", desc: "Connect industrial servers, browse nodes, subscribe changes", icon: <Factory className="h-5 w-5" />, category: "Industrial" },
  { name: "Image Analyzer", desc: "Standalone image detection with ONNX/CoreML/CUDA acceleration", icon: <Brain className="h-5 w-5" />, category: "AI Vision" },
]

interface ExtensionGridProps {
  extensions: Extension[]
  loading?: boolean
  onDetails?: (id: string) => void
  onUninstall?: (id: string) => void
  onReload?: (id: string) => Promise<boolean>
  /** Controlled search text — lifted to the page's toolbar row. */
  searchQuery: string
  onSearchChange: (value: string) => void
}

// Status filter options with counts
export function ExtensionGrid({
  extensions,
  loading = false,
  onDetails,
  onUninstall,
  onReload,
  searchQuery,
  onSearchChange,
}: ExtensionGridProps) {
  const { t } = useTranslation(["extensions", "common"])

  // Search is owned by the page (lifted into the toolbar row, per the
  // no-tab standard: content controls left, actions right).

  // Clear filters
  const clearFilters = () => {
    onSearchChange("")
  }

  // Filter extensions by the page-owned search query. The status filter
  // pills (All/Active/Error/Stopped counts) were removed in 1.2.8: they
  // only echoed the totals and status filtering had no real use.
  const { filteredExtensions, stats } = useMemo(() => {
    const exts = extensions || []

    let filtered = exts

    // Search filter
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(
        (ext) =>
          ext.name.toLowerCase().includes(query) ||
          ext.id.toLowerCase().includes(query) ||
          ext.description?.toLowerCase().includes(query) ||
          ext.commands?.some((cmd) =>
            cmd.id.toLowerCase().includes(query) ||
            cmd.display_name.toLowerCase().includes(query)
          )
      )
    }

    return { filteredExtensions: filtered, stats: { total: exts.length } }
  }, [extensions, searchQuery])

  if (loading) {
    return (
      <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-2 md:gap-5">
        {Array.from({ length: 6 }).map((_, i) => (
          <div
            key={i}
            className="border rounded-xl p-5 space-y-4 animate-fade-in-up"
            style={{ animationDelay: `${i * 60}ms`, animationFillMode: 'both' }}
          >
            <div className="flex items-start gap-3">
              <Skeleton className="h-10 w-10 rounded-xl" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-5 w-3/4" />
                <Skeleton className="h-3 w-1/2" />
              </div>
            </div>
            <Skeleton className="h-16 w-full" />
          </div>
        ))}
      </div>
    )
  }

  if (!extensions || extensions.length === 0) {
    const categories = [
      { icon: <Factory className="h-4 w-4" />, label: t("empty.catIndustrial") },
      { icon: <Eye className="h-4 w-4" />, label: t("empty.catVision") },
      { icon: <Camera className="h-4 w-4" />, label: t("empty.catCamera") },
      { icon: <Wifi className="h-4 w-4" />, label: t("empty.catIot") },
      { icon: <Home className="h-4 w-4" />, label: t("empty.catSmartHome") },
      { icon: <MonitorPlay className="h-4 w-4" />, label: t("empty.catMedia") },
      { icon: <CloudSun className="h-4 w-4" />, label: t("empty.catData") },
      { icon: <Wrench className="h-4 w-4" />, label: t("empty.catTool") },
    ]

    const marqueeItems = [...showcaseExtensions, ...showcaseExtensions]

    return (
      <div className="flex flex-col items-center px-1 py-10 sm:px-4 sm:py-12">
        {/* Hero */}
        <div className="mb-5 hidden h-16 w-16 items-center justify-center rounded-xl bg-muted ring-1 ring-border sm:flex">
          <Plug className="h-8 w-8 text-muted-foreground" />
        </div>
        <h3 className="text-lg font-semibold sm:text-xl">{t("empty.title")}</h3>
        <p className="mt-2 max-w-md text-center text-sm leading-relaxed text-muted-foreground">
          {t("empty.description")}
        </p>

        <div className="mt-8 w-full space-y-1 text-sm sm:hidden">
          {showcaseExtensions.slice(0, 4).map((ext) => (
            <div key={ext.name} className="rounded-xl px-1 py-3">
              <div className="flex items-baseline justify-between gap-3">
                <span className="font-medium text-foreground">{ext.name}</span>
                <span className="shrink-0 text-xs text-muted-foreground">{ext.category}</span>
              </div>
              <p className="mt-1 line-clamp-1 text-xs text-muted-foreground">{ext.desc}</p>
            </div>
          ))}
        </div>

        {/* Marquee */}
        <div className="relative mt-8 hidden w-full max-w-3xl overflow-hidden sm:block">
          {/* Edge fades */}
          <div className="pointer-events-none absolute inset-y-0 left-0 z-10 w-16 bg-gradient-to-r from-background to-transparent" />
          <div className="pointer-events-none absolute inset-y-0 right-0 z-10 w-16 bg-gradient-to-l from-background to-transparent" />

          <div className="flex animate-marquee" style={{ "--marquee-duration": "50s" } as React.CSSProperties}>
            {marqueeItems.map((ext, i) => (
              <div
                key={`${ext.name}-${i}`}
                className="flex-shrink-0 w-64 mx-2 rounded-xl border bg-card backdrop-blur-sm p-3.5 hover:border-foreground/30 hover:bg-background transition-colors"
              >
                <div className="flex items-start gap-3">
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
                    {ext.icon}
                  </div>
                  <div className="min-w-0">
                    <p className="text-sm font-medium truncate">{ext.name}</p>
                    <p className="text-mini text-muted-foreground mt-0.5 line-clamp-2 leading-tight">
                      {ext.desc}
                    </p>
                  </div>
                </div>
                <div className="mt-2">
                  <span className="inline-flex items-center rounded-full bg-muted-50 px-2 py-0.5 text-nano font-medium text-muted-foreground">
                    {ext.category}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Category tags */}
        <div className="mt-8 hidden flex-wrap items-center justify-center gap-2 sm:flex">
          {categories.map((c) => (
            <div
              key={c.label}
              className="inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-xs font-medium text-muted-foreground bg-card"
            >
              <span className="shrink-0">{c.icon}</span>
              {c.label}
            </div>
          ))}
        </div>
      </div>
    )
  }

  // Empty state for filtered results
  if (filteredExtensions.length === 0) {
    return (
      <EmptyState
        icon={<Search className="h-12 w-12" />}
        title={t("noMatchingExtensions", { defaultValue: "No Matching Extensions" })}
        description={t("tryDifferentFilters", { defaultValue: "Try adjusting your search or filters" })}
        action={{
          label: t("clearFilters", { ns: "common" }),
          onClick: clearFilters,
          variant: 'outline',
          icon: <X className="h-4 w-4" />,
        }}
      />
    )
  }

  return (
    <div className="space-y-4">
      {/* Search-active row: clear control left, filtered count right.
          The status filter pills were removed (1.2.8) — they only echoed
          the totals and status filtering had no real use; search lives in
          the page toolbar row. */}
      {searchQuery && (
        <div className="flex items-center justify-between gap-3">
          <Button variant="ghost" size="xs" onClick={clearFilters} className="h-10 md:h-7">
            <X className="h-3 w-3 mr-1" />
            {t("clearFilters", { ns: "common" })}
          </Button>
          <span className="text-sm text-muted-foreground tabular-nums whitespace-nowrap">
            {filteredExtensions.length} / {stats.total}
          </span>
        </div>
      )}

      {/* Extension Cards Grid */}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-2 md:gap-5">
        {filteredExtensions.map((extension, index) => (
          <div
            key={extension.id}
            className="animate-fade-in-up"
            style={{ animationDelay: `${index * 50}ms`, animationFillMode: 'both' }}
          >
            <ExtensionCard
              extension={extension}
              onDetails={() => onDetails?.(extension.id)}
              onUninstall={() => onUninstall?.(extension.id)}
              onReload={() => onReload?.(extension.id)}
            />
          </div>
        ))}
      </div>
    </div>
  )
}
