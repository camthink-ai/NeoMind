import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Bot,
  Server,
  Clock,
  Cpu,
  HardDrive,
  Database,
  Layers,
  Globe,
  RefreshCw,
  Activity,
  Monitor,
} from "lucide-react"
import { api } from "@/lib/api"

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
  gpus: GpuInfo[]
}

export function AboutTab() {
  const { t } = useTranslation(["common", "settings"])
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)

  const loadSystemInfo = async (showRefreshing = false) => {
    if (showRefreshing) setRefreshing(true)
    try {
      const response = await api.getSystemStats()
      setSystemInfo(response)
    } catch (e) {
      console.error("Failed to load system info:", e)
    } finally {
      setLoading(false)
      if (showRefreshing) setRefreshing(false)
    }
  }

  useEffect(() => {
    loadSystemInfo()
  }, [])

  const formatBytes = (bytes: number) => {
    const gb = bytes / (1024 * 1024 * 1024)
    return gb.toFixed(2) + " GB"
  }

  const formatUptime = (seconds: number) => {
    const days = Math.floor(seconds / 86400)
    const hours = Math.floor((seconds % 86400) / 3600)
    const minutes = Math.floor((seconds % 3600) / 60)

    if (days > 0) {
      return `${days}d ${hours}h ${minutes}m`
    }
    if (hours > 0) {
      return `${hours}h ${minutes}m`
    }
    return `${minutes}m`
  }

  const getMemoryPercent = (used: number, total: number) => {
    return Math.round((used / total) * 100)
  }

  const getMemoryColor = (percent: number) => {
    if (percent >= 80) return "text-red-500"
    if (percent >= 60) return "text-yellow-500"
    return "text-green-500"
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center justify-center w-12 h-12 rounded-xl bg-gradient-to-br from-gray-900 to-gray-700 dark:from-white dark:to-gray-300 text-white dark:text-gray-900 shadow-lg">
            <Bot className="w-6 h-6" />
          </div>
          <div>
            <h1 className="text-2xl font-bold">NeoTalk</h1>
            <p className="text-sm text-muted-foreground">
              {t("settings:aboutDesc")}
            </p>
          </div>
        </div>
        <Badge variant="outline" className="text-sm">
          {systemInfo?.version || "v0.1.0"}
        </Badge>
      </div>

      {/* System Info */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="flex items-center gap-2">
              <Activity className="h-5 w-5 text-blue-500" />
              {t("settings:systemInfo")}
            </CardTitle>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={() => loadSystemInfo(true)}
              disabled={refreshing}
            >
              <RefreshCw className={`h-4 w-4 ${refreshing ? "animate-spin" : ""}`} />
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="text-center py-8 text-muted-foreground text-sm">
              {t("common:loading")}
            </div>
          ) : systemInfo ? (
            <div className="space-y-4">
              {/* Platform & Architecture */}
              <div className="flex items-center justify-between p-3 bg-muted/50 rounded-lg">
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Server className="h-4 w-4" />
                  {t("settings:platform")}
                </div>
                <div className="text-sm font-medium">
                  {systemInfo.platform} {systemInfo.arch}
                </div>
              </div>

              {/* Uptime */}
              <div className="flex items-center justify-between p-3 bg-muted/50 rounded-lg">
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Clock className="h-4 w-4" />
                  {t("settings:uptime")}
                </div>
                <div className="text-sm font-medium">
                  {formatUptime(systemInfo.uptime)}
                </div>
              </div>

              {/* CPU */}
              <div className="flex items-center justify-between p-3 bg-muted/50 rounded-lg">
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Cpu className="h-4 w-4" />
                  {t("settings:cpuCores")}
                </div>
                <div className="text-sm font-medium">
                  {systemInfo.cpu_count} {t("settings:cores")}
                </div>
              </div>

              {/* GPU */}
              {systemInfo.gpus.length > 0 ? (
                <div className="p-3 bg-muted/50 rounded-lg space-y-2">
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <Monitor className="h-4 w-4" />
                    {t("settings:gpu")}
                  </div>
                  <div className="space-y-1">
                    {systemInfo.gpus.map((gpu, idx) => (
                      <div key={idx} className="text-sm flex items-center justify-between">
                        <span className="font-medium">{gpu.name}</span>
                        <div className="flex items-center gap-2 text-xs text-muted-foreground">
                          <Badge variant="outline" className="text-xs uppercase">
                            {gpu.vendor}
                          </Badge>
                          {gpu.total_memory_mb && (
                            <span>{(gpu.total_memory_mb / 1024).toFixed(1)} GB</span>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              {/* Memory with progress bar */}
              <div className="p-3 bg-muted/50 rounded-lg space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <div className="flex items-center gap-2 text-muted-foreground">
                    <HardDrive className="h-4 w-4" />
                    {t("settings:memory")}
                  </div>
                  <span className="text-muted-foreground">
                    {formatBytes(systemInfo.used_memory)} / {formatBytes(systemInfo.total_memory)}
                  </span>
                </div>
                <div className="w-full bg-background rounded-full h-2 overflow-hidden">
                  <div
                    className={`h-full ${getMemoryColor(getMemoryPercent(systemInfo.used_memory, systemInfo.total_memory))} transition-all`}
                    style={{ width: `${getMemoryPercent(systemInfo.used_memory, systemInfo.total_memory)}%` }}
                  />
                </div>
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span>{t("settings:usedMemory")}: {formatBytes(systemInfo.used_memory)}</span>
                  <span className={getMemoryColor(getMemoryPercent(systemInfo.used_memory, systemInfo.total_memory))}>
                    {getMemoryPercent(systemInfo.used_memory, systemInfo.total_memory)}%
                  </span>
                  <span>{t("settings:freeMemory")}: {formatBytes(systemInfo.available_memory)}</span>
                </div>
              </div>

              {/* Memory breakdown */}
              <div className="grid grid-cols-2 gap-3">
                <div className="p-3 bg-muted/30 rounded-lg text-center">
                  <div className="flex items-center justify-center gap-1 text-xs text-muted-foreground mb-1">
                    <Database className="h-3 w-3" />
                    {t("settings:usedMemory")}
                  </div>
                  <div className="text-sm font-medium">{formatBytes(systemInfo.used_memory)}</div>
                </div>
                <div className="p-3 bg-muted/30 rounded-lg text-center">
                  <div className="flex items-center justify-center gap-1 text-xs text-muted-foreground mb-1">
                    <Layers className="h-3 w-3" />
                    {t("settings:availableMemory")}
                  </div>
                  <div className="text-sm font-medium">{formatBytes(systemInfo.available_memory)}</div>
                </div>
              </div>
            </div>
          ) : (
            <div className="text-center py-8 text-muted-foreground text-sm">
              {t("settings:systemInfoUnavailable")}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Project Info */}
      <Card>
        <CardHeader>
          <CardTitle>{t("settings:projectInfo")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div className="flex items-center justify-between border-b pb-2">
            <span className="text-muted-foreground">{t("settings:version")}</span>
            <Badge variant="secondary">{systemInfo?.version || "v0.1.0"}</Badge>
          </div>
          <div className="flex items-center justify-between border-b pb-2">
            <span className="text-muted-foreground">{t("settings:license")}</span>
            <span>MIT</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">{t("settings:repository")}</span>
            <a
              href="https://github.com/shenmingming/NeoTalk"
              target="_blank"
              rel="noopener noreferrer"
              className="text-blue-500 hover:underline flex items-center gap-1"
            >
              <Globe className="h-3 w-3" />
              github.com/shenmingming/NeoTalk
            </a>
          </div>
        </CardContent>
      </Card>

      {/* Footer */}
      <div className="text-center text-sm text-muted-foreground">
        © 2025 NeoTalk
      </div>
    </div>
  )
}
