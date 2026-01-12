import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ChevronLeft, RefreshCw, Activity, Send, Clock, Server, Info } from "lucide-react"
import { toast } from "@/components/ui/use-toast"
import { formatTimestamp } from "@/lib/utils/format"
import type { Device, DeviceType, CommandDefinition, TelemetryDataResponse, CommandHistoryResponse, MqttSettings } from "@/types"
import { formatMetricValue, isBase64Image, getImageDataUrl } from "./utils"

interface DeviceDetailProps {
  device: Device | null
  deviceType: DeviceType | null
  telemetryData: TelemetryDataResponse | null
  commandHistory: CommandHistoryResponse | null
  telemetryLoading: boolean
  mqttSettings: MqttSettings | null
  selectedMetric: string | null
  onBack: () => void
  onRefresh: () => void
  onMetricClick: (metricName: string) => void
  onMetricBack: () => void
  onSendCommand: (commandName: string, params: string) => void
}

// Render metric value - returns either a text string or an img element
function renderMetricValue(
  value: unknown,
  dataType: string | undefined,
  onImageClick: (dataUrl: string) => void,
  t: (key: string) => string
): React.ReactNode {
  if (value === null || value === undefined) return <span className="text-muted-foreground">-</span>
  if (typeof value === "boolean") return value ? t('command.dialog.yes') : t('command.dialog.no')
  if (typeof value === "number") {
    if (dataType === "integer") {
      return value.toLocaleString("zh-CN")
    }
    return parseFloat(value.toFixed(2)).toString()
  }
  if (typeof value === "string" && isBase64Image(value)) {
    const dataUrl = getImageDataUrl(value)
    return (
      <img
        src={dataUrl || undefined}
        alt="Base64 image"
        className="h-12 w-auto object-contain rounded cursor-pointer hover:opacity-80 transition-opacity"
        onClick={(e) => {
          e.stopPropagation()
          if (dataUrl) onImageClick(dataUrl)
        }}
      />
    )
  }
  return String(value)
}

export function DeviceDetail({
  device,
  deviceType,
  telemetryData,
  commandHistory,
  telemetryLoading,
  mqttSettings,
  selectedMetric,
  onBack,
  onRefresh,
  onMetricClick,
  onMetricBack,
  onSendCommand,
}: DeviceDetailProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [previewImageDataUrl, setPreviewImageDataUrl] = useState<string | null>(null)
  const [imagePreviewOpen, setImagePreviewOpen] = useState(false)
  const [commandDialogOpen, setCommandDialogOpen] = useState(false)
  const [selectedCommandDef, setSelectedCommandDef] = useState<CommandDefinition | null>(null)
  const [dialogParams, setDialogParams] = useState<Record<string, unknown>>({})

  const commands = deviceType?.downlink?.commands || []

  const handleCommandClick = (cmd: CommandDefinition) => {
    setSelectedCommandDef(cmd)
    // Initialize params with default values
    const defaultParams: Record<string, unknown> = {}
    if (cmd.parameters) {
      cmd.parameters.forEach(param => {
        if (param.default_value !== undefined) {
          defaultParams[param.name] = param.default_value
        } else {
          // Set reasonable defaults based on data type
          switch (param.data_type) {
            case 'integer':
            case 'float':
              defaultParams[param.name] = 0
              break
            case 'boolean':
              defaultParams[param.name] = false
              break
            case 'string':
              defaultParams[param.name] = ''
              break
            default:
              defaultParams[param.name] = null
          }
        }
      })
    }
    setDialogParams(defaultParams)
    setCommandDialogOpen(true)
  }

  const handleSendCommand = () => {
    if (!selectedCommandDef) return

    // Convert params to JSON string for the API
    const paramsJson = JSON.stringify(dialogParams)
    onSendCommand(selectedCommandDef.name, paramsJson)
    setCommandDialogOpen(false)
    toast({
      title: t('devices:commandSent'),
      description: t('devices:command.dialog.sendCommand') + `: ${selectedCommandDef.display_name || selectedCommandDef.name}`,
    })
  }

  const updateParam = (name: string, value: unknown) => {
    setDialogParams(prev => ({ ...prev, [name]: value }))
  }

  // Get HASS state topics from config (for HASS discovered devices)
  const hassStateTopics = device?.config
    ? Object.entries(device.config)
        .filter(([key]) => key.startsWith('hass_state:'))
        .map(([key, value]) => ({ metric: key.replace('hass_state:', ''), topic: value }))
    : []

  const getMetricDisplayName = (metricName: string): string => {
    const metricDef = deviceType?.uplink?.metrics.find(m => m.name === metricName)
    return metricDef?.display_name || metricName
  }

  const handleImageClick = (dataUrl: string) => {
    setPreviewImageDataUrl(dataUrl)
    setImagePreviewOpen(true)
  }

  if (!device) return null

  return (
    <>
      <div className="flex-1 flex flex-col overflow-hidden -mt-6">
        {/* Header */}
        <div className="border-b px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <Button variant="ghost" size="sm" onClick={onBack}>
                <ChevronLeft className="mr-1 h-4 w-4" />
                {t('common:back')}
              </Button>
              <div>
                <h2 className="text-2xl font-bold flex items-center gap-2">
                  {device.name || device.id}
                </h2>
                <p className="text-sm text-muted-foreground">
                  {t('devices:detail.deviceInfo', { deviceId: device.device_id || device.id, deviceType: device.device_type })}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Badge variant={device.status === "online" ? "default" : "secondary"}>
                {device.status === "online" ? t('devices:status.online') : t('devices:status.offline')}
              </Badge>
              <Button variant="outline" size="sm" onClick={onRefresh} disabled={telemetryLoading}>
                <RefreshCw className={`h-4 w-4 ${telemetryLoading ? "animate-spin" : ""}`} />
              </Button>
            </div>
          </div>
        </div>

        {/* Content with Tabs */}
        <div className="flex-1 overflow-hidden">
          <Tabs defaultValue="connection" className="h-full flex flex-col">
            <div className="px-6 pt-4 pb-2">
              <TabsList>
                <TabsTrigger value="connection">
                  <Server className="mr-2 h-4 w-4" />
                  {t('devices:detail.tabs.connection')}
                </TabsTrigger>
                <TabsTrigger value="metrics">
                  <Activity className="mr-2 h-4 w-4" />
                  {t('devices:detail.tabs.metrics')}
                </TabsTrigger>
                <TabsTrigger value="commands" className={commands.length === 0 ? "hidden" : ""}>
                  <Send className="mr-2 h-4 w-4" />
                  {t('devices:detail.tabs.commands')}
                </TabsTrigger>
                <TabsTrigger value="history">
                  <Clock className="mr-2 h-4 w-4" />
                  {t('devices:detail.tabs.history')}
                </TabsTrigger>
              </TabsList>
            </div>

            {/* Connection Tab */}
            <TabsContent value="connection" className="flex-1 overflow-auto px-6 pb-6 m-0">
              <div className="space-y-4">
                <Card>
                  <CardContent className="pt-6">
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.id')}</p>
                        <p className="text-sm font-mono font-medium">{device.device_id || device.id || '-'}</p>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.name')}</p>
                        <p className="text-sm font-medium">{device.name || '-'}</p>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.type')}</p>
                        <p className="text-sm font-medium">{device.device_type || '-'}</p>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.status')}</p>
                        <div className="flex items-center gap-2">
                          <span className={`h-2 w-2 rounded-full ${
                            device.status === 'online' ? 'bg-green-500' :
                            device.status === 'offline' ? 'bg-gray-400' :
                            'bg-red-500'
                          }`} />
                          <p className="text-sm font-medium capitalize">{device.status || t('devices:status.unknown')}</p>
                        </div>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.lastOnline')}</p>
                        <p className="text-sm font-medium">
                          {device.last_seen ? formatTimestamp(new Date(device.last_seen).getTime() / 1000) : '-'}
                        </p>
                      </div>
                      <div className="space-y-1">
                        <p className="text-xs text-muted-foreground">{t('devices:detail.fields.adapter')}</p>
                        <div className="flex items-center gap-1">
                          <Badge variant="outline" className="text-xs">
                            {device.plugin_name || (mqttSettings ? `${mqttSettings.listen}:${mqttSettings.port}` : t('devices:builtinMqtt'))}
                          </Badge>
                          {device.plugin_id && device.plugin_id !== 'internal-mqtt' && (
                            <span className="text-xs text-muted-foreground">({device.plugin_id})</span>
                          )}
                        </div>
                      </div>
                      <div className="space-y-1 md:col-span-2 lg:col-span-3">
                        <p className="text-xs text-muted-foreground flex items-center gap-1">
                          <Activity className="h-3 w-3" />
                          {t('devices:detail.fields.uplinkTopic')}
                        </p>
                        <p className="text-sm font-mono text-xs bg-muted px-2 py-1 rounded">
                          device/{device.device_type}/{device.device_id || device.id}/uplink
                        </p>
                      </div>
                      <div className="space-y-1 md:col-span-2 lg:col-span-3">
                        <p className="text-xs text-muted-foreground flex items-center gap-1">
                          <Send className="h-3 w-3" />
                          {t('devices:detail.fields.downlinkTopic')}
                        </p>
                        <p className="text-sm font-mono text-xs bg-muted px-2 py-1 rounded">
                          device/{device.device_type}/{device.device_id || device.id}/downlink
                        </p>
                      </div>
                    </div>
                  </CardContent>
                </Card>

                {/* HASS Device State Topics */}
                {hassStateTopics.length > 0 && (
                  <Card>
                    <CardHeader className="pb-3">
                      <CardTitle className="text-base flex items-center gap-2">
                        <Info className="h-4 w-4" />
                        {t('devices:detail.hassSubscription')}
                      </CardTitle>
                      <CardDescription className="text-xs">
                        {t('devices:detail.hassDescription')}
                      </CardDescription>
                    </CardHeader>
                    <CardContent>
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>{t('devices:detail.hassTable.metric')}</TableHead>
                            <TableHead>{t('devices:detail.hassTable.topic')}</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {hassStateTopics.map(({ metric, topic }) => (
                            <TableRow key={metric}>
                              <TableCell className="font-medium text-sm">{metric}</TableCell>
                              <TableCell className="font-mono text-xs">{topic}</TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </CardContent>
                  </Card>
                )}
              </div>
            </TabsContent>

            {/* Metrics Tab */}
            <TabsContent value="metrics" className="flex-1 overflow-auto px-6 pb-6 m-0">

              {selectedMetric ? (
                // Metric History Detail View
                <div className="space-y-4">
                  <div className="flex items-center gap-4">
                    <Button variant="ghost" size="sm" onClick={onMetricBack}>
                      <ChevronLeft className="mr-1 h-4 w-4" />
                      {t('common:back')}
                    </Button>
                    <div>
                      <h3 className="text-lg font-semibold">
                        {getMetricDisplayName(selectedMetric)} {t('devices:detail.metricHistory.title')}
                      </h3>
                      <p className="text-sm text-muted-foreground">
                        {t('devices:detail.metricHistory.device', { name: device.name || device.id })}
                      </p>
                    </div>
                  </div>

                  {telemetryData && selectedMetric in telemetryData.data ? (
                    <Card>
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>{t('devices:detail.metricHistory.time')}</TableHead>
                            <TableHead>{t('devices:detail.metricHistory.value')}</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {telemetryData.data[selectedMetric].length > 0 ? (
                            telemetryData.data[selectedMetric]
                              .slice()
                              .reverse()
                              .slice(0, 100)
                              .map((point, index) => (
                                <TableRow key={index}>
                                  <TableCell className="text-sm text-muted-foreground align-baseline">
                                    {formatTimestamp(point.timestamp)}
                                  </TableCell>
                                  <TableCell className="font-medium">
                                    {renderMetricValue(point.value, undefined, handleImageClick, t)}
                                  </TableCell>
                                </TableRow>
                              ))
                          ) : (
                            <TableRow>
                              <TableCell colSpan={2} className="text-center text-muted-foreground py-8">
                                {t('devices:detail.metricHistory.noData')}
                              </TableCell>
                            </TableRow>
                          )}
                        </TableBody>
                      </Table>
                    </Card>
                  ) : (
                    <Card>
                      <CardContent className="flex items-center justify-center py-12">
                        <p className="text-muted-foreground">{t('devices:detail.noHistory')}</p>
                      </CardContent>
                    </Card>
                  )}
                </div>
              ) : (
                // Metrics Grid View
                device.current_values && Object.keys(device.current_values).length > 0 ? (
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {Object.entries(device.current_values).map(([metricName, value]) => {
                      const metricDef = deviceType?.uplink?.metrics.find(m => m.name === metricName)
                      const displayName = metricDef?.display_name || metricName
                      const unit = metricDef?.unit || ""
                      const dataType = metricDef?.data_type || ""

                      return (
                        <Card
                          key={metricName}
                          className="cursor-pointer transition-all hover:border-primary/50"
                          onClick={() => onMetricClick(metricName)}
                        >
                          <CardHeader className="pb-3">
                            <CardDescription className="text-xs truncate" title={displayName}>
                              {displayName}
                            </CardDescription>
                          </CardHeader>
                          <CardContent>
                            <div className="text-2xl font-bold truncate" title={formatMetricValue(value, dataType, t)}>
                              {renderMetricValue(value, dataType, handleImageClick, t)}
                              {unit && <span className="text-base font-normal text-muted-foreground ml-1">{unit}</span>}
                            </div>
                            <div className="text-xs text-muted-foreground mt-1 truncate" title={metricName}>
                              {metricName}
                            </div>
                          </CardContent>
                        </Card>
                      )
                    })}
                  </div>
                ) : (
                  <Card>
                    <CardContent className="flex items-center justify-center py-12">
                      <p className="text-muted-foreground">{t('devices:detail.noMetrics')}</p>
                    </CardContent>
                  </Card>
                )
              )}
            </TabsContent>

            {/* Commands Tab */}
            <TabsContent value="commands" className="flex-1 overflow-auto px-6 pb-6 m-0">
              <div className="space-y-4">
                {commands.length === 0 ? (
                  <Card>
                    <CardContent className="flex items-center justify-center py-12">
                      <p className="text-muted-foreground">{t('devices:noCommands')}</p>
                    </CardContent>
                  </Card>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {commands.map((cmd: CommandDefinition) => (
                      <Card
                        key={cmd.name}
                        className="cursor-pointer transition-all hover:border-primary/50 hover:shadow-md"
                        onClick={() => handleCommandClick(cmd)}
                      >
                        <CardHeader>
                          <div className="flex items-start justify-between">
                            <div className="flex-1">
                              <CardTitle className="text-base">{cmd.display_name || cmd.name}</CardTitle>
                              <CardDescription className="text-xs font-mono mt-1">
                                {cmd.name}
                              </CardDescription>
                            </div>
                            <Send className="h-4 w-4 text-muted-foreground" />
                          </div>
                        </CardHeader>
                        <CardContent>
                          {cmd.parameters && cmd.parameters.length > 0 ? (
                            <div className="space-y-1">
                              <div className="text-xs text-muted-foreground">{t('devices:command.parameters')}</div>
                              <div className="flex flex-wrap gap-1">
                                {cmd.parameters.map((param) => (
                                  <Badge key={param.name} variant="secondary" className="text-xs">
                                    {param.display_name || param.name}
                                    <span className="text-muted-foreground ml-1">({param.data_type})</span>
                                  </Badge>
                                ))}
                              </div>
                            </div>
                          ) : (
                            <p className="text-xs text-muted-foreground">{t('devices:command.noParameters')}</p>
                          )}
                        </CardContent>
                      </Card>
                    ))}
                  </div>
                )}
              </div>
            </TabsContent>

            {/* Command History Tab */}
            <TabsContent value="history" className="flex-1 overflow-auto px-6 pb-6 m-0">
              <Card>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('devices:history.time')}</TableHead>
                      <TableHead>{t('devices:history.command')}</TableHead>
                      <TableHead>{t('devices:history.parameters')}</TableHead>
                      <TableHead>{t('devices:history.status')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {commandHistory && commandHistory.commands.length > 0 ? (
                      commandHistory.commands.map((entry, index) => (
                        <TableRow key={index}>
                          <TableCell className="text-sm">
                            {entry.timestamp ? formatTimestamp(entry.timestamp) : "-"}
                          </TableCell>
                          <TableCell className="font-medium">{entry.command}</TableCell>
                          <TableCell className="text-sm text-muted-foreground max-w-[200px] truncate">
                            {JSON.stringify(entry.parameters)}
                          </TableCell>
                          <TableCell>
                            {entry.success ? (
                              <Badge variant="default" className="bg-green-500">{t('devices:history.success')}</Badge>
                            ) : (
                              <Badge variant="destructive">{t('devices:history.failed')}</Badge>
                            )}
                          </TableCell>
                        </TableRow>
                      ))
                    ) : (
                      <TableRow>
                        <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                          {commandHistory?.note || t('devices:history.noRecords')}
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </Card>
            </TabsContent>
          </Tabs>
        </div>
      </div>

      {/* Image Preview Dialog */}
      {imagePreviewOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80" onClick={() => setImagePreviewOpen(false)}>
          <div className="max-w-4xl max-h-[90vh] p-4" onClick={(e) => e.stopPropagation()}>
            {previewImageDataUrl && (
              <img
                src={previewImageDataUrl}
                alt="Preview"
                className="max-w-full max-h-[85vh] object-contain rounded"
              />
            )}
            <div className="flex justify-center mt-4">
              <Button onClick={() => setImagePreviewOpen(false)}>{t('common:close')}</Button>
            </div>
          </div>
        </div>
      )}

      {/* Command Send Dialog */}
      {selectedCommandDef && (
        <Dialog open={commandDialogOpen} onOpenChange={setCommandDialogOpen}>
          <DialogContent className="max-w-md">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Send className="h-5 w-5" />
                {selectedCommandDef.display_name || selectedCommandDef.name}
              </DialogTitle>
              <DialogDescription className="font-mono text-xs">
                {selectedCommandDef.name}
              </DialogDescription>
            </DialogHeader>

            <div className="space-y-4 py-4">
              {selectedCommandDef.parameters && selectedCommandDef.parameters.length > 0 ? (
                <div className="space-y-4">
                  {selectedCommandDef.parameters.map((param) => {
                    const value = dialogParams[param.name as keyof typeof dialogParams]

                    return (
                      <div key={param.name} className="space-y-2">
                        <Label htmlFor={`param-${param.name}`} className="flex items-center gap-2">
                          <span>{param.display_name || param.name}</span>
                          <Badge variant="outline" className="text-xs py-0 px-1">
                            {param.data_type}
                          </Badge>
                        </Label>

                        {param.data_type === 'boolean' ? (
                          <div className="flex items-center gap-2">
                            <button
                              type="button"
                              onClick={() => updateParam(param.name, true)}
                              className={`px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                                value === true
                                  ? 'bg-primary text-primary-foreground'
                                  : 'bg-muted text-muted-foreground hover:bg-muted/80'
                              }`}
                            >
                              {t('command.dialog.yes')}
                            </button>
                            <button
                              type="button"
                              onClick={() => updateParam(param.name, false)}
                              className={`px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                                value === false
                                  ? 'bg-primary text-primary-foreground'
                                  : 'bg-muted text-muted-foreground hover:bg-muted/80'
                              }`}
                            >
                              {t('command.dialog.no')}
                            </button>
                          </div>
                        ) : param.allowed_values && param.allowed_values.length > 0 ? (
                          <div className="flex flex-wrap gap-2">
                            {param.allowed_values.map((allowedValue) => (
                              <button
                                key={JSON.stringify(allowedValue)}
                                type="button"
                                onClick={() => updateParam(param.name, allowedValue)}
                                className={`px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                                  value === allowedValue
                                    ? 'bg-primary text-primary-foreground'
                                    : 'bg-muted text-muted-foreground hover:bg-muted/80'
                                }`}
                              >
                                {String(allowedValue)}
                              </button>
                            ))}
                          </div>
                        ) : (
                          <Input
                            id={`param-${param.name}`}
                            type={param.data_type === 'integer' || param.data_type === 'float' ? 'number' : 'text'}
                            value={value === undefined || value === null ? '' : String(value)}
                            onChange={(e) => {
                              const inputValue = e.target.value
                              let newValue: unknown = inputValue

                              if (param.data_type === 'integer') {
                                newValue = inputValue === '' ? 0 : parseInt(inputValue, 10)
                              } else if (param.data_type === 'float') {
                                newValue = inputValue === '' ? 0 : parseFloat(inputValue)
                              }

                              updateParam(param.name, newValue)
                            }}
                            placeholder={param.default_value !== undefined ? t('command.dialog.defaultParam', { value: JSON.stringify(param.default_value) }) : t('command.dialog.pleaseEnter')}
                          />
                        )}

                        {param.min !== undefined || param.max !== undefined ? (
                          <p className="text-xs text-muted-foreground">
                            {t('command.dialog.range', { min: param.min ?? '-∞', max: param.max ?? '+∞' })}
                          </p>
                        ) : null}
                      </div>
                    )
                  })}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground text-center py-4">
                  {t('devices:command.dialog.noParamsNeeded')}
                </p>
              )}

              {/* JSON Preview */}
              <div className="space-y-2">
                <Label className="text-xs text-muted-foreground">{t('devices:command.dialog.jsonPreview')}</Label>
                <pre className="text-xs bg-muted p-2 rounded overflow-x-auto">
                  {JSON.stringify(dialogParams, null, 2)}
                </pre>
              </div>
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={() => setCommandDialogOpen(false)}>
                {t('common:cancel')}
              </Button>
              <Button onClick={handleSendCommand}>
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
