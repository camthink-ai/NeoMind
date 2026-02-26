import { useState, useEffect } from "react"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs"
import {
  MoreHorizontal,
  Trash2,
  Settings,
  Code2,
  Terminal,
  Zap,
  Database,
  TrendingUp,
  X,
  ChevronLeft,
  ChevronRight,
  RefreshCw,
} from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import type { Extension } from "@/types"
import { api } from "@/lib/api"

interface ExtensionCardProps {
  extension: Extension
  onUnregister?: () => void
  onConfigure?: () => void
  onReload?: () => void
}

// Data type colors
const DATA_TYPE_COLORS: Record<string, string> = {
  integer: "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
  number: "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400",
  string: "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
  boolean: "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400",
  array: "bg-pink-100 text-pink-700 dark:bg-pink-900/30 dark:text-pink-400",
  object: "bg-gray-100 text-gray-700 dark:bg-gray-900/30 dark:text-gray-400",
  float: "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400",
}

// Metric History Dialog Component
function MetricHistoryDialog({
  extensionId,
  extensionName,
  metric,
  open,
  onOpenChange,
}: {
  extensionId: string
  extensionName: string
  metric: { name: string; display_name: string; data_type: string; unit?: string | null }
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation(["extensions", "common"])
  const [loading, setLoading] = useState(false)
  const [data, setData] = useState<Array<{ timestamp: number; value: unknown; quality: string }>>([])
  const [error, setError] = useState<string | null>(null)
  const [timeRange, setTimeRange] = useState<'1h' | '24h' | '7d' | '30d'>('24h')

  useEffect(() => {
    if (!open) return

    const fetchData = async () => {
      setLoading(true)
      setError(null)

      const now = Math.floor(Date.now() / 1000)
      let start: number
      switch (timeRange) {
        case '1h':
          start = now - 3600
          break
        case '24h':
          start = now - 86400
          break
        case '7d':
          start = now - 604800
          break
        case '30d':
          start = now - 2592000
          break
        default:
          start = now - 86400
      }

      try {
        const result = await api.getMetricData(extensionId, metric.name, { start, end: now, limit: 1000 })
        setData(result.data || [])
      } catch (err) {
        console.error('Failed to fetch metric history:', err)
        setError(t('failedToLoadData', { defaultValue: 'Failed to load historical data' }))
      } finally {
        setLoading(false)
      }
    }

    fetchData()
  }, [open, extensionId, metric.name, timeRange, t])

  // Format timestamp
  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp * 1000)
    return date.toLocaleTimeString()
  }

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp * 1000)
    return date.toLocaleDateString()
  }

  // Get numeric value for display
  const getDisplayValue = (val: unknown): string => {
    if (typeof val === 'number') return val.toFixed(2)
    if (typeof val === 'string') return val
    if (typeof val === 'boolean') return val ? 'true' : 'false'
    return JSON.stringify(val)
  }

  // Get numeric value for chart
  const getNumericValue = (val: unknown): number | null => {
    if (typeof val === 'number') return val
    return null
  }

  // Calculate min/max/avg
  const numericValues = data.map(d => getNumericValue(d.value)).filter((v): v is number => v !== null)
  const minValue = numericValues.length > 0 ? Math.min(...numericValues) : null
  const maxValue = numericValues.length > 0 ? Math.max(...numericValues) : null
  const avgValue = numericValues.length > 0 ? numericValues.reduce((a, b) => a + b, 0) / numericValues.length : null
  const latestValue = data.length > 0 ? data[0].value : null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[700px] sm:max-h-[90vh] flex flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <TrendingUp className="h-5 w-5" />
            {metric.display_name}
          </DialogTitle>
          <DialogDescription>
            {extensionName} · {metric.name}
            {metric.unit && ` · (${metric.unit})`}
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex-1 overflow-y-auto pt-6 pb-4">
          {/* Time Range Selector */}
          <div className="flex items-center gap-2 border-b pb-4">
            <span className="text-sm text-muted-foreground">{t('timeRange', { defaultValue: 'Time Range' })}:</span>
            <div className="flex gap-1 flex-wrap">
              {(['1h', '24h', '7d', '30d'] as const).map((range) => (
                <Button
                  key={range}
                  variant={timeRange === range ? 'default' : 'outline'}
                size="sm"
                className="h-7 text-xs"
                onClick={() => setTimeRange(range)}
              >
                {range === '1h' && t('1hour', { defaultValue: '1H' })}
                {range === '24h' && t('24hours', { defaultValue: '24H' })}
                {range === '7d' && t('7days', { defaultValue: '7D' })}
                {range === '30d' && t('30days', { defaultValue: '30D' })}
              </Button>
            ))}
              </div>
            </div>

            {loading ? (
              <div className="flex items-center justify-center py-12 text-muted-foreground">
                <div className="animate-pulse">{t('loading', { defaultValue: 'Loading...' })}</div>
              </div>
            ) : error ? (
              <div className="flex items-center justify-center py-12 text-destructive">
                {error}
              </div>
            ) : data.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
                <Database className="h-12 w-12 mb-4 opacity-50" />
                <p>{t('noHistoricalData', { defaultValue: 'No historical data available for this metric' })}</p>
              </div>
            ) : (
              <>
                {/* Stats Summary */}
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 sm:gap-4 py-4 border-b">
                  <div className="text-center">
                    <div className="text-xs text-muted-foreground">{t('latest', { defaultValue: 'Latest' })}</div>
                    <div className="text-lg font-semibold">
                      {latestValue !== null ? getDisplayValue(latestValue) : '-'}
                    </div>
                  </div>
                  <div className="text-center">
                    <div className="text-xs text-muted-foreground">{t('min', { defaultValue: 'Min' })}</div>
                    <div className="text-lg font-semibold text-blue-600">
                      {minValue !== null ? minValue.toFixed(2) : '-'}
                    </div>
                  </div>
                  <div className="text-center">
                    <div className="text-xs text-muted-foreground">{t('avg', { defaultValue: 'Avg' })}</div>
                    <div className="text-lg font-semibold text-amber-600">
                      {avgValue !== null ? avgValue.toFixed(2) : '-'}
                    </div>
                  </div>
                  <div className="text-center">
                    <div className="text-xs text-muted-foreground">{t('max', { defaultValue: 'Max' })}</div>
                    <div className="text-lg font-semibold text-green-600">
                      {maxValue !== null ? maxValue.toFixed(2) : '-'}
                    </div>
                  </div>
                </div>

                {/* Simple Chart Visualization */}
                {numericValues.length > 0 && (
                  <div className="py-4">
                    <div className="text-sm text-muted-foreground mb-2">{t('valueChart', { defaultValue: 'Value Chart' })}</div>
                    <div className="h-40 flex items-end gap-1 border-l border-b border-muted p-2">
                      {numericValues.slice().reverse().map((val, i) => {
                        const height = maxValue !== null && minValue !== null
                          ? ((val - minValue) / (maxValue - minValue || 1)) * 100
                          : 50
                        return (
                          <div
                            key={i}
                            className="flex-1 bg-primary/60 hover:bg-primary transition-colors rounded-t min-w-[2px]"
                            style={{ height: `${Math.max(height, 5)}%` }}
                            title={`${new Date(data[data.length - 1 - i].timestamp * 1000).toLocaleString()}: ${val.toFixed(2)}`}
                          />
                        )
                      })}
                    </div>
                  </div>
                )}

                {/* Data Table */}
                <div className="py-4">
                  <div className="text-sm text-muted-foreground mb-2">
                    {t('recentData', { defaultValue: 'Recent Data' })} ({data.length} points)
                  </div>
                  <div className="max-h-[200px] overflow-y-auto border rounded-md">
                    <table className="w-full text-sm">
                      <thead className="bg-muted sticky top-0">
                        <tr>
                          <th className="p-2 text-left">{t('time', { defaultValue: 'Time' })}</th>
                          <th className="p-2 text-right">{t('value', { defaultValue: 'Value' })}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {data.slice(0, 50).map((point, i) => (
                          <tr key={i} className="border-t">
                            <td className="p-2 text-muted-foreground">
                              {formatDate(point.timestamp)} {formatTime(point.timestamp)}
                            </td>
                            <td className="p-2 text-right font-mono">
                              {getDisplayValue(point.value)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              </>
            )}
        </DialogContentBody>
      </DialogContent>
    </Dialog>
  )
}

// Commands/Metrics Dialog Component
function ExtensionCapabilitiesDialog({
  extension,
  open,
  onOpenChange,
}: {
  extension: Extension
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation(["extensions", "common"])
  const [selectedMetric, setSelectedMetric] = useState<{ name: string; display_name: string; data_type: string; unit?: string | null } | null>(null)
  const [metricHistoryOpen, setMetricHistoryOpen] = useState(false)

  const handleViewHistory = (metric: { name: string; display_name: string; data_type: string; unit?: string | null }) => {
    setSelectedMetric(metric)
    setMetricHistoryOpen(true)
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-[600px] sm:max-h-[85vh] flex flex-col overflow-hidden">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Code2 className="h-5 w-5" />
              {extension.name}
            </DialogTitle>
            <DialogDescription>
              {t('capabilitiesDescription', { defaultValue: 'View commands and metrics provided by this extension' })}
            </DialogDescription>
          </DialogHeader>

          <DialogContentBody className="flex-1 overflow-y-auto pt-6 pb-4">
            <Tabs defaultValue="commands" className="w-full h-full flex flex-col">
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="commands" className="flex items-center gap-2">
                <Terminal className="h-4 w-4" />
                {t('commands', { defaultValue: 'Commands' })}
                <Badge variant="secondary" className="h-5 px-1.5 text-xs">
                  {extension.commands?.length || 0}
                </Badge>
              </TabsTrigger>
              <TabsTrigger value="metrics" className="flex items-center gap-2">
                <Database className="h-4 w-4" />
                {t('metrics', { defaultValue: 'Metrics' })}
                <Badge variant="secondary" className="h-5 px-1.5 text-xs">
                  {extension.metrics?.length || 0}
                </Badge>
              </TabsTrigger>
            </TabsList>

            {/* Commands Tab */}
            <TabsContent value="commands" className="mt-4">
              {extension.commands && extension.commands.length > 0 ? (
                <div className="space-y-2 max-h-[300px] overflow-y-auto">
                  {extension.commands.map((command) => (
                    <div
                      key={command.id}
                      className="p-3 rounded-lg border bg-muted/30 space-y-2"
                    >
                      <div className="flex items-center gap-2">
                        <Zap className="h-4 w-4 text-yellow-500" />
                        <span className="font-medium text-sm">{command.display_name}</span>
                        <Badge variant="outline" className="text-xs">
                          {command.id}
                        </Badge>
                      </div>
                      <p className="text-xs text-muted-foreground pl-6">{command.description}</p>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  {t('noCommands', { defaultValue: 'No commands available' })}
                </div>
              )}
            </TabsContent>

            {/* Metrics Tab */}
            <TabsContent value="metrics" className="mt-4">
              {extension.metrics && extension.metrics.length > 0 ? (
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-h-[300px] overflow-y-auto">
                  {extension.metrics.map((metric) => (
                    <div
                      key={metric.name}
                      className="p-3 rounded-lg border bg-muted/20 group"
                    >
                      <div className="flex items-center justify-between mb-2">
                        <span className="font-medium text-sm">{metric.display_name}</span>
                        <Badge variant="outline" className="text-xs">
                          {metric.name}
                        </Badge>
                      </div>
                      <div className="flex items-center gap-2 text-xs text-muted-foreground mb-2">
                        <Badge variant="outline" className={cn(
                          "text-[10px] h-4 px-1 border-0",
                          DATA_TYPE_COLORS[metric.data_type] || DATA_TYPE_COLORS.string
                        )}>
                          {metric.data_type}
                        </Badge>
                        {metric.unit && <span>({metric.unit})</span>}
                        {metric.min !== undefined && metric.max !== undefined && (
                          <span>[{metric.min} - {metric.max}]</span>
                        )}
                      </div>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="w-full h-7 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={() => handleViewHistory(metric)}
                      >
                        <TrendingUp className="h-3 w-3 mr-1" />
                        {t('viewHistory', { defaultValue: 'View History' })}
                      </Button>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  {t('noMetrics', { defaultValue: 'No metrics available' })}
                </div>
              )}
            </TabsContent>
          </Tabs>
          </DialogContentBody>
        </DialogContent>
      </Dialog>

      {/* Metric History Dialog */}
      {selectedMetric && (
        <MetricHistoryDialog
          extensionId={extension.id}
          extensionName={extension.name}
          metric={selectedMetric}
          open={metricHistoryOpen}
          onOpenChange={setMetricHistoryOpen}
        />
      )}
    </>
  )
}

export function ExtensionCard({
  extension,
  onUnregister,
  onConfigure,
  onReload,
}: ExtensionCardProps) {
  const { t } = useTranslation(["extensions"])
  const [capabilitiesDialogOpen, setCapabilitiesDialogOpen] = useState(false)

  const hasCapabilities = (extension.commands?.length ?? 0) > 0 || (extension.metrics?.length ?? 0) > 0
  // Extensions are always active once registered in V2 system
  const isActive = extension.state !== "Error"

  return (
    <>
      <Card className="group border-border/50 hover:border-primary/50 transition-all duration-200 overflow-hidden bg-card/50 backdrop-blur-sm">
        <CardContent className="p-4">
          {/* Header */}
          <div className="flex items-start justify-between mb-3">
            <div className="flex items-center gap-3">
              <div className={cn(
                "relative p-2 rounded-lg transition-all",
                isActive ? "bg-primary text-primary-foreground" : "bg-muted/80",
              )}>
                <Code2 className="h-4 w-4" />
                {/* Status indicator dot */}
                <div className={cn(
                  "absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 border-background",
                  isActive ? "bg-green-500" : "bg-red-500"
                )} />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h3 className="font-medium text-sm">{extension.name}</h3>
                  <span className={cn(
                    "text-[10px] px-1.5 py-0.5 rounded-full",
                    isActive ? "bg-green-500/10 text-green-600 dark:text-green-400" :
                    "bg-red-500/10 text-red-600 dark:text-red-400"
                  )}>
                    {isActive ? t('active', { defaultValue: 'Active' }) : extension.state}
                  </span>
                </div>
                <div className="flex items-center gap-2 mt-0.5">
                  <span className="text-[11px] text-muted-foreground font-mono truncate max-w-[100px]">{extension.id}</span>
                  {extension.version && (
                    <span className="text-[10px] text-muted-foreground/70">v{extension.version}</span>
                  )}
                </div>
              </div>
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuItem onClick={() => setCapabilitiesDialogOpen(true)}>
                  <Terminal className="mr-2 h-4 w-4" />
                  {t('viewCapabilities', { defaultValue: 'View Capabilities' })}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => onConfigure?.()}>
                  <Settings className="mr-2 h-4 w-4" />
                  {t('card.configure')}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => onReload?.()}>
                  <RefreshCw className="mr-2 h-4 w-4" />
                  {t('card.reload', { defaultValue: 'Reload' })}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => onUnregister?.()} className="text-destructive">
                  <Trash2 className="mr-2 h-4 w-4" />
                  {t('card.unregister')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          {/* Capabilities Badge */}
          <div className="flex items-center gap-2 mb-3">
            {extension.commands?.length > 0 && (
              <span className="text-[11px] text-muted-foreground flex items-center gap-1">
                <Terminal className="h-3 w-3" />
                {extension.commands.length} {t('commands', { defaultValue: 'commands' })}
              </span>
            )}
            {extension.metrics?.length > 0 && (
              <span className="text-[11px] text-muted-foreground flex items-center gap-1">
                <Database className="h-3 w-3" />
                {extension.metrics.length} {t('metrics', { defaultValue: 'metrics' })}
              </span>
            )}
          </div>

          {/* Description */}
          {extension.description && (
            <p className="text-xs text-muted-foreground line-clamp-2 h-8 mb-3 leading-4">
              {extension.description}
            </p>
          )}

          {/* Author */}
          {extension.author && (
            <p className="text-[10px] text-muted-foreground mb-3">
              {t('card.byAuthor', { author: extension.author })}
            </p>
          )}

          {/* Action Bar */}
          <div className="flex items-center justify-between pt-2 border-t border-border/50">
            <span className="text-[10px] text-muted-foreground">
              {isActive ? t('card.active', { defaultValue: 'Active' }) : extension.state}
            </span>
            <div className="flex items-center gap-1">
              {hasCapabilities && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 text-[10px] px-2"
                  onClick={() => setCapabilitiesDialogOpen(true)}
                >
                  <Terminal className="mr-1 h-2.5 w-2.5" />
                  {t('viewCapabilities', { defaultValue: 'Capabilities' })}
                </Button>
              )}
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Capabilities Dialog */}
      <ExtensionCapabilitiesDialog
        extension={extension}
        open={capabilitiesDialogOpen}
        onOpenChange={setCapabilitiesDialogOpen}
      />
    </>
  )
}
