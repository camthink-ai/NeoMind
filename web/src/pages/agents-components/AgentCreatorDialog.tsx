import { useState, useEffect, useCallback } from "react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Bot,
  ChevronDown,
  Zap,
  Thermometer,
  Droplets,
  Lightbulb,
  Fan,
  Check,
  Loader2,
  Save,
  AlertTriangle,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
  AgentScheduleType,
} from "@/types"

// 指标信息
interface MetricInfo {
  name: string
  display_name: string
  unit?: string
  is_virtual?: boolean
}

interface AgentCreatorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agent: AiAgentDetail | undefined
  devices: Device[]
  deviceTypes: DeviceType[]
  onSave: (data: CreateAgentRequest | Partial<AiAgentDetail>) => Promise<void>
}

interface SelectedResource {
  deviceId: string
  deviceName: string
  deviceType: string
  metrics: Array<{ name: string; displayName: string }>
  commands: Array<{ name: string; displayName: string }>
}

// 资源图标
const RESOURCE_ICONS: Record<string, React.ReactNode> = {
  temperature: <Thermometer className="h-4 w-4" />,
  humidity: <Droplets className="h-4 w-4" />,
  light: <Lightbulb className="h-4 w-4" />,
  switch: <Fan className="h-4 w-4" />,
  default: <Zap className="h-4 w-4" />,
}

export function AgentCreatorDialog({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  onSave,
}: AgentCreatorDialogProps) {
  const { toast } = useToast()
  const isEditing = !!agent

  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())
  const [name, setName] = useState("")
  const [userPrompt, setUserPrompt] = useState("")
  const [scheduleType, setScheduleType] = useState<AgentScheduleType>('interval')
  const [intervalSeconds, setIntervalSeconds] = useState(300)
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [loadingMetrics, setLoadingMetrics] = useState<Record<string, boolean>>({})
  const [saving, setSaving] = useState(false)
  const [validationErrors, setValidationErrors] = useState<string[]>([])

  useEffect(() => {
    if (agent) {
      setName(agent.name)
      setUserPrompt(agent.user_prompt)
      // Populate schedule info
      if (agent.schedule) {
        setScheduleType(agent.schedule.schedule_type as AgentScheduleType)
        if (agent.schedule.interval_seconds) {
          setIntervalSeconds(agent.schedule.interval_seconds)
        }
      }
      // Populate selected resources from agent resources
      if (agent.resources && agent.resources.length > 0) {
        const resourcesByDevice: Record<string, SelectedResource> = {}

        for (const resource of agent.resources) {
          // Parse resource_id which is in format "device_id:metric_name" or "device_id:command_name"
          const parts = resource.resource_id.split(':')
          if (parts.length < 2) continue

          const deviceId = parts[0]
          const resourceName = parts.slice(1).join(':')

          // Find device info
          const device = devices.find(d => d.device_id === deviceId)
          if (!device) continue

          // Initialize device entry if not exists
          if (!resourcesByDevice[deviceId]) {
            resourcesByDevice[deviceId] = {
              deviceId,
              deviceName: device.name,
              deviceType: device.device_type,
              metrics: [],
              commands: []
            }
          }

          // Add to metrics or commands based on resource type
          if (resource.resource_type === 'Metric') {
            resourcesByDevice[deviceId].metrics.push({
              name: resourceName,
              displayName: resource.name
            })
          } else if (resource.resource_type === 'Command') {
            resourcesByDevice[deviceId].commands.push({
              name: resourceName,
              displayName: resource.name
            })
          }
        }

        setSelectedResources(Object.values(resourcesByDevice))
        // Expand all devices that have resources
        setExpandedDevices(new Set(Object.keys(resourcesByDevice)))
      } else {
        setSelectedResources([])
        setExpandedDevices(new Set())
      }
    } else {
      setName("")
      setUserPrompt("")
      setScheduleType('interval')
      setIntervalSeconds(300)
      setExpandedDevices(new Set())
      setSelectedResources([])
    }
    setMetricsCache({})
    setLoadingMetrics({})
    setValidationErrors([])
  }, [agent, open, devices])

  const fetchDeviceMetrics = useCallback(async (deviceId: string): Promise<MetricInfo[]> => {
    if (metricsCache[deviceId]) return metricsCache[deviceId]
    setLoadingMetrics(prev => ({ ...prev, [deviceId]: true }))
    try {
      const device = devices.find(d => d.device_id === deviceId)
      let metrics: MetricInfo[] = []
      if (device?.device_type) {
        const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)
        if (deviceType?.metrics) {
          metrics = deviceType.metrics.map(m => ({
            name: m.name,
            display_name: m.display_name || m.name,
            unit: m.unit,
            is_virtual: false,
          }))
        }
      }
      try {
        const summary = await api.getDeviceTelemetrySummary(deviceId)
        for (const [metricName, metricData] of Object.entries(summary.summary)) {
          if (!metrics.find(m => m.name === metricName)) {
            metrics.push({
              name: metricName,
              display_name: metricData.display_name || metricName,
              unit: metricData.unit,
              is_virtual: metricData.is_virtual,
            })
          }
        }
      } catch { /* ignore */ }
      setMetricsCache(prev => ({ ...prev, [deviceId]: metrics }))
      return metrics
    } finally {
      setLoadingMetrics(prev => ({ ...prev, [deviceId]: false }))
    }
  }, [devices, deviceTypes, metricsCache])

  const getDeviceMetrics = (deviceId: string): MetricInfo[] => metricsCache[deviceId] || []
  const getDeviceIcon = (deviceType: string) => {
    const type = deviceTypes.find(dt => dt.device_type === deviceType)
    const category = type?.categories?.[0] || 'default'
    return RESOURCE_ICONS[category] || RESOURCE_ICONS.default
  }
  const getDeviceCommands = (deviceId: string) => {
    const device = devices.find(d => d.device_id === deviceId)
    if (!device) return []
    const type = deviceTypes.find(dt => dt.device_type === device.device_type)
    return type?.commands || []
  }

  const toggleDeviceExpanded = async (deviceId: string) => {
    const newExpanded = new Set(expandedDevices)
    if (newExpanded.has(deviceId)) {
      newExpanded.delete(deviceId)
    } else {
      newExpanded.add(deviceId)
      fetchDeviceMetrics(deviceId)
    }
    setExpandedDevices(newExpanded)
  }

  const toggleMetric = (deviceId: string, metricName: string, displayName: string) => {
    setSelectedResources(prev => {
      const existing = prev.findIndex(r => r.deviceId === deviceId)
      if (existing < 0) {
        const device = devices.find(d => d.device_id === deviceId)!
        return [...prev, {
          deviceId,
          deviceName: device.name,
          deviceType: device.device_type,
          metrics: [{ name: metricName, displayName }],
          commands: []
        }]
      }
      const newResources = [...prev]
      const resource = newResources[existing]
      const metricIndex = resource.metrics.findIndex(m => m.name === metricName)
      if (metricIndex >= 0) {
        resource.metrics = resource.metrics.filter(m => m.name !== metricName)
        if (resource.metrics.length === 0 && resource.commands.length === 0) {
          newResources.splice(existing, 1)
        }
      } else {
        resource.metrics = [...resource.metrics, { name: metricName, displayName }]
      }
      return newResources
    })
  }

  const toggleCommand = (deviceId: string, commandName: string, displayName: string) => {
    setSelectedResources(prev => {
      const existing = prev.findIndex(r => r.deviceId === deviceId)
      if (existing < 0) {
        const device = devices.find(d => d.device_id === deviceId)!
        return [...prev, {
          deviceId,
          deviceName: device.name,
          deviceType: device.device_type,
          metrics: [],
          commands: [{ name: commandName, displayName }]
        }]
      }
      const newResources = [...prev]
      const resource = newResources[existing]
      const commandIndex = resource.commands.findIndex(c => c.name === commandName)
      if (commandIndex >= 0) {
        resource.commands = resource.commands.filter(c => c.name !== commandName)
        if (resource.metrics.length === 0 && resource.commands.length === 0) {
          newResources.splice(existing, 1)
        }
      } else {
        resource.commands = [...resource.commands, { name: commandName, displayName }]
      }
      return newResources
    })
  }

  const isMetricSelected = (deviceId: string, metricName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.metrics.some(m => m.name === metricName) || false
  }

  const isCommandSelected = (deviceId: string, commandName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.commands.some(c => c.name === commandName) || false
  }

  const handleSave = async () => {
    // Clear and validate
    const errors: string[] = []

    if (!name.trim()) {
      errors.push("请输入智能体名称")
    }
    if (!userPrompt.trim()) {
      errors.push("请描述你的需求")
    }
    if (selectedResources.length === 0) {
      errors.push("请至少选择一个设备资源")
    }

    setValidationErrors(errors)
    if (errors.length > 0) {
      return
    }

    setSaving(true)
    try {
      if (isEditing && agent) {
        await onSave({ name: name.trim(), user_prompt: userPrompt.trim() })
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
          user_prompt: userPrompt.trim(),
          device_ids: selectedResources.map(r => r.deviceId),
          metrics: selectedResources.flatMap(r =>
            r.metrics.map(m => ({ device_id: r.deviceId, metric_name: m.name, display_name: m.displayName }))
          ),
          commands: selectedResources.flatMap(r =>
            r.commands.map(c => ({ device_id: r.deviceId, command_name: c.name, display_name: c.displayName, parameters: {} }))
          ),
          schedule: {
            schedule_type: scheduleType,
            interval_seconds: scheduleType === 'interval' ? intervalSeconds : undefined,
            cron_expression: scheduleType === 'cron' ? '' : undefined,
            timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          },
        }
        await onSave(data)
      }
      onOpenChange(false)
      toast({ title: "成功", description: isEditing ? "智能体已更新" : "智能体已创建" })
    } catch (error) {
      toast({ title: "失败", description: (error as Error).message, variant: "destructive" })
    } finally {
      setSaving(false)
    }
  }

  const selectedCount = selectedResources.reduce((acc, r) => acc + r.metrics.length + r.commands.length, 0)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl max-h-[92vh] p-0 flex flex-col">
        {/* Header */}
        <DialogHeader className="px-6 py-4 border-b">
          <div className="flex items-center gap-3">
            <Bot className="h-5 w-5 text-purple-500" />
            <div>
              <DialogTitle className="text-lg font-semibold">
                {isEditing ? "编辑 AI 智能体" : "创建 AI 智能体"}
              </DialogTitle>
              <DialogDescription className="text-sm">
                告诉 AI 你想要实现什么，它会自动执行
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        {/* Form Section - Basic Info */}
        <div className="border-b px-6 py-4 bg-muted/20 flex-shrink-0">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <Label htmlFor="agent-name" className="text-xs font-medium text-foreground/70">智能体名称 *</Label>
              <Input
                id="agent-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="例如：温度监控助手"
                className="mt-1.5 h-10 bg-background"
              />
            </div>
            <div>
              <Label htmlFor="schedule-type" className="text-xs font-medium text-foreground/70">执行频率</Label>
              <div className="flex gap-2 mt-1.5">
                <Select value={scheduleType} onValueChange={(v) => setScheduleType(v as AgentScheduleType)}>
                  <SelectTrigger className="h-10 bg-background flex-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="interval">定时间隔</SelectItem>
                    <SelectItem value="cron">Cron</SelectItem>
                    <SelectItem value="event">事件触发</SelectItem>
                  </SelectContent>
                </Select>
                {scheduleType === 'interval' && (
                  <Input
                    type="number"
                    value={intervalSeconds}
                    onChange={(e) => setIntervalSeconds(Number(e.target.value))}
                    min={10}
                    step={60}
                    className="h-10 w-24 bg-background"
                    placeholder="秒"
                  />
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Description Section */}
        <div className="border-b px-6 py-4 flex-shrink-0">
          <Label htmlFor="user-prompt" className="text-xs font-medium">描述你想实现的功能 *</Label>
          <Textarea
            id="user-prompt"
            value={userPrompt}
            onChange={(e) => setUserPrompt(e.target.value)}
            placeholder="例如：&#10;&#10;• 当客厅温度超过 30°C 时，打开空调&#10;• 当温度低于 26°C 时，关闭空调&#10;• 主要在夜间（18:00-08:00）执行"
            rows={4}
            className="mt-1.5 resize-y min-h-[80px]"
          />
        </div>

        {/* Validation Errors */}
        {validationErrors.length > 0 && (
          <div className="px-6 py-3 bg-destructive/10 border-b border-destructive/20">
            <div className="flex items-center gap-2 text-destructive font-medium text-sm mb-2">
              <AlertTriangle className="h-4 w-4" />
              <span>请修复以下错误</span>
            </div>
            <ul className="text-sm text-destructive space-y-1 list-disc list-inside">
              {validationErrors.map((error, i) => (
                <li key={i}>{error}</li>
              ))}
            </ul>
          </div>
        )}

        {/* Main Content - Device Resources */}
        <div className="flex-1 min-h-0 overflow-y-auto p-5">
          <div className="flex items-center gap-2 mb-4">
            <div className="p-1.5 bg-amber-500/20 rounded-md">
              <Zap className="h-4 w-4 text-amber-500" />
            </div>
            <h3 className="font-semibold text-sm">选择设备资源</h3>
            {selectedCount > 0 && (
              <Badge className="ml-auto bg-amber-500 text-white border-0">
                {selectedCount} 已选择
              </Badge>
            )}
          </div>

          {devices.length === 0 ? (
            <div className="border-2 border-dashed border-muted-foreground/25 rounded-xl p-8 text-center">
              <Zap className="h-10 w-10 mx-auto mb-4 text-muted-foreground/50" />
              <p className="text-sm text-muted-foreground">暂无可用设备</p>
            </div>
          ) : (
            <div className="space-y-2">
              {devices.map((device) => {
                const isExpanded = expandedDevices.has(device.device_id)
                const Icon = getDeviceIcon(device.device_type)
                const hasSelection = selectedResources.find(r => r.deviceId === device.device_id)
                const metricCount = selectedResources.find(r => r.deviceId === device.device_id)?.metrics.length || 0
                const commandCount = selectedResources.find(r => r.deviceId === device.device_id)?.commands.length || 0

                return (
                  <div
                    key={device.device_id}
                    className={cn(
                      "border rounded-lg overflow-hidden transition-colors",
                      hasSelection ? "border-amber-500/30 bg-amber-500/5" : "border-border"
                    )}
                  >
                    {/* Device Header */}
                    <div
                      className="p-3 cursor-pointer hover:bg-muted/50 transition-colors"
                      onClick={() => toggleDeviceExpanded(device.device_id)}
                    >
                      <div className="flex items-center gap-3">
                        <div className={cn(
                          "p-1.5 rounded-lg transition-colors",
                          hasSelection ? "bg-amber-500 text-white" : "bg-muted"
                        )}>
                          {Icon}
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="text-sm font-medium truncate">{device.name}</div>
                          <div className="text-xs text-muted-foreground truncate">{device.device_type}</div>
                        </div>
                        <div className="flex items-center gap-2">
                          {(metricCount > 0 || commandCount > 0) && (
                            <div className="flex gap-1">
                              {metricCount > 0 && (
                                <Badge variant="outline" className="text-xs bg-blue-500/10 text-blue-600 border-blue-500/30">
                                  数据 {metricCount}
                                </Badge>
                              )}
                              {commandCount > 0 && (
                                <Badge variant="outline" className="text-xs bg-green-500/10 text-green-600 border-green-500/30">
                                  指令 {commandCount}
                                </Badge>
                              )}
                            </div>
                          )}
                          <ChevronDown className={cn(
                            "h-4 w-4 text-muted-foreground transition-transform",
                            isExpanded && "rotate-180"
                          )} />
                        </div>
                      </div>
                    </div>

                    {/* Expanded Resources */}
                    {isExpanded && (
                      <div className="border-t border-border p-3 bg-muted/20 space-y-3">
                        {(() => {
                          const metrics = getDeviceMetrics(device.device_id)
                          const isLoading = loadingMetrics[device.device_id]
                          if (isLoading) {
                            return (
                              <div className="flex items-center justify-center py-4">
                                <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                              </div>
                            )
                          }
                          return (
                            <>
                              {metrics.length > 0 && (
                                <div>
                                  <div className="text-xs text-muted-foreground mb-2">监控指标</div>
                                  <div className="flex flex-wrap gap-1.5">
                                    {metrics.map((metric) => (
                                      <button
                                        key={metric.name}
                                        onClick={() => toggleMetric(device.device_id, metric.name, metric.display_name)}
                                        className={cn(
                                          "text-xs px-2.5 py-1 rounded-md border transition-all",
                                          isMetricSelected(device.device_id, metric.name)
                                            ? "bg-blue-500 text-white border-blue-500"
                                            : "bg-background hover:bg-muted border-border"
                                        )}
                                      >
                                        {isMetricSelected(device.device_id, metric.name) && <Check className="h-3 w-3 mr-1 inline" />}
                                        {metric.display_name}
                                      </button>
                                    ))}
                                  </div>
                                </div>
                              )}

                              {(() => {
                                const commands = getDeviceCommands(device.device_id)
                                return commands.length > 0 ? (
                                  <div>
                                    <div className="text-xs text-muted-foreground mb-2">可执行指令</div>
                                    <div className="flex flex-wrap gap-1.5">
                                      {commands.map((command) => (
                                        <button
                                          key={command.name}
                                          onClick={() => toggleCommand(device.device_id, command.name, command.display_name)}
                                          className={cn(
                                            "text-xs px-2.5 py-1 rounded-md border transition-all",
                                            isCommandSelected(device.device_id, command.name)
                                              ? "bg-green-500 text-white border-green-500"
                                              : "bg-background hover:bg-muted border-border"
                                          )}
                                        >
                                          {isCommandSelected(device.device_id, command.name) && <Check className="h-3 w-3 mr-1 inline" />}
                                          {command.display_name}
                                        </button>
                                      ))}
                                    </div>
                                  </div>
                                ) : null
                              })()}
                            </>
                          )
                        })()}
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t bg-muted/20 flex-shrink-0">
          <div className="flex gap-2 ml-auto">
            <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
              取消
            </Button>
            <Button onClick={handleSave} disabled={saving} className="min-w-[100px]">
              <Save className="h-4 w-4 mr-1.5" />
              {saving ? "保存中..." : isEditing ? "保存" : "创建"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
