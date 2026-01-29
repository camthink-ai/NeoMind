/**
 * Agent Editor Full Screen
 * Full-page editor for creating/editing AI Agents
 * Core layout: Prompt/Model (left) + Resources (right)
 */

import React, { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { cn } from "@/lib/utils"
import {
  Dialog,
  DialogContent,
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
  Eye,
  Fan,
  BarChart3,
  Thermometer,
  Droplets,
  Lightbulb,
  Search,
  ChevronDown,
  Loader2,
  Clock,
  Zap,
  Check,
  Target,
  Bell,
  Activity,
  Settings,
  X,
} from "lucide-react"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
  AgentRole,
} from "@/types"

interface AgentEditorFullScreenProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agent: AiAgentDetail | undefined
  devices: Device[]
  deviceTypes: DeviceType[]
  onSave: (data: CreateAgentRequest | Partial<AiAgentDetail>) => Promise<void>
}

interface MetricInfo {
  name: string
  display_name: string
  unit?: string
}

interface DataCollectionConfig {
  time_range_minutes: number
  include_history: boolean
  max_points: number
  include_trend: boolean
  include_baseline: boolean
}

const DEFAULT_DATA_COLLECTION: DataCollectionConfig = {
  time_range_minutes: 60,
  include_history: false,
  max_points: 100,
  include_trend: false,
  include_baseline: false,
}

interface SelectedResource {
  deviceId: string
  deviceName: string
  deviceType: string
  metrics: Array<{ name: string; displayName: string }>
  commands: Array<{ name: string; displayName: string }>
}

// Resource icons (static, no i18n needed)
const RESOURCE_ICONS: Record<string, React.ReactNode> = {
  temperature: <Thermometer className="h-4 w-4" />,
  humidity: <Droplets className="h-4 w-4" />,
  light: <Lightbulb className="h-4 w-4" />,
  switch: <Fan className="h-4 w-4" />,
  default: <Target className="h-4 w-4" />,
}

const INTERVALS = [1, 5, 10, 15, 30, 60]
const HOURS = Array.from({ length: 24 }, (_, i) => i)
const MINUTES = [0, 15, 30, 45]

// Helper to get role config
const getRoleConfig = (t: (key: string) => string) => [
  { value: 'Monitor' as const, label: t('creator.basicInfo.roles.monitor.label'), description: t('creator.basicInfo.roles.monitor.description'), icon: Eye },
  { value: 'Executor' as const, label: t('creator.basicInfo.roles.executor.label'), description: t('creator.basicInfo.roles.executor.description'), icon: Fan },
  { value: 'Analyst' as const, label: t('creator.basicInfo.roles.analyst.label'), description: t('creator.basicInfo.roles.analyst.description'), icon: BarChart3 },
]

// Helper to get schedule types config
const getScheduleTypes = (t: (key: string) => string) => [
  { value: 'interval' as const, label: t('creator.schedule.strategies.interval'), description: t('creator.schedule.config'), icon: <Clock className="h-4 w-4" /> },
  { value: 'daily' as const, label: t('creator.schedule.strategies.daily'), description: t('creator.schedule.daily.preview'), icon: <Zap className="h-4 w-4" /> },
  { value: 'weekly' as const, label: t('creator.schedule.strategies.weekly'), description: t('creator.schedule.weekly.preview'), icon: <Bell className="h-4 w-4" /> },
  { value: 'event' as const, label: t('creator.schedule.strategies.event'), description: t('creator.schedule.event.triggerEvent'), icon: <Target className="h-4 w-4" /> },
  { value: 'once' as const, label: t('creator.schedule.strategies.once'), description: t('creator.schedule.onceDescription'), icon: <Check className="h-4 w-4" /> },
]

export function AgentEditorFullScreen({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  onSave,
}: AgentEditorFullScreenProps) {
  const { toast } = useToast()
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { llmBackends, activeBackendId, loadBackends } = useStore()
  const isEditing = !!agent

  // Get dynamic config based on i18n
  const ROLES = getRoleConfig(tAgent)
  const SCHEDULE_TYPES = getScheduleTypes(tAgent)

  // Form state
  const [name, setName] = useState("")
  const [role, setRole] = useState<AgentRole>('Monitor')
  const [description, setDescription] = useState("")
  const [userPrompt, setUserPrompt] = useState("")
  const [llmBackendId, setLlmBackendId] = useState<string | null>(null)

  // Schedule state
  const [scheduleType, setScheduleType] = useState<'interval' | 'daily' | 'weekly' | 'event' | 'once'>('interval')
  const [intervalValue, setIntervalValue] = useState(5)
  const [scheduleHour, setScheduleHour] = useState(9)
  const [scheduleMinute, setScheduleMinute] = useState(0)
  const [selectedWeekdays, setSelectedWeekdays] = useState<number[]>([1]) // 0=Sun, 1=Mon, ..., 6=Sat

  // Event trigger state
  const [eventType, setEventType] = useState('device.online')
  const [eventDeviceId, setEventDeviceId] = useState('all')

  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [selectedDeviceId, setSelectedDeviceId] = useState<string>('') // Currently selected device for right panel
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [searchQuery, setSearchQuery] = useState("")
  const [showAdvancedConfig, setShowAdvancedConfig] = useState(false)

  // Advanced config state (per-metric data collection settings)
  const [metricConfigs, setMetricConfigs] = useState<Record<string, DataCollectionConfig>>({})

  // UI state
  const [saving, setSaving] = useState(false)
  const [loadingMetrics, setLoadingMetrics] = useState<Record<string, boolean>>({})

  // Load LLM backends
  useEffect(() => {
    if (open) {
      loadBackends()
    }
  }, [open, loadBackends])

  // Reset form when dialog opens
  useEffect(() => {
    if (open) {
      if (agent) {
        setName(agent.name || '')
        setRole(agent.role || 'Monitor')
        setDescription(agent.description || '')
        setUserPrompt(agent.user_prompt || '')
        setLlmBackendId(agent.llm_backend_id || null)

        // Parse schedule
        if (agent.schedule) {
          if (agent.schedule.schedule_type === 'interval') {
            setScheduleType('interval')
            if (agent.schedule.interval_seconds) {
              const secs = agent.schedule.interval_seconds
              if (secs >= 60) setIntervalValue(Math.floor(secs / 60))
            }
          } else if (agent.schedule.schedule_type === 'cron') {
            // Parse cron expression: "minute hour * * *" or "minute hour * * day"
            if (agent.schedule.cron_expression) {
              const parts = agent.schedule.cron_expression.split(' ')
              if (parts.length >= 5) {
                const minute = parseInt(parts[0])
                const hour = parseInt(parts[1])
                const dayOfWeek = parts[4]

                if (!isNaN(hour) && hour >= 0 && hour <= 23) setScheduleHour(hour)
                if (!isNaN(minute) && minute >= 0 && minute <= 59) setScheduleMinute(minute)

                // Check if it's a weekly schedule (day of week is specified)
                if (dayOfWeek !== '*') {
                  setScheduleType('weekly')
                  // Parse weekdays: "1,2,3" or "1-5" or "1"
                  const days: number[] = []
                  if (dayOfWeek.includes(',')) {
                    dayOfWeek.split(',').forEach(d => {
                      const num = parseInt(d)
                      if (!isNaN(num)) days.push(num)
                    })
                  } else if (dayOfWeek.includes('-')) {
                    const [start, end] = dayOfWeek.split('-').map(Number)
                    if (!isNaN(start) && !isNaN(end)) {
                      for (let i = start; i <= end; i++) days.push(i)
                    }
                  } else {
                    const num = parseInt(dayOfWeek)
                    if (!isNaN(num)) days.push(num)
                  }
                  if (days.length > 0) setSelectedWeekdays(days)
                }
              }
            } else {
              setScheduleHour(9)
              setScheduleMinute(0)
            }
          } else if (agent.schedule.schedule_type === 'event') {
            setScheduleType('event')
            // Parse event_filter to get event type and device
            const eventFilterStr = agent.schedule.event_filter
            if (eventFilterStr) {
              try {
                const eventFilter = JSON.parse(eventFilterStr)
                if (eventFilter.event_type === 'device.online' || eventFilter.event_type === 'device.offline' || eventFilter.event_type === 'device.metric') {
                  setEventType(eventFilter.event_type)
                  setEventDeviceId(eventFilter.device_id || 'all')
                } else {
                  setEventType('manual')
                }
              } catch {
                setEventType('manual')
              }
            }
          } else {
            setScheduleType(agent.schedule.schedule_type as any)
          }
        }

        // Parse resources
        if (agent.resources?.length > 0) {
          const resourcesByDevice: Record<string, SelectedResource> = {}
          const configs: Record<string, DataCollectionConfig> = {}
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
              // Parse data collection config
              const configKey = `${deviceId}:${resourceName}`
              const dataCollection = resource.config?.data_collection
              if (dataCollection) {
                configs[configKey] = { ...DEFAULT_DATA_COLLECTION, ...dataCollection }
              } else {
                configs[configKey] = { ...DEFAULT_DATA_COLLECTION }
              }
            } else if (resource.resource_type === 'Command') {
              resourcesByDevice[deviceId].commands.push({
                name: resourceName,
                displayName: resource.name
              })
            }
          }
          setSelectedResources(Object.values(resourcesByDevice))
          setMetricConfigs(configs)
          // Auto-select first device that has resources
          setSelectedDeviceId(Object.keys(resourcesByDevice)[0] || '')
        }
      } else {
        // Reset for new agent
        setName("")
        setRole('Monitor')
        setDescription("")
        setUserPrompt("")
        setLlmBackendId(null)
        setScheduleType('interval')
        setIntervalValue(5)
        setScheduleHour(9)
        setScheduleMinute(0)
        setSelectedWeekdays([1])
        setEventType('device.online')
        setEventDeviceId('all')
        setSelectedResources([])
        // Auto-select first device for new agent
        setSelectedDeviceId(devices.length > 0 ? devices[0].device_id : '')
      }
      setMetricsCache({})
      setLoadingMetrics({})
      setSearchQuery("")
    }
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

  // Select device for right panel
  const selectDevice = async (deviceId: string) => {
    if (selectedDeviceId === deviceId) {
      setSelectedDeviceId('') // Deselect if clicking the same device
    } else {
      setSelectedDeviceId(deviceId)
      await fetchDeviceMetrics(deviceId)
    }
  }

  // Toggle metric selection
  const toggleMetric = (deviceId: string, metricName: string, displayName: string) => {
    const configKey = `${deviceId}:${metricName}`
    const existing = selectedResources.findIndex(r => r.deviceId === deviceId)
    if (existing < 0) {
      const device = devices.find(d => d.device_id === deviceId)!
      setSelectedResources(prev => [...prev, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: [{ name: metricName, displayName }],
        commands: []
      }])
      // Initialize config
      setMetricConfigs(prev => ({
        ...prev,
        [configKey]: { ...DEFAULT_DATA_COLLECTION }
      }))
      return
    }
    setSelectedResources(prev => {
      const newResources = [...prev]
      const resource = newResources[existing]
      const metricIndex = resource.metrics.findIndex(m => m.name === metricName)
      if (metricIndex >= 0) {
        resource.metrics = resource.metrics.filter(m => m.name !== metricName)
        // Clean up config
        setMetricConfigs(prev => {
          const newConfigs = { ...prev }
          delete newConfigs[configKey]
          return newConfigs
        })
        if (resource.metrics.length === 0 && resource.commands.length === 0) {
          newResources.splice(existing, 1)
        }
      } else {
        resource.metrics = [...resource.metrics, { name: metricName, displayName }]
        // Initialize config
        setMetricConfigs(prev => ({
          ...prev,
          [configKey]: { ...DEFAULT_DATA_COLLECTION }
        }))
      }
      return newResources
    })
  }

  // Update metric config
  const updateMetricConfig = (configKey: string, updates: Partial<DataCollectionConfig>) => {
    setMetricConfigs(prev => ({
      ...prev,
      [configKey]: { ...prev[configKey], ...updates }
    }))
  }

  // Toggle command selection
  const toggleCommand = (deviceId: string, commandName: string, displayName: string) => {
    const existing = selectedResources.findIndex(r => r.deviceId === deviceId)
    if (existing < 0) {
      const device = devices.find(d => d.device_id === deviceId)!
      setSelectedResources(prev => [...prev, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: [],
        commands: [{ name: commandName, displayName }]
      }])
      return
    }
    setSelectedResources(prev => {
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

  // Check if metric/command is selected
  const isMetricSelected = (deviceId: string, metricName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.metrics.some(m => m.name === metricName) || false
  }

  const isCommandSelected = (deviceId: string, commandName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.commands.some(c => c.name === commandName) || false
  }

  // Get device icon
  const getDeviceIcon = (deviceType: string) => {
    const type = deviceTypes.find(dt => dt.device_type === deviceType)
    const category = type?.categories?.[0] || 'default'
    return RESOURCE_ICONS[category] || RESOURCE_ICONS.default
  }

  // Get device metrics/commands
  const getDeviceMetrics = (deviceId: string): MetricInfo[] => metricsCache[deviceId] || []
  const getDeviceCommands = (deviceId: string) => {
    const device = devices.find(d => d.device_id === deviceId)
    if (!device) return []
    const type = deviceTypes.find(dt => dt.device_type === device.device_type)
    return type?.commands || []
  }

  // Select all metrics/commands for a device
  const selectAllForDevice = (deviceId: string) => {
    const device = devices.find(d => d.device_id === deviceId)
    if (!device) return
    const metrics = getDeviceMetrics(deviceId)
    const commands = getDeviceCommands(deviceId)
    const existing = selectedResources.findIndex(r => r.deviceId === deviceId)
    // Initialize configs for all metrics
    const newConfigs: Record<string, DataCollectionConfig> = {}
    metrics.forEach(m => {
      newConfigs[`${deviceId}:${m.name}`] = { ...DEFAULT_DATA_COLLECTION }
    })
    setMetricConfigs(prev => ({ ...prev, ...newConfigs }))

    if (existing < 0) {
      setSelectedResources(prev => [...prev, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: metrics.map(m => ({ name: m.name, displayName: m.display_name })),
        commands: commands.map(c => ({ name: c.name, displayName: c.display_name || c.name }))
      }])
    } else {
      setSelectedResources(prev => {
        const newResources = [...prev]
        const resource = newResources[existing]
        resource.metrics = metrics.map(m => ({ name: m.name, displayName: m.display_name }))
        resource.commands = commands.map(c => ({ name: c.name, displayName: c.display_name || c.name }))
        return newResources
      })
    }
  }

  // Clear all selections for a device
  const clearDevice = (deviceId: string) => {
    setSelectedResources(prev => prev.filter(r => r.deviceId !== deviceId))
  }

  // Save handler
  const handleSave = async () => {
    // Validation
    if (!name.trim()) {
      toast({ title: tAgent('creator.validation.nameRequired'), variant: 'destructive' })
      return
    }
    if (!userPrompt.trim()) {
      toast({ title: tAgent('creator.validation.requirementRequired'), variant: 'destructive' })
      return
    }

    setSaving(true)
    try {
      let cronExpression: string | undefined = undefined
      let intervalSeconds: number | undefined = undefined
      let finalScheduleType: AgentScheduleType = 'interval'
      let eventFilter: any = undefined

      if (scheduleType === 'interval') {
        finalScheduleType = 'interval'
        intervalSeconds = intervalValue * 60
      } else if (scheduleType === 'daily') {
        cronExpression = `${scheduleMinute} ${scheduleHour} * * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'weekly') {
        // Sort weekdays and build comma-separated list
        const sortedDays = [...selectedWeekdays].sort((a, b) => a - b)
        cronExpression = `${scheduleMinute} ${scheduleHour} * * ${sortedDays.join(',')}`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'event') {
        finalScheduleType = 'event'
        // Build event filter (as JSON string)
        if (eventType === 'device.online' || eventType === 'device.offline' || eventType === 'device.metric') {
          eventFilter = JSON.stringify({
            event_type: eventType,
            device_id: eventDeviceId === 'all' ? undefined : eventDeviceId,
          })
        } else {
          eventFilter = JSON.stringify({
            event_type: 'manual',
          })
        }
      } else if (scheduleType === 'once') {
        finalScheduleType = 'once'
      }

      if (isEditing && agent) {
        const updateData: Partial<AiAgentDetail> = {
          name: name.trim(),
          description: description.trim(),
          role,
          user_prompt: userPrompt.trim(),
          llm_backend_id: llmBackendId ?? undefined,
          schedule: {
            schedule_type: finalScheduleType,
            interval_seconds: intervalSeconds,
            cron_expression: cronExpression,
            event_filter: eventFilter,
          },
        }
        await onSave(updateData)
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
          role,
          description: description.trim(),
          user_prompt: userPrompt.trim(),
          device_ids: selectedResources.map(r => r.deviceId),
          metrics: selectedResources.flatMap(r =>
            r.metrics.map(m => {
              const configKey = `${r.deviceId}:${m.name}`
              const config = metricConfigs[configKey]
              return {
                device_id: r.deviceId,
                metric_name: m.name,
                display_name: m.displayName,
                config: config ? { data_collection: config } : undefined
              }
            })
          ),
          commands: selectedResources.flatMap(r =>
            r.commands.map(c => ({ device_id: r.deviceId, command_name: c.name, display_name: c.displayName, parameters: {} }))
          ),
          schedule: {
            schedule_type: finalScheduleType,
            interval_seconds: intervalSeconds,
            cron_expression: cronExpression,
            event_filter: eventFilter,
          },
          llm_backend_id: llmBackendId ?? undefined,
        }
        await onSave(data)
      }
      onOpenChange(false)
      toast({ title: isEditing ? tAgent('agentUpdated') : tAgent('agentCreated') })
    } catch (error) {
      toast({ title: tCommon('failed'), description: (error as Error).message, variant: 'destructive' })
    } finally {
      setSaving(false)
    }
  }

  // Selected count
  const selectedCount = selectedResources.reduce((acc, r) => acc + r.metrics.length + r.commands.length, 0)

  // Filter devices
  const filteredDevices = devices.filter(device => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return device.name.toLowerCase().includes(query) || device.device_type.toLowerCase().includes(query)
  })

  return (
    <Dialog open={open} onOpenChange={(v) => !saving && v === false && onOpenChange(false)}>
      <DialogContent className="fixed left-0 top-0 bottom-0 right-0 h-[100dvh] max-h-[100dvh] w-full max-w-none translate-x-0 translate-y-0 !rounded-none rounded-none border-0 p-0 gap-0 flex flex-col bg-background">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b">
          <h1 className="text-lg font-semibold">
            {isEditing ? tAgent('creator.editTitle') : tAgent('creator.title')}
          </h1>
          <button
            type="button"
            onClick={() => !saving && onOpenChange(false)}
            disabled={saving}
            className="p-2 rounded-lg hover:bg-muted transition-colors"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Scrollable Content */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-3xl mx-auto py-6 space-y-6 px-6">

            {/* Basic Info + Prompt */}
            <div className="space-y-4">
              {/* Role Selection */}
              <div>
                <Label className="text-sm mb-3 block">{tAgent('creator.basicInfo.role')}</Label>
                <div className="grid grid-cols-3 gap-4">
                  {ROLES.map((r) => {
                    const Icon = r.icon
                    const isSelected = role === r.value
                    return (
                      <button
                        key={r.value}
                        type="button"
                        onClick={() => setRole(r.value)}
                        className={cn(
                          "relative p-4 rounded-xl border-2 text-left transition-all hover:shadow-sm",
                          isSelected ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/30 bg-card"
                        )}
                      >
                        <div className="flex items-center gap-3">
                          <div className={cn(
                            "flex items-center justify-center w-10 h-10 rounded-lg",
                            isSelected ? "bg-primary text-primary-foreground" : "bg-muted"
                          )}>
                            <Icon className="h-5 w-5" />
                          </div>
                          <div className="flex-1">
                            <div className="text-sm font-semibold">{r.label}</div>
                            <div className="text-xs text-muted-foreground line-clamp-1">{r.description}</div>
                          </div>
                        </div>
                        {isSelected && (
                          <div className="absolute top-3 right-3">
                            <div className="w-5 h-5 rounded-full bg-primary flex items-center justify-center">
                              <Check className="h-3 w-3 text-primary-foreground" />
                            </div>
                          </div>
                        )}
                      </button>
                    )
                  })}
                </div>
              </div>

              {/* Name */}
              <div className="space-y-2">
                <Label className="text-sm">
                  {tAgent('creator.basicInfo.name')}
                  <span className="text-destructive ml-1">*</span>
                </Label>
                <Input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={tAgent('agentNamePlaceholder')}
                />
              </div>

              {/* Description */}
              <div className="space-y-2">
                <Label className="text-sm">{tAgent('creator.basicInfo.description')}</Label>
                <Input
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder={`${tCommon('optional')}`}
                />
              </div>

              {/* User Prompt */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <Label className="text-sm">
                    {tAgent('creator.basicInfo.requirement')}
                    <span className="text-destructive ml-1">*</span>
                  </Label>
                  <Select value={llmBackendId ?? activeBackendId ?? ''} onValueChange={setLlmBackendId}>
                    <SelectTrigger className="w-40 h-8 text-xs">
                      <SelectValue placeholder={tAgent('creator.basicInfo.defaultBackend')} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="default">{tAgent('creator.basicInfo.defaultBackend')}</SelectItem>
                      {llmBackends.map((backend) => (
                        <SelectItem key={backend.id} value={backend.id}>{backend.name}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <Textarea
                  value={userPrompt}
                  onChange={(e) => setUserPrompt(e.target.value)}
                  placeholder={tAgent('creator.basicInfo.requirementPlaceholder')}
                  className="min-h-[120px] resize-y"
                />
              </div>
            </div>

            {/* Execution Strategy */}
            <div className="space-y-4">
              <Label className="text-sm font-medium">{tAgent('creator.schedule.selectStrategy')}</Label>
              <div className="flex items-center gap-2 flex-wrap">
                {SCHEDULE_TYPES.map((type) => (
                  <button
                    key={type.value}
                    type="button"
                    onClick={() => setScheduleType(type.value as any)}
                    className={cn(
                      "flex items-center gap-1.5 px-3 py-1.5 rounded-md border text-sm transition-all",
                      scheduleType === type.value
                        ? "bg-primary text-primary-foreground border-primary"
                        : "hover:bg-muted"
                    )}
                  >
                    {type.icon}
                    {type.label}
                  </button>
                ))}
              </div>

              {/* Schedule Config */}
              <div className="flex items-center gap-3 text-sm h-9">
                {scheduleType === 'interval' && (
                  <>
                    <span className="text-muted-foreground">{tAgent('creator.schedule.interval.every')}</span>
                    <div className="flex items-center gap-1">
                      {INTERVALS.map((mins) => (
                        <button
                          key={mins}
                          type="button"
                          onClick={() => setIntervalValue(mins)}
                          className={cn(
                            "px-3 py-1 rounded text-sm transition-colors min-w-[2.5rem]",
                            intervalValue === mins
                              ? "bg-primary text-primary-foreground font-medium"
                              : "hover:bg-muted"
                          )}
                        >
                          {mins}
                        </button>
                      ))}
                    </div>
                    <span className="text-muted-foreground">{tAgent('creator.schedule.interval.minutes')}</span>
                  </>
                )}

                {scheduleType === 'daily' && (
                  <>
                    <span className="text-muted-foreground">{tAgent('creator.schedule.daily.at')}</span>
                    <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                      <SelectTrigger className="w-16 h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {HOURS.map((h) => (
                          <SelectItem key={h} value={h.toString()}>
                            {h.toString().padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <span className="text-muted-foreground">:</span>
                    <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                      <SelectTrigger className="w-16 h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {MINUTES.map((m) => (
                          <SelectItem key={m} value={m.toString()}>
                            {m.toString().padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </>
                )}

                {scheduleType === 'weekly' && (
                  <>
                    <div className="flex items-center gap-1">
                      {Array.from({ length: 7 }, (_, index) => (
                        <button
                          key={index}
                          type="button"
                          onClick={() => {
                            if (selectedWeekdays.includes(index)) {
                              setSelectedWeekdays(selectedWeekdays.filter(d => d !== index))
                            } else {
                              setSelectedWeekdays([...selectedWeekdays, index])
                            }
                          }}
                          className={cn(
                            "w-8 h-8 rounded text-sm font-medium transition-colors",
                            selectedWeekdays.includes(index)
                              ? "bg-primary text-primary-foreground"
                              : "hover:bg-muted"
                          )}
                        >
                          {tAgent(`creator.weekdays.${index}`)}
                      </button>
                      ))}
                    </div>
                    <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                      <SelectTrigger className="w-16 h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {HOURS.map((h) => (
                          <SelectItem key={h} value={h.toString()}>
                            {h.toString().padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <span className="text-muted-foreground">:</span>
                    <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                      <SelectTrigger className="w-16 h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {MINUTES.map((m) => (
                          <SelectItem key={m} value={m.toString()}>
                            {m.toString().padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </>
                )}

                {scheduleType === 'once' && (
                  <span className="text-muted-foreground">{tAgent('creator.schedule.onceDescription')}</span>
                )}

                {scheduleType === 'event' && (
                  <>
                    <div className="flex items-center gap-1">
                      <button
                        type="button"
                        onClick={() => setEventType('device.online')}
                        className={cn(
                          "px-3 py-1 rounded text-sm transition-colors",
                          eventType === 'device.online'
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-muted"
                        )}
                      >
                        <Zap className="h-3.5 w-3.5 inline mr-1" />
                        {tAgent('creator.schedule.event.events.device.online')}
                      </button>
                      <button
                        type="button"
                        onClick={() => setEventType('device.offline')}
                        className={cn(
                          "px-3 py-1 rounded text-sm transition-colors",
                          eventType === 'device.offline'
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-muted"
                        )}
                      >
                        <Target className="h-3.5 w-3.5 inline mr-1" />
                        {tAgent('creator.schedule.event.events.device.offline')}
                      </button>
                      <button
                        type="button"
                        onClick={() => setEventType('device.metric')}
                        className={cn(
                          "px-3 py-1 rounded text-sm transition-colors",
                          eventType === 'device.metric'
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-muted"
                        )}
                      >
                        <Activity className="h-3.5 w-3.5 inline mr-1" />
                        {tAgent('creator.schedule.event.events.device.metric')}
                      </button>
                      <button
                        type="button"
                        onClick={() => setEventType('manual')}
                        className={cn(
                          "px-3 py-1 rounded text-sm transition-colors",
                          eventType === 'manual'
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-muted"
                        )}
                      >
                        <Clock className="h-3.5 w-3.5 inline mr-1" />
                        {tAgent('creator.schedule.event.events.manual')}
                      </button>
                    </div>
                    {(eventType === 'device.online' || eventType === 'device.offline' || eventType === 'device.metric') && (
                      <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                        <SelectTrigger className="w-32 h-9">
                          <SelectValue placeholder={tAgent('creator.schedule.event.allDevices')} />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="all">{tAgent('creator.schedule.event.allDevices')}</SelectItem>
                          {devices.map((d) => (
                            <SelectItem key={d.device_id} value={d.device_id}>{d.name}</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                  </>
                )}
              </div>
            </div>

            {/* Device & Resources */}
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium">{tAgent('creator.resources.title')}</Label>
                {selectedCount > 0 && (
                  <Badge variant="secondary" className="text-xs">
                    {selectedCount} {tCommon('selected')}
                  </Badge>
                )}
              </div>

              <div className="grid grid-cols-12 gap-4">
                {/* Device List */}
                <div className="col-span-4 space-y-2">
                  <Input
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={tAgent('creator.resources.searchPlaceholder')}
                  />
                  <div className="space-y-1 max-h-[300px] overflow-y-auto">
                    {filteredDevices.length === 0 ? (
                      <div className="text-center py-8 text-muted-foreground text-sm">
                        {tAgent('creator.noDevices')}
                      </div>
                    ) : (
                      filteredDevices.map((device) => {
                        const Icon = getDeviceIcon(device.device_type)
                        const hasSelection = selectedResources.find(r => r.deviceId === device.device_id)
                        const isSelected = selectedDeviceId === device.device_id
                        const selectionCount = (hasSelection?.metrics.length || 0) + (hasSelection?.commands.length || 0)

                        return (
                          <button
                            key={device.device_id}
                            type="button"
                            onClick={() => void selectDevice(device.device_id)}
                            className={cn(
                              "w-full p-2 rounded-md text-left transition-all flex items-center gap-2 text-sm",
                              isSelected && "bg-primary text-primary-foreground",
                              !isSelected && "hover:bg-muted"
                            )}
                          >
                            {Icon}
                            <span className="flex-1 truncate">{device.name}</span>
                            {selectionCount > 0 && (
                              <Badge variant={isSelected ? "secondary" : "outline"} className="text-xs h-5 px-1.5">
                                {selectionCount}
                              </Badge>
                            )}
                          </button>
                        )
                      })
                    )}
                  </div>
                </div>

                {/* Selected Device Panel */}
                <div className="col-span-8">
                  {!selectedDeviceId ? (
                    <div className="h-[300px] flex items-center justify-center text-muted-foreground text-sm border rounded-lg">
                      {tAgent('creator.resources.selectDevice')}
                    </div>
                  ) : (
                    <div className="border rounded-lg overflow-hidden">
                      {/* Device Header */}
                      {(() => {
                        const device = devices.find(d => d.device_id === selectedDeviceId)
                        if (!device) return null
                        const metrics = getDeviceMetrics(selectedDeviceId)
                        const commands = getDeviceCommands(selectedDeviceId)
                        const isLoading = loadingMetrics[selectedDeviceId]

                        return (
                          <div className="px-3 py-2 border-b bg-muted/30 flex items-center justify-between">
                            <span className="font-medium text-sm">{device.name}</span>
                            <div className="flex items-center gap-2">
                              <button
                                type="button"
                                onClick={() => selectAllForDevice(selectedDeviceId)}
                                className="text-xs px-2 py-1 rounded hover:bg-muted transition-colors text-primary"
                              >
                                {tCommon('selectAll')}
                              </button>
                              <button
                                type="button"
                                onClick={() => clearDevice(selectedDeviceId)}
                                className="text-xs px-2 py-1 rounded hover:bg-muted transition-colors text-muted-foreground"
                              >
                                {tCommon('clear')}
                              </button>
                            </div>
                          </div>
                        )
                      })()}

                      {/* Metrics and Commands */}
                      <div className="p-3 max-h-[260px] overflow-y-auto">
                        {(() => {
                          const device = devices.find(d => d.device_id === selectedDeviceId)
                          if (!device) return null
                          const metrics = getDeviceMetrics(selectedDeviceId)
                          const commands = getDeviceCommands(selectedDeviceId)
                          const isLoading = loadingMetrics[selectedDeviceId]

                          if (isLoading) {
                            return (
                              <div className="flex items-center justify-center py-16">
                                <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                              </div>
                            )
                          }

                          if (metrics.length === 0 && commands.length === 0) {
                            return (
                              <div className="text-center py-16 text-muted-foreground text-sm">
                                {tAgent('creator.resources.noMetrics')}
                              </div>
                            )
                          }

                          return (
                            <div className="space-y-3">
                              {/* Metrics */}
                              {metrics.length > 0 && (
                                <div>
                                  <div className="text-xs text-muted-foreground mb-2 flex items-center gap-1">
                                    <BarChart3 className="h-3 w-3" />
                                    {tAgent('creator.resources.metrics')}
                                  </div>
                                  <div className="grid grid-cols-2 gap-2">
                                    {metrics.map((metric) => {
                                      const isSelected = isMetricSelected(selectedDeviceId, metric.name)
                                      return (
                                        <button
                                          key={metric.name}
                                          type="button"
                                          onClick={() => toggleMetric(selectedDeviceId, metric.name, metric.display_name)}
                                          className={cn(
                                            "p-2 rounded text-left transition-all flex items-center gap-2 text-sm",
                                            isSelected
                                              ? "bg-blue-600 text-white"
                                              : "hover:bg-blue-50 border border-blue-200"
                                          )}
                                        >
                                          <div className={cn(
                                            "w-3.5 h-3.5 rounded-sm border flex items-center justify-center flex-shrink-0",
                                            isSelected ? "border-white/50" : "border-blue-300"
                                          )}>
                                            {isSelected && <Check className="h-2.5 w-2.5" />}
                                          </div>
                                          <span className="flex-1 truncate">{metric.display_name}</span>
                                          {metric.unit && <span className="text-xs opacity-70">{metric.unit}</span>}
                                        </button>
                                      )
                                    })}
                                  </div>
                                </div>
                              )}

                              {/* Commands */}
                              {commands.length > 0 && (
                                <div>
                                  <div className="text-xs text-muted-foreground mb-2 flex items-center gap-1">
                                    <Zap className="h-3 w-3" />
                                    {tAgent('creator.resources.commands')}
                                  </div>
                                  <div className="space-y-2">
                                    {commands.map((command) => {
                                      const isSelected = isCommandSelected(selectedDeviceId, command.name)
                                      return (
                                        <button
                                          key={command.name}
                                          type="button"
                                          onClick={() => toggleCommand(selectedDeviceId, command.name, command.display_name)}
                                          className={cn(
                                            "p-2 rounded text-left transition-all flex items-center gap-2 text-sm w-full",
                                            isSelected
                                              ? "bg-orange-600 text-white"
                                              : "hover:bg-orange-50 border border-orange-200"
                                          )}
                                        >
                                          <div className={cn(
                                            "w-3.5 h-3.5 rounded-sm border flex items-center justify-center flex-shrink-0",
                                            isSelected ? "border-white/50" : "border-orange-300"
                                          )}>
                                            {isSelected && <Check className="h-2.5 w-2.5" />}
                                          </div>
                                          <span className="flex-1 truncate">{command.display_name}</span>
                                        </button>
                                      )
                                    })}
                                  </div>
                                </div>
                              )}
                            </div>
                          )
                        })()}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>

            {/* Advanced Config (Collapsible) */}
            <div className="space-y-4">
              <button
                type="button"
                onClick={() => setShowAdvancedConfig(!showAdvancedConfig)}
                className="w-full flex items-center justify-between p-3 border rounded-lg hover:bg-muted/50 transition-colors"
              >
                <span className="text-sm font-medium flex items-center gap-2">
                  <Settings className="h-4 w-4 text-muted-foreground" />
                  {tAgent('creator.advanced.title')}
                </span>
                <ChevronDown className={cn("h-4 w-4 text-muted-foreground transition-transform", showAdvancedConfig && "rotate-180")} />
              </button>

              {showAdvancedConfig && (
                <div className="border rounded-lg p-4">
                  {selectedResources.filter(r => r.metrics.length > 0).length === 0 ? (
                    <p className="text-sm text-muted-foreground text-center py-4">
                      {tAgent('creator.advanced.selectMetricsFirst')}
                    </p>
                  ) : (
                    <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
                      {selectedResources.flatMap(r =>
                        r.metrics.map(m => ({
                          deviceName: r.deviceName,
                          displayName: m.displayName,
                          configKey: `${r.deviceId}:${m.name}`
                        }))
                      ).map((metric) => {
                        const config = metricConfigs[metric.configKey] || DEFAULT_DATA_COLLECTION
                        return (
                          <div key={metric.configKey} className="border rounded p-3 space-y-2">
                            <div className="text-sm font-medium truncate" title={`${metric.deviceName}/${metric.displayName}`}>
                              {metric.displayName}
                            </div>
                            <div>
                              <label className="text-xs text-muted-foreground">{tAgent('creator.dataCollection.timeRange')}</label>
                              <Select
                                value={config.time_range_minutes.toString()}
                                onValueChange={(v) => updateMetricConfig(metric.configKey, { time_range_minutes: parseInt(v) })}
                              >
                                <SelectTrigger className="h-8 text-xs mt-1">
                                  <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                  <SelectItem value="5">5 {tAgent('creator.schedule.minutes')}</SelectItem>
                                  <SelectItem value="15">15 {tAgent('creator.schedule.minutes')}</SelectItem>
                                  <SelectItem value="30">30 {tAgent('creator.schedule.minutes')}</SelectItem>
                                  <SelectItem value="60">1 {tCommon('hour')}</SelectItem>
                                  <SelectItem value="180">3 {tCommon('hours')}</SelectItem>
                                  <SelectItem value="360">6 {tCommon('hours')}</SelectItem>
                                  <SelectItem value="720">12 {tCommon('hours')}</SelectItem>
                                  <SelectItem value="1440">24 {tCommon('hours')}</SelectItem>
                                </SelectContent>
                              </Select>
                            </div>
                            <div className="space-y-1">
                              <label className="flex items-center gap-2 text-xs cursor-pointer">
                                <input
                                  type="checkbox"
                                  id={`history-${metric.configKey}`}
                                  checked={config.include_history}
                                  onChange={(e) => updateMetricConfig(metric.configKey, { include_history: e.target.checked })}
                                  className="h-3 w-3 rounded"
                                />
                                {tAgent('creator.dataCollection.includeHistory')}
                              </label>
                              <label className="flex items-center gap-2 text-xs cursor-pointer">
                                <input
                                  type="checkbox"
                                  id={`trend-${metric.configKey}`}
                                  checked={config.include_trend}
                                  onChange={(e) => updateMetricConfig(metric.configKey, { include_trend: e.target.checked })}
                                  className="h-3 w-3 rounded"
                                />
                                {tAgent('creator.dataCollection.includeTrend')}
                              </label>
                              <label className="flex items-center gap-2 text-xs cursor-pointer">
                                <input
                                  type="checkbox"
                                  id={`baseline-${metric.configKey}`}
                                  checked={config.include_baseline}
                                  onChange={(e) => updateMetricConfig(metric.configKey, { include_baseline: e.target.checked })}
                                  className="h-3 w-3 rounded"
                                />
                                {tAgent('creator.dataCollection.includeBaseline')}
                              </label>
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>

          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t">
          {selectedCount > 0 && (
            <Badge variant="secondary">
              {selectedCount} {tCommon('selected')}
            </Badge>
          )}
          <Button onClick={handleSave} disabled={saving || !name.trim() || !userPrompt.trim()}>
            {saving ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                {tCommon('saving')}
              </>
            ) : (
              isEditing ? tCommon('save') : tCommon('create')
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

// Type imports
type AgentScheduleType = 'interval' | 'cron' | 'event' | 'once'
