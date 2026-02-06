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
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { cn } from "@/lib/utils"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
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
  BarChart3,
  Thermometer,
  Droplets,
  Lightbulb,
  Fan,
  Search,
  Loader2,
  Clock,
  Zap,
  Check,
  Target,
  Bell,
  Activity,
  X,
  Plus,
  Sparkles,
} from "lucide-react"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
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

const INTERVALS = [5, 10, 15, 30, 60]
const HOURS = Array.from({ length: 24 }, (_, i) => i)
const MINUTES = [0, 15, 30, 45]

// Prompt templates
const getPromptTemplates = (t: (key: string) => string, tCommon: (key: string) => string) => [
  {
    id: 'empty',
    label: tCommon('optional'),
    description: t('creator.promptTemplates.empty.description'),
    icon: <Sparkles className="h-4 w-4" />,
    template: '',
  },
  {
    id: 'monitor',
    label: t('creator.promptTemplates.monitor.label'),
    description: t('creator.promptTemplates.monitor.description'),
    icon: <Activity className="h-4 w-4" />,
    template: t('creator.promptTemplates.monitor.template'),
  },
  {
    id: 'control',
    label: t('creator.promptTemplates.control.label'),
    description: t('creator.promptTemplates.control.description'),
    icon: <Zap className="h-4 w-4" />,
    template: t('creator.promptTemplates.control.template'),
  },
  {
    id: 'analysis',
    label: t('creator.promptTemplates.analysis.label'),
    description: t('creator.promptTemplates.analysis.description'),
    icon: <BarChart3 className="h-4 w-4" />,
    template: t('creator.promptTemplates.analysis.template'),
  },
]

// Helper to get schedule types config
const getScheduleTypes = (t: (key: string) => string) => [
  { value: 'interval' as const, label: t('creator.schedule.strategies.interval'), description: t('creator.schedule.config'), icon: <Clock className="h-4 w-4" /> },
  { value: 'daily' as const, label: t('creator.schedule.strategies.daily'), description: t('creator.schedule.daily.preview'), icon: <Zap className="h-4 w-4" /> },
  { value: 'weekly' as const, label: t('creator.schedule.strategies.weekly'), description: t('creator.schedule.weekly.preview'), icon: <Bell className="h-4 w-4" /> },
  { value: 'event' as const, label: t('creator.schedule.strategies.event'), description: t('creator.schedule.event.triggerEvent'), icon: <Target className="h-4 w-4" /> },
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
  const { handleError } = useErrorHandler()
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { llmBackends, activeBackendId, loadBackends } = useStore()
  const isEditing = !!agent

  // Get dynamic config based on i18n
  const PROMPT_TEMPLATES = getPromptTemplates(tAgent, tCommon)
  const SCHEDULE_TYPES = getScheduleTypes(tAgent)

  // Form state
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [userPrompt, setUserPrompt] = useState("")
  const [selectedTemplateId, setSelectedTemplateId] = useState<string>('empty')
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

  // Device selection state
  const [showDeviceList, setShowDeviceList] = useState(false)

  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [selectedDeviceId, setSelectedDeviceId] = useState<string>('') // Currently selected device for right panel
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [searchQuery, setSearchQuery] = useState("")

  // Advanced config state (per-metric data collection settings) - uses defaults
  const [metricConfigs, setMetricConfigs] = useState<Record<string, DataCollectionConfig>>({})
  const [openMetricConfig, setOpenMetricConfig] = useState<string | null>(null)

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
        setDescription(agent.description || '')
        setUserPrompt(agent.user_prompt || '')
        setLlmBackendId(agent.llm_backend_id || null)
        setSelectedTemplateId('empty') // Keep template empty when editing

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
                } else {
                  // dayOfWeek is '*', this is a daily schedule
                  setScheduleType('daily')
                }
              }
            } else {
              setScheduleType('daily')
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
                                // Map event_type to valid values
                const validEventTypes = ['device.online', 'device.offline', 'device.metric', 'manual']
                const eventTypeValue = eventFilter.event_type || 'manual'
                if (validEventTypes.includes(eventTypeValue)) {
                  setEventType(eventTypeValue)
                } else {
                  console.warn('[AgentEditor] Invalid event_type:', eventTypeValue, ', using manual')
                  setEventType('manual')
                }
                setEventDeviceId(eventFilter.device_id || 'all')
              } catch (e) {
                handleError(e, { operation: 'Parse event filter', showToast: false })
                setEventType('manual')
                setEventDeviceId('all')
              }
            } else {
              // No event_filter, set defaults
                            setEventType('manual')
              setEventDeviceId('all')
            }
          } else {
            setScheduleType(agent.schedule.schedule_type as any)
          }
        }
      } else {
        // Reset for new agent
        setName("")
        setDescription("")
        setUserPrompt("")
        setLlmBackendId(null)
        setSelectedTemplateId('empty')
        setScheduleType('interval')
        setIntervalValue(5)
        setScheduleHour(9)
        setScheduleMinute(0)
        setSelectedWeekdays([1])
        setEventType('device.online')
        setEventDeviceId('all')
        setSelectedResources([])
        setMetricConfigs({})
        // Auto-select first device for new agent
        setSelectedDeviceId(devices.length > 0 ? devices[0].device_id : '')
      }
      setMetricsCache({})
      setLoadingMetrics({})
      setSearchQuery("")
    }
  }, [agent, open])

  // Load agent resources separately - this runs when devices are loaded
  useEffect(() => {
    if (open && agent?.resources && agent.resources.length > 0 && devices.length > 0) {
            const resourcesByDevice: Record<string, SelectedResource> = {}
      const configs: Record<string, DataCollectionConfig> = {}
      for (const resource of agent.resources) {
        const parts = resource.resource_id.split(':')
        if (parts.length < 2) continue
        const deviceId = parts[0]
        const resourceName = parts.slice(1).join(':')
        const device = devices.find(d => d.device_id === deviceId)
        if (!device) {
          console.warn('[AgentEditor] Device not found:', deviceId)
          continue
        }

        if (!resourcesByDevice[deviceId]) {
          resourcesByDevice[deviceId] = {
            deviceId,
            deviceName: device.name,
            deviceType: device.device_type,
            metrics: [],
            commands: []
          }
        }

        // Normalize resource_type to lowercase for comparison
        const resourceType = resource.resource_type?.toLowerCase()
        
        if (resourceType === 'metric') {
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
        } else if (resourceType === 'command') {
          resourcesByDevice[deviceId].commands.push({
            name: resourceName,
            displayName: resource.name
          })
        }
      }
            setSelectedResources(Object.values(resourcesByDevice))
      setMetricConfigs(configs)
      // Auto-select first device that has resources
      if (Object.keys(resourcesByDevice).length > 0) {
        setSelectedDeviceId(Object.keys(resourcesByDevice)[0])
      }
    }
  }, [open, agent?.resources, devices])

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

  // Fetch device metrics when device list dialog opens and there's a selected device
  useEffect(() => {
    if (showDeviceList && selectedDeviceId && !metricsCache[selectedDeviceId] && !loadingMetrics[selectedDeviceId]) {
      fetchDeviceMetrics(selectedDeviceId)
    }
  }, [showDeviceList, selectedDeviceId, metricsCache, loadingMetrics, fetchDeviceMetrics])

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
    const recommended = getRecommendedConfig()
    if (existing < 0) {
      const device = devices.find(d => d.device_id === deviceId)!
      setSelectedResources(prev => [...prev, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: [{ name: metricName, displayName }],
        commands: []
      }])
      // Initialize config with recommended settings
      setMetricConfigs(prev => ({
        ...prev,
        [configKey]: { ...recommended }
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
        // Initialize config with recommended settings
        setMetricConfigs(prev => ({
          ...prev,
          [configKey]: { ...recommended }
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

  // Get recommended data collection config based on schedule type
  const getRecommendedConfig = (): DataCollectionConfig => {
    switch (scheduleType) {
      case 'interval':
        // Interval: need trend data over short time
        return {
          time_range_minutes: Math.max(intervalValue * 2, 30), // At least 2x interval
          include_history: true,
          max_points: 100,
          include_trend: intervalValue >= 30, // Only include trend for longer intervals
          include_baseline: false,
        }
      case 'daily':
        // Daily: need full day data for daily report
        return {
          time_range_minutes: 1440, // 24 hours
          include_history: true,
          max_points: 1000,
          include_trend: true,
          include_baseline: true,
        }
      case 'weekly':
        // Weekly: need full week data for weekly report
        return {
          time_range_minutes: 10080, // 7 days
          include_history: true,
          max_points: 2000,
          include_trend: true,
          include_baseline: true,
        }
      case 'event':
        // Event: need current data + some history for comparison
        return {
          time_range_minutes: 30, // Recent data
          include_history: false,
          max_points: 100,
          include_trend: false,
          include_baseline: true, // Compare with baseline
        }
      default:
        return { ...DEFAULT_DATA_COLLECTION }
    }
  }

  // Update all metric configs when schedule type changes
  useEffect(() => {
    const recommended = getRecommendedConfig()
    setMetricConfigs(prev => {
      const newConfigs: Record<string, DataCollectionConfig> = {}
      // Update all existing configs with recommended values
      Object.keys(prev).forEach(key => {
        newConfigs[key] = { ...recommended }
      })
      return newConfigs
    })
  }, [scheduleType, intervalValue])

  // Calculate config from time range
  const getConfigFromTimeRange = (minutes: number): DataCollectionConfig => {
    if (minutes >= 10080) { // 7+ days
      return { time_range_minutes: minutes, include_history: true, max_points: 2000, include_trend: true, include_baseline: true }
    } else if (minutes >= 720) { // 12+ hours
      return { time_range_minutes: minutes, include_history: true, max_points: 1000, include_trend: true, include_baseline: true }
    } else if (minutes >= 180) { // 3+ hours
      return { time_range_minutes: minutes, include_history: true, max_points: 500, include_trend: true, include_baseline: false }
    } else if (minutes >= 60) { // 1+ hour
      return { time_range_minutes: minutes, include_history: true, max_points: 200, include_trend: false, include_baseline: false }
    } else { // < 1 hour
      return { time_range_minutes: minutes, include_history: false, max_points: 100, include_trend: false, include_baseline: true }
    }
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
    // Initialize configs for all metrics with recommended settings
    const recommended = getRecommendedConfig()
    const newConfigs: Record<string, DataCollectionConfig> = {}
    metrics.forEach(m => {
      newConfigs[`${deviceId}:${m.name}`] = { ...recommended }
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
    // Validate at least one resource is selected
    if (selectedResources.length === 0) {
      toast({ title: tAgent('creator.validation.resourceRequired'), variant: 'destructive' })
      return
    }
    // Validate event type is selected for event-based schedule
    if (scheduleType === 'event' && !eventType) {
      toast({ title: tAgent('creator.validation.eventTypeRequired'), variant: 'destructive' })
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
      }

      if (isEditing && agent) {
        const updateData: Partial<AiAgentDetail> = {
          name: name.trim(),
          description: description.trim(),
          user_prompt: userPrompt.trim(),
          llm_backend_id: llmBackendId ?? undefined,
          schedule: {
            schedule_type: finalScheduleType,
            interval_seconds: intervalSeconds,
            cron_expression: cronExpression,
            event_filter: eventFilter,
          },
          resources: selectedResources.flatMap(r => [
            ...r.metrics.map(m => {
              const configKey = `${r.deviceId}:${m.name}`
              const config = metricConfigs[configKey]
              return {
                resource_id: `${r.deviceId}:${m.name}`,
                resource_type: 'Metric' as const,
                name: m.displayName || m.name,
                config: config ? { data_collection: config, device_id: r.deviceId, metric_name: m.name, display_name: m.displayName } : { device_id: r.deviceId, metric_name: m.name, display_name: m.displayName }
              }
            }),
            ...r.commands.map(c => ({
              resource_id: `${r.deviceId}:${c.name}`,
              resource_type: 'Command' as const,
              name: c.displayName || c.name,
              config: { device_id: r.deviceId, command_name: c.name, display_name: c.displayName, parameters: {} }
            }))
          ]),
        }
        await onSave(updateData)
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
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
        </div>

        {/* Scrollable Content */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-3xl mx-auto py-6 space-y-6 px-6">

            {/* Basic Info + Prompt */}
            <div className="space-y-4">
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

              {/* User Prompt with Templates */}
              <div className="space-y-2">
                <Label className="text-sm">
                  {tAgent('creator.basicInfo.requirement')}
                  <span className="text-destructive ml-1">*</span>
                </Label>

                {/* Template Selector */}
                <div className="flex items-center justify-between">
                  <div className="flex gap-1">
                    {PROMPT_TEMPLATES.map((template) => (
                      <button
                        key={template.id}
                        type="button"
                        onClick={() => {
                          setSelectedTemplateId(template.id)
                          if (template.id !== 'empty') {
                            setUserPrompt(template.template)
                          } else {
                            setUserPrompt('')
                          }
                        }}
                        className={cn(
                          "flex items-center gap-1 px-2 py-1 rounded-md text-xs transition-colors",
                          selectedTemplateId === template.id
                            ? "bg-primary text-primary-foreground"
                            : "hover:bg-muted border"
                        )}
                        title={template.description}
                      >
                        {template.icon}
                        <span className="hidden sm:inline">{template.label}</span>
                      </button>
                    ))}
                  </div>
                  <div>
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
                </div>

                <Textarea
                  value={userPrompt}
                  onChange={(e) => setUserPrompt(e.target.value)}
                  placeholder={tAgent('creator.basicInfo.requirementPlaceholder')}
                  className="min-h-[140px] resize-y"
                />
              </div>
            </div>

            {/* Execution Strategy */}
            <div className="space-y-4">
              <Label className="text-sm font-medium">{tAgent('creator.schedule.selectStrategy')}</Label>

              {/* Strategy Type Cards - smaller */}
              <div className="grid grid-cols-4 gap-2">
                {/* Interval */}
                <button
                  type="button"
                  onClick={() => setScheduleType('interval')}
                  className={cn(
                    "flex flex-col items-center gap-1.5 py-2.5 rounded-lg border transition-all",
                    scheduleType === 'interval' ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/50"
                  )}
                >
                  <Clock className={cn("h-4 w-4", scheduleType === 'interval' ? "text-primary" : "text-muted-foreground")} />
                  <span className={cn("text-xs font-medium", scheduleType === 'interval' ? "text-primary" : "")}>{tAgent('creator.schedule.strategies.interval')}</span>
                </button>

                {/* Daily */}
                <button
                  type="button"
                  onClick={() => setScheduleType('daily')}
                  className={cn(
                    "flex flex-col items-center gap-1.5 py-2.5 rounded-lg border transition-all",
                    scheduleType === 'daily' ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/50"
                  )}
                >
                  <Zap className={cn("h-4 w-4", scheduleType === 'daily' ? "text-primary" : "text-muted-foreground")} />
                  <span className={cn("text-xs font-medium", scheduleType === 'daily' ? "text-primary" : "")}>{tAgent('creator.schedule.strategies.daily')}</span>
                </button>

                {/* Weekly */}
                <button
                  type="button"
                  onClick={() => setScheduleType('weekly')}
                  className={cn(
                    "flex flex-col items-center gap-1.5 py-2.5 rounded-lg border transition-all",
                    scheduleType === 'weekly' ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/50"
                  )}
                >
                  <Bell className={cn("h-4 w-4", scheduleType === 'weekly' ? "text-primary" : "text-muted-foreground")} />
                  <span className={cn("text-xs font-medium", scheduleType === 'weekly' ? "text-primary" : "")}>{tAgent('creator.schedule.strategies.weekly')}</span>
                </button>

                {/* Event */}
                <button
                  type="button"
                  onClick={() => setScheduleType('event')}
                  className={cn(
                    "flex flex-col items-center gap-1.5 py-2.5 rounded-lg border transition-all",
                    scheduleType === 'event' ? "border-primary bg-primary/5" : "border-border hover:border-muted-foreground/50"
                  )}
                >
                  <Target className={cn("h-4 w-4", scheduleType === 'event' ? "text-primary" : "text-muted-foreground")} />
                  <span className={cn("text-xs font-medium", scheduleType === 'event' ? "text-primary" : "")}>{tAgent('creator.schedule.strategies.event')}</span>
                </button>
              </div>

              {/* Strategy Configuration Area */}
              <div className="bg-muted/30 rounded-lg p-4 border border-border/50">
                {scheduleType === 'interval' && (
                  <div className="flex items-center gap-3">
                    <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.interval.every')}</span>
                    <div className="flex items-center gap-2">
                      {INTERVALS.map((mins) => (
                        <button
                          key={mins}
                          type="button"
                          onClick={() => setIntervalValue(mins)}
                          className={cn(
                            "w-12 h-9 rounded-lg flex items-center justify-center text-sm font-medium transition-colors",
                            intervalValue === mins ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                          )}
                        >
                          {mins}
                        </button>
                      ))}
                    </div>
                    <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.interval.minutes')}</span>
                  </div>
                )}

                {scheduleType === 'daily' && (
                  <div className="flex items-center gap-4">
                    <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.daily.at')}</span>
                    <div className="flex items-center gap-2">
                      <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                        <SelectTrigger className="w-20 h-10">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {HOURS.map((h) => <SelectItem key={h} value={h.toString()}>{h.toString().padStart(2, '0')}:00</SelectItem>)}
                        </SelectContent>
                      </Select>
                      <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.daily.execute')}</span>
                    </div>
                  </div>
                )}

                {scheduleType === 'weekly' && (
                  <div className="space-y-4">
                    <div className="flex items-center gap-3">
                      <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.weekly.on')}</span>
                      <div className="flex items-center gap-2">
                        {Array.from({ length: 7 }, (_, i) => (
                          <button
                            key={i}
                            type="button"
                            onClick={() => {
                              if (selectedWeekdays.includes(i)) {
                                setSelectedWeekdays(selectedWeekdays.filter(d => d !== i))
                              } else {
                                setSelectedWeekdays([...selectedWeekdays, i])
                              }
                            }}
                            className={cn(
                              "w-10 h-10 rounded-lg text-sm font-medium transition-colors",
                              selectedWeekdays.includes(i) ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                            )}
                          >
                            {tAgent(`creator.weekdays.${i}`)}
                          </button>
                        ))}
                      </div>
                    </div>
                    <div className="flex items-center gap-3">
                      <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.weekly.at')}</span>
                      <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                        <SelectTrigger className="w-24 h-10">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {HOURS.map((h) => <SelectItem key={h} value={h.toString()}>{h.toString().padStart(2, '0')}:00</SelectItem>)}
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                )}

                {scheduleType === 'event' && (
                  <div className="space-y-3">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.event.triggerEvent')}</span>
                      <div className="flex items-center gap-2 flex-wrap">
                        <button
                          type="button"
                          onClick={() => setEventType('device.online')}
                          className={cn(
                            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors",
                            eventType === 'device.online' ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                          )}
                        >
                          <Zap className="h-3.5 w-3.5" />
                          {tAgent('creator.schedule.event.events.device.online')}
                        </button>
                        <button
                          type="button"
                          onClick={() => setEventType('device.offline')}
                          className={cn(
                            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors",
                            eventType === 'device.offline' ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                          )}
                        >
                          <Target className="h-3.5 w-3.5" />
                          {tAgent('creator.schedule.event.events.device.offline')}
                        </button>
                        <button
                          type="button"
                          onClick={() => setEventType('device.metric')}
                          className={cn(
                            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors",
                            eventType === 'device.metric' ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                          )}
                        >
                          <Activity className="h-3.5 w-3.5" />
                          {tAgent('creator.schedule.event.events.device.metric')}
                        </button>
                        <button
                          type="button"
                          onClick={() => setEventType('manual')}
                          className={cn(
                            "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors",
                            eventType === 'manual' ? "bg-primary text-primary-foreground" : "hover:bg-muted border"
                          )}
                        >
                          <Clock className="h-3.5 w-3.5" />
                          {tAgent('creator.schedule.event.events.manual')}
                        </button>
                      </div>
                    </div>
                    {(eventType === 'device.online' || eventType === 'device.offline' || eventType === 'device.metric') && (
                      <div className="flex items-center gap-3">
                        <span className="text-sm text-muted-foreground">{tAgent('creator.schedule.event.relatedDevice')}</span>
                        <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                          <SelectTrigger className="w-40 h-10">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="all">{tAgent('creator.schedule.event.allDevices')}</SelectItem>
                            {devices.map((d) => (
                              <SelectItem key={d.device_id} value={d.device_id}>{d.name}</SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                    {eventType === 'manual' && (
                      <div className="text-sm text-muted-foreground">
                        {tAgent('creator.schedule.event.manualHint')}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {/* Data Strategy Hint */}
              <div className="flex items-center gap-2 text-xs text-muted-foreground bg-muted/30 rounded-md px-3 py-2">
                <BarChart3 className="h-3.5 w-3.5" />
                <span>
                  {scheduleType === 'interval' && tAgent('creator.dataStrategy.hint.interval', { minutes: Math.max(intervalValue * 2, 30) })}
                  {scheduleType === 'daily' && tAgent('creator.dataStrategy.hint.daily')}
                  {scheduleType === 'weekly' && tAgent('creator.dataStrategy.hint.weekly')}
                  {scheduleType === 'event' && tAgent('creator.dataStrategy.hint.event')}
                </span>
              </div>
            </div>

            {/* Device & Resources */}
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium">{tAgent('creator.resources.title')}</Label>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setShowDeviceList(true)}
                  className="h-8"
                >
                  <Plus className="h-3.5 w-3.5 mr-1" />
                  {tCommon('add')}
                </Button>
              </div>

              {/* Selected Resources Display */}
              {selectedResources.length === 0 ? (
                <div className="text-center py-12 border-2 border-dashed rounded-lg">
                  <Target className="h-10 w-10 mx-auto mb-3 text-muted-foreground/50" />
                  <p className="text-sm text-muted-foreground">{tAgent('creator.resources.selectDevice')}</p>
                </div>
              ) : (
                <div className="space-y-3">
                  {selectedResources.map((resource) => {
                    const device = devices.find(d => d.device_id === resource.deviceId)
                    if (!device) return null
                    const DeviceIcon = getDeviceIcon(device.device_type)

                    return (
                      <div key={resource.deviceId} className="border rounded-lg overflow-hidden">
                        {/* Device Header */}
                        <div className="px-4 py-2.5 bg-muted/30 flex items-center justify-between">
                          <div className="flex items-center gap-2">
                            <div className="p-1.5 rounded-md bg-background shadow-sm">{DeviceIcon}</div>
                            <span className="font-medium text-sm">{device.name}</span>
                            <span className="text-xs text-muted-foreground">({device.device_type})</span>
                          </div>
                          <div className="flex items-center gap-3">
                            <span className="text-xs text-muted-foreground">
                              {resource.metrics.length} {tAgent('creator.resources.metrics').toLowerCase()}, {resource.commands.length} {tAgent('creator.resources.commands').toLowerCase()}
                            </span>
                            <button
                              type="button"
                              onClick={() => clearDevice(resource.deviceId)}
                              className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-destructive transition-colors"
                            >
                              <X className="h-4 w-4" />
                            </button>
                          </div>
                        </div>

                        {/* Selected Metrics and Commands */}
                        <div className="p-3 flex flex-wrap gap-2">
                          {resource.metrics.map((m) => {
                            const configKey = `${resource.deviceId}:${m.name}`
                            const config = metricConfigs[configKey] || getRecommendedConfig()
                            return (
                              <Popover
                                key={m.name}
                                open={openMetricConfig === configKey}
                                onOpenChange={(open) => setOpenMetricConfig(open ? configKey : null)}
                              >
                                <PopoverTrigger asChild>
                                  <button
                                    type="button"
                                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-blue-50 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800 text-blue-700 dark:text-blue-400 text-xs hover:bg-blue-100 dark:hover:bg-blue-950/50 transition-colors cursor-help"
                                  >
                                    <BarChart3 className="h-3 w-3" />
                                    <span>{m.displayName}</span>
                                    <span className="opacity-60">({tAgent(`creator.metricConfig.timeRanges.${config.time_range_minutes}`)})</span>
                                    <button
                                      type="button"
                                      onClick={(e) => {
                                        e.stopPropagation()
                                        toggleMetric(resource.deviceId, m.name, m.displayName)
                                      }}
                                      className="p-0.5 rounded-sm hover:bg-blue-200 dark:hover:bg-blue-800 transition-colors"
                                    >
                                      <X className="h-3 w-3" />
                                    </button>
                                  </button>
                                </PopoverTrigger>
                                <PopoverContent className="w-48 p-3" align="start">
                                  <div className="space-y-2">
                                    <div className="text-xs font-medium">{m.displayName}</div>
                                    <div>
                                      <label className="text-xs text-muted-foreground">{tAgent('creator.metricConfig.timeRange')}</label>
                                      <Select
                                        value={config.time_range_minutes.toString()}
                                        onValueChange={(v) => {
                                          const minutes = parseInt(v)
                                          updateMetricConfig(configKey, getConfigFromTimeRange(minutes))
                                        }}
                                      >
                                        <SelectTrigger className="h-8 text-xs mt-1">
                                          <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                          <SelectItem value="30">{tAgent('creator.metricConfig.timeRanges.30')}</SelectItem>
                                          <SelectItem value="60">{tAgent('creator.metricConfig.timeRanges.60')}</SelectItem>
                                          <SelectItem value="180">{tAgent('creator.metricConfig.timeRanges.180')}</SelectItem>
                                          <SelectItem value="360">{tAgent('creator.metricConfig.timeRanges.360')}</SelectItem>
                                          <SelectItem value="720">{tAgent('creator.metricConfig.timeRanges.720')}</SelectItem>
                                          <SelectItem value="1440">{tAgent('creator.metricConfig.timeRanges.1440')}</SelectItem>
                                          <SelectItem value="10080">{tAgent('creator.metricConfig.timeRanges.10080')}</SelectItem>
                                        </SelectContent>
                                      </Select>
                                    </div>
                                    <div className="text-xs text-muted-foreground text-center pt-1 border-t">
                                      {config.time_range_minutes >= 10080 ? tAgent('creator.metricConfig.descriptions.fullWeek') :
                                       config.time_range_minutes >= 720 ? tAgent('creator.metricConfig.descriptions.fullDay') :
                                       config.time_range_minutes >= 180 ? tAgent('creator.metricConfig.descriptions.trend') :
                                       config.time_range_minutes >= 60 ? tAgent('creator.metricConfig.descriptions.history') :
                                       tAgent('creator.metricConfig.descriptions.baseline')}
                                    </div>
                                  </div>
                                </PopoverContent>
                              </Popover>
                            )
                          })}
                          {resource.commands.map((c) => (
                            <div key={c.name} className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-orange-50 dark:bg-orange-950/30 border border-orange-200 dark:border-orange-800 text-orange-700 dark:text-orange-400 text-xs">
                              <Zap className="h-3 w-3" />
                              <span>{c.displayName}</span>
                              <button
                                type="button"
                                onClick={() => toggleCommand(resource.deviceId, c.name, c.displayName)}
                                className="p-0.5 rounded-sm hover:bg-orange-200 dark:hover:bg-orange-800 transition-colors"
                              >
                                <X className="h-3 w-3" />
                              </button>
                            </div>
                          ))}
                        </div>
                      </div>
                    )
                  })}
                </div>
              )}
            </div>

          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t">
          {selectedCount > 0 && (
            <Badge variant="secondary">
              {tCommon('selected', { count: selectedCount })}
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

      {/* Device Selection Dialog */}
      <Dialog open={showDeviceList} onOpenChange={setShowDeviceList}>
        <DialogContent className="max-w-3xl max-h-[85vh] overflow-hidden flex flex-col">
          <DialogHeader>
            <DialogTitle>{tAgent('creator.resources.title')}</DialogTitle>
          </DialogHeader>

          <div className="flex-1 overflow-y-auto space-y-4 -mt-2">
            {/* Search */}
            <div className="relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={tAgent('creator.resources.searchPlaceholder')}
                className="pl-9"
              />
            </div>

            {/* Device Grid */}
            <div className="grid grid-cols-3 gap-3">
              {filteredDevices.length === 0 ? (
                <div className="col-span-full text-center py-8 text-muted-foreground text-sm">
                  {tAgent('creator.resources.noResourcesFound')}
                </div>
              ) : (
                filteredDevices.map((device) => {
                  const Icon = getDeviceIcon(device.device_type)
                  const hasSelection = selectedResources.find(r => r.deviceId === device.device_id)
                  const selectionCount = (hasSelection?.metrics.length || 0) + (hasSelection?.commands.length || 0)
                  const isSelected = selectedDeviceId === device.device_id

                  return (
                    <button
                      key={device.device_id}
                      type="button"
                      onClick={() => {
                        if (isSelected) {
                          setSelectedDeviceId('')
                        } else {
                          selectDevice(device.device_id)
                        }
                      }}
                      className={cn(
                        "p-3 rounded-lg border-2 text-left transition-all",
                        isSelected ? "border-primary bg-primary/5" : hasSelection ? "border-primary/50 bg-primary/5" : "border-border hover:border-muted-foreground/30"
                      )}
                    >
                      <div className="flex items-center gap-2 mb-2">
                        <div className={cn(
                          "p-1.5 rounded-md",
                          isSelected || hasSelection ? "bg-primary text-primary-foreground" : "bg-muted"
                        )}>
                          {Icon}
                        </div>
                        <span className="font-medium text-sm truncate flex-1">{device.name}</span>
                      </div>
                      <div className="flex items-center justify-between text-xs text-muted-foreground">
                        <span className="truncate">{device.device_type}</span>
                        {selectionCount > 0 && (
                          <Badge variant="secondary" className="text-xs h-5 ml-1 shrink-0">
                            {selectionCount}
                          </Badge>
                        )}
                      </div>
                    </button>
                  )
                })
              )}
            </div>

            {/* Device Detail Panel (when a device is selected) */}
            {selectedDeviceId && (() => {
              const device = devices.find(d => d.device_id === selectedDeviceId)
              if (!device) return null
              const metrics = getDeviceMetrics(selectedDeviceId)
              const commands = getDeviceCommands(selectedDeviceId)
              const isLoading = loadingMetrics[selectedDeviceId]

              if (isLoading) {
                return (
                  <div className="flex items-center justify-center py-8 border-t">
                    <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                  </div>
                )
              }

              if (metrics.length === 0 && commands.length === 0) {
                return (
                  <div className="text-center py-8 text-muted-foreground text-sm border-t">
                    {tAgent('creator.resources.noMetrics')}
                  </div>
                )
              }

              return (
                <div className="border-t pt-4 space-y-4">
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-sm flex items-center gap-2">
                      {getDeviceIcon(device.device_type)}
                      {device.name}
                    </span>
                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => selectAllForDevice(selectedDeviceId)}
                        className="text-xs px-3 py-1.5 rounded-md hover:bg-muted transition-colors text-primary"
                      >
                        {tCommon('selectAll')}
                      </button>
                      <button
                        type="button"
                        onClick={() => clearDevice(selectedDeviceId)}
                        className="text-xs px-3 py-1.5 rounded-md hover:bg-muted transition-colors text-muted-foreground"
                      >
                        {tCommon('clear')}
                      </button>
                    </div>
                  </div>

                  <div className="space-y-3">
                    {/* Metrics */}
                    {metrics.length > 0 && (
                      <div>
                        <div className="text-xs text-muted-foreground mb-2 flex items-center gap-1">
                          <BarChart3 className="h-3 w-3" />
                          {tAgent('creator.resources.metrics')}
                        </div>
                        <div className="grid grid-cols-3 gap-2">
                          {metrics.map((metric) => {
                            const isSelected = isMetricSelected(selectedDeviceId, metric.name)
                            return (
                              <button
                                key={metric.name}
                                type="button"
                                onClick={() => toggleMetric(selectedDeviceId, metric.name, metric.display_name)}
                                className={cn(
                                  "p-2 rounded-lg text-left transition-all text-sm",
                                  isSelected
                                    ? "bg-blue-600 dark:bg-blue-700 text-white"
                                    : "hover:bg-blue-50 dark:hover:bg-blue-950/30 border border-blue-200 dark:border-blue-800"
                                )}
                              >
                                <div className="flex items-center gap-1.5">
                                  {isSelected ? <Check className="h-3 w-3" /> : <div className="w-3 h-3 rounded border border-blue-300 dark:border-blue-700" />}
                                  <span className="flex-1 truncate">{metric.display_name}</span>
                                </div>
                              </button>
                            )
                          })}
                        </div>
                        <div className="text-xs text-muted-foreground mt-2 text-center">
                          {tAgent('creator.metricConfig.configureHint')}
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
                        <div className="grid grid-cols-3 gap-2">
                          {commands.map((command) => {
                            const isSelected = isCommandSelected(selectedDeviceId, command.name)
                            return (
                              <button
                                key={command.name}
                                type="button"
                                onClick={() => toggleCommand(selectedDeviceId, command.name, command.display_name)}
                                className={cn(
                                  "p-2 rounded-lg text-left transition-all text-sm",
                                  isSelected
                                    ? "bg-orange-600 dark:bg-orange-700 text-white"
                                    : "hover:bg-orange-50 dark:hover:bg-orange-950/30 border border-orange-200 dark:border-orange-800"
                                )}
                              >
                                <div className="flex items-center gap-1.5">
                                  {isSelected ? <Check className="h-3 w-3" /> : <div className="w-3 h-3 rounded border border-orange-300 dark:border-orange-700" />}
                                  <span className="flex-1 truncate">{command.display_name}</span>
                                </div>
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )
            })()}
          </div>

          {/* Dialog Footer */}
          <div className="flex items-center justify-end gap-3 pt-4 border-t">
            <div className="flex-1 text-sm text-muted-foreground">
              {selectedCount > 0 && (
                <Badge variant="secondary">{tCommon('selected', { count: selectedCount })}</Badge>
              )}
            </div>
            <Button variant="outline" onClick={() => setShowDeviceList(false)}>
              {tCommon('close')}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </Dialog>
  )
}

// Type imports
type AgentScheduleType = 'interval' | 'cron' | 'event' | 'once'
