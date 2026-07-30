import { useState, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { ExtensionCard } from "./ExtensionCard"
import { Skeleton } from "@/components/ui/skeleton"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { EmptyState } from "@/components/shared/EmptyState"
import {
  Search,
  X,
  Grid3x3,
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
import { cn } from "@/lib/utils"
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
}

// Status filter options with counts
interface StatusOption {
  value: string
  label: string
  icon: React.ReactNode
  className: string
}

export function ExtensionGrid({
  extensions,
  loading = false,
  onDetails,
  onUninstall,
  onReload,
}: ExtensionGridProps) {
  const { t } = useTranslation(["extensions", "common"])

  // Filter states
  const [searchQuery, setSearchQuery] = useState("")
  const [statusFilter, setStatusFilter] = useState<string>("all")

  // Clear filters
  const clearFilters = () => {
    setSearchQuery("")
    setStatusFilter("all")
  }

  // Filter extensions and compute stats
  const { filteredExtensions, stats, statusOptions, hasActiveFilters } = useMemo(() => {
    const exts = extensions || []

    // In V2 system, extensions are always active once registered
    // Error/Warning/Stopped indicate problems
    const activeCount = exts.filter((ext) => ext.state !== "Error" && ext.state !== "Warning" && ext.state !== "Stopped" && ext.state !== "Failed").length
    const errorCount = exts.filter((ext) => ext.state === "Error" || ext.state === "Warning").length
    const stoppedCount = exts.filter((ext) => ext.state === "Stopped" || ext.state === "Failed").length

    // Build status options (only show categories that have items)
    const options: StatusOption[] = [
      { value: "all", label: t("allStatuses"), icon: <Grid3x3 className="h-3 w-3" />, className: "text-muted-foreground" },
      { value: "active", label: t("categories.active", { defaultValue: "Active" }), icon: <div className="w-2 h-2 rounded-full bg-success" />, className: "text-success" },
      { value: "error", label: t("categories.error"), icon: <div className="w-2 h-2 rounded-full bg-error" />, className: "text-error" },
      { value: "stopped", label: t("categories.stopped"), icon: <div className="w-2 h-2 rounded-full bg-muted-foreground" />, className: "text-muted-foreground" },
    ].filter((opt) => opt.value === "all" || (opt.value === "active" && activeCount > 0) || (opt.value === "error" && errorCount > 0) || (opt.value === "stopped" && stoppedCount > 0))

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

    // Status filter
    if (statusFilter !== "all") {
      filtered = filtered.filter((ext) => {
        switch (statusFilter) {
          case "active":
            return ext.state !== "Error" && ext.state !== "Warning" && ext.state !== "Stopped" && ext.state !== "Failed"
          case "error":
            return ext.state === "Error" || ext.state === "Warning"
          case "stopped":
            return ext.state === "Stopped" || ext.state === "Failed"
          default:
            return true
        }
      })
    }

    const computedStats = {
      total: exts.length,
      active: activeCount,
      error: errorCount,
      stopped: stoppedCount,
    }

    return { filteredExtensions: filtered, stats: computedStats, statusOptions: options, hasActiveFilters: searchQuery || statusFilter !== "all" }
  }, [extensions, searchQuery, statusFilter, t])

  if (loading) {
    return (
      <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-5">
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
      <div className="flex flex-col items-center py-12 px-4">
        {/* Hero */}
        <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-muted ring-1 ring-border mb-5">
          <Plug className="h-8 w-8 text-muted-foreground" />
        </div>
        <h3 className="text-xl font-semibold">{t("empty.title")}</h3>
        <p className="mt-2 text-sm text-muted-foreground max-w-md text-center leading-relaxed">
          {t("empty.description")}
        </p>

        {/* Marquee */}
        <div className="mt-8 w-full max-w-3xl overflow-hidden relative">
          {/* Edge fades */}
          <div className="pointer-events-none absolute inset-y-0 left-0 z-10 w-16 bg-gradient-to-r from-background to-transparent" />
          <div className="pointer-events-none absolute inset-y-0 right-0 z-10 w-16 bg-gradient-to-l from-background to-transparent" />

          <div className="flex animate-marquee" style={{ "--marquee-duration": "50s" } as React.CSSProperties}>
            {marqueeItems.map((ext, i) => (
              <div
                key={`${ext.name}-${i}`}
                className="flex-shrink-0 w-56 mx-2 rounded-xl border bg-card backdrop-blur-sm p-3.5 hover:border-brand hover:bg-background transition-colors"
              >
                <div className="flex items-start gap-3">
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-brand-bg text-brand">
                    {ext.icon}
                  </div>
                  <div className="min-w-0">
                    <p className="text-sm font-medium truncate">{ext.name}</p>
                    <p className="text-[11px] text-muted-foreground mt-0.5 line-clamp-2 leading-tight">
                      {ext.desc}
                    </p>
                  </div>
                </div>
                <div className="mt-2">
                  <span className="inline-flex items-center rounded-full bg-muted-50 px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
                    {ext.category}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Category tags */}
        <div className="mt-8 flex flex-wrap items-center justify-center gap-2">
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
      {/* Search Bar */}
      <div className="relative">
        <Search className="absolute left-3.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          placeholder={t("searchPlaceholder", { defaultValue: "Search extensions by name, ID, or description..." })}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="pl-10 h-10 bg-card"
        />
        {searchQuery && (
          <button
            onClick={() => setSearchQuery("")}
            aria-label={t('common:clear')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        )}
      </div>

      {/* Results summary + Status filters on one row */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        {/* Left: results count */}
        <div className="flex items-center gap-3 text-sm min-w-0">
          <span className="text-muted-foreground whitespace-nowrap">
            {hasActiveFilters
              ? t("filteredResults", { count: filteredExtensions.length, total: stats.total }).replace("{{count}}", String(filteredExtensions.length)).replace("{{total}}", String(stats.total))
              : t("showingResults", { count: filteredExtensions.length, defaultValue: "Showing {{count}} extensions" }).replace("{{count}}", String(filteredExtensions.length))}
          </span>
          {!hasActiveFilters && stats.active > 0 && (
            <span className="flex items-center gap-1 text-success whitespace-nowrap">
              <div className="w-2 h-2 rounded-full bg-success" />
              {stats.active} {t("active", { defaultValue: "active" })}
            </span>
          )}
        </div>

        {/* Right: status filter pills */}
        <div className="flex items-center gap-1.5 flex-wrap">
          {statusOptions.map((option) => {
            const isSelected = statusFilter === option.value
            const count = option.value === "all" ? stats.total :
                         option.value === "active" ? stats.active :
                         option.value === "error" ? stats.error : stats.stopped

            return (
              <button
                key={option.value}
                onClick={() => setStatusFilter(option.value)}
                className={cn(
                  "inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-xs font-medium transition-all border",
                  isSelected
                    ? "bg-primary text-primary-foreground border-primary shadow-sm"
                    : "bg-card hover:bg-muted-50 text-muted-foreground border-border hover:border-input"
                )}
              >
                {option.icon}
                <span>{option.label}</span>
                <span className={cn(
                  "inline-flex items-center justify-center min-w-[18px] h-[16px] px-1 rounded text-[10px] font-semibold tabular-nums",
                  isSelected
                    ? "bg-white/20 text-primary-foreground"
                    : "bg-muted text-muted-foreground"
                )}>
                  {count}
                </span>
              </button>
            )
          })}
          {hasActiveFilters && (
            <Button variant="ghost" size="xs" onClick={clearFilters} className="h-7">
              <X className="h-3 w-3 mr-1" />
              {t("clearFilters", { ns: "common" })}
            </Button>
          )}
        </div>
      </div>

      {/* Extension Cards Grid */}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-5">
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
