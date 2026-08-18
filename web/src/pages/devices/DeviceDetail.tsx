import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { ParameterForm } from "@/components/devices/ParameterForm"
import { seedCommandDefaults } from "@/components/devices/seedCommandDefaults"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from "@/components/automation/dialog"
import { ResponsiveTable, type TableColumn, EmptyState, Pagination } from "@/components/shared"
import { DeviceStatusBadge } from "@/components/shared/DeviceStatusBadge"
import { ChevronLeft, Send, Clock, Zap, Settings, Info, ChevronRight, X, Image as ImageIcon, Database, Download } from "lucide-react"
import { toast } from "@/components/ui/use-toast"
import { formatTimestamp } from "@/lib/utils/format"
import { useServerUrl } from "@/lib/server-url"
import type { Device, DeviceType, CommandDefinition, TelemetryDataResponse, DeviceCurrentStateResponse } from "@/types"
import { isBase64Image, getImageDataUrl } from "./utils"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import type { TFunction } from "i18next"

/** Map adapter_id to a localized broker display name.
 *  Avoids rendering backend-hardcoded Chinese (`内置MQTT`/`外部MQTT`) directly. */
function formatBrokerName(t: TFunction, device: Device): string {
  const aid = device.adapter_id
  if (aid === "internal-mqtt" || !aid) {
    return t('devices:detailPage.brokerInternalMqtt', { defaultValue: 'Internal MQTT' })
  }
  if (aid.startsWith("external-mqtt")) {
    return t('devices:detailPage.brokerExternalMqtt', { defaultValue: 'External MQTT ({{id}})', id: aid })
  }
  // Real plugin — show id as-is (plugin metadata is the source of truth)
  return aid
}

// Pagination constants
const PAGE_SIZE = 10

interface DeviceDetailProps {
  device: Device | null
  deviceType: DeviceType | null
  deviceCurrentState: DeviceCurrentStateResponse | null
  telemetryData: TelemetryDataResponse | null
  telemetryLoading: boolean
  selectedMetric: string | null
  onBack: () => void
  onRefresh: () => void
  onMetricClick: (metricName: string, offset?: number, limit?: number) => Promise<void>
  onMetricBack: () => void
  onSendCommand: (commandName: string, params: string) => void
}

function renderMetricValue(
  value: unknown,
  onImageClick?: (src: string) => void,
  truncate = true,
  maxLength = 50,
  t?: TFunction
): React.ReactNode {
  if (value === null || value === undefined) return <span className="text-muted-foreground">-</span>
  // Note: This is a helper function that will receive t through props when needed in i18n context
  // For now, we'll use the component's i18n context by moving this inside the component
  if (typeof value === "boolean") {
    return (
      <span className={value ? "text-success" : "text-error"}>
        {value ? (t?.("common:yes") ?? "Yes") : (t?.("common:no") ?? "No")}
      </span>
    )
  }
  if (typeof value === "number") return <span className="font-semibold tabular-nums">{parseFloat(value.toFixed(2))}</span>
  if (typeof value === "string" && isBase64Image(value)) {
    const imgSrc = getImageDataUrl(value) ?? value
    return (
      <div
        className="cursor-pointer hover:opacity-80 transition-opacity"
        onClick={(e) => {
          e.stopPropagation() // Prevent opening history dialog
          onImageClick?.(imgSrc)
        }}
      >
        <img src={imgSrc} alt="metric" className="h-16 w-16 object-cover rounded-lg" />
      </div>
    )
  }
  if (typeof value === "string") {
    if (truncate && value.length > maxLength) {
      return value.slice(0, maxLength) + "..."
    }
    return value
  }
  if (typeof value === "object" && value !== null) {
    if (Array.isArray(value)) {
      // Show array elements with better formatting
      if (value.length === 0) return <span className="text-muted-foreground text-xs">[]</span>
      // For arrays of objects or complex types, show count and preview
      const hasObjects = value.some(v => typeof v === 'object' && v !== null)
      if (hasObjects || value.length > 5) {
        return (
          <span className="text-xs font-mono" title={JSON.stringify(value, null, 2)}>
            <span className="text-muted-foreground">Array[{value.length}]</span>
            {value.length > 0 && (
              <span className="ml-1 text-muted-foreground">
                {typeof value[0] === 'object' ? '{...}' : String(value[0])}
                {value.length > 1 && ', ...'}
              </span>
            )}
          </span>
        )
      }
      // For small arrays of primitives, show all elements
      const elements = value.map((v, i) => {
        if (typeof v === 'string') {
          return <span key={i} className="text-info">"{v}"</span>
        }
        if (typeof v === 'number') {
          return <span key={i} className="text-warning">{v}</span>
        }
        if (typeof v === 'boolean') {
          return <span key={i} className={v ? "text-success" : "text-error"}>{String(v)}</span>
        }
        return <span key={i} className="text-muted-foreground">{String(v)}</span>
      })
      return (
        <span className="text-xs font-mono">
          <span className="text-muted-foreground">[</span>
          {elements.map((el, i) => (
            <span key={i}>{i > 0 && <span className="text-muted-foreground">, </span>}{el}</span>
          ))}
          <span className="text-muted-foreground">]</span>
        </span>
      )
    }
    // For objects, show as formatted JSON with truncation
    const str = JSON.stringify(value, null, 2)
    if (truncate && str.length > maxLength) {
      return (
        <span className="text-xs font-mono text-muted-foreground" title={str}>
          {str.slice(0, maxLength)}...
        </span>
      )
    }
    return <span className="text-xs font-mono text-muted-foreground">{str}</span>
  }
  return String(value)
}

export function DeviceDetail({
  device,
  deviceType,
  deviceCurrentState,
  telemetryData,
  telemetryLoading,
  selectedMetric,
  onBack,
  onRefresh,
  onMetricClick,
  onMetricBack,
  onSendCommand,
}: DeviceDetailProps) {
  const { t } = useTranslation(['common', 'devices'])
  const isMobile = useIsMobile()
  const serverUrl = useServerUrl()
  const [metricHistoryOpen, setMetricHistoryOpen] = useState(false)
  const [imagePreviewOpen, setImagePreviewOpen] = useState(false)
  const [previewImageSrc, setPreviewImageSrc] = useState<string | null>(null)
  const [commandDialogOpen, setCommandDialogOpen] = useState(false)
  const [selectedCommandDef, setSelectedCommandDef] = useState<CommandDefinition | null>(null)
  const [dialogParams, setDialogParams] = useState<Record<string, unknown>>({})
  const [commandExecuting, setCommandExecuting] = useState(false)
  const [commandResult, setCommandResult] = useState<{ success: boolean; data?: unknown; error?: string } | null>(null)

  // Pagination state
  const [currentPage, setCurrentPage] = useState(1)

  // Use commands from unified response or fallback to deviceType
  const commands = deviceCurrentState?.commands || deviceType?.commands || []
  const templateMetrics = deviceType?.metrics || []

  // Build all metrics from unified response (contains both template and virtual metrics)
  const metricsMap = deviceCurrentState?.metrics || {}

  // Get metric definitions: template metrics + virtual metrics from unified response
  const metricDefinitions = [
    // Template metrics (for reference - values come from deviceCurrentState.metrics)
    ...templateMetrics,
    // Virtual metrics (from deviceCurrentState - those with is_virtual=true)
    ...Object.entries(metricsMap)
      .filter(([, v]) => v.is_virtual)
      .map(([name, v]) => ({
        name,
        display_name: v.display_name || name,
        data_type: v.data_type as 'float' | 'integer' | 'string' | 'boolean' | 'binary' | 'array',
        unit: v.unit,
      }))
  ]

  const handleCommandClick = (cmd: CommandDefinition) => {
    setSelectedCommandDef(cmd)
    setCommandResult(null)
    setDialogParams(seedCommandDefaults(cmd.parameters))
    setCommandDialogOpen(true)
  }

  const handleSendCommand = async () => {
    if (!selectedCommandDef) return
    setCommandExecuting(true)
    setCommandResult(null)
    try {
      await onSendCommand(selectedCommandDef.name, JSON.stringify(dialogParams))
      setCommandResult({ success: true })
    } catch (e: any) {
      setCommandResult({ success: false, error: e?.message || String(e) })
    } finally {
      setCommandExecuting(false)
    }
  }

  const handleCommandDialogClose = (open: boolean) => {
    setCommandDialogOpen(open)
    if (!open) {
      setCommandResult(null)
      setSelectedCommandDef(null)
    }
  }

  const handleMetricCardClick = async (key: string) => {
    // Reset pagination when opening a new metric
    setCurrentPage(1)
    // Open dialog immediately for instant visual feedback
    setMetricHistoryOpen(true)
    // Fetch data in background — dialog shows loading skeleton via telemetryLoading
    await onMetricClick(key, 0, PAGE_SIZE)
  }

  const getMetricDisplayName = (metricName: string): string => {
    // First check unified response
    if (metricsMap[metricName]?.display_name) {
      return metricsMap[metricName].display_name
    }
    // Fallback to template
    const metricDef = deviceType?.metrics?.find(m => m.name === metricName)
    if (metricDef) return metricDef.display_name || metricName
    if (metricName.includes('.')) {
      return metricName.split('.').map(part => {
        const def = deviceType?.metrics?.find(m => m.name === part)
        return def?.display_name || part
      }).join(' / ')
    }
    return metricName
  }

  const isMetricImage = (value: unknown): boolean => {
    if (typeof value === 'string' && isBase64Image(value)) return true
    return false
  }

  const downloadImage = (src: string, timestamp?: string) => {
    try {
      let filename = `${device?.name || 'device'}_${selectedMetric || 'metric'}${timestamp ? '_' + timestamp.replace(/[:\s]/g, '-') : ''}`

      // For /api/images/ URLs: fetch the image file, convert to blob, download
      if (src.startsWith('http') && src.includes('/api/images/')) {
        fetch(src).then(r => r.blob()).then(blob => {
          const ext = blob.type.split('/')[1] === 'jpeg' ? 'jpg' : blob.type.split('/')[1] || 'png'
          const link = document.createElement('a')
          link.href = URL.createObjectURL(blob)
          link.download = `${filename}.${ext}`
          document.body.appendChild(link)
          link.click()
          document.body.removeChild(link)
          URL.revokeObjectURL(link.href)
          toast({ title: t('devices:detailPage.downloadSuccess') })
        }).catch(() => toast({ variant: 'destructive', title: t('devices:detailPage.downloadFailed') }))
        return
      }

      // For base64 data URLs: direct download
      const mimeMatch = src.match(/^data:image\/(\w+);/)
      const ext = mimeMatch?.[1] === 'jpeg' ? 'jpg' : mimeMatch?.[1] || 'png'
      filename += `.${ext}`

      const link = document.createElement('a')
      link.href = src
      link.download = filename
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)

      toast({ title: t('devices:detailPage.downloadSuccess') })
    } catch {
      toast({ variant: 'destructive', title: t('devices:detailPage.downloadFailed') })
    }
  }

  if (!device) return null

  // Effective offline timeout: device override > template default > none
  const effectiveTimeout = device.effective_offline_timeout_secs ?? device.offline_timeout_secs

  // Get current metric data from API response (already paginated)
  const currentMetricData = selectedMetric && telemetryData?.data[selectedMetric]
    ? telemetryData.data[selectedMetric]
    : []
    
  // Pagination info from API response
  const pagination = telemetryData?.pagination
  const totalCount = pagination?.total ?? currentMetricData.length
  const totalPages = Math.ceil(totalCount / PAGE_SIZE)
  const hasNextPage = currentPage < totalPages
  const hasPrevPage = currentPage > 1
  
  // Handle page change
  const handlePageChange = async (newPage: number) => {
    if (!selectedMetric) return
    const offset = (newPage - 1) * PAGE_SIZE
    setCurrentPage(newPage)
    await onMetricClick(selectedMetric, offset, PAGE_SIZE)
  }

  // Metric history table columns
  const metricHistoryColumns: TableColumn[] = [
    { key: 'timestamp', label: t('devices:detailPage.timeLabel'), width: '40%' },
    { key: 'value', label: t('devices:detailPage.valueLabel'), width: '60%' },
  ]

  const renderMetricHistoryCell = (columnKey: string, rowData: Record<string, unknown>) => {
    const point = rowData as unknown as { timestamp: number; value: unknown }
    switch (columnKey) {
      case 'timestamp':
        return (
          <span className="text-sm text-muted-foreground">
            {formatTimestamp(point.timestamp)}
          </span>
        )
      case 'value': {
        if (isMetricImage(point.value)) {
          const imgSrc = getImageDataUrl(String(point.value)) ?? String(point.value)
          return (
            <div className="flex items-center gap-2">
              <div
                className="cursor-pointer hover:opacity-80 transition-opacity inline-block"
                onClick={() => {
                  setPreviewImageSrc(imgSrc)
                  setImagePreviewOpen(true)
                }}
              >
                <img src={imgSrc} alt="metric" className="h-12 w-12 object-cover rounded-lg" loading="lazy" />
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 shrink-0"
                onClick={(e) => {
                  e.stopPropagation()
                  downloadImage(imgSrc, formatTimestamp(point.timestamp))
                }}
                title={t('devices:detailPage.downloadImage')}
              >
                <Download className="h-4 w-4" />
              </Button>
            </div>
          )
        }
        const isComplexValue = typeof point.value === 'object' && point.value !== null
        const isLongString = typeof point.value === 'string' && point.value.length > 100
        if (isComplexValue || isLongString) {
          return (
            <details className="group">
              <summary className="cursor-pointer text-sm text-muted-foreground hover:text-foreground transition-colors list-none flex items-center gap-2">
                <span>{t('devices:detailPage.clickToViewFull')}</span>
                <ChevronRight className="h-4 w-4 transition-transform group-open:rotate-90" />
              </summary>
              <div className="mt-2 p-3 bg-muted-50 rounded-lg max-h-[300px] overflow-auto">
                <pre className="text-xs font-mono whitespace-pre-wrap break-all">
                  {typeof point.value === 'string'
                    ? point.value
                    : JSON.stringify(point.value, null, 2)
                  }
                </pre>
              </div>
            </details>
          )
        }
        return <span className="text-sm">{renderMetricValue(point.value, undefined, false, 50, t)}</span>
      }
      default:
        return null
    }
  }

  return (
    <>
      <div className="flex flex-col flex-1 h-full overflow-hidden bg-background">
        {/* Header — relative z-20: sits above the app's window-drag strip
            (z-10, top 56px) so the back/edit/delete buttons stay clickable.
            Top padding adds the traffic-light / safe-area clearance (the
            detail view is full-bleed with no page title row). */}
        <div
          className={cn("relative z-20 flex items-center justify-between shrink-0 gap-2", isMobile ? "px-3 py-3" : "px-4 sm:px-6 md:px-8 py-5")}
          style={{ paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px) + 2.75rem)" }}
        >
          <div className="flex items-center gap-3 min-w-0 flex-1">
            <Button
              variant="outline"
              size="icon"
              onClick={onBack}
              className="rounded-full shrink-0 bg-background hidden md:inline-flex"
            >
              <ChevronLeft className="h-5 w-5" />
            </Button>
            <div className="flex items-center gap-2 sm:gap-3 min-w-0">
              <div className={cn(
                "w-10 h-10 sm:w-12 sm:h-12 rounded-xl flex items-center justify-center shrink-0",
                device.status === 'online'
                  ? "bg-success-light text-success"
                  : "bg-muted text-muted-foreground"
              )}>
                <Zap className={cn(
                  "h-5 w-5 sm:h-6 sm:w-6",
                  device.status === 'online' ? "text-success" : "text-muted-foreground"
                )} />
              </div>
              <div className="min-w-0">
                <h1 className="text-lg sm:text-xl font-semibold truncate">{device.name || device.id}</h1>
                <p className="text-xs sm:text-sm text-muted-foreground mt-0.5 truncate">
                  {device.device_type} · {device.device_id || device.id}
                </p>
              </div>
            </div>
          </div>
          <div className="flex items-center gap-1.5 sm:gap-2 shrink-0">
            <DeviceStatusBadge device={device} className="px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm" />
          </div>
        </div>

        {/* Content */}
        <div className={cn("flex-1 overflow-auto", isMobile ? "px-3 pb-3" : "px-6 pb-6")}>
          <div className="max-w-6xl mx-auto space-y-5">

            {/* Device Info Card */}
            <div className={cn("bg-card border border-border rounded-lg shadow-sm", isMobile ? "p-4" : "p-6")}>
              <div className="flex items-center gap-2 mb-3 sm:mb-4">
                <Info className="h-5 w-5 text-muted-foreground" />
                <h2 className="font-semibold">{t('devices:detailPage.deviceInfo')}</h2>
              </div>
              <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] md:grid-cols-4 gap-3 sm:gap-4">
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:detailPage.connectionMethod')}</p>
                  <Badge variant="secondary" className="text-xs">{device.adapter_type || 'mqtt'}</Badge>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:detailPage.deviceTypeLabel')}</p>
                  <p className="text-sm font-medium">{device.device_type || '-'}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:detailPage.lastOnlineLabel')}</p>
                  <p className="text-sm font-medium">
                    {device.last_seen ? formatTimestamp(new Date(device.last_seen).getTime() / 1000) : '-'}
                  </p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:detailPage.deviceId', { defaultValue: 'Device ID' })}</p>
                  <code className="text-xs font-mono text-muted-foreground break-all">{device.device_id || device.id}</code>
                </div>

                {/* Row 2: transport + offline timeout + counts */}
                {device.transport_connected !== undefined && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.transportState', { defaultValue: 'Transport' })}</p>
                    <p className="text-sm font-medium">
                      {device.transport_connected
                        ? t('devices:detailPage.transportConnected', { defaultValue: 'Connected' })
                        : t('devices:detailPage.transportDisconnected', { defaultValue: 'Disconnected' })}
                    </p>
                  </div>
                )}
                {effectiveTimeout !== undefined && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.offlineTimeout', { defaultValue: 'Offline Timeout' })}</p>
                    <p className="text-sm font-medium">
                      {device.offline_timeout_secs
                        ? `${device.offline_timeout_secs}s (${t('devices:detailPage.timeoutOverride', { defaultValue: 'override' })})`
                        : `${effectiveTimeout}s (${t('devices:detailPage.timeoutDefault', { defaultValue: 'default' })})`}
                    </p>
                  </div>
                )}
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:headers.metrics', { defaultValue: 'Metrics' })}</p>
                  <p className="text-sm font-medium tabular-nums">{device.metric_count ?? metricDefinitions.length ?? '-'}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t('devices:headers.commands', { defaultValue: 'Commands' })}</p>
                  <p className="text-sm font-medium tabular-nums">{device.command_count ?? commands.length ?? '-'}</p>
                </div>

                {/* Row 3: plugin / broker association */}
                {(device.plugin_name || device.adapter_id) && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.association', { defaultValue: 'Associated Broker' })}</p>
                    <p className="text-sm font-medium truncate">{formatBrokerName(t, device)}</p>
                  </div>
                )}

                {/* Row 4: topics */}
                {device.connection_config?.telemetry_topic && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.telemetryTopicLabel')}</p>
                    <p className="text-sm font-mono text-muted-foreground truncate">
                      {device.connection_config.telemetry_topic}
                    </p>
                  </div>
                )}
                {device.connection_config?.command_topic && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.commandTopic', { defaultValue: 'Command Topic' })}</p>
                    <p className="text-sm font-mono text-muted-foreground truncate">
                      {device.connection_config.command_topic}
                    </p>
                  </div>
                )}
                {device.adapter_type === 'webhook' && (() => {
                  const webhookUrl = `${serverUrl}/api/devices/${device.device_id}/webhook`
                  const token = (device.config?.webhook_token || device.connection_config?.webhook_token) as string | undefined
                  return (
                    <>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">Webhook URL</p>
                        <p className="text-xs font-mono text-muted-foreground break-all">{webhookUrl}</p>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:add.webhookToken')}</p>
                        <p className="text-xs font-mono text-muted-foreground">
                          {token ? `${token.slice(0, 8)}${'•'.repeat(8)}${token.slice(-4)}` : t('devices:add.webhookTokenNone')}
                        </p>
                      </div>
                    </>
                  )
                })()}
              </div>
            </div>

            {/* Raw Data Section - for Simple Mode devices */}
            {deviceType?.mode === 'simple' && (
              <div className={cn("bg-card border border-border rounded-lg shadow-sm", isMobile ? "p-4" : "p-6")}>
                <div className="flex items-center gap-2 mb-3 sm:mb-4">
                  <Database className="h-5 w-5 text-muted-foreground" />
                  <h2 className="font-semibold">{t('devices:detailPage.rawDataTitle')}</h2>
                  <Badge variant="outline" className="text-xs">Raw Mode</Badge>
                </div>
                <div className="bg-muted-50 rounded-lg p-4 overflow-x-auto">
                  <pre className="text-xs font-mono text-muted-foreground break-all whitespace-pre-wrap">
                    {device?.current_values?._raw ? (
                      typeof device.current_values._raw === 'string'
                        ? device.current_values._raw
                        : JSON.stringify(device.current_values._raw, null, 2)
                    ) : (
                      <span className="text-muted-foreground">{t('devices:detailPage.noData')}</span>
                    )}
                  </pre>
                </div>
                <p className="text-xs text-muted-foreground mt-2">
                  {t('devices:detailPage.rawDataDescription')}
                </p>
              </div>
            )}

            {/* Metrics Grid */}
            {metricDefinitions.length > 0 && (
              <div>
                <div className="flex items-center gap-2 mb-4">
                  <Settings className="h-5 w-5 text-muted-foreground" />
                  <h2 className="font-semibold">{t('devices:detailPage.realtimeMetrics')}</h2>
                  <span className="text-xs text-muted-foreground">({metricDefinitions.length})</span>
                  {Object.values(metricsMap).filter(m => m.is_virtual).length > 0 && (
                    <Badge variant="secondary" className="text-xs ml-2">
                      {Object.values(metricsMap).filter(m => m.is_virtual).length} {t('devices:detailPage.virtualMetrics')}
                    </Badge>
                  )}
                </div>
                <div className={cn("grid gap-3 sm:gap-4", isMobile ? "grid-cols-1" : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3")}>
                  {metricDefinitions.map((metricDef) => {
                    // Get value from unified response (deviceCurrentState.metrics)
                    // This is the single source of truth for all metric values
                    let value = metricsMap[metricDef.name]?.value

                    // Fallback to device.current_values for real-time updates
                    if (value === undefined) {
                      const currentValue = device?.current_values?.[metricDef.name]
                      if (currentValue !== undefined) {
                        value = currentValue as string | number | boolean | unknown[] | null
                      }
                    }

                    const isVirtual = metricsMap[metricDef.name]?.is_virtual || false
                    const hasImage = isMetricImage(value)
                    return (
                      <button
                        key={metricDef.name}
                        onClick={() => handleMetricCardClick(metricDef.name)}
                        className={cn(
                          "group rounded-lg text-left shadow-sm transition-all duration-200 hover:shadow-md",
                          isMobile ? "p-3 active:scale-[0.99]" : "p-6 hover:scale-[1.02]",
                          isVirtual
                            ? "bg-muted border-border hover:border-border"
                            : "bg-card border-border hover:border-border"
                        )}
                      >
                        <div className="flex items-start justify-between">
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-1">
                              <p className="text-sm text-muted-foreground truncate">
                                {metricDef.display_name || metricDef.name}
                              </p>
                              {isVirtual && (
                                <Badge variant="outline" className="text-xs px-1.5 py-0 shrink-0">
                                  {t('devices:detailPage.virtual')}
                                </Badge>
                              )}
                            </div>
                            <div className="text-sm break-all leading-relaxed overflow-hidden line-clamp-3" title={typeof value === 'string' ? value : JSON.stringify(value)}>
                              {renderMetricValue(value, (src) => {
                                setPreviewImageSrc(src)
                                setImagePreviewOpen(true)
                              }, true, 40, t)}
                            </div>
                          </div>
                          {isVirtual ? (
                            <Zap className="h-5 w-5 text-muted-foreground group-hover:text-accent-purple transition-colors shrink-0 ml-2" />
                          ) : hasImage ? (
                            <ImageIcon className="h-5 w-5 text-muted-foreground group-hover:text-muted-foreground transition-colors shrink-0 ml-2" />
                          ) : (
                            <ChevronRight className="h-5 w-5 text-muted-foreground group-hover:text-muted-foreground transition-colors shrink-0 ml-2" />
                          )}
                        </div>
                      </button>
                    )
                  })}
                </div>
              </div>
            )}

            {/* Commands Section */}
            {commands.length > 0 && (
              <div>
                <div className="flex items-center gap-2 mb-4">
                  <Send className="h-5 w-5 text-muted-foreground" />
                  <h2 className="font-semibold">{t('devices:detailPage.commandControl')}</h2>
                  <span className="text-xs text-muted-foreground">({commands.length})</span>
                </div>
                <div className={cn("grid gap-3 sm:gap-4", isMobile ? "grid-cols-1" : "grid-cols-1 sm:grid-cols-2 lg:grid-cols-3")}>
                  {commands.map((cmd) => (
                    <button
                      key={cmd.name}
                      onClick={() => handleCommandClick(cmd)}
                      className={cn(
                        "group bg-gradient-to-br from-card to-muted rounded-lg text-left transition-all duration-200 hover:shadow-md border border-border hover:border-border",
                        isMobile ? "p-3 active:scale-[0.99]" : "p-6 hover:scale-[1.02]"
                      )}
                    >
                      <div className="flex items-start justify-between mb-3">
                        <div className="flex-1 min-w-0">
                          <p className="font-medium truncate">{cmd.display_name || cmd.name}</p>
                          <p className="text-xs text-muted-foreground font-mono mt-1 truncate">{cmd.name}</p>
                        </div>
                        <div className="w-10 h-10 rounded-xl bg-muted flex items-center justify-center group-hover:bg-muted-50 transition-colors">
                          <Send className="h-5 w-5 text-primary" />
                        </div>
                      </div>
                      {cmd.parameters && cmd.parameters.length > 0 && (
                        <div className="flex flex-wrap gap-1.5">
                          {cmd.parameters.slice(0, 3).map((p) => (
                            <Badge key={p.name} variant="outline" className="text-xs">
                              {p.display_name || p.name}
                            </Badge>
                          ))}
                          {cmd.parameters.length > 3 && (
                            <Badge variant="outline" className="text-xs">
                              +{cmd.parameters.length - 3}
                            </Badge>
                          )}
                        </div>
                      )}
                    </button>
                  ))}
                </div>
              </div>
            )}

          </div>
        </div>
      </div>

      {/* Metric History Dialog */}
      <FullScreenDialog open={metricHistoryOpen} onOpenChange={(open) => {
        setMetricHistoryOpen(open)
        if (!open) onMetricBack()
      }}>
        <FullScreenDialogHeader
          icon={<Clock className="h-5 w-5" />}
          iconBg="bg-accent-indigo-light"
          iconColor="text-accent-indigo"
          title={selectedMetric ? getMetricDisplayName(selectedMetric) : t('devices:detailPage.metricHistory')}
          onClose={() => { setMetricHistoryOpen(false); onMetricBack() }}
        />
        <FullScreenDialogContent>
          <FullScreenDialogMain className="overflow-hidden">
            <div className="h-full flex flex-col">
              <div className="flex-1 overflow-y-auto px-4 py-4">
                <ResponsiveTable
                  columns={metricHistoryColumns}
                  data={currentMetricData as unknown as Record<string, unknown>[]}
                  renderCell={renderMetricHistoryCell}
                  rowKey={(row) => String((row as { timestamp: number }).timestamp)}
                  loading={telemetryLoading}
                  flexHeight={false}
                  emptyState={
                    <EmptyState
                      icon={<Clock className="h-12 w-12" />}
                      title={t('devices:detailPage.noHistoryData')}
                    />
                  }
                />
              </div>
              {totalCount > PAGE_SIZE && (
                <div className="border-t px-4 py-3">
                  <Pagination
                    total={totalCount}
                    pageSize={PAGE_SIZE}
                    currentPage={currentPage}
                    onPageChange={handlePageChange}
                    hideOnMobile={false}
                  />
                </div>
              )}
            </div>
          </FullScreenDialogMain>
        </FullScreenDialogContent>
      </FullScreenDialog>

      {/* Image Preview Dialog */}
      <Dialog open={imagePreviewOpen} onOpenChange={setImagePreviewOpen}>
        <DialogContent className="sm:max-w-4xl p-0 sm:p-0 overflow-hidden z-[110]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t('devices:detailPage.preview')}</DialogTitle>
            <DialogDescription>{t('devices:detailPage.imagePreview')}</DialogDescription>
          </DialogHeader>
          <div className="absolute right-4 top-4 flex items-center gap-2 z-10">
            {previewImageSrc && (
              <button
                type="button"
                onClick={() => downloadImage(previewImageSrc)}
                className="inline-flex h-9 w-9 items-center justify-center rounded-full bg-overlay-medium text-white transition-colors hover:bg-overlay-heavy"
                title={t('devices:detailPage.downloadImage')}
              >
                <Download className="h-5 w-5" />
              </button>
            )}
            <button
              type="button"
              onClick={() => setImagePreviewOpen(false)}
              className="inline-flex h-9 w-9 items-center justify-center rounded-full bg-overlay-medium text-white transition-colors hover:bg-overlay-heavy"
            >
              <X className="h-5 w-5" />
            </button>
          </div>
          {previewImageSrc && (
            <div className="flex items-center justify-center min-h-[300px]">
              <img src={previewImageSrc} alt={t('devices:detailPage.preview')} className="max-w-full max-h-[70vh] object-contain" />
            </div>
          )}
        </DialogContent>
      </Dialog>

      {/* Command Dialog */}
      {selectedCommandDef && (
        <UnifiedFormDialog
          open={commandDialogOpen}
          onOpenChange={handleCommandDialogClose}
          title={selectedCommandDef.display_name || selectedCommandDef.name}
          icon={<Send className="h-5 w-5" />}
          width="md"
          isSubmitting={commandExecuting}
          onSubmit={handleSendCommand}
          submitLabel={commandExecuting
            ? t('devices:command.dialog.executing', { defaultValue: 'Sending...' })
            : t('devices:command.dialog.sendCommand')}
          submitDisabled={commandExecuting}
        >
          {(!selectedCommandDef.parameters || selectedCommandDef.parameters.length === 0) ? (
            <p className="text-sm text-muted-foreground py-2">
              {t('devices:command.dialog.noParameters', { defaultValue: 'No parameters required' })}
            </p>
          ) : (
            <ParameterForm
              parameters={selectedCommandDef.parameters}
              groups={selectedCommandDef.parameter_groups}
              values={dialogParams}
              onChange={(name, value) => setDialogParams((p) => ({ ...p, [name]: value }))}
              hideDefault={false}
              variant={isMobile ? 'compact' : 'default'}
              grouped={(selectedCommandDef.parameter_groups?.length ?? 0) > 0}
            />
          )}

          {/* Execution Result */}
          {commandResult && (
            <div className={cn(
              "rounded-lg p-3 text-xs font-mono overflow-auto max-h-48",
              commandResult.success ? "bg-success-light" : "bg-error-light"
            )}>
              <pre className="whitespace-pre-wrap break-words">
                {commandResult.success
                  ? t('devices:commandSent')
                  : commandResult.error}
              </pre>
            </div>
          )}
        </UnifiedFormDialog>
      )}
    </>
  )
}
