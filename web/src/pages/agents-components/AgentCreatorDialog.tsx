import { useState, useEffect, useCallback } from "react"
import { FullScreenBuilder, TipCard } from "@/components/automation/FullScreenBuilder"
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
  ChevronUp,
  ChevronRight,
  Clock,
  Zap,
  Bell,
  Thermometer,
  Droplets,
  Lightbulb,
  Fan,
  CheckCircle2,
  Loader2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api } from "@/lib/api"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
  AgentScheduleType,
} from "@/types"

// 指标信息（包含虚拟指标标记）
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

// 资源类型图标
const RESOURCE_ICONS: Record<string, React.ReactNode> = {
  temperature: <Thermometer className="h-4 w-4" />,
  humidity: <Droplets className="h-4 w-4" />,
  light: <Lightbulb className="h-4 w-4" />,
  switch: <Fan className="h-4 w-4" />,
  default: <Zap className="h-4 w-4" />,
}

// 选中的资源
interface SelectedResource {
  deviceId: string
  deviceName: string
  deviceType: string
  metrics: Array<{ name: string; displayName: string }>
  commands: Array<{ name: string; displayName: string }>
}

type Step = 'resources' | 'prompt' | 'schedule'

export function AgentCreatorDialog({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  onSave,
}: AgentCreatorDialogProps) {
  const isEditing = !!agent

  // 步骤状态
  const [currentStep, setCurrentStep] = useState<Step>('resources')
  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())

  // 表单状态
  const [name, setName] = useState("")
  const [userPrompt, setUserPrompt] = useState("")

  // 调度状态
  const [scheduleType, setScheduleType] = useState<AgentScheduleType>('interval')
  const [intervalSeconds, setIntervalSeconds] = useState(60)
  const [cronExpression, setCronExpression] = useState("")

  // 选中的资源
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])

  // 指标缓存（包含虚拟指标）
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [loadingMetrics, setLoadingMetrics] = useState<Record<string, boolean>>({})

  // 保存状态
  const [saving, setSaving] = useState(false)

  // 初始化
  useEffect(() => {
    if (agent) {
      setName(agent.name)
      setUserPrompt(agent.user_prompt)
      // TODO: 从 agent 解析已选资源和调度
    } else {
      setName("")
      setUserPrompt("")
      setExpandedDevices(new Set())
      setSelectedResources([])
    }
    // 清空指标缓存
    setMetricsCache({})
    setLoadingMetrics({})
  }, [agent, open])

  // 获取设备的指标（包括虚拟指标）
  const fetchDeviceMetrics = useCallback(async (deviceId: string): Promise<MetricInfo[]> => {
    // 如果已有缓存，直接返回
    if (metricsCache[deviceId]) {
      return metricsCache[deviceId]
    }

    setLoadingMetrics(prev => ({ ...prev, [deviceId]: true }))

    try {
      const device = devices.find(d => d.device_id === deviceId)
      let metrics: MetricInfo[] = []

      // 首先从设备类型定义获取指标
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

      // 然后从遥测汇总 API 获取虚拟指标
      try {
        const summary = await api.getDeviceTelemetrySummary(deviceId)
        // 添加不在列表中的指标（虚拟指标）
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
      } catch {
        // 忽略遥测 API 错误
      }

      // 缓存结果
      setMetricsCache(prev => ({ ...prev, [deviceId]: metrics }))
      return metrics
    } finally {
      setLoadingMetrics(prev => ({ ...prev, [deviceId]: false }))
    }
  }, [devices, deviceTypes, metricsCache])

  // 获取设备的指标（使用缓存）
  const getDeviceMetrics = (deviceId: string): MetricInfo[] => {
    return metricsCache[deviceId] || []
  }

  // 获取设备的图标
  const getDeviceIcon = (deviceType: string) => {
    const type = deviceTypes.find(dt => dt.device_type === deviceType)
    const category = type?.categories?.[0] || 'default'
    return RESOURCE_ICONS[category] || RESOURCE_ICONS.default
  }

  // 获取设备的指令
  const getDeviceCommands = (deviceId: string) => {
    const device = devices.find(d => d.device_id === deviceId)
    if (!device) return []
    const type = deviceTypes.find(dt => dt.device_type === device.device_type)
    return type?.commands || []
  }

  // 切换设备展开
  const toggleDeviceExpanded = async (deviceId: string) => {
    const newExpanded = new Set(expandedDevices)
    if (newExpanded.has(deviceId)) {
      newExpanded.delete(deviceId)
    } else {
      newExpanded.add(deviceId)
      // 展开时获取指标（包括虚拟指标）
      fetchDeviceMetrics(deviceId)
    }
    setExpandedDevices(newExpanded)
  }

  // 切换指标选中
  const toggleMetric = (deviceId: string, metricName: string, displayName: string) => {
    setSelectedResources(prev => {
      const existing = prev.findIndex(r => r.deviceId === deviceId)
      if (existing < 0) {
        // 新设备
        const device = devices.find(d => d.device_id === deviceId)!
        return [...prev, {
          deviceId,
          deviceName: device.name,
          deviceType: device.device_type,
          metrics: [{ name: metricName, displayName }],
          commands: []
        }]
      } else {
        // 已存在设备
        const newResources = [...prev]
        const resource = newResources[existing]
        const metricIndex = resource.metrics.findIndex(m => m.name === metricName)
        if (metricIndex >= 0) {
          // 取消选中
          resource.metrics = resource.metrics.filter(m => m.name !== metricName)
          // 如果都没有选中了，移除设备
          if (resource.metrics.length === 0 && resource.commands.length === 0) {
            newResources.splice(existing, 1)
          }
        } else {
          // 添加选中
          resource.metrics = [...resource.metrics, { name: metricName, displayName }]
        }
        return newResources
      }
    })
  }

  // 切换指令选中
  const toggleCommand = (deviceId: string, commandName: string, displayName: string) => {
    setSelectedResources(prev => {
      const existing = prev.findIndex(r => r.deviceId === deviceId)
      if (existing < 0) {
        // 新设备
        const device = devices.find(d => d.device_id === deviceId)!
        return [...prev, {
          deviceId,
          deviceName: device.name,
          deviceType: device.device_type,
          metrics: [],
          commands: [{ name: commandName, displayName }]
        }]
      } else {
        // 已存在设备
        const newResources = [...prev]
        const resource = newResources[existing]
        const commandIndex = resource.commands.findIndex(c => c.name === commandName)
        if (commandIndex >= 0) {
          // 取消选中
          resource.commands = resource.commands.filter(c => c.name !== commandName)
          // 如果都没有选中了，移除设备
          if (resource.metrics.length === 0 && resource.commands.length === 0) {
            newResources.splice(existing, 1)
          }
        } else {
          // 添加选中
          resource.commands = [...resource.commands, { name: commandName, displayName }]
        }
        return newResources
      }
    })
  }

  // 检查指标是否选中
  const isMetricSelected = (deviceId: string, metricName: string) => {
    const resource = selectedResources.find(r => r.deviceId === deviceId)
    return resource?.metrics.some(m => m.name === metricName) || false
  }

  // 检查指令是否选中
  const isCommandSelected = (deviceId: string, commandName: string) => {
    const resource = selectedResources.find(r => r.deviceId === deviceId)
    return resource?.commands.some(c => c.name === commandName) || false
  }

  // 获取设备选中统计
  const getDeviceSelectionSummary = (deviceId: string) => {
    const resource = selectedResources.find(r => r.deviceId === deviceId)
    if (!resource) return null
    const metricCount = resource.metrics.length
    const commandCount = resource.commands.length
    const parts = []
    if (metricCount > 0) parts.push(`${metricCount} 个指标`)
    if (commandCount > 0) parts.push(`${commandCount} 个指令`)
    return parts.length > 0 ? parts.join(', ') : null
  }

  // 清空设备选择
  const clearDeviceSelection = (deviceId: string) => {
    setSelectedResources(prev => prev.filter(r => r.deviceId !== deviceId))
  }

  // 保存
  const handleSave = async () => {
    if (!name.trim() || !userPrompt.trim()) {
      return
    }

    setSaving(true)
    try {
      if (isEditing && agent) {
        await onSave({
          name: name.trim(),
          user_prompt: userPrompt.trim(),
        })
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
          user_prompt: userPrompt.trim(),
          device_ids: selectedResources.map(r => r.deviceId),
          metrics: selectedResources.flatMap(r =>
            r.metrics.map(m => ({
              device_id: r.deviceId,
              metric_name: m.name,
              display_name: m.displayName
            }))
          ),
          commands: selectedResources.flatMap(r =>
            r.commands.map(c => ({
              device_id: r.deviceId,
              command_name: c.name,
              display_name: c.displayName,
              parameters: {}
            }))
          ),
          schedule: {
            schedule_type: scheduleType,
            interval_seconds: scheduleType === 'interval' ? intervalSeconds : undefined,
            cron_expression: scheduleType === 'cron' ? cronExpression : undefined,
            timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          },
        }
        await onSave(data)
      }
      onOpenChange(false)
    } catch (error) {
      console.error('Failed to save agent:', error)
    } finally {
      setSaving(false)
    }
  }

  // 渲染资源选择步骤
  const renderResourcesStep = () => (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-lg font-semibold">选择要使用的资源</h3>
          <p className="text-sm text-muted-foreground">点击展开设备，选择需要监控的指标和可执行的指令</p>
        </div>
        <Badge variant="outline" className="text-sm">
          {selectedResources.length} 个设备
        </Badge>
      </div>

      {devices.length === 0 ? (
        <div className="text-center py-12 text-muted-foreground border rounded-lg border-dashed">
          <Zap className="h-12 w-12 mx-auto mb-4 opacity-50" />
          <p>暂无可用设备</p>
          <p className="text-sm">请先添加设备后再创建智能体</p>
        </div>
      ) : (
        <div className="space-y-3">
          {devices.map((device) => {
            const isExpanded = expandedDevices.has(device.device_id)
            const selectionSummary = getDeviceSelectionSummary(device.device_id)
            const Icon = getDeviceIcon(device.device_type)

            return (
              <div key={device.device_id} className="border rounded-lg overflow-hidden">
                {/* 设备头部 */}
                <div
                  className={cn(
                    "flex items-center justify-between px-4 py-3 cursor-pointer transition-colors",
                    isExpanded ? "bg-muted/50" : "hover:bg-muted/30"
                  )}
                  onClick={() => toggleDeviceExpanded(device.device_id)}
                >
                  <div className="flex items-center gap-3">
                    <div className="flex-shrink-0 text-muted-foreground">
                      {Icon}
                    </div>
                    <div>
                      <div className="font-medium">{device.name}</div>
                      <div className="text-xs text-muted-foreground">
                        {device.device_type}
                        {selectionSummary && (
                          <span className="ml-2 text-primary">· 已选: {selectionSummary}</span>
                        )}
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    {selectionSummary && (
                      <CheckCircle2 className="h-4 w-4 text-primary" />
                    )}
                    {isExpanded ? (
                      <ChevronUp className="h-4 w-4 text-muted-foreground" />
                    ) : (
                      <ChevronDown className="h-4 w-4 text-muted-foreground" />
                    )}
                  </div>
                </div>

                {/* 展开的资源选择 */}
                {isExpanded && (
                  <div className="border-t bg-muted/20 p-4 space-y-4">
                    {/* 指标 */}
                    {(() => {
                      const metrics = getDeviceMetrics(device.device_id)
                      const isLoading = loadingMetrics[device.device_id]

                      // 加载状态
                      if (isLoading) {
                        return (
                          <div className="flex items-center justify-center py-6 text-muted-foreground">
                            <Loader2 className="h-4 w-4 animate-spin mr-2" />
                            <span className="text-xs">加载指标中...</span>
                          </div>
                        )
                      }

                      // 有指标时显示
                      if (metrics.length > 0) {
                        const virtualMetrics = metrics.filter(m => m.is_virtual)

                        return (
                          <div>
                            <div className="flex items-center justify-between mb-2">
                              <span className="text-sm font-medium flex items-center gap-1">
                                <Thermometer className="h-3.5 w-3.5" />
                                监控指标
                                {virtualMetrics.length > 0 && (
                                  <Badge variant="secondary" className="ml-2 text-xs">
                                    {virtualMetrics.length} 虚拟
                                  </Badge>
                                )}
                              </span>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-6 text-xs"
                                onClick={(e) => {
                                  e.stopPropagation()
                                  // 全选指标
                                  metrics.forEach(m => toggleMetric(device.device_id, m.name, m.display_name))
                                }}
                              >
                                全选
                              </Button>
                            </div>
                            <div className="flex flex-wrap gap-2">
                              {metrics.map((metric) => (
                                <button
                                  key={metric.name}
                                  onClick={() => toggleMetric(device.device_id, metric.name, metric.display_name)}
                                  className={cn(
                                    "text-xs px-3 py-1.5 rounded-full border transition-all relative",
                                    isMetricSelected(device.device_id, metric.name)
                                      ? "bg-primary text-primary-foreground border-primary shadow-sm"
                                      : "bg-background hover:bg-muted border-border"
                                  )}
                                >
                                  <span className="flex items-center gap-1">
                                    {metric.display_name}
                                    {metric.is_virtual && (
                                      <Badge variant="outline" className="h-3 px-1 text-[10px] border-primary/50">
                                        虚拟
                                      </Badge>
                                    )}
                                  </span>
                                </button>
                              ))}
                            </div>
                          </div>
                        )
                      }

                      // 无指标时显示提示
                      return (
                        <div className="text-center py-4 text-muted-foreground">
                          <p className="text-xs">暂无可用的指标</p>
                          <p className="text-xs text-muted-foreground mt-1">
                            设备可能未上报数据，或未创建数据转换
                          </p>
                        </div>
                      )
                    })()}

                    {/* 指令 */}
                    {(() => {
                      const commands = getDeviceCommands(device.device_id)
                      return commands.length > 0 ? (
                        <div>
                          <div className="flex items-center justify-between mb-2">
                            <span className="text-sm font-medium flex items-center gap-1">
                              <Zap className="h-3.5 w-3.5" />
                              可执行指令
                            </span>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 text-xs"
                              onClick={(e) => {
                                e.stopPropagation()
                                // 全选指令
                                commands.forEach(c => toggleCommand(device.device_id, c.name, c.display_name))
                              }}
                            >
                              全选
                            </Button>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            {commands.map((command) => (
                              <button
                                key={command.name}
                                onClick={() => toggleCommand(device.device_id, command.name, command.display_name)}
                                className={cn(
                                  "text-xs px-3 py-1.5 rounded-full border transition-all",
                                  isCommandSelected(device.device_id, command.name)
                                    ? "bg-primary text-primary-foreground border-primary shadow-sm"
                                    : "bg-background hover:bg-muted border-border"
                                )}
                              >
                                {command.display_name}
                              </button>
                            ))}
                          </div>
                        </div>
                      ) : null
                    })()}

                    {/* 清空按钮 */}
                    {selectionSummary && (
                      <div className="flex justify-end pt-2 border-t">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-xs text-destructive"
                          onClick={(e) => {
                            e.stopPropagation()
                            clearDeviceSelection(device.device_id)
                          }}
                        >
                          清空选择
                        </Button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )

  // 渲染需求编写步骤
  const renderPromptStep = () => (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold mb-2">描述你的需求</h3>
        <p className="text-sm text-muted-foreground mb-4">
          用自然语言告诉智能体它需要做什么
        </p>
      </div>

      {/* 已选资源提示 */}
      {selectedResources.length > 0 && (
        <div className="bg-muted/50 rounded-lg p-4">
          <div className="text-sm font-medium mb-3 flex items-center gap-2">
            <CheckCircle2 className="h-4 w-4 text-primary" />
            已选择的资源
          </div>
          <div className="space-y-2">
            {selectedResources.map((resource) => (
              <div key={resource.deviceId} className="flex items-start gap-2 text-sm">
                <span className="font-medium">{resource.deviceName}:</span>
                <span className="text-muted-foreground">
                  {[
                    ...resource.metrics.map(m => m.displayName),
                    ...resource.commands.map(c => c.displayName)
                  ].join('、')}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 名称输入 */}
      <div className="space-y-2">
        <Label htmlFor="name">智能体名称 *</Label>
        <Input
          id="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="例如: 温度监控智能体"
        />
      </div>

      {/* 需求描述 */}
      <div className="space-y-2">
        <Label htmlFor="prompt">需求描述 *</Label>
        <Textarea
          id="prompt"
          value={userPrompt}
          onChange={(e) => setUserPrompt(e.target.value)}
          placeholder="描述这个智能体需要做什么...&#10;&#10;例如：当温度超过30度时，打开空调；当温度低于25度时，关闭空调"
          rows={6}
          className="resize-none"
        />
        <p className="text-xs text-muted-foreground">
          提示：智能体会根据你选择的资源和描述来执行任务。描述越清晰，执行效果越好。
        </p>
      </div>
    </div>
  )

  // 渲染调度配置步骤
  const renderScheduleStep = () => (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold mb-2">配置调度</h3>
        <p className="text-sm text-muted-foreground">
          设置智能体的执行频率
        </p>
      </div>

      <div className="space-y-4">
        <div>
          <Label>调度类型</Label>
          <Select
            value={scheduleType}
            onValueChange={(v) => setScheduleType(v as AgentScheduleType)}
          >
            <SelectTrigger className="mt-2">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="interval">
                <div className="flex items-center gap-2">
                  <Clock className="h-4 w-4" />
                  <div>
                    <div className="font-medium">定时间隔</div>
                    <div className="text-xs text-muted-foreground">按固定间隔执行</div>
                  </div>
                </div>
              </SelectItem>
              <SelectItem value="cron">
                <div className="flex items-center gap-2">
                  <Clock className="h-4 w-4" />
                  <div>
                    <div className="font-medium">Cron 表达式</div>
                    <div className="text-xs text-muted-foreground">使用 Cron 高级调度</div>
                  </div>
                </div>
              </SelectItem>
              <SelectItem value="event">
                <div className="flex items-center gap-2">
                  <Bell className="h-4 w-4" />
                  <div>
                    <div className="font-medium">事件触发</div>
                    <div className="text-xs text-muted-foreground">当设备状态变化时执行</div>
                  </div>
                </div>
              </SelectItem>
              <SelectItem value="once">
                <div className="flex items-center gap-2">
                  <Zap className="h-4 w-4" />
                  <div>
                    <div className="font-medium">单次执行</div>
                    <div className="text-xs text-muted-foreground">执行一次后停止</div>
                  </div>
                </div>
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        {scheduleType === 'interval' && (
          <div className="space-y-2">
            <Label>执行间隔（秒）</Label>
            <Input
              type="number"
              value={intervalSeconds}
              onChange={(e) => setIntervalSeconds(Number(e.target.value))}
              min={10}
              step={10}
            />
            <p className="text-xs text-muted-foreground">
              智能体将每 {intervalSeconds} 秒执行一次
            </p>
          </div>
        )}

        {scheduleType === 'cron' && (
          <div className="space-y-2">
            <Label>Cron 表达式</Label>
            <Input
              value={cronExpression}
              onChange={(e) => setCronExpression(e.target.value)}
              placeholder="0 * * * *"
            />
            <p className="text-xs text-muted-foreground">
              格式：分钟 小时 日 月 星期。例如：0 * * * * 表示每小时执行
            </p>
          </div>
        )}
      </div>
    </div>
  )

  // 侧边栏内容
  const sidePanelContent = (
    <div className="space-y-4">
      <TipCard
        title="创建提示"
        icon={<Bot className="h-4 w-4" />}
      >
        <div className="space-y-2 text-xs">
          <p>1. 选择需要监控的设备和指标</p>
          <p>2. 选择可以执行的指令</p>
          <p>3. 描述智能体的任务</p>
          <p>4. 设置执行频率</p>
        </div>
      </TipCard>

      {selectedResources.length > 0 && (
        <TipCard
          title="已选资源"
          icon={<CheckCircle2 className="h-4 w-4" />}
          variant="success"
        >
          <div className="space-y-2 text-xs">
            {selectedResources.map((r) => (
              <div key={r.deviceId} className="font-medium">
                {r.deviceName}
                <div className="text-[10px] opacity-80 normal">
                  {r.metrics.length} 指标 · {r.commands.length} 指令
                </div>
              </div>
            ))}
          </div>
        </TipCard>
      )}
    </div>
  )

  // 验证状态
  const isValid = name.trim() !== "" && userPrompt.trim() !== ""
  const hasResources = selectedResources.length > 0

  return (
    <FullScreenBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={isEditing ? "编辑智能体" : "创建智能体"}
      description={isEditing ? "修改智能体配置" : "创建一个新的 AI 智能体"}
      icon={<Bot className="h-5 w-5 text-primary" />}
      fullWidth={true}
      isValid={isValid}
      isDirty={true}
      isSaving={saving}
      saveLabel={isEditing ? "保存" : "创建"}
      onSave={handleSave}
      validationMessage={
        currentStep === 'resources' && !hasResources
          ? "请至少选择一个设备资源"
          : !name.trim()
          ? "请输入智能体名称"
          : !userPrompt.trim()
          ? "请描述你的需求"
          : ""
      }
      sidePanel={{
        content: sidePanelContent,
        title: "创建指南"
      }}
      footerLeftActions={
        <div className="flex items-center gap-2">
          {currentStep !== 'resources' && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                if (currentStep === 'schedule') setCurrentStep('prompt')
                else if (currentStep === 'prompt') setCurrentStep('resources')
              }}
            >
              上一步
            </Button>
          )}
          <div className="flex items-center gap-1 text-sm">
            <div className={cn(
              "w-6 h-6 rounded-full flex items-center justify-center text-xs",
              currentStep === 'resources' ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"
            )}>1</div>
            <ChevronRight className="h-3 w-3 text-muted-foreground" />
            <div className={cn(
              "w-6 h-6 rounded-full flex items-center justify-center text-xs",
              currentStep === 'prompt' ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"
            )}>2</div>
            <ChevronRight className="h-3 w-3 text-muted-foreground" />
            <div className={cn(
              "w-6 h-6 rounded-full flex items-center justify-center text-xs",
              currentStep === 'schedule' ? "bg-primary text-primary-foreground" : "bg-muted text-muted-foreground"
            )}>3</div>
          </div>
          {currentStep === 'resources' && (
            <span className="text-xs text-muted-foreground ml-2">选择资源</span>
          )}
          {currentStep === 'prompt' && (
            <span className="text-xs text-muted-foreground ml-2">编写需求</span>
          )}
          {currentStep === 'schedule' && (
            <span className="text-xs text-muted-foreground ml-2">配置调度</span>
          )}
        </div>
      }
    >
      {currentStep === 'resources' && renderResourcesStep()}
      {currentStep === 'prompt' && renderPromptStep()}
      {currentStep === 'schedule' && renderScheduleStep()}

      {/* 步骤导航按钮 */}
      <div className="flex justify-end mt-6">
        {currentStep === 'resources' && (
          <Button
            onClick={() => setCurrentStep('prompt')}
            disabled={!hasResources}
          >
            下一步
          </Button>
        )}
        {currentStep === 'prompt' && (
          <Button
            onClick={() => setCurrentStep('schedule')}
            disabled={!name.trim() || !userPrompt.trim()}
          >
            下一步
          </Button>
        )}
      </div>
    </FullScreenBuilder>
  )
}
