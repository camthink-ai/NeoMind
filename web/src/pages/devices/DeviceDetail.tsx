import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { ChevronLeft, RefreshCw, Send, Clock, Zap, Settings, Info, ChevronRight, X, Image as ImageIcon, Database } from "lucide-react"
import { toast } from "@/components/ui/use-toast"
import { formatTimestamp } from "@/lib/utils/format"
import type { Device, DeviceType, CommandDefinition, TelemetryDataResponse, DeviceCurrentStateResponse } from "@/types"
import { isBase64Image } from "./utils"
import { cn } from "@/lib/utils"

interface DeviceDetailProps {
  device: Device | null
  deviceType: DeviceType | null
  deviceCurrentState: DeviceCurrentStateResponse | null
  telemetryData: TelemetryDataResponse | null
  telemetryLoading: boolean
  selectedMetric: string | null
  onBack: () => void
  onRefresh: () => void
  onMetricClick: (metricName: string) => Promise<void>
  onMetricBack: () => void
  onSendCommand: (commandName: string, params: string) => void
}

function renderMetricValue(
  value: unknown,
  onImageClick?: (src: string) => void,
  truncate = true,
  maxLength = 50
): React.ReactNode {
  if (value === null || value === undefined) return <span className="text-muted-foreground/60">-</span>
  // Note: This is a helper function that will receive t through props when needed in i18n context
  // For now, we'll use the component's i18n context by moving this inside the component
  if (typeof value === "boolean") return <span className={value ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>{value ? "Yes" : "No"}</span>
  if (typeof value === "number") return <span className="font-semibold tabular-nums">{parseFloat(value.toFixed(2))}</span>
  if (typeof value === "string" && isBase64Image(value)) {
    return (
      <div
        className="cursor-pointer hover:opacity-80 transition-opacity"
        onClick={(e) => {
          e.stopPropagation() // Prevent opening history dialog
          onImageClick?.(value)
        }}
      >
        <img src={value} alt="metric" className="h-16 w-16 object-cover rounded-lg" />
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
      if (value.length === 0) return <span className="text-muted-foreground/60 text-xs">[]</span>
      // For arrays of objects or complex types, show count and preview
      const hasObjects = value.some(v => typeof v === 'object' && v !== null)
      if (hasObjects || value.length > 5) {
        return (
          <span className="text-xs font-mono" title={JSON.stringify(value, null, 2)}>
            <span className="text-muted-foreground">Array[{value.length}]</span>
            {value.length > 0 && (
              <span className="ml-1 text-muted-foreground/60">
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
          return <span key={i} className="text-blue-600 dark:text-blue-400">"{v}"</span>
        }
        if (typeof v === 'number') {
          return <span key={i} className="text-amber-600 dark:text-amber-400">{v}</span>
        }
        if (typeof v === 'boolean') {
          return <span key={i} className={v ? "text-green-600" : "text-red-600"}>{String(v)}</span>
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
        <span className="text-xs font-mono text-muted-foreground/60" title={str}>
          {str.slice(0, maxLength)}...
        </span>
      )
    }
    return <span className="text-xs font-mono text-muted-foreground/60">{str}</span>
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
  const [commandDialogOpen, setCommandDialogOpen] = useState(false)
  const [metricHistoryOpen, setMetricHistoryOpen] = useState(false)
  const [imagePreviewOpen, setImagePreviewOpen] = useState(false)
  const [previewImageSrc, setPreviewImageSrc] = useState<string | null>(null)
  const [selectedCommandDef, setSelectedCommandDef] = useState<CommandDefinition | null>(null)
  const [dialogParams, setDialogParams] = useState<Record<string, unknown>>({})

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
    const defaultParams: Record<string, unknown> = {}
    if (cmd.parameters) {
      cmd.parameters.forEach(param => {
        defaultParams[param.name] = param.default_value ??
          (param.data_type === 'boolean' ? false : param.data_type === 'integer' || param.data_type === 'float' ? 0 : '')
      })
    }
    setDialogParams(defaultParams)
    setCommandDialogOpen(true)
  }

  const handleSendCommand = () => {
    if (!selectedCommandDef) return
    onSendCommand(selectedCommandDef.name, JSON.stringify(dialogParams))
    setCommandDialogOpen(false)
    toast({
      title: t('devices:commandSent'),
      description: `${t('devices:command.dialog.sendCommand')}: ${selectedCommandDef.display_name || selectedCommandDef.name}`,
    })
  }

  const handleMetricCardClick = async (key: string) => {
    // Fetch the latest data first, then open the dialog
    // This ensures the dialog shows fresh data, not stale cached data
    await onMetricClick(key)
    setMetricHistoryOpen(true)
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

  if (!device) return null

  // Don't reverse - keep newest first for table display
  // Charts will handle their own ordering
  const currentMetricData = selectedMetric && telemetryData?.data[selectedMetric]
    ? telemetryData.data[selectedMetric].slice(0, 100)
    : []

  return (
    <>
      <div className="flex flex-col h-full bg-gradient-to-b from-background to-muted/20">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-5">
          <div className="flex items-center gap-3">
            <Button variant="ghost" size="icon" onClick={onBack} className="rounded-full">
              <ChevronLeft className="h-5 w-5" />
            </Button>
            <div className="flex items-center gap-3">
              <div className={cn(
                "w-12 h-12 rounded-2xl flex items-center justify-center",
                device.status === 'online'
                  ? "bg-gradient-to-br from-green-500/20 to-emerald-500/10"
                  : "bg-gradient-to-br from-gray-500/20 to-gray-500/10"
              )}>
                <Zap className={cn(
                  "h-6 w-6",
                  device.status === 'online' ? "text-green-600 dark:text-green-400" : "text-gray-500"
                )} />
              </div>
              <div>
                <h1 className="text-xl font-semibold">{device.name || device.id}</h1>
                <p className="text-sm text-muted-foreground mt-0.5">
                  {device.device_type} · {device.device_id || device.id}
                </p>
              </div>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <div className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-sm",
              device.status === 'online'
                ? "bg-green-500/10 text-green-700 dark:text-green-400"
                : "bg-gray-500/10 text-gray-600 dark:text-gray-400"
            )}>
              <span className={cn(
                "h-2 w-2 rounded-full animate-pulse",
                device.status === 'online' ? "bg-green-500" : "bg-gray-400"
              )} />
              {device.status === 'online' ? t('devices:status.online') : t('devices:status.offline')}
            </div>
            <Button variant="ghost" size="icon" onClick={onRefresh} disabled={telemetryLoading} className="rounded-full">
              <RefreshCw className={cn("h-5 w-5", telemetryLoading && "animate-spin")} />
            </Button>
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-auto px-6 pb-6">
          <div className="max-w-6xl mx-auto space-y-5">

            {/* Device Info Card */}
            <div className="bg-gradient-to-br from-card to-muted/30 rounded-3xl p-6 shadow-sm">
              <div className="flex items-center gap-2 mb-4">
                <Info className="h-5 w-5 text-muted-foreground" />
                <h2 className="font-semibold">{t('devices:detailPage.deviceInfo')}</h2>
              </div>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
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
                {device.connection_config?.telemetry_topic && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">{t('devices:detailPage.telemetryTopicLabel')}</p>
                    <p className="text-sm font-mono text-muted-foreground truncate">
                      {device.connection_config.telemetry_topic}
                    </p>
                  </div>
                )}
              </div>
            </div>

            {/* Raw Data Section - for Simple Mode devices */}
            {deviceType?.mode === 'simple' && (
              <div className="bg-gradient-to-br from-card to-muted/30 rounded-3xl p-6 shadow-sm">
                <div className="flex items-center gap-2 mb-4">
                  <Database className="h-5 w-5 text-muted-foreground" />
                  <h2 className="font-semibold">{t('devices:detailPage.rawDataTitle')}</h2>
                  <Badge variant="outline" className="text-xs">Raw Mode</Badge>
                </div>
                <div className="bg-muted/50 rounded-lg p-4 overflow-x-auto">
                  <pre className="text-xs font-mono text-muted-foreground break-all whitespace-pre-wrap">
                    {device?.current_values?._raw ? (
                      typeof device.current_values._raw === 'string'
                        ? device.current_values._raw
                        : JSON.stringify(device.current_values._raw, null, 2)
                    ) : (
                      <span className="text-muted-foreground/60">{t('devices:detailPage.noData')}</span>
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
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
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
                          "group rounded-2xl p-5 text-left transition-all duration-200 hover:shadow-md hover:scale-[1.02] border",
                          isVirtual
                            ? "bg-gradient-to-br from-purple-500/10 to-blue-500/5 border-purple-500/20 hover:border-purple-500/40"
                            : "bg-gradient-to-br from-primary/5 to-primary/0 border-primary/10 hover:border-primary/30"
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
                            <div className="text-sm break-words leading-relaxed" title={typeof value === 'string' ? value : JSON.stringify(value)}>
                              {renderMetricValue(value, (src) => {
                                setPreviewImageSrc(src)
                                setImagePreviewOpen(true)
                              }, true, 40)}
                            </div>
                          </div>
                          {isVirtual ? (
                            <Zap className="h-5 w-5 text-purple-500/60 group-hover:text-purple-500 transition-colors shrink-0 ml-2" />
                          ) : hasImage ? (
                            <ImageIcon className="h-5 w-5 text-muted-foreground/40 group-hover:text-muted-foreground transition-colors shrink-0 ml-2" />
                          ) : (
                            <ChevronRight className="h-5 w-5 text-muted-foreground/40 group-hover:text-muted-foreground transition-colors shrink-0 ml-2" />
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
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                  {commands.map((cmd) => (
                    <button
                      key={cmd.name}
                      onClick={() => handleCommandClick(cmd)}
                      className="group bg-gradient-to-br from-card to-muted/30 rounded-2xl p-5 text-left transition-all duration-200 hover:shadow-md hover:scale-[1.02] border border-border/50 hover:border-primary/30"
                    >
                      <div className="flex items-start justify-between mb-3">
                        <div className="flex-1 min-w-0">
                          <p className="font-medium truncate">{cmd.display_name || cmd.name}</p>
                          <p className="text-xs text-muted-foreground font-mono mt-1 truncate">{cmd.name}</p>
                        </div>
                        <div className="w-10 h-10 rounded-xl bg-primary/10 flex items-center justify-center group-hover:bg-primary/20 transition-colors">
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
      <Dialog open={metricHistoryOpen} onOpenChange={(open) => {
        setMetricHistoryOpen(open)
        if (!open) onMetricBack()
      }}>
        <DialogContent className="sm:max-w-3xl rounded-2xl flex flex-col">
          <DialogHeader>
            <DialogTitle className="text-xl flex items-center gap-2">
              <Clock className="h-5 w-5" />
              {selectedMetric && getMetricDisplayName(selectedMetric)}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {t('devices:detailPage.metricHistory')}
            </DialogDescription>
          </DialogHeader>
          <DialogContentBody className="max-h-[500px] overflow-y-auto pr-4">
              {currentMetricData.length > 0 ? (
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent border-border/50">
                      <TableHead className="text-muted-foreground w-[180px]">{t('devices:detailPage.timeLabel')}</TableHead>
                      <TableHead className="text-muted-foreground">{t('devices:detailPage.valueLabel')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {currentMetricData.map((point, i) => {
                      const isComplexValue = typeof point.value === 'object' && point.value !== null
                      const isLongString = typeof point.value === 'string' && point.value.length > 100
                      return (
                        <TableRow key={i} className="hover:bg-muted/30 border-border/50">
                          <TableCell className="text-sm text-muted-foreground align-top">
                            {formatTimestamp(point.timestamp)}
                          </TableCell>
                          <TableCell>
                            {isMetricImage(point.value) ? (
                              <div
                                className="cursor-pointer hover:opacity-80 transition-opacity inline-block"
                                onClick={() => {
                                  setPreviewImageSrc(String(point.value))
                                  setImagePreviewOpen(true)
                                }}
                              >
                                <img src={String(point.value)} alt="metric" className="h-12 w-12 object-cover rounded-lg" loading="lazy" />
                              </div>
                            ) : isComplexValue || isLongString ? (
                              <details className="group">
                                <summary className="cursor-pointer text-sm text-muted-foreground hover:text-foreground transition-colors list-none flex items-center gap-2">
                                  <span>{t('devices:detailPage.clickToViewFull')}</span>
                                  <ChevronRight className="h-4 w-4 transition-transform group-open:rotate-90" />
                                </summary>
                                <div className="mt-2 p-3 bg-muted/50 rounded-lg max-h-[300px] overflow-auto">
                                  <pre className="text-xs font-mono whitespace-pre-wrap break-all">
                                    {typeof point.value === 'string'
                                      ? point.value
                                      : JSON.stringify(point.value, null, 2)
                                    }
                                  </pre>
                                </div>
                              </details>
                            ) : (
                              <span className="text-sm">{renderMetricValue(point.value, undefined, false)}</span>
                            )}
                          </TableCell>
                        </TableRow>
                      )
                    })}
                  </TableBody>
                </Table>
              ) : (
                <div className="text-center py-8 text-muted-foreground">{t('devices:detailPage.noHistoryData')}</div>
              )}
          </DialogContentBody>
        </DialogContent>
      </Dialog>

      {/* Image Preview Dialog */}
      <Dialog open={imagePreviewOpen} onOpenChange={setImagePreviewOpen}>
        <DialogContent className="sm:max-w-4xl rounded-2xl p-2">
          <DialogHeader className="sr-only">
            <DialogTitle>{t('devices:detailPage.preview')}</DialogTitle>
            <DialogDescription>{t('devices:detailPage.imagePreview')}</DialogDescription>
          </DialogHeader>
          <button
            onClick={() => setImagePreviewOpen(false)}
            className="absolute right-4 top-4 rounded-full bg-black/50 p-2 text-white hover:bg-black/70 transition-colors z-10"
          >
            <X className="h-5 w-5" />
          </button>
          {previewImageSrc && (
            <div className="flex items-center justify-center min-h-[300px]">
              <img src={previewImageSrc} alt={t('devices:detailPage.preview')} className="max-w-full max-h-[70vh] object-contain rounded-lg" />
            </div>
          )}
        </DialogContent>
      </Dialog>

      {/* Command Dialog */}
      {selectedCommandDef && (
        <Dialog open={commandDialogOpen} onOpenChange={setCommandDialogOpen}>
          <DialogContent className="sm:max-w-md rounded-2xl flex flex-col">
            <DialogHeader>
              <DialogTitle className="text-xl">{selectedCommandDef.display_name || selectedCommandDef.name}</DialogTitle>
              <DialogDescription className="font-mono text-xs text-muted-foreground">
                {selectedCommandDef.name}
              </DialogDescription>
            </DialogHeader>
            <DialogContentBody className="space-y-5 py-4">
              {selectedCommandDef.parameters?.map((param) => {
                const value = dialogParams[param.name]
                // Format data type for display
                const formatDataType = (dt: string | { enum: string[] }): string => {
                  if (typeof dt === 'string') return dt
                  if ('enum' in dt) return `enum: ${dt.enum.join(', ')}`
                  return String(dt)
                }
                return (
                  <div key={param.name} className="space-y-2">
                    <Label className="text-sm font-medium">
                      {param.display_name || param.name}
                      <Badge variant="outline" className="ml-2 text-xs">{formatDataType(param.data_type)}</Badge>
                    </Label>
                    {param.data_type === 'boolean' ? (
                      <div className="flex gap-2">
                        <Button
                          type="button"
                          variant={value === true ? "default" : "outline"}
                          size="sm"
                          onClick={() => setDialogParams(p => ({ ...p, [param.name]: true }))}
                          className="rounded-full"
                        >
                          {t('devices:command.dialog.yes')}
                        </Button>
                        <Button
                          type="button"
                          variant={value === false ? "default" : "outline"}
                          size="sm"
                          onClick={() => setDialogParams(p => ({ ...p, [param.name]: false }))}
                          className="rounded-full"
                        >
                          {t('devices:command.dialog.no')}
                        </Button>
                      </div>
                    ) : param.allowed_values ? (
                      <div className="flex flex-wrap gap-2">
                        {param.allowed_values.map((v) => (
                          <Button
                            key={String(v)}
                            type="button"
                            variant={value === v ? "default" : "outline"}
                            size="sm"
                            onClick={() => setDialogParams(p => ({ ...p, [param.name]: v }))}
                            className="rounded-full"
                          >
                            {String(v)}
                          </Button>
                        ))}
                      </div>
                    ) : (
                      <Input
                        type={param.data_type === 'integer' || param.data_type === 'float' ? 'number' : 'text'}
                        value={typeof value === 'string' || typeof value === 'number' ? String(value) : ''}
                        onChange={(e) => {
                          const parsed = param.data_type === 'integer'
                            ? parseInt(e.target.value) || 0
                            : param.data_type === 'float'
                            ? parseFloat(e.target.value) || 0
                            : e.target.value
                          setDialogParams(p => ({ ...p, [param.name]: parsed }))
                        }}
                        className="rounded-xl"
                      />
                    )}
                  </div>
                )
              })}
            </DialogContentBody>
            <DialogFooter>
              <Button variant="outline" onClick={() => setCommandDialogOpen(false)} className="rounded-full">
                {t('common:cancel')}
              </Button>
              <Button onClick={handleSendCommand} className="rounded-full">
                <Send className="h-4 w-4 mr-2" />
                {t('devices:command.dialog.sendCommand')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </>
  )
}
