// DeviceRealtime Component
//
// Real-time device monitoring interface with live metric updates,
// event streaming, and quick control buttons.

import { useState, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useDeviceEvents } from "@/hooks/useEvents"
import type { NeoTalkEvent, DeviceMetricEvent } from "@/lib/events"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Activity,
  Wifi,
  WifiOff,
  Clock,
  Zap,
  AlertTriangle,
  CheckCircle,
  XCircle,
  RefreshCw,
} from "lucide-react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export interface DeviceMetric {
  name: string
  displayName: string
  value: number | string | boolean | null
  unit?: string
  dataType: string
  timestamp: number
  quality?: number
}

export interface DeviceRealtimeProps {
  /**
   * Filter events by device ID
   */
  deviceId?: string

  /**
   * Filter events by device type
   */
  deviceType?: string

  /**
   * Maximum number of events to display
   */
  maxEvents?: number

  /**
   * Whether to show the event log
   */
  showEventLog?: boolean

  /**
   * Whether to enable auto-refresh
   */
  autoRefresh?: boolean

  /**
   * Callback when a device goes offline
   */
  onDeviceOffline?: (deviceId: string) => void

  /**
   * Callback when a device comes online
   */
  onDeviceOnline?: (deviceId: string) => void

  /**
   * Callback when a metric threshold is breached
   */
  onMetricAlert?: (deviceId: string, metric: string, value: number) => void
}

interface DeviceStatus {
  deviceId: string
  status: "online" | "offline" | "unknown"
  lastSeen: number
  metrics: Record<string, DeviceMetric>
  eventCount: number
}

interface MetricThreshold {
  metric: string
  min?: number
  max?: number
}

/**
 * DeviceRealtime - Real-time device monitoring component
 *
 * @example
 * ```tsx
 * <DeviceRealtime
 *   deviceId="sensor-001"
 *   showEventLog={true}
 *   onDeviceOffline={(id) => console.log(`Device ${id} went offline`)}
 * />
 * ```
 */
export function DeviceRealtime({
  deviceId,
  maxEvents = 100,
  showEventLog = true,
  autoRefresh = true,
  onDeviceOffline,
  onDeviceOnline,
  onMetricAlert,
}: DeviceRealtimeProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [selectedDevice, setSelectedDevice] = useState<string | null>(deviceId || null)
  const [eventFilter, setEventFilter] = useState<"all" | "metrics" | "status" | "alerts">("all")
  // Reserved for future threshold configuration
  const [metricThresholds] = useState<MetricThreshold[]>([])
  // Reserved for future metric expansion state - prefix with underscore to indicate intentionally unused
  const [_expandedMetrics] = useState<Set<string>>(new Set())

  // Subscribe to device events
  const { isConnected, events, clearEvents, reconnect } = useDeviceEvents({
    enabled: autoRefresh,
    onEvent: handleDeviceEvent,
    onConnected: (connected) => {
      console.log(`[DeviceRealtime] Event stream ${connected ? "connected" : "disconnected"}`)
    },
    onError: (error) => {
      console.error("[DeviceRealtime] Event stream error:", error)
    },
  })

  // Track device statuses
  const [deviceStatuses, setDeviceStatuses] = useState<Map<string, DeviceStatus>>(new Map())

  // Filter events for this component
  const filteredEvents = useMemo(() => {
    let filtered = events

    // Apply device filter
    if (selectedDevice) {
      filtered = filtered.filter((e) => {
        if (e.type === "DeviceMetric" || e.type === "DeviceOnline" || e.type === "DeviceOffline") {
          const data = e.data as { device_id?: string }
          return data.device_id === selectedDevice
        }
        return false
      })
    }

    // Apply event type filter
    if (eventFilter !== "all") {
      switch (eventFilter) {
        case "metrics":
          filtered = filtered.filter((e) => e.type === "DeviceMetric")
          break
        case "status":
          filtered = filtered.filter((e) => e.type === "DeviceOnline" || e.type === "DeviceOffline")
          break
        case "alerts":
          filtered = filtered.filter((e) => e.type === "AlertCreated")
          break
      }
    }

    return filtered.slice(-maxEvents)
  }, [events, selectedDevice, eventFilter, maxEvents])

  // Aggregate metrics by device
  const deviceMetrics = useMemo(() => {
    const metrics = new Map<string, DeviceMetric[]>()

    filteredEvents.forEach((event) => {
      if (event.type === "DeviceMetric") {
        const data = event.data as DeviceMetricEvent["data"]
        const deviceId = data.device_id

        if (!metrics.has(deviceId)) {
          metrics.set(deviceId, [])
        }

        metrics.get(deviceId)!.push({
          name: data.metric,
          displayName: data.metric,
          value: data.value,
          quality: data.quality,
          dataType: "unknown",
          timestamp: event.timestamp,
        })
      }
    })

    return metrics
  }, [filteredEvents])

  // Get unique device IDs from events
  const deviceIds = useMemo(() => {
    const ids = new Set<string>()
    events.forEach((event) => {
      if (event.type === "DeviceMetric" || event.type === "DeviceOnline" || event.type === "DeviceOffline") {
        const data = event.data as { device_id?: string }
        if (data.device_id) {
          ids.add(data.device_id)
        }
      }
    })
    return Array.from(ids)
  }, [events])

  // Handle incoming device events
  function handleDeviceEvent(event: NeoTalkEvent) {
    const data = event.data as { device_id?: string }

    if (!data.device_id) return

    setDeviceStatuses((prev) => {
      const updated = new Map(prev)
      const current = updated.get(data.device_id!)

      if (event.type === "DeviceOnline") {
        updated.set(data.device_id!, {
          deviceId: data.device_id!,
          status: "online",
          lastSeen: event.timestamp,
          metrics: current?.metrics || {},
          eventCount: (current?.eventCount || 0) + 1,
        })
        onDeviceOnline?.(data.device_id!)
      } else if (event.type === "DeviceOffline") {
        updated.set(data.device_id!, {
          deviceId: data.device_id!,
          status: "offline",
          lastSeen: event.timestamp,
          metrics: current?.metrics || {},
          eventCount: (current?.eventCount || 0) + 1,
        })
        onDeviceOffline?.(data.device_id!)
      } else if (event.type === "DeviceMetric") {
        const metricData = event.data as DeviceMetricEvent["data"]
        const metrics = current?.metrics || {}
        metrics[metricData.metric] = {
          name: metricData.metric,
          displayName: metricData.metric,
          value: metricData.value,
          quality: metricData.quality,
          dataType: "unknown",
          timestamp: event.timestamp,
        }

        updated.set(data.device_id!, {
          deviceId: data.device_id!,
          status: "online",
          lastSeen: event.timestamp,
          metrics,
          eventCount: (current?.eventCount || 0) + 1,
        })

        // Check metric thresholds
        if (typeof metricData.value === "number") {
          const numValue = metricData.value
          metricThresholds.forEach((threshold) => {
            if (threshold.metric === metricData.metric) {
              if (threshold.min !== undefined && numValue < threshold.min) {
                onMetricAlert?.(data.device_id!, metricData.metric, numValue)
              }
              if (threshold.max !== undefined && numValue > threshold.max) {
                onMetricAlert?.(data.device_id!, metricData.metric, numValue)
              }
            }
          })
        }
      }

      return updated
    })
  }

  // Format timestamp for display
  const formatTimestamp = useCallback((timestamp: number): string => {
    const date = new Date(timestamp * 1000)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return t('devices:realtime.timeAgo.justNow')
    if (diff < 3600000) return t('devices:realtime.timeAgo.minutesAgo', { minutes: Math.floor(diff / 60000) })
    if (diff < 86400000) return t('devices:realtime.timeAgo.hoursAgo', { hours: Math.floor(diff / 3600000) })

    return date.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    })
  }, [t])

  // Format metric value for display
  const formatValue = useCallback((value: number | string | boolean | null, dataType?: string): string => {
    if (value === null || value === undefined) return "-"
    if (typeof value === "boolean") return value ? t('devices:yes') : t('devices:no')
    if (typeof value === "number") {
      if (dataType === "integer") {
        return value.toLocaleString("zh-CN")
      }
      return parseFloat(value.toFixed(2)).toString()
    }
    return String(value)
  }, [t])

  // Get event icon and color
  const getEventIcon = useCallback((eventType: string): React.ReactNode => {
    switch (eventType) {
      case "DeviceOnline":
        return <CheckCircle className="h-4 w-4 text-green-500" />
      case "DeviceOffline":
        return <XCircle className="h-4 w-4 text-red-500" />
      case "DeviceMetric":
        return <Activity className="h-4 w-4 text-info" />
      case "AlertCreated":
        return <AlertTriangle className="h-4 w-4 text-yellow-500" />
      default:
        return <Zap className="h-4 w-4 text-gray-500" />
    }
  }, [])

  // Get event type display name
  const getEventTypeName = useCallback((eventType: string): string => {
    return t(`devices.realtime.eventTypes.${eventType}`) || eventType
  }, [t])

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            {isConnected ? (
              <Wifi className="h-5 w-5 text-green-500" />
            ) : (
              <WifiOff className="h-5 w-5 text-gray-400" />
            )}
            <h2 className="text-xl font-semibold">{t('devices:realtime.title')}</h2>
          </div>
          <Badge variant={isConnected ? "default" : "secondary"}>
            {isConnected ? t('devices:realtime.connected') : t('devices:realtime.disconnected')}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          {/* Device Filter */}
          {deviceIds.length > 0 && (
            <Select value={selectedDevice || "all"} onValueChange={(v) => setSelectedDevice(v === "all" ? null : v)}>
              <SelectTrigger className="w-[200px]">
                <SelectValue placeholder={t('devices:realtime.selectDevice')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('devices:realtime.allDevices')}</SelectItem>
                {deviceIds.map((id) => (
                  <SelectItem key={id} value={id}>
                    {id}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}

          {/* Event Type Filter */}
          <Select value={eventFilter} onValueChange={(v: typeof eventFilter) => setEventFilter(v)}>
            <SelectTrigger className="w-[120px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('devices:realtime.allEvents')}</SelectItem>
              <SelectItem value="metrics">{t('devices:realtime.eventMetrics')}</SelectItem>
              <SelectItem value="status">{t('devices:realtime.eventStatus')}</SelectItem>
              <SelectItem value="alerts">{t('devices:realtime.eventAlerts')}</SelectItem>
            </SelectContent>
          </Select>

          <Button variant="outline" size="icon" onClick={reconnect} title={t('devices:realtime.reconnect')}>
            <RefreshCw className="h-4 w-4" />
          </Button>

          <Button variant="outline" size="sm" onClick={clearEvents}>
            {t('devices:realtime.clearEvents')}
          </Button>
        </div>
      </div>

      {/* Device Status Cards */}
      {deviceIds.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          {deviceIds.slice(0, 8).map((id) => {
            const status = deviceStatuses.get(id)
            const metricsCount = status ? Object.keys(status.metrics).length : 0
            const latestMetric = deviceMetrics.get(id)?.slice(-1)[0]

            return (
              <Card
                key={id}
                className={`cursor-pointer transition-all ${
                  selectedDevice === id ? "ring-2 ring-primary" : "hover:border-primary/50"
                }`}
                onClick={() => setSelectedDevice(selectedDevice === id ? null : id)}
              >
                <CardHeader className="pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-mono truncate" title={id}>
                      {id.slice(-12)}
                    </CardTitle>
                    <Badge
                      variant={status?.status === "online" ? "default" : "secondary"}
                      className="text-xs"
                    >
                      {status?.status === "online" ? t('devices:realtime.online') : t('devices:realtime.offline')}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent>
                  <div className="space-y-2">
                    <div className="flex items-center justify-between text-xs">
                      <span className="text-muted-foreground">{t('devices:realtime.metricCount')}</span>
                      <span className="font-medium">{metricsCount}</span>
                    </div>
                    {latestMetric && (
                      <div className="flex items-center justify-between text-xs">
                        <span className="text-muted-foreground">{latestMetric.displayName}</span>
                        <span className="font-medium">{formatValue(latestMetric.value)}</span>
                      </div>
                    )}
                    <div className="flex items-center gap-1 text-xs text-muted-foreground">
                      <Clock className="h-3 w-3" />
                      {status?.lastSeen ? formatTimestamp(status.lastSeen) : "-"}
                    </div>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      )}

      {/* Metrics and Events Layout */}
      <div className="flex-1 grid grid-cols-1 lg:grid-cols-2 gap-4 min-h-0">
        {/* Metrics Panel */}
        {selectedDevice && deviceMetrics.has(selectedDevice) && (
          <Card className="flex flex-col">
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Activity className="h-5 w-5" />
                {t('devices:realtime.realtimeMetrics')}
                <span className="text-sm font-normal text-muted-foreground font-mono">
                  {selectedDevice.slice(-8)}
                </span>
              </CardTitle>
            </CardHeader>
            <CardContent className="flex-1 overflow-auto">
              <div className="space-y-2">
                {Array.from(deviceMetrics.get(selectedDevice)! || [])
                  .reverse()
                  .slice(0, 20)
                  .map((metric, idx) => (
                    <div
                      key={idx}
                      className="flex items-center justify-between p-2 rounded-md border hover:bg-muted/50 transition-colors"
                    >
                      <div className="flex items-center gap-2 flex-1 min-w-0">
                        <Badge variant="outline" className="text-xs shrink-0">
                          {metric.name}
                        </Badge>
                        <span className="font-medium">{formatValue(metric.value)}</span>
                        {metric.unit && <span className="text-xs text-muted-foreground">{metric.unit}</span>}
                      </div>
                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                        {metric.quality !== undefined && (
                          <span className="shrink-0">{t('devices:realtime.quality', { quality: metric.quality })}</span>
                        )}
                        <span className="shrink-0">{formatTimestamp(metric.timestamp)}</span>
                      </div>
                    </div>
                  ))}
              </div>
            </CardContent>
          </Card>
        )}

        {/* Event Log */}
        {showEventLog && (
          <Card className="flex flex-col">
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Clock className="h-5 w-5" />
                {t('devices:realtime.eventLog')}
                <Badge variant="outline" className="text-xs">
                  {filteredEvents.length}
                </Badge>
              </CardTitle>
            </CardHeader>
            <CardContent className="flex-1 p-0">
              <ScrollArea className="h-full">
                <div className="p-4 space-y-1">
                  {filteredEvents.length === 0 ? (
                    <div className="text-center text-muted-foreground py-8">
                      {isConnected ? t('devices:realtime.waitingForEvents') : t('devices:realtime.notConnectedToStream')}
                    </div>
                  ) : (
                    filteredEvents
                      .slice()
                      .reverse()
                      .map((event, idx) => (
                        <div
                          key={`${event.id}-${idx}`}
                          className="flex items-start gap-2 p-2 rounded-md hover:bg-muted/50 transition-colors text-sm"
                        >
                          <div className="shrink-0 mt-0.5">{getEventIcon(event.type)}</div>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="font-medium">{getEventTypeName(event.type)}</span>
                              {(() => {
                                if (event.data && typeof event.data === "object") {
                                  const deviceId = (event.data as { device_id?: string }).device_id
                                  if (deviceId) {
                                    return <span className="font-mono text-xs text-muted-foreground">{deviceId.slice(-8)}</span>
                                  }
                                }
                                return null
                              })()}
                            </div>
                            <div className="text-xs text-muted-foreground mt-0.5">
                              {formatTimestamp(event.timestamp)}
                            </div>
                          </div>
                        </div>
                      ))
                  )}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  )
}

/**
 * DeviceMetricCard - Individual metric display card
 */
export interface DeviceMetricCardProps {
  name: string
  displayName: string
  value: number | string | boolean | null
  unit?: string
  dataType?: string
  timestamp: number
  quality?: number
  trend?: "up" | "down" | "stable"
  onClick?: () => void
}

export function DeviceMetricCard({
  name,
  displayName,
  value,
  unit,
  dataType,
  timestamp,
  quality,
  trend,
  onClick,
}: DeviceMetricCardProps) {
  const { t } = useTranslation(['devices'])
  // Timestamp is available for future use in showing last update time
  void timestamp
  const formatValue = (val: number | string | boolean | null): string => {
    if (val === null || val === undefined) return "-"
    if (typeof val === "boolean") return val ? t('devices:yes') : t('devices:no')
    if (typeof val === "number") {
      if (dataType === "integer") {
        return val.toLocaleString("zh-CN")
      }
      return parseFloat(val.toFixed(2)).toString()
    }
    return String(val)
  }

  const getTrendIcon = () => {
    switch (trend) {
      case "up":
        return <span className="text-green-500">↑</span>
      case "down":
        return <span className="text-red-500">↓</span>
      default:
        return <span className="text-gray-400">−</span>
    }
  }

  return (
    <Card
      className={`cursor-pointer transition-all hover:border-primary/50 ${onClick ? "" : "pointer-events-none"}`}
      onClick={onClick}
    >
      <CardContent className="p-4">
        <div className="flex items-start justify-between">
          <div className="flex-1 min-w-0">
            <p className="text-xs text-muted-foreground truncate" title={displayName}>
              {displayName}
            </p>
            <p className="text-2xl font-bold truncate mt-1" title={formatValue(value)}>
              {formatValue(value)}
              {unit && <span className="text-base font-normal text-muted-foreground ml-1">{unit}</span>}
            </p>
            <p className="text-xs text-muted-foreground font-mono mt-1 truncate" title={name}>
              {name}
            </p>
          </div>
          {trend && <div className="text-xl">{getTrendIcon()}</div>}
        </div>
        {quality !== undefined && (
          <div className="mt-2 flex items-center gap-2">
            <div className="flex-1 h-1 bg-muted rounded-full overflow-hidden">
              <div
                className={`h-full ${
                  quality >= 80 ? "bg-green-500" : quality >= 50 ? "bg-yellow-500" : "bg-red-500"
                }`}
                style={{ width: `${quality}%` }}
              />
            </div>
            <span className="text-xs text-muted-foreground">{quality}%</span>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
