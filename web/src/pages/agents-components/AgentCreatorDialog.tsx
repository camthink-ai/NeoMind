import { useState, useEffect, useCallback } from "react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Thermometer,
  Droplets,
  Lightbulb,
  Fan,
  Loader2,
  Save,
  Eye,
  Target,
  BarChart3,
  ChevronDown,
  Search,
  Clock,
  Zap,
  Bell,
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
  AgentRole,
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

// 角色配置
const ROLES: Array<{ value: AgentRole; label: string; icon: React.ReactNode; description: string; color: string }> = [
  {
    value: 'Monitor',
    label: '监控专员',
    icon: <Eye className="h-4 w-4" />,
    description: '监控设备状态，检测异常',
    color: 'text-blue-600'
  },
  {
    value: 'Executor',
    label: '执行专员',
    icon: <Fan className="h-4 w-4" />,
    description: '自动控制设备',
    color: 'text-orange-600'
  },
  {
    value: 'Analyst',
    label: '分析专员',
    icon: <BarChart3 className="h-4 w-4" />,
    description: '分析数据趋势',
    color: 'text-purple-600'
  },
]

// 资源图标
const RESOURCE_ICONS: Record<string, React.ReactNode> = {
  temperature: <Thermometer className="h-4 w-4" />,
  humidity: <Droplets className="h-4 w-4" />,
  light: <Lightbulb className="h-4 w-4" />,
  switch: <Fan className="h-4 w-4" />,
  default: <Target className="h-4 w-4" />,
}

// 间隔选项（分钟）
const INTERVAL_MINUTES = [1, 5, 10, 15, 30, 60]

// 间隔选项（小时）
const INTERVAL_HOURS = [1, 2, 3, 6, 12, 24]

// 星期选项
const WEEKDAYS = [
  { value: 1, label: '周一' },
  { value: 2, label: '周二' },
  { value: 3, label: '周三' },
  { value: 4, label: '周四' },
  { value: 5, label: '周五' },
  { value: 6, label: '周六' },
  { value: 0, label: '周日' },
]

// 生成小时选项
const HOURS = Array.from({ length: 24 }, (_, i) => ({
  value: i,
  label: i.toString().padStart(2, '0')
}))

// 生成分钟选项
const MINUTES = Array.from({ length: 60 }, (_, i) => ({
  value: i,
  label: i.toString().padStart(2, '0')
}))

// 事件类型
const EVENT_TYPES = [
  { value: 'device.online', label: '设备上线', icon: <Zap className="h-3 w-3" /> },
  { value: 'device.offline', label: '设备离线', icon: <Target className="h-3 w-3" /> },
  { value: 'metric.threshold', label: '指标阈值', icon: <Bell className="h-3 w-3" /> },
  { value: 'metric.anomaly', label: '异常检测', icon: <Eye className="h-3 w-3" /> },
  { value: 'manual', label: '手动触发', icon: <Clock className="h-3 w-3" /> },
]

// 拓扑图组件 - AI Agent 为中心的聚合拓扑（无图标）
function ResourceTopology({
  selectedResources,
  role,
}: {
  selectedResources: SelectedResource[]
  role: AgentRole
}) {
  const hasSelection = selectedResources.length > 0

  if (!hasSelection) {
    return (
      <div className="h-24 flex items-center justify-center text-muted-foreground text-xs border rounded-lg bg-muted/10">
        选择设备资源后显示拓扑关系
      </div>
    )
  }

  // 统计资源
  const totalMetrics = selectedResources.reduce((sum, r) => sum + r.metrics.length, 0)
  const totalCommands = selectedResources.reduce((sum, r) => sum + r.commands.length, 0)

  const isMonitor = role === 'Monitor'
  const isExecutor = role === 'Executor'
  const primaryColor = isMonitor ? '#3b82f6' : isExecutor ? '#f97316' : '#a855f7'
  const bgColor = isMonitor ? 'bg-blue-50' : isExecutor ? 'bg-orange-50' : 'bg-purple-50'
  const borderColor = isMonitor ? 'border-blue-200' : isExecutor ? 'border-orange-200' : 'border-purple-200'

  return (
    <div className={cn("border rounded-lg p-3", borderColor, bgColor)}>
      {/* 简化的聚合视图 - 以文本和颜色区分，无图标 */}
      <div className="flex items-center justify-center gap-4 py-2">
        {/* AI Agent 节点 */}
        <div className="flex flex-col items-center">
          <div
            className="w-20 h-20 rounded-full flex flex-col items-center justify-center text-white shadow-lg"
            style={{ background: primaryColor }}
          >
            <span className="text-xs font-medium">AI Agent</span>
            <span className="text-[10px] opacity-80 mt-0.5">
              {isMonitor ? '监控' : isExecutor ? '执行' : '分析'}
            </span>
          </div>
        </div>

        {/* 连接线 */}
        <div className="flex-1 flex items-center justify-center px-4">
          <div className="h-0.5 flex-1 bg-gradient-to-r from-transparent via-current to-current opacity-30" />
          <div className="flex gap-2 px-2">
            <div className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-blue-100 text-blue-700 text-[10px]">
              {totalMetrics} 指标
            </div>
            <div className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-orange-100 text-orange-700 text-[10px]">
              {totalCommands} 指令
            </div>
          </div>
          <div className="h-0.5 flex-1 bg-gradient-to-l from-transparent via-current to-current opacity-30" />
        </div>

        {/* 设备节点聚合 */}
        <div className="flex flex-col items-center">
          <div className="flex items-center gap-1 flex-wrap max-w-xs justify-end">
            {selectedResources.map((resource) => {
              const metricCount = resource.metrics.length
              const commandCount = resource.commands.length
              return (
                <div
                  key={resource.deviceId}
                  className="px-2 py-1 rounded-md bg-white border border-gray-200 shadow-sm text-xs"
                >
                  <div className="font-medium text-gray-700 truncate max-w-[80px]">
                    {resource.deviceName}
                  </div>
                  <div className="flex gap-1 mt-0.5">
                    {metricCount > 0 && (
                      <span className="text-[10px] text-blue-600">
                        M{metricCount}
                      </span>
                    )}
                    {commandCount > 0 && (
                      <span className="text-[10px] text-orange-600">
                        C{commandCount}
                      </span>
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </div>

      {/* 图例 */}
      <div className="flex items-center justify-center gap-4 mt-2 text-[10px] text-muted-foreground">
        <div className="flex items-center gap-1">
          <span className="font-medium">AI Agent</span>
          <span>→</span>
          <span>设备资源</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="w-2 h-2 rounded bg-blue-500" />
          <span>M = 指标</span>
        </div>
        <div className="flex items-center gap-1">
          <span className="w-2 h-2 rounded bg-orange-500" />
          <span>C = 指令</span>
        </div>
      </div>
    </div>
  )
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

  // State
  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())
  const [name, setName] = useState("")
  const [role, setRole] = useState<AgentRole>('Monitor')
  const [userPrompt, setUserPrompt] = useState("")
  // 执行策略: interval=间隔, daily=每天, weekly=每周, monthly=每月, event=事件
  const [scheduleType, setScheduleType] = useState<'interval' | 'daily' | 'weekly' | 'monthly' | 'event'>('interval')
  const [intervalValue, setIntervalValue] = useState(5) // 间隔数值
  const [intervalUnit, setIntervalUnit] = useState<'minute' | 'hour'>('minute') // 间隔单位
  const [scheduleHour, setScheduleHour] = useState(9) // 执行时间-小时
  const [scheduleMinute, setScheduleMinute] = useState(0) // 执行时间-分钟
  const [weekday, setWeekday] = useState(1) // 星期几
  const [monthDay, setMonthDay] = useState(1) // 每月几号
  const [eventType, setEventType] = useState('device.online')
  const [eventDeviceId, setEventDeviceId] = useState<string>('all')
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [loadingMetrics, setLoadingMetrics] = useState<Record<string, boolean>>({})
  const [saving, setSaving] = useState(false)
  const [validationErrors, setValidationErrors] = useState<string[]>([])
  const [searchQuery, setSearchQuery] = useState("")

  // Load agent data when editing
  useEffect(() => {
    if (agent) {
      setName(agent.name)
      setRole(agent.role || 'Monitor')
      setUserPrompt(agent.user_prompt)
      if (agent.schedule) {
        const type = agent.schedule.schedule_type as AgentScheduleType
        if (type === 'event') {
          setScheduleType('event')
        } else if (agent.schedule.cron_expression) {
          // Parse cron expression
          const cron = agent.schedule.cron_expression
          // Simple cron parsing for common patterns
          if (cron.startsWith('0 ')) {
            // Daily format: 0 9 * * * (daily at 9:00)
            const parts = cron.split(' ')
            if (parts.length === 5 || parts.length === 6) {
              const hour = parseInt(parts[1]) || 9
              const minute = parseInt(parts[0]) || 0
              setScheduleHour(hour)
              setScheduleMinute(minute)
              if (parts[2] === '*') {
                setScheduleType('daily')
              } else {
                setWeekday(parseInt(parts[4]) || 1)
                setScheduleType('weekly')
              }
            }
          } else {
            setScheduleType('interval')
            if (agent.schedule.interval_seconds) {
              const seconds = agent.schedule.interval_seconds
              if (seconds >= 3600) {
                setIntervalUnit('hour')
                setIntervalValue(seconds / 3600)
              } else {
                setIntervalUnit('minute')
                setIntervalValue(seconds / 60)
              }
            }
          }
        } else {
          setScheduleType('interval')
          if (agent.schedule.interval_seconds) {
            const seconds = agent.schedule.interval_seconds
            if (seconds >= 3600) {
              setIntervalUnit('hour')
              setIntervalValue(seconds / 3600)
            } else {
              setIntervalUnit('minute')
              setIntervalValue(seconds / 60)
            }
          }
        }
      }
      if (agent.resources && agent.resources.length > 0) {
        const resourcesByDevice: Record<string, SelectedResource> = {}
        for (const resource of agent.resources) {
          const parts = resource.resource_id.split(':')
          if (parts.length < 2) continue
          const deviceId = parts[0]
          const resourceName = parts.slice(1).join(':')
          const device = devices.find(d => d.device_id === deviceId)
          if (!device) continue
          if (!resourcesByDevice[deviceId]) {
            resourcesByDevice[deviceId] = {
              deviceId,
              deviceName: device.name,
              deviceType: device.device_type,
              metrics: [],
              commands: []
            }
          }
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
        setExpandedDevices(new Set(Object.keys(resourcesByDevice)))
      } else {
        setSelectedResources([])
        setExpandedDevices(new Set())
      }
    } else {
      setName("")
      setRole('Monitor')
      setUserPrompt("")
      setScheduleType('interval')
      setIntervalValue(5)
      setIntervalUnit('minute')
      setScheduleHour(9)
      setScheduleMinute(0)
      setWeekday(1)
      setMonthDay(1)
      setEventType('device.online')
      setEventDeviceId('all')
      setExpandedDevices(new Set())
      setSelectedResources([])
    }
    setMetricsCache({})
    setLoadingMetrics({})
    setValidationErrors([])
    setSearchQuery("")
  }, [agent, open, devices])

  // Fetch device metrics
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

  // Helpers
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
      await fetchDeviceMetrics(deviceId)
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

  const filteredDevices = devices.filter(device => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return (
      device.name.toLowerCase().includes(query) ||
      device.device_type.toLowerCase().includes(query)
    )
  })

  const selectedCount = selectedResources.reduce((acc, r) => acc + r.metrics.length + r.commands.length, 0)

  const handleSave = async () => {
    const errors: string[] = []
    if (!name.trim()) errors.push("请输入智能体名称")
    if (!userPrompt.trim()) errors.push("请描述你的需求")
    if (selectedResources.length === 0) errors.push("请至少选择一个设备资源")

    setValidationErrors(errors)
    if (errors.length > 0) return

    setSaving(true)
    try {
      // 计算cron表达式和interval_seconds
      let cronExpression: string | undefined = undefined
      let intervalSeconds: number | undefined = undefined
      let finalScheduleType: AgentScheduleType = 'interval'

      if (scheduleType === 'interval') {
        // 间隔执行
        finalScheduleType = 'interval'
        intervalSeconds = intervalUnit === 'hour'
          ? intervalValue * 3600
          : intervalValue * 60
      } else if (scheduleType === 'daily') {
        // 每天执行：格式: 分 时 * * *
        cronExpression = `${scheduleMinute} ${scheduleHour} * * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'weekly') {
        // 每周执行：格式: 分 时 * * 星期
        cronExpression = `${scheduleMinute} ${scheduleHour} * * ${weekday}`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'monthly') {
        // 每月执行：格式: 分 时 日 * *
        cronExpression = `${scheduleMinute} ${scheduleHour} ${monthDay} * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'event') {
        finalScheduleType = 'event'
      }

      if (isEditing && agent) {
        await onSave({ name: name.trim(), role, user_prompt: userPrompt.trim() })
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
          role,
          user_prompt: userPrompt.trim(),
          device_ids: selectedResources.map(r => r.deviceId),
          metrics: selectedResources.flatMap(r =>
            r.metrics.map(m => ({ device_id: r.deviceId, metric_name: m.name, display_name: m.displayName }))
          ),
          commands: selectedResources.flatMap(r =>
            r.commands.map(c => ({ device_id: r.deviceId, command_name: c.name, display_name: c.displayName, parameters: {} }))
          ),
          schedule: {
            schedule_type: finalScheduleType,
            interval_seconds: intervalSeconds,
            cron_expression: cronExpression,
            timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
            event_filter: scheduleType === 'event' ? JSON.stringify({
              event_type: eventType,
              device_id: eventDeviceId === 'all' ? undefined : eventDeviceId,
            }) : undefined,
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-5xl max-h-[95vh] flex flex-col p-0">
        <DialogHeader className="px-6 py-4 border-b">
          <DialogTitle className="text-lg">
            {isEditing ? "编辑智能体" : "创建智能体"}
          </DialogTitle>
        </DialogHeader>

        {/* Main Content - Two Column Layout */}
        <div className="flex-1 overflow-hidden flex flex-col">
          <div className="flex-1 flex min-h-0">
            {/* Left Panel - Form */}
            <div className="w-1/2 border-r flex flex-col">
              <div className="px-5 py-3 border-b bg-muted/20">
                <h3 className="text-sm font-semibold">基本信息</h3>
              </div>
              <div className="flex-1 overflow-y-auto p-5 space-y-5">
                {/* 角色Tab */}
                <div>
                  <Label className="text-xs text-muted-foreground mb-2 block">选择角色</Label>
                  <Tabs value={role} onValueChange={(v) => setRole(v as AgentRole)}>
                    <TabsList className="grid w-full grid-cols-3 h-auto p-1 gap-1">
                      {ROLES.map((r) => (
                        <TabsTrigger key={r.value} value={r.value} className="flex-col gap-1 py-2 px-2">
                          <div className={cn("flex items-center justify-center", r.color)}>
                            {r.icon}
                          </div>
                          <span className="text-[10px] font-medium">{r.label}</span>
                        </TabsTrigger>
                      ))}
                    </TabsList>
                  </Tabs>
                  <p className="text-[10px] text-muted-foreground mt-1.5">
                    {ROLES.find(r => r.value === role)?.description}
                  </p>
                </div>

                {/* 名称 */}
                <div>
                  <Label htmlFor="name" className="text-xs text-muted-foreground">名称</Label>
                  <Input
                    id="name"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="例如：仓库温度监控"
                    className="mt-1.5 h-9"
                  />
                </div>

                {/* 需求描述 */}
                <div>
                  <Label className="text-xs text-muted-foreground">需求描述</Label>
                  <Textarea
                    value={userPrompt}
                    onChange={(e) => setUserPrompt(e.target.value)}
                    placeholder="例如：当温度超过30度时发送告警，同时通知相关人员"
                    rows={6}
                    className="mt-1.5 resize-none text-sm"
                  />
                </div>

                {/* 执行策略 */}
                <div className="space-y-3">
                  <Label className="text-xs text-muted-foreground">执行策略</Label>

                  {/* 策略类型选择 */}
                  <div className="grid grid-cols-5 gap-2">
                    <button
                      onClick={() => setScheduleType('interval')}
                      className={cn(
                        "flex items-center justify-center gap-1 px-2 py-1.5 rounded border text-xs font-medium transition-colors",
                        scheduleType === 'interval'
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      <Clock className="h-3 w-3" />
                      间隔
                    </button>
                    <button
                      onClick={() => setScheduleType('daily')}
                      className={cn(
                        "flex items-center justify-center gap-1 px-2 py-1.5 rounded border text-xs font-medium transition-colors",
                        scheduleType === 'daily'
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      每天
                    </button>
                    <button
                      onClick={() => setScheduleType('weekly')}
                      className={cn(
                        "flex items-center justify-center gap-1 px-2 py-1.5 rounded border text-xs font-medium transition-colors",
                        scheduleType === 'weekly'
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      每周
                    </button>
                    <button
                      onClick={() => setScheduleType('monthly')}
                      className={cn(
                        "flex items-center justify-center gap-1 px-2 py-1.5 rounded border text-xs font-medium transition-colors",
                        scheduleType === 'monthly'
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      每月
                    </button>
                    <button
                      onClick={() => setScheduleType('event')}
                      className={cn(
                        "flex items-center justify-center gap-1 px-2 py-1.5 rounded border text-xs font-medium transition-colors",
                        scheduleType === 'event'
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      <Bell className="h-3 w-3" />
                      事件
                    </button>
                  </div>

                  {/* 策略配置 */}
                  <div className="bg-muted/30 rounded-lg p-3">
                    {scheduleType === 'interval' && (
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs text-muted-foreground">每</span>
                        <div className="flex items-center gap-1">
                          {intervalUnit === 'minute' ? (
                            <>
                              {INTERVAL_MINUTES.map((mins) => (
                                <button
                                  key={mins}
                                  onClick={() => setIntervalValue(mins)}
                                  className={cn(
                                    "px-2 py-1 rounded border text-xs transition-colors min-w-[2.5rem]",
                                    intervalValue === mins
                                      ? "border-primary bg-primary text-primary-foreground"
                                      : "border-border hover:bg-muted"
                                  )}
                                >
                                  {mins}
                                </button>
                              ))}
                            </>
                          ) : (
                            INTERVAL_HOURS.map((hours) => (
                              <button
                                key={hours}
                                onClick={() => setIntervalValue(hours)}
                                className={cn(
                                  "px-2 py-1 rounded border text-xs transition-colors min-w-[2.5rem]",
                                  intervalValue === hours
                                    ? "border-primary bg-primary text-primary-foreground"
                                    : "border-border hover:bg-muted"
                                )}
                              >
                                {hours}
                              </button>
                            ))
                          )}
                        </div>
                        <Select value={intervalUnit} onValueChange={(v: 'minute' | 'hour') => setIntervalUnit(v)}>
                          <SelectTrigger className="w-16 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="minute">分钟</SelectItem>
                            <SelectItem value="hour">小时</SelectItem>
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">执行一次</span>
                      </div>
                    )}

                    {scheduleType === 'daily' && (
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs text-muted-foreground">每天</span>
                        <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {HOURS.map((h) => (
                              <SelectItem key={h.value} value={h.value.toString()}>
                                {h.label}:00
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">点</span>
                        <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {MINUTES.map((m) => (
                              <SelectItem key={m.value} value={m.value.toString()}>
                                :{m.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">执行</span>
                      </div>
                    )}

                    {scheduleType === 'weekly' && (
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs text-muted-foreground">每</span>
                        <Select value={weekday.toString()} onValueChange={(v) => setWeekday(parseInt(v))}>
                          <SelectTrigger className="w-16 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {WEEKDAYS.map((d) => (
                              <SelectItem key={d.value} value={d.value.toString()}>
                                {d.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {HOURS.map((h) => (
                              <SelectItem key={h.value} value={h.value.toString()}>
                                {h.label}:00
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">点</span>
                        <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {MINUTES.map((m) => (
                              <SelectItem key={m.value} value={m.value.toString()}>
                                :{m.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">执行</span>
                      </div>
                    )}

                    {scheduleType === 'monthly' && (
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs text-muted-foreground">每月</span>
                        <Select value={monthDay.toString()} onValueChange={(v) => setMonthDay(parseInt(v))}>
                          <SelectTrigger className="w-16 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {Array.from({ length: 28 }, (_, i) => i + 1).map((d) => (
                              <SelectItem key={d} value={d.toString()}>
                                {d}日
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {HOURS.map((h) => (
                              <SelectItem key={h.value} value={h.value.toString()}>
                                {h.label}:00
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">点</span>
                        <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                          <SelectTrigger className="w-14 h-7">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {MINUTES.map((m) => (
                              <SelectItem key={m.value} value={m.value.toString()}>
                                :{m.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <span className="text-xs text-muted-foreground">执行</span>
                      </div>
                    )}

                    {scheduleType === 'event' && (
                      <div className="grid grid-cols-2 gap-3">
                        <div>
                          <Label className="text-[10px] text-muted-foreground mb-1 block">事件类型</Label>
                          <Select value={eventType} onValueChange={setEventType}>
                            <SelectTrigger className="h-7">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {EVENT_TYPES.map((event) => (
                                <SelectItem key={event.value} value={event.value}>
                                  <div className="flex items-center gap-2">
                                    {event.icon}
                                    <span>{event.label}</span>
                                  </div>
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                        <div>
                          <Label className="text-[10px] text-muted-foreground mb-1 block">关联设备</Label>
                          <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                            <SelectTrigger className="h-7">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="all">全部设备</SelectItem>
                              {selectedResources.map((r) => (
                                <SelectItem key={r.deviceId} value={r.deviceId}>
                                  {r.deviceName}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                {/* 错误提示 */}
                {validationErrors.length > 0 && (
                  <div className="p-2.5 bg-destructive/10 border border-destructive/20 rounded text-xs text-destructive">
                    {validationErrors.join('，')}
                  </div>
                )}
              </div>
            </div>

            {/* Right Panel - Resources & Topology */}
            <div className="w-1/2 flex flex-col min-w-0">
              {/* Right Top: Resource Selection */}
              <div className="flex-1 flex flex-col border-b">
                <div className="px-5 py-3 border-b bg-muted/20 flex items-center justify-between">
                  <h3 className="text-sm font-semibold">资源选择</h3>
                  {selectedCount > 0 && (
                    <Badge variant="secondary" className="text-xs h-5">
                      已选 {selectedCount} 项
                    </Badge>
                  )}
                </div>
                <div className="flex-1 overflow-y-auto">
                  <div className="p-4 space-y-3">
                    {/* 搜索框 */}
                    <div className="relative">
                      <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                      <Input
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        placeholder="搜索设备..."
                        className="pl-9 h-8 text-sm"
                      />
                    </div>

                    {/* 设备列表 */}
                    {filteredDevices.length === 0 ? (
                      <div className="text-center py-8 text-muted-foreground text-sm">
                        没有找到设备
                      </div>
                    ) : (
                      <div className="space-y-1.5">
                        {filteredDevices.map((device) => {
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
                                hasSelection ? "border-primary/50 bg-primary/5" : "border-border"
                              )}
                            >
                              <button
                                onClick={() => void toggleDeviceExpanded(device.device_id)}
                                className="w-full px-3 py-2 flex items-center gap-3 hover:bg-muted/30 text-left"
                              >
                                <div className="p-1.5 rounded bg-muted">{Icon}</div>
                                <div className="flex-1 min-w-0">
                                  <div className="text-sm font-medium truncate">{device.name}</div>
                                  <div className="text-xs text-muted-foreground truncate">{device.device_type}</div>
                                </div>
                                {(metricCount > 0 || commandCount > 0) && (
                                  <Badge variant="secondary" className="text-xs h-5">
                                    {metricCount > 0 && `指标${metricCount}`}
                                    {metricCount > 0 && commandCount > 0 && ' + '}
                                    {commandCount > 0 && `指令${commandCount}`}
                                  </Badge>
                                )}
                                <ChevronDown className={cn(
                                  "h-4 w-4 text-muted-foreground transition-transform flex-shrink-0",
                                  isExpanded && "rotate-180"
                                )} />
                              </button>

                              {isExpanded && (
                                <div className="border-t p-2 bg-muted/20">
                                  {(() => {
                                    const metrics = getDeviceMetrics(device.device_id)
                                    const isLoading = loadingMetrics[device.device_id]
                                    if (isLoading) {
                                      return (
                                        <div className="flex items-center justify-center py-3">
                                          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                                        </div>
                                      )
                                    }
                                    return (
                                      <div className="space-y-2">
                                        {metrics.length > 0 && (
                                          <div>
                                            <div className="text-xs text-muted-foreground mb-1.5 px-1">监控指标</div>
                                            <div className="flex flex-wrap gap-1">
                                              {metrics.map((metric) => (
                                                <button
                                                  key={metric.name}
                                                  onClick={() => toggleMetric(device.device_id, metric.name, metric.display_name)}
                                                  className={cn(
                                                    "text-xs px-2 py-1 rounded transition-colors",
                                                    isMetricSelected(device.device_id, metric.name)
                                                      ? "bg-primary text-primary-foreground"
                                                      : "bg-background hover:bg-muted border"
                                                  )}
                                                >
                                                  {metric.display_name}
                                                </button>
                                              ))}
                                            </div>
                                          </div>
                                        )}

                                        {(() => {
                                          const commands = getDeviceCommands(device.device_id)
                                          return commands.length > 0 && (
                                            <div>
                                              <div className="text-xs text-muted-foreground mb-1.5 px-1">执行指令</div>
                                              <div className="flex flex-wrap gap-1">
                                                {commands.map((command) => (
                                                  <button
                                                    key={command.name}
                                                    onClick={() => toggleCommand(device.device_id, command.name, command.display_name)}
                                                    className={cn(
                                                      "text-xs px-2 py-1 rounded transition-colors",
                                                      isCommandSelected(device.device_id, command.name)
                                                        ? "bg-primary text-primary-foreground"
                                                        : "bg-background hover:bg-muted border"
                                                    )}
                                                  >
                                                    {command.display_name}
                                                  </button>
                                                ))}
                                              </div>
                                            </div>
                                          )
                                        })()}
                                      </div>
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
                </div>
              </div>

              {/* Right Bottom: Topology Preview */}
              <div className="h-64 flex flex-col">
                <div className="px-5 py-3 border-b bg-muted/20 flex items-center justify-between">
                  <h3 className="text-sm font-semibold">资源拓扑预览</h3>
                  <span className="text-xs text-muted-foreground">以 AI Agent 为中心的聚合视图</span>
                </div>
                <div className="flex-1 p-4 overflow-auto">
                  <ResourceTopology selectedResources={selectedResources} role={role} />
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-6 py-3 border-t">
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)} disabled={saving}>
            取消
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                保存中
              </>
            ) : (
              <>
                <Save className="h-4 w-4 mr-2" />
                {isEditing ? "保存" : "创建"}
              </>
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
