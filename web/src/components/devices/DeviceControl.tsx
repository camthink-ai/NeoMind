// DeviceControl Component
//
// Device command interface for sending commands to devices,
// tracking command history, and viewing results.

import { useState, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Send,
  RefreshCw,
  CheckCircle,
  XCircle,
  Clock,
  Terminal,
  AlertTriangle,
  Plus,
  RotateCcw,
} from "lucide-react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { EmptyStateInline } from "@/components/shared"
import { formatTimestamp as formatTs } from "@/lib/utils/format"

export interface CommandDefinition {
  name: string
  displayName?: string
  topic: string
  payloadTemplate?: string
  parameters?: CommandParameter[]
  timeoutMs?: number
}

export interface CommandParameter {
  name: string
  displayName?: string
  dataType: string
  defaultValue?: unknown
  minValue?: number
  maxValue?: number
  unit?: string
  allowedValues?: unknown[]
  required?: boolean
}

export interface CommandExecution {
  id: string
  deviceId: string
  deviceName: string
  command: string
  parameters: Record<string, unknown>
  timestamp: number
  status: "pending" | "sent" | "success" | "failed" | "timeout"
  result?: unknown
  error?: string
  executionTime?: number
}

export interface DeviceControlProps {
  /**
   * Available devices
   */
  devices: Array<{ id: string; name?: string; deviceType: string; status: string }>

  /**
   * Command definitions per device type (legacy)
   * @deprecated Use deviceTypes instead - commands will be extracted from templates
   */
  commandDefinitions?: Record<string, CommandDefinition[]>

  /**
   * Device type templates - commands will be automatically extracted from templates
   */
  deviceTypes?: Array<{ device_type: string; commands: Array<{ name: string; display_name?: string; payload_template: string; parameters?: Array<{ name: string; display_name?: string; data_type: string; default_value?: unknown; min?: number; max?: number; unit?: string; allowed_values?: unknown[] }> }> }>

  /**
   * Recent command history
   */
  commandHistory?: CommandExecution[]

  /**
   * Callback when a command is sent
   */
  onSendCommand?: (
    deviceId: string,
    command: string,
    parameters: Record<string, unknown>
  ) => Promise<boolean>

  /**
   * Callback to refresh device list
   */
  onRefreshDevices?: () => void

  /**
   * Maximum number of history entries to display
   */
  maxHistory?: number
}

/**
 * DeviceControl - Device command interface component
 *
 * @example
 * ```tsx
 * <DeviceControl
 *   devices={devices}
 *   commandDefinitions={commandDefs}
 *   onSendCommand={handleSendCommand}
 * />
 * ```
 */
export function DeviceControl({
  devices,
  commandDefinitions,
  deviceTypes = [],
  commandHistory = [],
  onSendCommand,
  onRefreshDevices,
  maxHistory = 50,
}: DeviceControlProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null)
  const [selectedCommand, setSelectedCommand] = useState<string | null>(null)
  const [commandParams, setCommandParams] = useState<Record<string, unknown>>({})
  const [isSending, setIsSending] = useState(false)
  const [expandedHistory, setExpandedHistory] = useState<Set<string>>(new Set())
  const [batchMode, setBatchMode] = useState(false)
  const [selectedDevicesForBatch, setSelectedDevicesForBatch] = useState<Set<string>>(new Set())
  const [quickCommandOpen, setQuickCommandOpen] = useState(false)
  const [quickCommandPayload, setQuickCommandPayload] = useState("{}")

  // Get selected device
  const selectedDevice = useMemo(() => {
    return devices.find((d) => d.id === selectedDeviceId)
  }, [devices, selectedDeviceId])

  // Get available commands for selected device (from template or legacy definitions)
  const availableCommands = useMemo(() => {
    if (!selectedDevice) return []
    
    // Try to get from device type template first (new architecture)
    const deviceType = deviceTypes.find(dt => dt.device_type === selectedDevice.deviceType)
    if (deviceType && deviceType.commands) {
      return deviceType.commands.map(cmd => ({
        name: cmd.name,
        displayName: cmd.display_name || cmd.name,
        topic: '', // Not needed in new architecture
        payloadTemplate: cmd.payload_template,
        parameters: cmd.parameters?.map(p => ({
          name: p.name,
          displayName: p.display_name || p.name,
          dataType: p.data_type,
          defaultValue: p.default_value,
          minValue: p.min,
          maxValue: p.max,
          unit: p.unit,
          allowedValues: p.allowed_values,
        })) || [],
      }))
    }
    
    // Fallback to legacy commandDefinitions
    if (commandDefinitions) {
      return commandDefinitions[selectedDevice.deviceType] || []
    }
    
    return []
  }, [selectedDevice, deviceTypes, commandDefinitions])

  // Get current command definition
  const currentCommandDef = useMemo(() => {
    if (!selectedCommand) return null
    return availableCommands.find((c) => c.name === selectedCommand) || null
  }, [selectedCommand, availableCommands])

  // Filter and sort history
  const filteredHistory = useMemo(() => {
    let filtered = commandHistory

    if (selectedDeviceId) {
      filtered = filtered.filter((h) => h.deviceId === selectedDeviceId)
    }

    return filtered
      .sort((a, b) => b.timestamp - a.timestamp)
      .slice(0, maxHistory)
  }, [commandHistory, selectedDeviceId, maxHistory])

  // Get status icon
  const getStatusIcon = useCallback((status: CommandExecution["status"]) => {
    switch (status) {
      case "pending":
        return <Clock className="h-4 w-4 text-warning" />
      case "sent":
        return <Send className="h-4 w-4 text-info" />
      case "success":
        return <CheckCircle className="h-4 w-4 text-green-500" />
      case "failed":
        return <XCircle className="h-4 w-4 text-red-500" />
      case "timeout":
        return <AlertTriangle className="h-4 w-4 text-orange-500" />
    }
  }, [])

  // Get status label
  const getStatusLabel = useCallback((status: CommandExecution["status"]): string => {
    return t(`devices:control.statusLabels.${status}`)
  }, [t])

  // Handle device selection
  const handleDeviceSelect = useCallback((deviceId: string) => {
    setSelectedDeviceId(deviceId)
    setSelectedCommand(null)
    setCommandParams({})
  }, [])

  // Handle command selection
  const handleCommandSelect = useCallback((commandName: string) => {
    setSelectedCommand(commandName)

    // Initialize parameters with defaults
    const commandDef = availableCommands.find((c) => c.name === commandName)
    if (commandDef?.parameters) {
      const defaults: Record<string, unknown> = {}
      commandDef.parameters.forEach((param) => {
        if (param.defaultValue !== undefined) {
          defaults[param.name] = param.defaultValue
        } else if (param.dataType === "integer" || param.dataType === "float") {
          defaults[param.name] = 0
        } else if (param.dataType === "boolean") {
          defaults[param.name] = false
        } else if (param.dataType === "string") {
          defaults[param.name] = ""
        }
      })
      setCommandParams(defaults)
    } else {
      setCommandParams({})
    }
  }, [availableCommands])

  // Update parameter value
  const updateParameter = useCallback((name: string, value: unknown) => {
    setCommandParams((prev) => ({
      ...prev,
      [name]: value,
    }))
  }, [])

  // Send command
  const handleSendCommand = async () => {
    if (!selectedDeviceId || !selectedCommand || !onSendCommand) return

    setIsSending(true)
    try {
      const success = await onSendCommand(selectedDeviceId, selectedCommand, commandParams)
      if (success) {
        setSelectedCommand(null)
        setCommandParams({})
      }
    } finally {
      setIsSending(false)
    }
  }

  // Send batch command
  const handleBatchCommand = async () => {
    if (selectedDevicesForBatch.size === 0 || !selectedCommand || !onSendCommand) return

    setIsSending(true)
    try {
      for (const deviceId of selectedDevicesForBatch) {
        await onSendCommand(deviceId, selectedCommand, commandParams)
      }
      setSelectedDevicesForBatch(new Set())
      setBatchMode(false)
    } finally {
      setIsSending(false)
    }
  }

  // Send quick command with custom JSON payload
  const handleQuickCommand = async () => {
    if (!selectedDeviceId || !onSendCommand) return

    let payload: Record<string, unknown>
    try {
      payload = JSON.parse(quickCommandPayload)
    } catch {
      alert(t('devices:control.jsonFormatError'))
      return
    }

    setIsSending(true)
    try {
      await onSendCommand(selectedDeviceId, "custom", payload)
      setQuickCommandPayload("{}")
      setQuickCommandOpen(false)
    } finally {
      setIsSending(false)
    }
  }

  // Toggle history expansion
  const toggleHistoryExpansion = useCallback((id: string) => {
    setExpandedHistory((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  // Render parameter input
  const renderParameterInput = (param: CommandParameter) => {
    const value = commandParams[param.name]

    if (param.allowedValues && param.allowedValues.length > 0) {
      return (
        <Select
          value={String(value ?? "")}
          onValueChange={(v) => updateParameter(param.name, v)}
        >
          <SelectTrigger>
            <SelectValue placeholder={t('devices:control.selectValue')} />
          </SelectTrigger>
          <SelectContent>
            {param.allowedValues.map((allowed, idx) => (
              <SelectItem key={idx} value={String(allowed)}>
                {String(allowed)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )
    }

    if (param.dataType === "boolean") {
      return (
        <Select
          value={String(value ?? "false")}
          onValueChange={(v) => updateParameter(param.name, v === "true")}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="true">{t('devices:yes')}</SelectItem>
            <SelectItem value="false">{t('devices:no')}</SelectItem>
          </SelectContent>
        </Select>
      )
    }

    if (param.dataType === "integer" || param.dataType === "float") {
      return (
        <div className="flex items-center gap-2">
          <Input
            type="number"
            value={String(value ?? 0)}
            onChange={(e) =>
              updateParameter(
                param.name,
                param.dataType === "integer"
                  ? parseInt(e.target.value) || 0
                  : parseFloat(e.target.value) || 0
              )
            }
            min={param.minValue}
            max={param.maxValue}
            step={param.dataType === "integer" ? 1 : 0.1}
          />
          {param.unit && <span className="text-xs text-muted-foreground">{param.unit}</span>}
        </div>
      )
    }

    return (
      <Input
        value={String(value ?? "")}
        onChange={(e) => updateParameter(param.name, e.target.value)}
        placeholder={param.displayName || param.name}
      />
    )
  }

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Terminal className="h-5 w-5" />
          <h2 className="text-xl font-semibold">{t('devices:control.title')}</h2>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={onRefreshDevices}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t('devices:control.refreshDevices')}
          </Button>

          <Dialog open={quickCommandOpen} onOpenChange={setQuickCommandOpen}>
            <DialogTrigger asChild>
              <Button variant="outline" size="sm" disabled={!selectedDeviceId}>
                <Terminal className="mr-2 h-4 w-4" />
                {t('devices:control.quickCommand')}
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t('devices:control.sendCustomCommand')}</DialogTitle>
              </DialogHeader>
              <div className="space-y-4 py-4">
                <div className="space-y-2">
                  <Label>{t('devices:control.targetDevice')}</Label>
                  <p className="text-sm text-muted-foreground">
                    {selectedDevice?.name || selectedDevice?.id || t('devices:control.notSelected')}
                  </p>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="quick-payload">{t('devices:control.commandParamsJson')}</Label>
                  <Textarea
                    id="quick-payload"
                    value={quickCommandPayload}
                    onChange={(e) => setQuickCommandPayload(e.target.value)}
                    placeholder='{"action": "value"}'
                    rows={6}
                    className="font-mono text-sm"
                  />
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setQuickCommandOpen(false)}>
                  {t('common:cancel')}
                </Button>
                <Button onClick={handleQuickCommand} disabled={isSending}>
                  {isSending ? t('devices:control.sending') : t('devices:control.send')}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {batchMode ? (
            <>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setBatchMode(false)
                  setSelectedDevicesForBatch(new Set())
                }}
              >
                {t('devices:control.cancelBatch')}
              </Button>
              <Button
                size="sm"
                onClick={handleBatchCommand}
                disabled={selectedDevicesForBatch.size === 0 || !selectedCommand || isSending}
              >
                <Send className="mr-2 h-4 w-4" />
                {t('devices:control.sendBatch')} ({selectedDevicesForBatch.size})
              </Button>
            </>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setBatchMode(true)}
              disabled={!selectedCommand}
            >
              <Plus className="mr-2 h-4 w-4" />
              {t('devices:control.batchMode')}
            </Button>
          )}
        </div>
      </div>

      <div className="flex-1 grid grid-cols-1 lg:grid-cols-2 gap-4 min-h-0">
        {/* Command Panel */}
        <Card className="flex flex-col">
          <CardHeader>
            <CardTitle>{t('devices:control.sendCommand')}</CardTitle>
            <CardDescription>{t('devices:control.targetDevice')}</CardDescription>
          </CardHeader>
          <CardContent className="flex-1 space-y-4 overflow-auto">
            {/* Device Selection */}
            <div className="space-y-2">
              <Label>{t('devices:control.targetDevice')}</Label>
              <Select value={selectedDeviceId || ""} onValueChange={handleDeviceSelect}>
                <SelectTrigger>
                  <SelectValue placeholder={t('devices:control.selectDevice')} />
                </SelectTrigger>
                <SelectContent>
                  {devices.map((device) => (
                    <SelectItem key={device.id} value={device.id}>
                      <div className="flex items-center gap-2">
                        <span>{device.name || device.id}</span>
                        <Badge
                          variant={device.status === "online" ? "default" : "secondary"}
                          className="text-xs"
                        >
                          {device.status === "online" ? t('devices:status.online') : t('devices:status.offline')}
                        </Badge>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Batch Device Selection */}
            {batchMode && (
              <div className="space-y-2">
                <Label>{t('devices:control.batchTargetDevices')}</Label>
                <ScrollArea className="h-32 border rounded-md p-2">
                  <div className="space-y-1">
                    {devices.map((device) => (
                      <div
                        key={device.id}
                        className="flex items-center gap-2 p-2 rounded hover:bg-muted cursor-pointer"
                        onClick={() => {
                          setSelectedDevicesForBatch((prev) => {
                            const next = new Set(prev)
                            if (next.has(device.id)) {
                              next.delete(device.id)
                            } else {
                              next.add(device.id)
                            }
                            return next
                          })
                        }}
                      >
                        <input
                          type="checkbox"
                          checked={selectedDevicesForBatch.has(device.id)}
                          onChange={() => {}}
                          className="pointer-events-none"
                        />
                        <span className="text-sm flex-1">{device.name || device.id}</span>
                        <Badge
                          variant={device.status === "online" ? "default" : "secondary"}
                          className="text-xs"
                        >
                          {device.status}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              </div>
            )}

            {/* Command Selection */}
            {!batchMode && selectedDevice && (
              <div className="space-y-2">
                <Label>{t('devices:control.selectCommand')}</Label>
                {availableCommands.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    {t('devices:control.noCommandsAvailable')}
                  </p>
                ) : (
                  <Select value={selectedCommand || ""} onValueChange={handleCommandSelect}>
                    <SelectTrigger>
                      <SelectValue placeholder={t('devices:control.selectCommand')} />
                    </SelectTrigger>
                    <SelectContent>
                      {availableCommands.map((cmd) => (
                        <SelectItem key={cmd.name} value={cmd.name}>
                          {cmd.displayName || cmd.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </div>
            )}

            {/* Parameters */}
            {currentCommandDef && currentCommandDef.parameters && (
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <Label>{t('devices:command.parameters')}</Label>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      const defaults: Record<string, unknown> = {}
                      currentCommandDef.parameters?.forEach((param) => {
                        if (param.defaultValue !== undefined) {
                          defaults[param.name] = param.defaultValue
                        }
                      })
                      setCommandParams(defaults)
                    }}
                  >
                    <RotateCcw className="h-3 w-3 mr-1" />
                    {t('devices:control.reset')}
                  </Button>
                </div>
                {currentCommandDef.parameters.map((param) => (
                  <div key={param.name} className="space-y-2">
                    <div className="flex items-center justify-between">
                      <Label className="text-sm">
                        {param.displayName || param.name}
                        {(param.defaultValue === undefined) && <span className="text-red-500 ml-1">*</span>}
                      </Label>
                      {(param.minValue !== undefined || param.maxValue !== undefined) && (
                        <span className="text-xs text-muted-foreground">
                          {param.minValue !== undefined && t('devices:control.minValue', { value: param.minValue })}
                          {param.minValue !== undefined && param.maxValue !== undefined && " | "}
                          {param.maxValue !== undefined && t('devices:control.maxValue', { value: param.maxValue })}
                        </span>
                      )}
                    </div>
                    {renderParameterInput(param)}
                  </div>
                ))}
              </div>
            )}

            {/* Send Button */}
            {selectedCommand && !batchMode && (
              <Button
                className="w-full"
                size="lg"
                onClick={handleSendCommand}
                disabled={isSending || !selectedDeviceId}
              >
                {isSending ? (
                  <>
                    <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                    {t('devices:control.sending')}
                  </>
                ) : (
                  <>
                    <Send className="mr-2 h-4 w-4" />
                    {t('devices:control.sendCommand')}
                  </>
                )}
              </Button>
            )}
          </CardContent>
        </Card>

        {/* History Panel */}
        <Card className="flex flex-col">
          <CardHeader>
            <CardTitle>{t('devices:control.commandHistory')}</CardTitle>
            <CardDescription>
              {t('devices:control.recordsCount', { count: filteredHistory.length })}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex-1 p-0 min-h-0">
            <ScrollArea className="h-full">
              <Table>
                <TableHeader className="sticky top-0 bg-background">
                  <TableRow>
                    <TableHead className="w-[180px]">{t('devices:control.historyHeaders.time')}</TableHead>
                    <TableHead>{t('devices:control.historyHeaders.device')}</TableHead>
                    <TableHead>{t('devices:control.historyHeaders.command')}</TableHead>
                    <TableHead align="center">{t('devices:control.historyHeaders.status')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredHistory.length === 0 ? (
                    <EmptyStateInline title={t('devices:control.noCommandHistory')} colSpan={4} />
                  ) : (
                    filteredHistory.map((entry) => {
                      const isExpanded = expandedHistory.has(entry.id)
                      return (
                        <>
                          <TableRow
                            key={entry.id}
                            className="cursor-pointer hover:bg-muted/50"
                            onClick={() => toggleHistoryExpansion(entry.id)}
                          >
                            <TableCell className="text-sm text-muted-foreground">
                              {formatTs(entry.timestamp)}
                            </TableCell>
                            <TableCell>{entry.deviceName}</TableCell>
                            <TableCell className="font-mono text-xs">{entry.command}</TableCell>
                            <TableCell align="center">
                              <div className="flex items-center justify-center gap-2">
                                {getStatusIcon(entry.status)}
                                <span className="text-sm">{getStatusLabel(entry.status)}</span>
                              </div>
                            </TableCell>
                          </TableRow>
                          {isExpanded && (
                            <TableRow>
                              <TableCell colSpan={4} className="bg-muted/50">
                                <div className="space-y-2 py-2">
                                  <div className="text-sm">
                                    <span className="font-medium">{t('devices:control.historyDetails.parameters')}</span>
                                    <pre className="mt-1 text-xs bg-background p-2 rounded overflow-x-auto">
                                      {String(JSON.stringify(entry.parameters, null, 2))}
                                    </pre>
                                  </div>
                                  {entry.result != null && (
                                    <div className="text-sm">
                                      <span className="font-medium">{t('devices:control.historyDetails.result')}</span>
                                      <pre className="mt-1 text-xs bg-background p-2 rounded overflow-x-auto">
                                        {String(JSON.stringify(entry.result, null, 2))}
                                      </pre>
                                    </div>
                                  )}
                                  {entry.error && (
                                    <div className="text-sm text-destructive">
                                      <span className="font-medium">{t('devices:control.historyDetails.error')}</span>
                                      <p className="mt-1">{entry.error}</p>
                                    </div>
                                  )}
                                  {entry.executionTime && (
                                    <div className="text-xs text-muted-foreground">
                                      {t('devices:control.historyDetails.executionTime', { time: entry.executionTime })}
                                    </div>
                                  )}
                                </div>
                              </TableCell>
                            </TableRow>
                          )}
                        </>
                      )
                    })
                  )}
                </TableBody>
              </Table>
            </ScrollArea>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
