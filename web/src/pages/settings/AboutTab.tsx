import { useState, useEffect, useCallback, type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { BrandName } from "@/components/shared/BrandName"
import type { LucideIcon } from "lucide-react"
import {
  Server,
  Clock,
  Cpu,
  HardDrive,
  Layers,
  Monitor,
  Download,
  Upload,
  Wifi,
  Loader2,
  ExternalLink,
  ArrowUpCircle,
  CheckCircle2,
  RefreshCw,
} from "lucide-react"
import { api, isTauriEnv } from "@/lib/api"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useUpdateCheck } from "@/hooks/useUpdateCheck"
import { useAppStore } from "@/store"

interface GpuInfo {
  name: string
  vendor: string
  total_memory_mb: number | null
  driver_version: string | null
}

interface SystemInfo {
  version: string
  uptime: number
  platform: string
  arch: string
  cpu_count: number
  total_memory: number
  used_memory: number
  free_memory: number
  available_memory: number
  cpu_usage: number
  gpus: GpuInfo[]
  disks: DiskInfo[]
  networks: NetInfo[]
}

interface DiskInfo {
  name: string
  mount: string
  total: number
  used: number
  available: number
}

interface NetInfo {
  name: string
  ip: string
  mac: string
  rx_bytes: number
  tx_bytes: number
}

/* ============================================================================
 * Sub-components
 * ========================================================================== */

function MetricTile({
  icon: Icon,
  label,
  value,
  sub,
  mono,
}: {
  icon: LucideIcon
  label: string
  value: string
  sub?: string
  mono?: boolean
}) {
  return (
    <div className="rounded-lg border bg-muted-30 p-4 space-y-2 transition-colors hover:bg-muted-50">
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <Icon className="h-3.5 w-3.5" />
        <span className="uppercase tracking-wide truncate">{label}</span>
      </div>
      <div
        className={`text-2xl font-semibold leading-none truncate ${mono ? "font-mono" : ""}`}
      >
        {value}
      </div>
      {/* Unified secondary line: always rendered (preserves vertical rhythm across tiles) */}
      <div className="text-xs text-muted-foreground font-mono uppercase tracking-wide truncate min-h-[1rem]">
        {sub ?? " "}
      </div>
    </div>
  )
}

function UsageGauge({
  icon: Icon,
  label,
  pct,
  sub,
  rightLabel,
  rightValue,
  footer,
}: {
  icon: LucideIcon
  label: string
  pct: number
  sub?: string
  rightLabel?: string
  rightValue?: string
  footer?: string
}) {
  const clamped = Math.max(0, Math.min(100, Math.round(pct)))
  const barColor = clamped >= 80 ? "bg-error" : clamped >= 60 ? "bg-info" : "bg-success"
  const textColor = clamped >= 80 ? "text-error" : clamped >= 60 ? "text-info" : "text-success"

  return (
    <div className="rounded-lg border bg-muted-30 p-4 space-y-3">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Icon className="h-3.5 w-3.5" />
            <span className="uppercase tracking-wide">{label}</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="font-mono text-2xl font-semibold leading-none">
              {clamped}
              <span className="text-lg text-muted-foreground">%</span>
            </span>
            {sub && <span className={`text-xs font-mono ${textColor}`}>{sub}</span>}
          </div>
        </div>
        {rightLabel && (
          <div className="text-right">
            <div className="text-xs text-muted-foreground uppercase tracking-wide">
              {rightLabel}
            </div>
            {rightValue && <div className="font-mono text-sm font-medium">{rightValue}</div>}
          </div>
        )}
      </div>
      {/* Segmented gauge with tick marks */}
      <div className="relative h-2.5 w-full rounded-full bg-muted overflow-hidden">
        <div
          className={`h-full ${barColor} rounded-full transition-all duration-700 ease-out`}
          style={{ width: `${clamped}%` }}
        />
        {[25, 50, 75].map((p) => (
          <div
            key={p}
            className="absolute top-0 bottom-0 w-px bg-border"
            style={{ left: `${p}%` }}
          />
        ))}
      </div>
      {footer && (
        <div className="flex justify-between text-nano font-mono uppercase tracking-wide text-muted-foreground">
          <span>{footer}</span>
        </div>
      )}
    </div>
  )
}

function InfoRow({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <span className="text-sm text-muted-foreground">{label}</span>
      <div className="text-sm text-right">{children}</div>
    </div>
  )
}

function ExternalLinkValue({ href, text }: { href: string; text: string }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex items-center gap-1 font-mono text-info hover:underline"
    >
      <span>{text}</span>
      <ExternalLink className="h-3 w-3 text-muted-foreground" />
    </a>
  )
}

function TelemetrySkeleton() {
  return (
    <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
      {Array.from({ length: 4 }).map((_, i) => (
        <div key={i} className="rounded-lg border bg-muted-30 p-4 space-y-2.5">
          <Skeleton className="h-3 w-16" />
          <Skeleton className="h-7 w-20" />
          <Skeleton className="h-3 w-12" />
        </div>
      ))}
    </div>
  )
}

/* ============================================================================
 * Main component
 * ========================================================================== */

export function AboutTab() {
  const { t } = useTranslation(["common", "settings"])
  const { handleError, showSuccess } = useErrorHandler()
  const { updateInfo, setUpdateDialogOpen } = useAppStore()
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null)
  const [appVersion, setAppVersion] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [checkingUpdate, setCheckingUpdate] = useState(false)

  const handleUpToDate = useCallback(() => {
    showSuccess(t("settings:alreadyUpToDate"))
  }, [showSuccess, t])

  const { checkUpdate, getAppVersion } = useUpdateCheck({
    autoCheck: false,
    onUpToDate: handleUpToDate,
  })

  const loadSystemInfo = async () => {
    try {
      const response = await api.getSystemStats()
      setSystemInfo(response)
    } catch (e) {
      handleError(e, { operation: "Load system info", showToast: false })
      if (isTauriEnv() && !appVersion) {
        try {
          const v = await getAppVersion()
          setAppVersion(v)
        } catch {
          /* ignore */
        }
      }
    } finally {
      setLoading(false)
    }
  }

  const handleCheckForUpdates = async () => {
    setCheckingUpdate(true)
    try {
      // checkUpdate() handles both outcomes itself: on "available" it opens
      // the update dialog, on "up to date" it fires onUpToDate (→ showSuccess).
      // Re-checking + toasting here fired the "already up to date" toast a
      // second time, so only surface errors here.
      await checkUpdate()
    } catch (error) {
      console.error("[AboutTab] checkUpdate error:", error)
      handleError(error, { operation: "Check for updates" })
    } finally {
      setCheckingUpdate(false)
    }
  }

  useEffect(() => {
    loadSystemInfo()
    // Resources (CPU/memory/disk) drift over time — refresh every 5s, aligned
    // with the backend's 5s stats cache. Cleared on unmount.
    const id = setInterval(loadSystemInfo, 5000)
    return () => clearInterval(id)
  }, [])

  const formatBytes = (bytes: number) => {
    const gb = bytes / (1024 * 1024 * 1024)
    return gb.toFixed(2) + " GB"
  }

  const formatUptimeParts = (
    seconds: number
  ): { primary: string; secondary: string } => {
    const days = Math.floor(seconds / 86400)
    const hours = Math.floor((seconds % 86400) / 3600)
    const minutes = Math.floor((seconds % 3600) / 60)
    if (days > 0) return { primary: `${days}d`, secondary: `${hours}h ${minutes}m` }
    if (hours > 0) return { primary: `${hours}h`, secondary: `${minutes}m` }
    return { primary: `${minutes}m`, secondary: t("common:runStatus.running") }
  }

  const versionTag = systemInfo?.version || (appVersion ? `v${appVersion}` : "")

  const heroVersion = versionTag || "---"

  const updateTitle = checkingUpdate
    ? t("settings:checkingForUpdates")
    : updateInfo?.available
      ? `${t("settings:updateNow")} · v${updateInfo.version}`
      : updateInfo
        ? t("settings:alreadyUpToDate")
        : t("settings:checkForUpdates")

  return (
    <div className="space-y-8">
      {/* Hero — brand wordmark + build (borderless) */}
      <div className="flex flex-col gap-6 md:flex-row md:items-end md:justify-between">
        <div className="space-y-3 min-w-0">
          <h1 className="text-4xl md:text-5xl font-bold tracking-tight leading-none">
            <BrandName />
          </h1>
          <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-muted-foreground font-mono">
            <span>{t("settings:aboutDesc")}</span>
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full rounded-full bg-success opacity-75 animate-ping" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-success" />
            </span>
          </div>
          <p className="text-sm text-muted-foreground max-w-md">
            {t("settings:aboutDesc1")}
          </p>
        </div>

        <div className="flex flex-col items-start md:items-end gap-2 shrink-0">
          <div className="font-mono text-nano text-muted-foreground uppercase tracking-wide">
            build
          </div>
          <div className="flex items-center gap-2">
            <div className="font-mono text-2xl md:text-3xl font-semibold text-foreground leading-none">
              [{heroVersion}]
            </div>
            {isTauriEnv() && (
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={updateTitle}
                title={updateTitle}
                onClick={() =>
                  updateInfo?.available
                    ? setUpdateDialogOpen(true)
                    : handleCheckForUpdates()
                }
                disabled={checkingUpdate}
              >
                {checkingUpdate ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : updateInfo?.available ? (
                  <ArrowUpCircle className="h-4 w-4 text-info" />
                ) : updateInfo ? (
                  <CheckCircle2 className="h-4 w-4 text-success" />
                ) : (
                  <RefreshCw className="h-4 w-4 text-muted-foreground" />
                )}
              </Button>
            )}
          </div>
        </div>
      </div>

      {/* System Information */}
      <section>
        <h3 className="text-base font-semibold">
          {t("settings:systemInfo")}
        </h3>
        <div className="mt-4 space-y-4">
          {loading ? (
            <TelemetrySkeleton />
          ) : systemInfo ? (
            (() => {
              const uptime = formatUptimeParts(systemInfo.uptime)
              return (
            <>
              {/* Telemetry tiles */}
              <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                <MetricTile
                  icon={Server}
                  label={t("settings:platform")}
                  value={systemInfo.platform}
                  sub={systemInfo.arch}
                />
                <MetricTile
                  icon={Clock}
                  label={t("settings:uptime")}
                  value={uptime.primary}
                  sub={uptime.secondary}
                  mono
                />
                <MetricTile
                  icon={Cpu}
                  label={t("settings:cpuCores")}
                  value={String(systemInfo.cpu_count)}
                  sub={t("settings:cores")}
                  mono
                />
                {systemInfo.gpus.length > 0 ? (
                  <MetricTile
                    icon={Monitor}
                    label={t("settings:gpu")}
                    value={String(systemInfo.gpus.length)}
                    sub={systemInfo.gpus[0]?.vendor ?? "GPU"}
                    mono
                  />
                ) : (
                  <MetricTile
                    icon={Layers}
                    label={t("settings:memory")}
                    value={formatBytes(systemInfo.total_memory)}
                    sub="total"
                    mono
                  />
                )}
              </div>

              {/* Resource gauges — CPU / Memory / Disk (refresh every 5s) */}
              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                <UsageGauge
                  icon={Cpu}
                  label={t("settings:cpu", "CPU")}
                  pct={systemInfo.cpu_usage ?? 0}
                  sub={`${systemInfo.cpu_count} ${t("settings:cores")}`}
                />
                <UsageGauge
                  icon={HardDrive}
                  label={t("settings:memory")}
                  pct={
                    systemInfo.total_memory > 0
                      ? (systemInfo.used_memory / systemInfo.total_memory) * 100
                      : 0
                  }
                  sub={`${formatBytes(systemInfo.used_memory)} / ${formatBytes(systemInfo.total_memory)}`}
                  rightLabel={t("settings:availableMemory")}
                  rightValue={formatBytes(systemInfo.available_memory)}
                  footer={`${t("settings:usedMemory")}: ${formatBytes(systemInfo.used_memory)}`}
                />
                {(systemInfo.disks ?? []).map((disk, idx) => (
                  <UsageGauge
                    key={`disk-${idx}-${disk.mount}`}
                    icon={HardDrive}
                    label={disk.mount || disk.name || t("settings:disk", "Disk")}
                    pct={disk.total > 0 ? (disk.used / disk.total) * 100 : 0}
                    sub={`${formatBytes(disk.used)} / ${formatBytes(disk.total)}`}
                    rightLabel={t("settings:free", "free")}
                    rightValue={formatBytes(disk.available)}
                  />
                ))}
              </div>

              {/* Network interfaces */}
              {(systemInfo.networks ?? []).length > 0 && (
                <div className="rounded-lg border bg-muted-30 p-4">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-3">
                    <Wifi className="h-3.5 w-3.5" />
                    <span className="uppercase tracking-wide">{t("settings:network", "Network")}</span>
                  </div>
                  <div className="space-y-2">
                    {(systemInfo.networks ?? []).map((net, idx) => (
                      <div
                        key={`net-${idx}-${net.name}`}
                        className="flex flex-wrap items-center justify-between gap-2 text-xs"
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="font-medium">{net.name}</span>
                          {net.ip && (
                            <span className="font-mono text-xs text-muted-foreground">{net.ip}</span>
                          )}
                        </div>
                        <div className="flex items-center gap-3 font-mono text-xs">
                          <span className="inline-flex items-center gap-1 text-info">
                            <Download className="h-3 w-3" />
                            {formatBytes(net.rx_bytes)}
                          </span>
                          <span className="inline-flex items-center gap-1 text-success">
                            <Upload className="h-3 w-3" />
                            {formatBytes(net.tx_bytes)}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* GPU detail rows */}
              {systemInfo.gpus.length > 0 && (
                <div className="space-y-2">
                  {systemInfo.gpus.map((gpu, idx) => (
                    <div
                      key={idx}
                      className="rounded-lg border bg-muted-30 p-3 flex items-center justify-between gap-3"
                    >
                      <div className="flex items-center gap-3 min-w-0">
                        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-muted text-foreground">
                          <Monitor className="h-4 w-4" />
                        </div>
                        <div className="min-w-0">
                          <div className="text-sm font-medium truncate">{gpu.name}</div>
                          <div className="text-xs text-muted-foreground font-mono uppercase tracking-wide">
                            {gpu.vendor}
                          </div>
                        </div>
                      </div>
                      {gpu.total_memory_mb && (
                        <div className="text-right shrink-0">
                          <div className="font-mono text-base font-bold leading-none">
                            {(gpu.total_memory_mb / 1024).toFixed(1)}
                          </div>
                          <div className="text-nano text-muted-foreground uppercase tracking-wide mt-1">
                            GB VRAM
                          </div>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </>
              )
            })()
          ) : (
            <div className="text-center py-8 text-muted-foreground text-sm">
              {t("settings:systemInfoUnavailable")}
            </div>
          )}
        </div>
      </section>

      {/* Project Information */}
      <section>
        <h3 className="text-base font-semibold">
          {t("settings:projectInfo")}
        </h3>
        <div className="mt-4 rounded-xl bg-muted-30 p-5">
          <div>
            <InfoRow label={t("settings:version")}>
              <div className="flex items-center gap-2">
                <Badge variant="secondary" className="font-mono">
                  {versionTag || "---"}
                </Badge>
                {isTauriEnv() &&
                  updateInfo?.available &&
                  updateInfo.version !== systemInfo?.version && (
                    <Badge variant="default" className="text-xs gap-1">
                      <Download className="h-3 w-3" />
                      v{updateInfo.version} {t("settings:update")}
                    </Badge>
                  )}
              </div>
            </InfoRow>
            <InfoRow label={t("settings:license")}>
              <span className="font-mono">Apache-2.0</span>
            </InfoRow>
            <InfoRow label={t("settings:repository")}>
              <ExternalLinkValue
                href="https://github.com/camthink-ai/NeoMind"
                text="github.com/camthink-ai/NeoMind"
              />
            </InfoRow>
            <InfoRow label={t("settings:website")}>
              <ExternalLinkValue href="https://www.camthink.ai" text="www.camthink.ai" />
            </InfoRow>
            <InfoRow label={t("settings:documentation")}>
              <ExternalLinkValue
                href="https://wiki.camthink.ai/docs/neomind/product-overview/what-is-neomind"
                text="wiki.camthink.ai"
              />
            </InfoRow>
          </div>
        </div>
      </section>

      {/* Footer */}
      <div className="text-center text-xs text-muted-foreground">
        © 2025–2026 CamThink · NeoMind
      </div>
    </div>
  )
}
