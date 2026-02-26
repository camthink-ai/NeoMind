/**
 * Agent Editor Full Screen (Single Page Layout)
 *
 * All configuration in one page:
 * - Left: Basic info (name, description, prompt, schedule)
 * - Right: Selected resources with dialog to add more
 *
 * Features:
 * - Real-time summary preview
 * - AI-recommended resources based on prompt
 * - Resource selection in dialog
 * - Single-page, no wizard steps
 */

import React, { useState, useEffect, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { createPortal } from "react-dom"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useIsMobile, useSafeAreaInsets } from "@/hooks/useMobile"
import { useBodyScrollLock } from "@/hooks/useBodyScrollLock"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import * as DialogPrimitive from "@radix-ui/react-dialog"
import {
  Search,
  Loader2,
  Clock,
  Zap,
  Check,
  Target,
  Activity,
  BarChart3,
  X,
  Sparkles,
  Puzzle,
  Plus,
  Info,
  Wand2,
  ChevronRight,
  Bell,
  Lightbulb,
} from "lucide-react"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
  Extension,
  ExtensionDataSourceInfo,
  ExtensionCommandDescriptor,
} from "@/types"
import { AgentLogicPreview } from "./AgentLogicPreview"

interface AgentEditorFullScreenProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agent: AiAgentDetail | undefined
  devices: Device[]
  deviceTypes: DeviceType[]
  extensions?: Extension[]
  extensionDataSources?: ExtensionDataSourceInfo[]
  onSave: (data: CreateAgentRequest | Partial<AiAgentDetail>) => Promise<void>
}

// ============================================================================
// Types
// ============================================================================

interface MetricInfo {
  name: string
  display_name: string
  unit?: string
  data_type?: string
  source: 'device' | 'extension'
  extensionId?: string
}

interface CommandInfo {
  name: string
  display_name: string
  description?: string
  source: 'device' | 'extension'
  extensionId?: string
  parameters?: Record<string, unknown>
}

interface SelectedResource {
  id: string
  name: string
  type: 'device' | 'extension'
  deviceType?: string
  // All available metrics/commands
  allMetrics: MetricInfo[]
  allCommands: CommandInfo[]
  // Selected metric/command names
  selectedMetrics: Set<string>
  selectedCommands: Set<string>
}

interface ResourceRecommendation {
  id: string
  name: string
  type: 'device' | 'extension'
  reason: string
  metrics?: MetricInfo[]
  commands?: CommandInfo[]
}

type ScheduleType = 'interval' | 'daily' | 'weekly' | 'event'

// ============================================================================
// Constants
// ============================================================================

const INTERVALS = [5, 10, 15, 30, 60]
const HOURS = Array.from({ length: 24 }, (_, i) => i)

const PROMPT_TEMPLATES = [
  { id: 'empty', label: 'Custom', icon: null, description: 'Write your own prompt', template: '' },
  {
    id: 'monitor',
    label: 'Monitor',
    icon: <Activity className="h-3 w-3" />,
    description: 'Monitor data and detect anomalies',
    template: `Monitor the following metrics and alert when anomalies are detected:

Monitoring Targets:
- Data Range: Check if values exceed thresholds (e.g., temperature > 30°C)
- Anomaly Detection: Look for sudden changes or abnormal patterns
- Alert Method: Send notification when conditions are met

Please analyze the current data and compare with historical baselines to identify:
1. Values outside normal range
2. Sudden spikes or drops
3. Data gaps or missing readings

When an anomaly is detected, send an alert with:
- What metric is affected
- Current value vs expected range
- Severity level (info/warning/critical)`
  },
  {
    id: 'control',
    label: 'Control',
    icon: <Zap className="h-3 w-3" />,
    description: 'Automatically control devices',
    template: `Automatically control devices based on the following conditions:

Trigger Conditions:
- Check current sensor readings
- Compare against threshold values
- Verify device states before taking action

Control Actions:
- Device ID: [target device]
- Command: [turn_on / turn_off / adjust]
- Parameters: [any required settings]

Please:
1. First verify the current condition by checking sensor data
2. Only execute commands when the condition is clearly met
3. Confirm the action was successful
4. Avoid rapid repeated switching (add a cooldown between same actions)`
  },
  {
    id: 'analysis',
    label: 'Analysis',
    icon: <BarChart3 className="h-3 w-3" />,
    description: 'Analyze trends and generate reports',
    template: `Analyze the following data and generate a comprehensive report:

Analysis Scope:
- Time Range: Use available historical data
- Metrics: All selected metrics
- Comparison: Compare with previous periods if available

Report Contents:
1. **Data Overview**: Summary of current values and status
2. **Trend Analysis**: Increasing, decreasing, or stable patterns
3. **Anomalies**: Any unusual readings or deviations
4. **Correlations**: Relationships between different metrics
5. **Recommendations**: Actionable insights based on the data

Please provide specific numbers and percentages when describing trends and changes.`
  },
  {
    id: 'alert',
    label: 'Alert',
    icon: <Bell className="h-3 w-3" />,
    description: 'Send notifications based on conditions',
    template: `Monitor the selected metrics and send alerts when specific conditions occur:

Alert Conditions:
- Threshold exceeded: When metric goes above/below a value
- Rate of change: When value changes too quickly
- Status change: When device state changes

Alert Content:
- Which metric/device triggered the alert
- Current value and threshold
- Time of occurrence
- Suggested actions if applicable

Please avoid duplicate alerts - only alert when:
1. This is a new incident (not previously reported)
2. The condition has significantly worsened
3. A sufficient cooldown period has passed since the last alert`
  },
]

// ============================================================================
// Main Component
// ============================================================================

export function AgentEditorFullScreen({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  extensions = [],
  extensionDataSources = [],
  onSave,
}: AgentEditorFullScreenProps) {
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Lock body scroll when dialog is open (mobile only)
  useBodyScrollLock(open, { mobileOnly: true })

  // ========================================================================
  // State
  // ========================================================================

  // Basic info
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [userPrompt, setUserPrompt] = useState("")
  const [llmBackendId, setLlmBackendId] = useState<string | null>(null)

  // Schedule state
  const [scheduleType, setScheduleType] = useState<ScheduleType>('interval')
  const [intervalValue, setIntervalValue] = useState(5)
  const [scheduleHour, setScheduleHour] = useState(9)
  const [scheduleMinute, setScheduleMinute] = useState(0)
  const [selectedWeekdays, setSelectedWeekdays] = useState<number[]>([1])

  // Event trigger state
  const [eventConfig, setEventConfig] = useState<{
    type: 'device.metric' | 'manual'
    deviceId?: string
  }>({
    type: 'device.metric',
    deviceId: 'all',
  })

  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [resourceDialogOpen, setResourceDialogOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState("")
  const [recommendations, setRecommendations] = useState<ResourceRecommendation[]>([])
  const [generatingRecommendations, setGeneratingRecommendations] = useState(false)

  // UI state
  const [saving, setSaving] = useState(false)

  // LLM backends
  const [llmBackends, setLlmBackends] = useState<any[]>([])
  const [activeBackendId, setActiveBackendId] = useState<string | null>(null)

  // Advanced configuration state
  const [enableToolChaining, setEnableToolChaining] = useState(false)
  const [maxChainDepth, setMaxChainDepth] = useState(3)
  const [priority, setPriority] = useState(5)
  const [contextWindowSize, setContextWindowSize] = useState(8192)
  const [showAdvanced, setShowAdvanced] = useState(false)

  // LLM validation state
  const [llmValidating, setLlmValidating] = useState(false)
  const [llmValid, setLlmValid] = useState<boolean | null>(null)
  const [llmValidationError, setLlmValidationError] = useState<string | null>(null)

  // ========================================================================
  // Effects
  // ========================================================================

  // Load LLM backends
  const loadBackends = useCallback(async () => {
    try {
      const response = await api.listLlmBackends()
      setLlmBackends(response.backends)
      setActiveBackendId(response.active_id)
    } catch (e) {
      console.error("Failed to load LLM backends:", e)
    }
  }, [])

  useEffect(() => {
    if (open) {
      loadBackends()
    }
  }, [open, loadBackends])

  // Reset/Load form when dialog opens
  useEffect(() => {
    if (open) {
      if (agent) {
        // Edit mode
        setName(agent.name || '')
        setDescription(agent.description || '')
        setUserPrompt(agent.user_prompt || '')
        setLlmBackendId(agent.llm_backend_id || null)
        // Load advanced config from agent
        setEnableToolChaining(agent.enable_tool_chaining ?? false)
        setMaxChainDepth(agent.max_chain_depth ?? 3)
        setPriority(agent.priority ?? 5)
        setContextWindowSize(agent.context_window_size ?? 8192)
        parseSchedule(agent.schedule)
        loadAgentResources(agent)
      } else {
        // Create mode - reset
        setName("")
        setDescription("")
        setUserPrompt("")
        setLlmBackendId(null)
        // Reset to defaults
        setEnableToolChaining(false)
        setMaxChainDepth(3)
        setPriority(5)
        setContextWindowSize(8192)
        setShowAdvanced(false)
        setScheduleType('interval')
        setIntervalValue(5)
        setScheduleHour(9)
        setScheduleMinute(0)
        setSelectedWeekdays([1])
        setEventConfig({ type: 'device.metric', deviceId: 'all' })
        setSelectedResources([])
        setRecommendations([])
        setSearchQuery("")
        setLlmValid(null)
        setLlmValidationError(null)
      }
    }
  }, [agent, open])

  // Generate recommendations when prompt changes or dialog opens
  useEffect(() => {
    if ((userPrompt.length > 20 || resourceDialogOpen) && !agent) {
      generateRecommendations()
    }
  }, [userPrompt, resourceDialogOpen])

  // ========================================================================
  // Helpers
  // ========================================================================

  const parseSchedule = (schedule: any) => {
    if (!schedule) return
    if (schedule.schedule_type === 'interval') {
      setScheduleType('interval')
      if (schedule.interval_seconds) {
        setIntervalValue(Math.floor(schedule.interval_seconds / 60))
      }
    } else if (schedule.schedule_type === 'cron') {
      if (schedule.cron_expression) {
        const parts = schedule.cron_expression.split(' ')
        if (parts.length === 5) {
          setScheduleMinute(parseInt(parts[0]) || 0)
          setScheduleHour(parseInt(parts[1]) || 9)
          if (parts[4] !== '*') {
            setScheduleType('weekly')
            const days: number[] = []
            if (parts[4].includes(',')) {
              parts[4].split(',').forEach((d: string) => {
                const num = parseInt(d)
                if (!isNaN(num)) days.push(num)
              })
            } else {
              const num = parseInt(parts[4])
              if (!isNaN(num)) days.push(num)
            }
            if (days.length > 0) setSelectedWeekdays(days)
          } else {
            setScheduleType('daily')
          }
        } else if (parts.length >= 6) {
          setScheduleMinute(parseInt(parts[1]) || 0)
          setScheduleHour(parseInt(parts[2]) || 9)
          if (parts[5] !== '*') {
            setScheduleType('weekly')
            const days: number[] = []
            if (parts[5].includes(',')) {
              parts[5].split(',').forEach((d: string) => {
                const num = parseInt(d)
                if (!isNaN(num)) days.push(num)
              })
            } else {
              const num = parseInt(parts[5])
              if (!isNaN(num)) days.push(num)
            }
            if (days.length > 0) setSelectedWeekdays(days)
          } else {
            setScheduleType('daily')
          }
        }
      }
    } else if (schedule.schedule_type === 'event') {
      setScheduleType('event')
      try {
        const eventFilter = JSON.parse(schedule.event_filter || '{}')
        const eventType = eventFilter.event_type || 'device.metric'
        if (eventType === 'manual') {
          setEventConfig({ type: 'manual' })
        } else {
          setEventConfig({
            type: 'device.metric',
            deviceId: eventFilter.device_id || 'all',
          })
        }
      } catch {
        setEventConfig({ type: 'device.metric', deviceId: 'all' })
      }
    }
  }

  const loadAgentResources = async (agent: AiAgentDetail) => {
    const resourcesMap = new Map<string, SelectedResource>()
    const deviceTypeMap = new Map(deviceTypes.map(dt => [dt.device_type, dt]))

    // Collect all selected metric and command names per device/extension
    const selectedMetricNames = new Map<string, Set<string>>()
    const selectedCommandNames = new Map<string, Set<string>>()

    // Collect extension metrics/commands separately (with extension:extension_id as key)
    const extMetricNames = new Map<string, Set<string>>()
    const extCommandNames = new Map<string, Set<string>>()

    for (const res of agent.resources || []) {
      const parts = res.resource_id.split(':')

      if (res.resource_type === 'metric') {
        // Device metric format: device_id:metric_name
        const deviceId = parts[0]
        const itemName = parts[1]
        if (!selectedMetricNames.has(deviceId)) {
          selectedMetricNames.set(deviceId, new Set())
        }
        selectedMetricNames.get(deviceId)!.add(itemName)
      } else if (res.resource_type === 'command') {
        // Device command format: device_id:command_name
        const deviceId = parts[0]
        const itemName = parts[1]
        if (!selectedCommandNames.has(deviceId)) {
          selectedCommandNames.set(deviceId, new Set())
        }
        selectedCommandNames.get(deviceId)!.add(itemName)
      } else if (res.resource_type === 'extension_metric') {
        // Extension metric format: extension:extension_id:metric_name
        if (parts.length >= 3 && parts[0] === 'extension') {
          const extId = `extension:${parts[1]}`
          const itemName = parts[2]
          if (!extMetricNames.has(extId)) {
            extMetricNames.set(extId, new Set())
          }
          extMetricNames.get(extId)!.add(itemName)
        }
      } else if (res.resource_type === 'extension_tool') {
        // Extension tool format: extension:extension_id:command_name
        if (parts.length >= 3 && parts[0] === 'extension') {
          const extId = `extension:${parts[1]}`
          const itemName = parts[2]
          if (!extCommandNames.has(extId)) {
            extCommandNames.set(extId, new Set())
          }
          extCommandNames.get(extId)!.add(itemName)
        }
      }
    }

    // Build device resources
    for (const [deviceId, selectedMetrics] of selectedMetricNames) {
      const device = devices.find(d => d.id === deviceId)
      if (!device) continue

      const deviceType = deviceTypeMap.get(device.device_type)
      if (!deviceType) continue

      const allMetrics = deviceType?.metrics?.map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        unit: m.unit,
        data_type: m.data_type,
        source: 'device' as const,
      })) || []

      const allCommands = deviceType?.commands?.map(c => ({
        name: c.name,
        display_name: c.display_name || c.name,
        description: c.llm_hints,
        source: 'device' as const,
      })) || []

      const selectedCmds = selectedCommandNames.get(deviceId) || new Set()

      resourcesMap.set(deviceId, {
        id: deviceId,
        name: device.name,
        type: 'device',
        deviceType: device.device_type,
        allMetrics,
        allCommands,
        selectedMetrics: selectedMetrics,
        selectedCommands: selectedCmds,
      })
    }

    // Build extension resources
    for (const [extKey, selectedMetrics] of extMetricNames) {
      const extId = extKey.replace('extension:', '')
      const extension = extensions.find(e => e.id === extId)
      if (!extension) continue

      const allMetrics = extension.metrics?.map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        unit: m.unit,
        data_type: m.data_type,
        source: 'extension' as const,
        extensionId: extId,
      })) || []

      const allCommands = extension.commands?.map(c => ({
        name: c.id,
        display_name: c.display_name,
        description: c.description,
        source: 'extension' as const,
        extensionId: extId,
      })) || []

      const selectedCmds = extCommandNames.get(extKey) || new Set()

      resourcesMap.set(extKey, {
        id: extKey,
        name: extension.name,
        type: 'extension',
        allMetrics,
        allCommands,
        selectedMetrics: selectedMetrics,
        selectedCommands: selectedCmds,
      })
    }

    // Also handle extensions that only have commands (no metrics)
    for (const [extKey, selectedCmds] of extCommandNames) {
      if (resourcesMap.has(extKey)) continue // Already processed above

      const extId = extKey.replace('extension:', '')
      const extension = extensions.find(e => e.id === extId)
      if (!extension) continue

      const allMetrics = extension.metrics?.map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        unit: m.unit,
        data_type: m.data_type,
        source: 'extension' as const,
        extensionId: extId,
      })) || []

      const allCommands = extension.commands?.map(c => ({
        name: c.id,
        display_name: c.display_name,
        description: c.description,
        source: 'extension' as const,
        extensionId: extId,
      })) || []

      resourcesMap.set(extKey, {
        id: extKey,
        name: extension.name,
        type: 'extension',
        allMetrics,
        allCommands,
        selectedMetrics: new Set(),
        selectedCommands: selectedCmds,
      })
    }

    setSelectedResources(Array.from(resourcesMap.values()))
  }

  const generateRecommendations = async () => {
    setGeneratingRecommendations(true)
    try {
      // Simple keyword-based recommendation (can be enhanced with AI)
      const recs: ResourceRecommendation[] = []
      const prompt = userPrompt.toLowerCase()
      const deviceTypeMap = new Map(deviceTypes.map(dt => [dt.device_type, dt]))

      // Analyze prompt for keywords
      const tempKeywords = ['temperature', 'temp', '热', '温', 'climate', 'climate']
      const humidityKeywords = ['humidity', 'humid', '湿度', 'moisture']
      const lightKeywords = ['light', 'lamp', '灯', '亮度', 'brightness']
      const motionKeywords = ['motion', 'move', 'movement', '移动', '人', 'person']
      const alertKeywords = ['alert', 'notify', '告警', '通知', 'send', 'push']

      devices.forEach(device => {
        const deviceType = deviceTypeMap.get(device.device_type)
        if (!deviceType) return

        let reason = ''
        let shouldRecommend = false
        const metrics: MetricInfo[] = []
        const commands: CommandInfo[] = []

        // Check metrics
        deviceType.metrics?.forEach(m => {
          const metricName = m.name.toLowerCase() + ' ' + (m.display_name || '').toLowerCase()
          if (tempKeywords.some(k => prompt.includes(k) || metricName.includes(k))) {
            metrics.push({
              name: m.name,
              display_name: m.display_name || m.name,
              unit: m.unit,
              data_type: m.data_type,
              source: 'device',
            })
            shouldRecommend = true
            reason = 'Temperature monitoring'
          }
          if (humidityKeywords.some(k => prompt.includes(k) || metricName.includes(k))) {
            metrics.push({
              name: m.name,
              display_name: m.display_name || m.name,
              unit: m.unit,
              data_type: m.data_type,
              source: 'device',
            })
            shouldRecommend = true
            reason = 'Humidity monitoring'
          }
          if (lightKeywords.some(k => prompt.includes(k) || metricName.includes(k))) {
            metrics.push({
              name: m.name,
              display_name: m.display_name || m.name,
              unit: m.unit,
              data_type: m.data_type,
              source: 'device',
            })
            shouldRecommend = true
            reason = 'Light control'
          }
        })

        // Check commands
        deviceType.commands?.forEach(c => {
          if (lightKeywords.some(k => prompt.includes(k))) {
            commands.push({
              name: c.name,
              display_name: c.display_name || c.name,
              description: c.llm_hints,
              source: 'device',
            })
            shouldRecommend = true
          }
        })

        if (shouldRecommend) {
          recs.push({
            id: device.id,
            name: device.name,
            type: 'device',
            reason,
            metrics,
            commands,
          })
        }
      })

      setRecommendations(recs)
    } catch (e) {
      console.error("Failed to generate recommendations:", e)
    } finally {
      setGeneratingRecommendations(false)
    }
  }

  // ========================================================================
  // Derived State
  // ========================================================================

  const deviceTypeMap = useMemo(() => {
    const map = new Map<string, DeviceType>()
    deviceTypes.forEach(dt => map.set(dt.device_type, dt))
    return map
  }, [deviceTypes])

  const availableResources = useMemo((): AvailableResource[] => {
    const resources: AvailableResource[] = []

    // Add devices
    devices.forEach(device => {
      const deviceType = deviceTypeMap.get(device.device_type)
      const metrics: MetricInfo[] = []
      const commands: CommandInfo[] = []

      if (deviceType?.metrics) {
        deviceType.metrics.forEach(m => {
          metrics.push({
            name: m.name,
            display_name: m.display_name || m.name,
            unit: m.unit,
            data_type: m.data_type,
            source: 'device',
          })
        })
      }
      if (deviceType?.commands) {
        deviceType.commands.forEach(c => {
          commands.push({
            name: c.name,
            display_name: c.display_name || c.name,
            description: c.llm_hints,
            source: 'device',
          })
        })
      }

      resources.push({
        id: device.id,
        name: device.name,
        type: 'device',
        deviceType: device.device_type,
        metrics,
        commands,
      })
    })

    // Add extensions
    extensions.forEach(ext => {
      const metrics: MetricInfo[] = ext.metrics.map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        unit: m.unit,
        data_type: m.data_type,
        source: 'extension',
        extensionId: ext.id,
      }))

      const commands: CommandInfo[] = ext.commands.map(c => ({
        name: c.id,
        display_name: c.display_name,
        description: c.description,
        source: 'extension',
        extensionId: ext.id,
      }))

      resources.push({
        id: `extension:${ext.id}`,
        name: ext.name,
        type: 'extension',
        metrics,
        commands,
      })
    })

    return resources
  }, [devices, deviceTypes, extensions])

  const filteredResources = useMemo(() => {
    if (!searchQuery) return availableResources
    const query = searchQuery.toLowerCase()
    return availableResources.filter(r => {
      const searchableFields = [
        r.name,
        r.deviceType || '',
        ...r.metrics.map(m => m.display_name),
        ...r.commands.map(c => c.display_name),
      ].join(' ').toLowerCase()
      return searchableFields.includes(query)
    })
  }, [availableResources, searchQuery])

  const agentSummary = useMemo(() => {
    if (!name && !userPrompt) return null

    const parts = []
    if (name) parts.push(`"${name}"`)
    if (scheduleType === 'interval') parts.push(`runs every ${intervalValue} minutes`)
    if (scheduleType === 'daily') parts.push(`runs daily at ${scheduleHour}:${scheduleMinute.toString().padStart(2, '0')}`)
    if (scheduleType === 'weekly') parts.push(`runs weekly on ${selectedWeekdays.map(d => ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][d]).join(', ')} at ${scheduleHour}:${scheduleMinute.toString().padStart(2, '0')}`)
    if (scheduleType === 'event' && eventConfig.type === 'device.metric') parts.push(`triggers on ${eventConfig.deviceId === 'all' ? 'any' : 'specific'} device metric updates`)
    if (scheduleType === 'event' && eventConfig.type === 'manual') parts.push(`runs manually`)

    const selectedDeviceCount = selectedResources.filter(r => r.type === 'device').length
    const selectedExtCount = selectedResources.filter(r => r.type === 'extension').length
    const metricCount = selectedResources.reduce((sum, r) => sum + r.selectedMetrics.size, 0)
    const commandCount = selectedResources.reduce((sum, r) => sum + r.selectedCommands.size, 0)

    if (selectedDeviceCount > 0 || selectedExtCount > 0) {
      parts.push(`monitors ${selectedDeviceCount} device(s) and ${selectedExtCount} extension(s)`)
      parts.push(`tracks ${metricCount} metric(s) and ${commandCount} command(s)`)
    }

    return parts.join(', ')
  }, [name, scheduleType, intervalValue, scheduleHour, scheduleMinute, selectedWeekdays, eventConfig, selectedResources])

  // Validation - name and prompt are required
  // Metric selection is optional for event-triggered agents (device-level deduplication prevents loops)
  const isValid: boolean = name.trim().length > 0 && userPrompt.trim().length > 0

  // ========================================================================
  // Handlers
  // ========================================================================

  // Validate LLM backend availability
  const handleValidateLlm = async () => {
    setLlmValidating(true)
    setLlmValid(null)
    setLlmValidationError(null)
    try {
      const result = await api.validateLlmBackend({
        backend_id: llmBackendId || undefined,
      })
      setLlmValid(result.valid)
      if (!result.valid) {
        setLlmValidationError(result.error || 'LLM validation failed')
      }
      toast({
        title: result.valid ? tCommon('success') : tCommon('failed'),
        description: result.valid
          ? `LLM is available: ${result.backend_name || 'Unknown'} (${result.model || 'Unknown'})`
          : result.error || 'LLM validation failed',
        variant: result.valid ? 'default' : 'destructive',
      })
    } catch (error) {
      setLlmValid(false)
      setLlmValidationError((error as Error).message)
      handleError(error, { operation: 'Validate LLM', showToast: false })
    } finally {
      setLlmValidating(false)
    }
  }

  const handleSave = async () => {
    if (!isValid) return

    setSaving(true)
    try {
      let cronExpression: string | undefined = undefined
      let intervalSeconds: number | undefined = undefined
      let finalScheduleType: 'interval' | 'cron' | 'event' = 'interval'
      let eventFilter: any = undefined

      if (scheduleType === 'interval') {
        intervalSeconds = intervalValue * 60
      } else if (scheduleType === 'daily') {
        cronExpression = `0 ${scheduleMinute} ${scheduleHour} * * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'weekly') {
        const sortedDays = [...selectedWeekdays].sort((a, b) => a - b)
        cronExpression = `0 ${scheduleMinute} ${scheduleHour} * * ${sortedDays.join(',')}`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'event') {
        finalScheduleType = 'event'
        if (eventConfig.type === 'device.metric') {
          // Device-level event trigger: agent executes when device data updates
          // Agent will fetch data from Resources configured separately
          eventFilter = JSON.stringify({
            event_type: 'device.metric',
            device_id: eventConfig.deviceId === 'all' ? undefined : eventConfig.deviceId,
          })
        } else {
          eventFilter = JSON.stringify({ event_type: 'manual' })
        }
      }

      // Build resources array in the new format that supports both devices and extensions
      const resources = selectedResources.flatMap(r => {
        const result: any[] = []

        // Add metrics
        Array.from(r.selectedMetrics).forEach(metricName => {
          const metric = r.allMetrics.find(m => m.name === metricName)
          if (r.type === 'extension') {
            // Extension metric format: extension:extension_id:metric_name
            // Note: r.id already contains "extension:" prefix, so we use it directly
            result.push({
              resource_id: `${r.id}:${metricName}`,
              resource_type: 'extension_metric',
              name: metric?.display_name || metricName,
              config: {
                extension_id: r.id.replace('extension:', ''),
                metric_name: metricName,
              },
            })
          } else {
            // Device metric format: device_id:metric_name
            result.push({
              resource_id: `${r.id}:${metricName}`,
              resource_type: 'metric',
              name: metric?.display_name || metricName,
              config: {
                device_id: r.id,
                metric_name: metricName,
              },
            })
          }
        })

        // Add commands/tools
        Array.from(r.selectedCommands).forEach(commandName => {
          const command = r.allCommands.find(c => c.name === commandName)
          if (r.type === 'extension') {
            // Extension tool format: extension:extension_id:command_name
            // Note: r.id already contains "extension:" prefix, so we use it directly
            result.push({
              resource_id: `${r.id}:${commandName}`,
              resource_type: 'extension_tool',
              name: command?.display_name || commandName,
              config: {
                extension_id: r.id.replace('extension:', ''),
                command_name: commandName,
                parameters: command?.parameters || {},
              },
            })
          } else {
            // Device command format: device_id:command_name
            result.push({
              resource_id: `${r.id}:${commandName}`,
              resource_type: 'command',
              name: command?.display_name || commandName,
              config: {
                device_id: r.id,
                command_name: commandName,
                parameters: command?.parameters || {},
              },
            })
          }
        })

        return result
      })

      // Also provide legacy format for backward compatibility
      const deviceIds = selectedResources
        .filter(r => r.type === 'device')
        .map(r => r.id)

      const metrics = selectedResources.flatMap(r =>
        Array.from(r.selectedMetrics).map(metricName => {
          const metric = r.allMetrics.find(m => m.name === metricName)
          return {
            device_id: r.id,
            metric_name: metricName,
            display_name: metric?.display_name || metricName,
          }
        })
      )

      const commands = selectedResources.flatMap(r =>
        Array.from(r.selectedCommands).map(commandName => {
          const command = r.allCommands.find(c => c.name === commandName)
          return {
            device_id: r.id,
            command_name: commandName,
            display_name: command?.display_name || commandName,
            parameters: {} as Record<string, unknown>,
          }
        })
      )

      const data: CreateAgentRequest = {
        name: name.trim(),
        description: description.trim(),
        user_prompt: userPrompt.trim(),
        llm_backend_id: llmBackendId ?? undefined,
        // Use new resources format
        resources,
        // Legacy format (optional, for backward compatibility)
        device_ids: deviceIds.length > 0 ? deviceIds : undefined,
        metrics: metrics.length > 0 ? metrics : undefined,
        commands: commands.length > 0 ? commands : undefined,
        schedule: {
          schedule_type: finalScheduleType,
          interval_seconds: intervalSeconds,
          cron_expression: cronExpression,
          event_filter: eventFilter,
        },
        // Advanced configuration
        enable_tool_chaining: enableToolChaining || undefined,
        max_chain_depth: enableToolChaining ? maxChainDepth : undefined,
        priority: priority !== 5 ? priority : undefined,
        context_window_size: contextWindowSize !== 8192 ? contextWindowSize : undefined,
      }

      await onSave(data)
      onOpenChange(false)
      toast({ title: tCommon('success'), description: agent ? tAgent('agentUpdated') : tAgent('agentCreated') })
    } catch (error) {
      handleError(error, { operation: 'Save agent', showToast: false })
      toast({ title: tCommon('failed'), description: (error as Error).message, variant: 'destructive' })
    } finally {
      setSaving(false)
    }
  }

  const toggleRecommendation = (rec: ResourceRecommendation) => {
    const existing = selectedResources.find(r => r.id === rec.id)
    if (existing) {
      setSelectedResources(prev => prev.filter(r => r.id !== rec.id))
    } else {
      // Add with all recommended metrics/commands selected
      const allMetrics = rec.metrics || []
      const allCommands = rec.commands || []
      setSelectedResources(prev => [...prev, {
        id: rec.id,
        name: rec.name,
        type: rec.type,
        allMetrics,
        allCommands,
        selectedMetrics: new Set(allMetrics.map(m => m.name)),
        selectedCommands: new Set(allCommands.map(c => c.name)),
      }])
    }
  }

  const toggleResource = (resource: AvailableResource) => {
    const existing = selectedResources.find(r => r.id === resource.id)
    if (existing) {
      setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
    } else {
      // Add with NO metrics/commands selected by default - user must explicitly select
      setSelectedResources(prev => [...prev, {
        id: resource.id,
        name: resource.name,
        type: resource.type,
        deviceType: resource.deviceType,
        allMetrics: resource.metrics,
        allCommands: resource.commands,
        selectedMetrics: new Set(), // Empty by default - user must select explicitly
        selectedCommands: new Set(), // Empty by default - user must select explicitly
      }])
    }
  }

  // ========================================================================
  // Render
  // ========================================================================

  // Get dialog root for portal rendering
  const dialogRoot = typeof document !== 'undefined'
    ? document.getElementById('dialog-root') || document.body
    : null

  if (!dialogRoot) return null

  return createPortal(
    <>
      <div
        className={cn(
          "fixed inset-0 z-[100] bg-background flex flex-col",
          !open && "hidden"
        )}
      >
        {/* Header */}
        <header
          className="border-b shrink-0 bg-background"
          style={isMobile ? { paddingTop: `${insets.top}px` } : undefined}
        >
          <div className={cn(
            "flex items-center gap-3",
            isMobile ? "px-4 py-4" : "px-4 py-3"
          )}>
            <Button
              variant="ghost"
              size="icon"
              className={cn(
                "shrink-0",
                isMobile ? "h-10 w-10" : "h-8 w-8"
              )}
              onClick={() => onOpenChange(false)}
            >
              <X className={cn(isMobile ? "h-5 w-5" : "h-4 w-4")} />
            </Button>
            <div className="flex items-center gap-2 min-w-0 flex-1">
              <div className={cn(
                "rounded-lg bg-purple-500/10 flex items-center justify-center shrink-0",
                isMobile ? "w-8 h-8" : "w-7 h-7"
              )}>
                <Sparkles className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5", "text-purple-500")} />
              </div>
              <h1 className={cn(
                "font-medium truncate",
                isMobile ? "text-base" : "text-sm"
              )}>
                {agent ? tAgent('editAgent') : tAgent('createAgent')}
              </h1>
            </div>
          </div>
        </header>

        {/* Two Column Content: Preview (Left, narrow) + Config (Right, wide) */}
        <main className="flex-1 overflow-hidden">
          <div className="h-full grid grid-cols-1 lg:grid-cols-12 gap-0">
            {/* Left: Logic Preview (narrower) */}
            <div className="hidden lg:block lg:col-span-4 border-r overflow-hidden">
              <AgentLogicPreview props={{
                name,
                userPrompt,
                scheduleType,
                intervalValue,
                scheduleHour,
                scheduleMinute,
                selectedWeekdays,
                eventConfig,
                selectedResources,
                isValid,
              }} />
            </div>

            {/* Right: Configuration Form (wider) */}
            <div className="col-span-12 lg:col-span-8 overflow-y-auto">
              <div className={cn(
                "space-y-6",
                isMobile ? "px-4 py-6" : "px-4 py-6"
              )}>
            {/* Name */}
            <div className="space-y-2">
              <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                {tAgent('creator.basicInfo.name')} <span className="text-destructive">*</span>
              </Label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={tAgent('creator.basicInfo.namePlaceholder')}
                className={cn(isMobile ? "h-12 text-base" : "h-10")}
              />
            </div>

            {/* Description (Optional) */}
            <div className="space-y-2">
              <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>{tAgent('creator.basicInfo.description')}</Label>
              <Input
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={tAgent('creator.basicInfo.descriptionPlaceholder')}
                className={cn(isMobile ? "h-12 text-base" : "h-10")}
              />
            </div>

            {/* Prompt */}
            <div className="space-y-3">
              <Label className="text-sm font-medium">
                {tAgent('creator.basicInfo.requirement')} <span className="text-destructive">*</span>
              </Label>

              {/* Quick templates */}
              <div className="flex gap-2 flex-wrap">
                {PROMPT_TEMPLATES.filter(t => t.id !== 'empty').map((template) => (
                  <button
                    key={template.id}
                    type="button"
                    onClick={() => setUserPrompt(template.template)}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border hover:bg-muted transition-colors"
                  >
                    {template.icon}
                    <span>{template.label}</span>
                  </button>
                ))}
              </div>

              <Textarea
                value={userPrompt}
                onChange={(e) => setUserPrompt(e.target.value)}
                placeholder={tAgent('creator.basicInfo.promptPlaceholder')}
                className="min-h-[140px] resize-y text-sm leading-relaxed"
              />

              {/* AI Helper Tip */}
              <div className="flex items-start gap-2 p-3 bg-primary/5 rounded-lg border border-primary/20">
                <Wand2 className="h-4 w-4 text-primary shrink-0 mt-0.5" />
                <p className="text-xs text-muted-foreground">
                  <span className="font-medium text-primary">Tip:</span> {tAgent('creator.basicInfo.promptTip')}
                </p>
              </div>
            </div>

            {/* Model Selection */}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium">{tAgent('creator.basicInfo.llmBackend')}</Label>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs"
                  onClick={handleValidateLlm}
                  disabled={llmValidating}
                >
                  {llmValidating ? (
                    <Loader2 className="h-3 w-3 mr-1 animate-spin" />
                  ) : llmValid === true ? (
                    <Check className="h-3 w-3 mr-1 text-green-500" />
                  ) : llmValid === false ? (
                    <span className="text-destructive">!</span>
                  ) : null}
                  {llmValidating ? 'Checking...' : llmValid === true ? 'OK' : llmValid === false ? 'Failed' : 'Test'}
                </Button>
              </div>
              <Select value={llmBackendId ?? activeBackendId ?? ''} onValueChange={setLlmBackendId}>
                <SelectTrigger className="h-10">
                  <SelectValue placeholder={tAgent('creator.basicInfo.defaultBackend')} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">{tAgent('creator.basicInfo.defaultBackend')}</SelectItem>
                  {llmBackends.map((backend) => (
                    <SelectItem key={backend.id} value={backend.id}>{backend.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {llmValidationError && (
                <p className="text-xs text-destructive">{llmValidationError}</p>
              )}
            </div>

            {/* Advanced Configuration */}
            <div className="space-y-3">
              <div
                className="flex items-center justify-between cursor-pointer"
                onClick={() => setShowAdvanced(!showAdvanced)}
              >
                <Label className="text-sm font-medium flex items-center gap-2">
                  <Lightbulb className="h-4 w-4 text-muted-foreground" />
                  Advanced Configuration
                </Label>
                <ChevronRight
                  className={cn(
                    "h-4 w-4 text-muted-foreground transition-transform",
                    showAdvanced && "rotate-90"
                  )}
                />
              </div>

              {showAdvanced && (
                <div className="bg-muted/50 rounded-xl p-4 border space-y-4">
                  {/* Tool Chaining */}
                  <div className="space-y-3">
                    <div className="flex items-center justify-between">
                      <div className="space-y-0.5">
                        <Label className="text-sm font-medium">Enable Tool Chaining</Label>
                        <p className="text-xs text-muted-foreground">
                          Allow the agent to chain multiple tool calls for complex tasks
                        </p>
                      </div>
                      <button
                        type="button"
                        onClick={() => setEnableToolChaining(!enableToolChaining)}
                        className={cn(
                          "relative inline-flex h-5 w-9 items-center rounded-full transition-colors",
                          enableToolChaining ? "bg-primary" : "bg-input"
                        )}
                      >
                        <span
                          className={cn(
                            "inline-block h-4 w-4 transform rounded-full bg-white transition-transform",
                            enableToolChaining ? "translate-x-5" : "translate-x-0.5"
                          )}
                        />
                      </button>
                    </div>

                    {enableToolChaining && (
                      <div className="space-y-2">
                        <Label className="text-xs text-muted-foreground">Max Chain Depth</Label>
                        <div className="flex items-center gap-2">
                          <input
                            type="range"
                            min="1"
                            max="10"
                            value={maxChainDepth}
                            onChange={(e) => setMaxChainDepth(parseInt(e.target.value))}
                            className="flex-1 h-2 bg-muted rounded-lg appearance-none cursor-pointer"
                          />
                          <span className="text-sm font-medium w-8 text-center">{maxChainDepth}</span>
                        </div>
                        <p className="text-xs text-muted-foreground">
                          Maximum number of sequential tool calls (1-10)
                        </p>
                      </div>
                    )}
                  </div>

                  <div className="h-px bg-border" />

                  {/* Agent Priority */}
                  <div className="space-y-2">
                    <Label className="text-sm font-medium">Agent Priority</Label>
                    <div className="flex items-center gap-2">
                      <input
                        type="range"
                        min="1"
                        max="10"
                        value={priority}
                        onChange={(e) => setPriority(parseInt(e.target.value))}
                        className="flex-1 h-2 bg-muted rounded-lg appearance-none cursor-pointer"
                      />
                      <span className="text-sm font-medium w-8 text-center">{priority}</span>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Execution priority (1=lowest, 10=highest)
                    </p>
                  </div>

                  <div className="h-px bg-border" />

                  {/* Context Window Size */}
                  <div className="space-y-2">
                    <Label className="text-sm font-medium">Context Window Size</Label>
                    <div className="flex items-center gap-2">
                      <Select value={contextWindowSize.toString()} onValueChange={(v) => setContextWindowSize(parseInt(v))}>
                        <SelectTrigger className="h-9 flex-1">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="4096">4,096 tokens</SelectItem>
                          <SelectItem value="8192">8,192 tokens</SelectItem>
                          <SelectItem value="16384">16,384 tokens</SelectItem>
                          <SelectItem value="32768">32,768 tokens</SelectItem>
                          <SelectItem value="65536">65,536 tokens</SelectItem>
                          <SelectItem value="131072">131,072 tokens</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Maximum context size for the LLM
                    </p>
                  </div>
                </div>
              )}
            </div>

            {/* Execution Schedule */}
            <div className="space-y-3">
              <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>{tAgent('creator.basicInfo.scheduleLabel')}</Label>

              {/* Strategy Cards */}
              <div className={cn(
                "gap-2",
                isMobile ? "grid grid-cols-2" : "grid grid-cols-4"
              )}>
                <ScheduleCard
                  icon={<Clock className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.interval')}
                  description={tAgent('creator.schedule.interval.preview', { value: intervalValue, unit: tAgent('creator.schedule.interval.minutes') })}
                  active={scheduleType === 'interval'}
                  onClick={() => setScheduleType('interval')}
                  isMobile={isMobile}
                />
                <ScheduleCard
                  icon={<Zap className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.daily')}
                  description={tAgent('creator.schedule.daily.preview', { hour: scheduleHour, minute: scheduleMinute })}
                  active={scheduleType === 'daily'}
                  onClick={() => setScheduleType('daily')}
                  isMobile={isMobile}
                />
                <ScheduleCard
                  icon={<Bell className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.weekly')}
                  description={tAgent('creator.schedule.weekly.preview', { day: selectedWeekdays.length > 0 ? selectedWeekdays[0] : 1, hour: scheduleHour, minute: scheduleMinute })}
                  active={scheduleType === 'weekly'}
                  onClick={() => setScheduleType('weekly')}
                  isMobile={isMobile}
                />
                <ScheduleCard
                  icon={<Target className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.event')}
                  description={tAgent('creator.schedule.event.descriptions.device.metric')}
                  active={scheduleType === 'event'}
                  onClick={() => setScheduleType('event')}
                  isMobile={isMobile}
                />
              </div>

              {/* Schedule Configuration */}
              <div className={cn("border rounded-xl", isMobile ? "p-4" : "p-4 bg-muted/50")}>
                {scheduleType === 'interval' && (
                  <div className={cn(
                    "flex items-center gap-3",
                    isMobile ? "flex-wrap" : ""
                  )}>
                    <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.schedule.interval.every')}</span>
                    <div className={cn(
                      "flex gap-1",
                      isMobile ? "flex-wrap gap-2" : ""
                    )}>
                      {INTERVALS.map((mins) => (
                        <button
                          key={mins}
                          type="button"
                          onClick={() => setIntervalValue(mins)}
                          className={cn(
                            "rounded-lg font-medium transition-colors",
                            isMobile
                              ? "px-4 py-3 text-base min-w-[60px]"
                              : "px-3 py-1.5 text-sm",
                            intervalValue === mins
                              ? "bg-primary text-primary-foreground"
                              : "bg-background hover:bg-muted"
                          )}
                        >
                          {mins}m
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {scheduleType === 'daily' && (
                  <div className={cn(
                    "flex items-center gap-3",
                    isMobile ? "flex-col items-start gap-4" : ""
                  )}>
                    <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.schedule.daily.everyDay')}</span>
                    <div className="flex items-center gap-1 bg-background rounded-lg p-1">
                      <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                        <SelectTrigger className={cn("border-0 bg-transparent", isMobile ? "w-24 h-11 text-base" : "w-20 h-9")}>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {HOURS.map((h) => (
                            <SelectItem key={h} value={h.toString()}>{h.toString().padStart(2, '0')}:00</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                )}

                {scheduleType === 'weekly' && (
                  <div className="space-y-3">
                    <div className="flex items-center gap-3">
                      <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.basicInfo.runOn')}</span>
                    </div>
                    <div className={cn(
                      "flex gap-1 flex-wrap",
                      isMobile ? "gap-2" : ""
                    )}>
                      {[0, 1, 2, 3, 4, 5, 6].map((d) => (
                        <button
                          key={d}
                          type="button"
                          onClick={() => {
                            const newWeekdays = selectedWeekdays.includes(d)
                              ? selectedWeekdays.filter(day => day !== d)
                              : [...selectedWeekdays, d].sort((a, b) => a - b)
                            setSelectedWeekdays(newWeekdays)
                          }}
                          className={cn(
                            "rounded-lg font-medium transition-colors",
                            isMobile
                              ? "w-12 h-12 text-base"
                              : "w-10 h-10 text-sm",
                            selectedWeekdays.includes(d)
                              ? "bg-primary text-primary-foreground"
                              : "bg-background hover:bg-muted"
                          )}
                        >
                          {tAgent(`creator.weekdays.${d}`)}
                        </button>
                      ))}
                    </div>
                    <div className={cn(
                      "flex items-center gap-3",
                      isMobile ? "flex-col items-start gap-4" : ""
                    )}>
                      <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.schedule.daily.at')}</span>
                      <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                        <SelectTrigger className={cn(isMobile ? "w-24 h-11 text-base" : "w-20 h-9")}>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {HOURS.map((h) => (
                            <SelectItem key={h} value={h.toString()}>{h.toString().padStart(2, '0')}:00</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                )}

                {scheduleType === 'event' && (
                  <div className="space-y-3">
                    <div className={cn(
                      "flex items-center gap-3",
                      isMobile ? "flex-col items-start gap-3" : ""
                    )}>
                      <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.basicInfo.triggerWhen')}</span>
                      <div className={cn(
                        "flex gap-1",
                        isMobile ? "flex-col w-full gap-2" : ""
                      )}>
                        <button
                          type="button"
                          onClick={() => setEventConfig({ type: 'device.metric', deviceId: 'all' })}
                          className={cn(
                            "flex items-center gap-1.5 rounded-lg transition-colors",
                            isMobile
                              ? "px-4 py-3 text-base flex-1 justify-center"
                              : "px-3 py-1.5 text-sm",
                            eventConfig.type === 'device.metric' ? "bg-primary text-primary-foreground" : "bg-background hover:bg-muted"
                          )}
                        >
                          <Activity className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} />
                          {tAgent('creator.basicInfo.metricUpdates')}
                        </button>
                        <button
                          type="button"
                          onClick={() => setEventConfig({ type: 'manual' })}
                          className={cn(
                            "flex items-center gap-1.5 rounded-lg transition-colors",
                            isMobile
                              ? "px-4 py-3 text-base flex-1 justify-center"
                              : "px-3 py-1.5 text-sm",
                            eventConfig.type === 'manual' ? "bg-primary text-primary-foreground" : "bg-background hover:bg-muted"
                          )}
                        >
                          <Clock className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} />
                          {tAgent('creator.basicInfo.manualTrigger')}
                        </button>
                      </div>
                    </div>

                    {eventConfig.type === 'device.metric' && (
                      <div className={cn(
                        "flex items-center gap-3",
                        isMobile ? "flex-col items-start gap-3" : ""
                      )}>
                        <span className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>{tAgent('creator.basicInfo.fromDevice')}</span>
                        <select
                          value={eventConfig.deviceId || 'all'}
                          onChange={(e) => setEventConfig({ ...eventConfig, deviceId: e.target.value })}
                          className={cn(
                            "rounded-lg border bg-background",
                            isMobile ? "flex-1 h-11 px-4 text-base" : "flex-1 h-9 px-3 text-sm"
                          )}
                        >
                          <option value="all">{tAgent('creator.schedule.event.allDevices')}</option>
                          {devices.map((d) => (
                            <option key={d.id} value={d.id}>{d.name}</option>
                          ))}
                        </select>
                        <p className={cn("text-muted-foreground", isMobile ? "text-xs" : "text-xs")}>
                          Agent will fetch data from Resources when device updates
                        </p>
                      </div>
                    )}

                    {eventConfig.type === 'manual' && (
                      <p className={cn("text-muted-foreground", isMobile ? "text-xs" : "text-xs")}>
                        {tAgent('creator.basicInfo.manualHint')}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </div>

            {/* Resources Section */}
            <div className="space-y-3">
              <div className={cn(
                "flex items-center justify-between",
                isMobile ? "flex-col items-start gap-3" : ""
              )}>
                <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>{tAgent('creator.resources.title')}</Label>
                <Button
                  type="button"
                  variant="outline"
                  size={isMobile ? "default" : "sm"}
                  onClick={() => setResourceDialogOpen(true)}
                  className={isMobile ? "w-full justify-center h-11" : ""}
                >
                  <Plus className={cn(isMobile ? "h-5 w-5" : "h-4 w-4", "mr-2")} />
                  {tAgent('creator.resources.addResources')}
                </Button>
              </div>

              {selectedResources.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-8 border rounded-lg bg-muted/30">
                  <Target className="h-8 w-8 text-muted-foreground/50 mb-2" />
                  <p className="text-sm text-muted-foreground">
                    {tAgent('creator.resources.dialog.noResourcesHint')}
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  {selectedResources.map((resource) => (
                    <SelectedResourceItem
                      key={resource.id}
                      resource={resource}
                      setSelectedResources={setSelectedResources}
                      onRemove={() => {
                        setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
                      }}
                      onToggleMetric={(resourceId, metricName) => {
                        setSelectedResources((prev) =>
                          prev.map(r =>
                            r.id === resourceId
                              ? {
                                  ...r,
                                  selectedMetrics: new Set(
                                    r.selectedMetrics.has(metricName)
                                      ? Array.from(r.selectedMetrics).filter(n => n !== metricName)
                                      : [...r.selectedMetrics, metricName]
                                  ),
                                }
                              : r
                          )
                        )
                      }}
                      onToggleCommand={(resourceId, commandName) => {
                        setSelectedResources((prev) =>
                          prev.map(r =>
                            r.id === resourceId
                              ? {
                                  ...r,
                                  selectedCommands: new Set(
                                    r.selectedCommands.has(commandName)
                                      ? Array.from(r.selectedCommands).filter(n => n !== commandName)
                                      : [...r.selectedCommands, commandName]
                                  ),
                                }
                              : r
                          )
                        )
                      }}
                      isMobile={isMobile}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
        </div>
        </main>

        {/* Footer with action buttons and summary */}
        <footer
          className="border-t bg-background shrink-0"
          style={isMobile ? { paddingBottom: `${insets.bottom}px` } : undefined}
        >
          <div className={cn(
            "flex gap-2",
            isMobile ? "px-4 py-4 justify-end" : "px-5 py-3 justify-end"
          )}>
            <Button
              variant="outline"
              size={isMobile ? "default" : "sm"}
              onClick={() => onOpenChange(false)}
              disabled={saving}
              className={isMobile ? "min-w-[100px] h-12" : ""}
            >
              {tCommon('cancel')}
            </Button>
            <Button
              size={isMobile ? "default" : "sm"}
              onClick={handleSave}
              disabled={!isValid || saving}
              className={isMobile ? "min-w-[100px] h-12" : ""}
            >
              {saving ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : null}
              {saving ? 'Saving...' : agent ? tCommon('save') : tCommon('create')}
            </Button>
          </div>
          {agentSummary && (
            <div className={cn(
              "flex items-center gap-2 text-muted-foreground border-t pt-2",
              isMobile ? "px-4 pb-2 text-xs" : "px-5 pb-3 text-xs"
            )}>
              <Info className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5", "shrink-0")} />
              <span className="line-clamp-1">{agentSummary}</span>
            </div>
          )}
        </footer>
      </div>

      {/* Resource Selection Dialog */}
      <ResourceSelectionDialog
        open={resourceDialogOpen}
        onOpenChange={setResourceDialogOpen}
        availableResources={filteredResources}
        selectedResources={selectedResources}
        setSelectedResources={setSelectedResources}
        recommendations={recommendations}
        generatingRecommendations={generatingRecommendations}
        searchQuery={searchQuery}
        setSearchQuery={setSearchQuery}
        toggleResource={toggleResource}
        toggleRecommendation={toggleRecommendation}
        scheduleType={scheduleType}
      />
    </>,
    dialogRoot
  )
}

// ============================================================================
// Resource Selection Dialog
// ============================================================================

interface ResourceSelectionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  availableResources: AvailableResource[]
  selectedResources: SelectedResource[]
  setSelectedResources: React.Dispatch<React.SetStateAction<SelectedResource[]>>
  recommendations: ResourceRecommendation[]
  generatingRecommendations: boolean
  searchQuery: string
  setSearchQuery: (v: string) => void
  toggleResource: (r: AvailableResource) => void
  toggleRecommendation: (r: ResourceRecommendation) => void
  scheduleType: ScheduleType
}

function ResourceSelectionDialog({
  open,
  onOpenChange,
  availableResources,
  selectedResources,
  setSelectedResources,
  recommendations,
  generatingRecommendations,
  searchQuery,
  setSearchQuery,
  toggleResource,
  toggleRecommendation,
  scheduleType,
}: ResourceSelectionDialogProps) {
  const { t: tAgent } = useTranslation('agents')
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Lock body scroll when dialog is open (mobile only)
  useBodyScrollLock(open, { mobileOnly: true })

  const isSelected = (id: string) => selectedResources.some(r => r.id === id)

  // Mobile full-screen portal
  if (isMobile) {
    return createPortal(
      <div
        className={cn(
          "fixed inset-0 z-[150] bg-background flex flex-col",
          !open && "hidden"
        )}
        style={{
          paddingTop: `${insets.top}px`,
          paddingBottom: `${insets.bottom}px`,
        }}
      >
        {/* Mobile Header */}
        <div className="flex items-center justify-between px-4 py-4 border-b shrink-0">
          <h2 className="text-base font-semibold">{tAgent('creator.resources.dialog.title')}</h2>
          <Button
            variant="ghost"
            size="icon"
            className="h-10 w-10"
            onClick={() => onOpenChange(false)}
          >
            <X className="h-5 w-5" />
          </Button>
        </div>

        {/* Mobile Content */}
        <div className="flex-1 overflow-y-auto">
          {/* Recommendations */}
          {recommendations.length > 0 && (
            <div className="px-4 py-3 border-b bg-primary/5 shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />}
              </div>
              <div className="flex gap-2 overflow-x-auto pb-1">
                {recommendations.map((rec) => (
                  <RecommendationCard
                    key={rec.id}
                    recommendation={rec}
                    selected={isSelected(rec.id)}
                    onClick={() => toggleRecommendation(rec)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Available Resources Section */}
          <div className="px-4 py-3 border-b">
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm font-medium">{tAgent('creator.resources.dialog.available')}</span>
              <Badge variant="secondary">{availableResources.length}</Badge>
            </div>
            <div className="relative mb-3">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={tAgent('creator.resources.dialog.searchPlaceholder')}
                className="h-11 text-base pl-10"
              />
            </div>
            <div className="space-y-2">
              {availableResources.map((resource) => {
                const selected = isSelected(resource.id)
                return (
                  <ResourceListItem
                    key={resource.id}
                    resource={resource}
                    selected={selected}
                    onClick={() => toggleResource(resource)}
                    isMobile={true}
                  />
                )
              })}
              {availableResources.length === 0 && (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  {tAgent('creator.resources.noResourcesFound')}
                </div>
              )}
            </div>
          </div>

          {/* Selected Resources Section */}
          <div className="px-4 py-3">
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm font-medium">{tAgent('creator.resources.dialog.selected')}</span>
              <Badge variant={selectedResources.length === 0 ? "secondary" : "default"}>
                {selectedResources.length}
              </Badge>
            </div>
            {selectedResources.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-center py-8">
                <Target className="h-8 w-8 text-muted-foreground/50 mb-2" />
                <p className="text-sm text-muted-foreground">
                  {tAgent('creator.resources.dialog.noResourcesHint')}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                {selectedResources.map((resource) => (
                  <SelectedResourceItem
                    key={resource.id}
                    resource={resource}
                    setSelectedResources={setSelectedResources}
                    onRemove={() => {
                      setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
                    }}
                    onToggleMetric={(resourceId, metricName) => {
                      setSelectedResources((prev) =>
                        prev.map(r =>
                          r.id === resourceId
                            ? {
                                ...r,
                                selectedMetrics: new Set(
                                  r.selectedMetrics.has(metricName)
                                    ? Array.from(r.selectedMetrics).filter(n => n !== metricName)
                                    : [...r.selectedMetrics, metricName]
                                ),
                              }
                            : r
                        )
                      )
                    }}
                    onToggleCommand={(resourceId, commandName) => {
                      setSelectedResources((prev) =>
                        prev.map(r =>
                          r.id === resourceId
                            ? {
                                ...r,
                                selectedCommands: new Set(
                                  r.selectedCommands.has(commandName)
                                    ? Array.from(r.selectedCommands).filter(n => n !== commandName)
                                    : [...r.selectedCommands, commandName]
                                ),
                              }
                            : r
                        )
                      )
                    }}
                    isMobile={true}
                  />
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Mobile Footer */}
        <div className="px-4 py-4 border-t flex justify-between items-center shrink-0">
          <p className="text-sm text-muted-foreground">
            {tAgent('creator.resources.dialog.selectedCount', { count: selectedResources.length })}
          </p>
          <Button className="min-w-[100px] h-12" onClick={() => onOpenChange(false)}>
            {tAgent('creator.resources.dialog.done')}
          </Button>
        </div>
      </div>,
      document.body
    )
  }

  // Desktop Dialog
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* Override overlay z-index */}
      <DialogPrimitive.Overlay className="z-[100] fixed inset-0 bg-black/80" />
      <DialogContent className="z-[100] sm:max-w-3xl sm:max-h-[80vh] flex flex-col p-0 gap-0 m-0">
        <DialogHeader className="px-6 py-4 border-b">
          <DialogTitle>{tAgent('creator.resources.dialog.title')}</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-hidden flex flex-col min-h-0">
          {/* Recommendations */}
          {recommendations.length > 0 && (
            <div className="px-6 py-3 border-b bg-primary/5 shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />}
              </div>
              <div className="flex gap-2 overflow-x-auto pb-1">
                {recommendations.map((rec) => (
                  <RecommendationCard
                    key={rec.id}
                    recommendation={rec}
                    selected={isSelected(rec.id)}
                    onClick={() => toggleRecommendation(rec)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Dual-pane layout */}
          <div className="flex-1 flex gap-4 min-h-0 p-6 overflow-hidden">
            {/* Available Resources */}
            <div className="flex-1 flex flex-col bg-muted/30 rounded-xl overflow-hidden">
              <div className="p-3 border-b bg-background/50">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium">{tAgent('creator.resources.dialog.available')}</span>
                  <Badge variant="secondary">{availableResources.length}</Badge>
                </div>
                <div className="relative">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={tAgent('creator.resources.dialog.searchPlaceholder')}
                    className="h-8 text-sm pl-9"
                  />
                </div>
              </div>
              <ScrollArea className="flex-1">
                <div className="p-2 space-y-1">
                  {availableResources.map((resource) => {
                    const selected = isSelected(resource.id)
                    return (
                      <ResourceListItem
                        key={resource.id}
                        resource={resource}
                        selected={selected}
                        onClick={() => toggleResource(resource)}
                      />
                    )
                  })}
                  {availableResources.length === 0 && (
                    <div className="text-center py-8 text-muted-foreground text-sm">
                      {tAgent('creator.resources.noResourcesFound')}
                    </div>
                  )}
                </div>
              </ScrollArea>
            </div>

            {/* Selected Resources */}
            <div className="flex-1 flex flex-col bg-background rounded-xl overflow-hidden border">
              <div className="p-3 border-b">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">{tAgent('creator.resources.dialog.selected')}</span>
                  <Badge variant={selectedResources.length === 0 ? "secondary" : "default"}>
                    {selectedResources.length}
                  </Badge>
                </div>
              </div>
              <ScrollArea className="flex-1">
                <div className="p-2 space-y-1">
                  {selectedResources.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-center py-8">
                      <Target className="h-8 w-8 text-muted-foreground/50 mb-2" />
                      <p className="text-sm text-muted-foreground">
                        {tAgent('creator.resources.dialog.noResourcesHint')}
                      </p>
                    </div>
                  ) : (
                    selectedResources.map((resource) => (
                      <SelectedResourceItem
                        key={resource.id}
                        resource={resource}
                        setSelectedResources={setSelectedResources}
                        onRemove={() => {
                          setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
                        }}
                        onToggleMetric={(resourceId, metricName) => {
                          setSelectedResources((prev) =>
                            prev.map(r =>
                              r.id === resourceId
                                ? {
                                    ...r,
                                    selectedMetrics: new Set(
                                      r.selectedMetrics.has(metricName)
                                        ? Array.from(r.selectedMetrics).filter(n => n !== metricName)
                                        : [...r.selectedMetrics, metricName]
                                    ),
                                  }
                                : r
                            )
                          )
                        }}
                        onToggleCommand={(resourceId, commandName) => {
                          setSelectedResources((prev) =>
                            prev.map(r =>
                              r.id === resourceId
                                ? {
                                    ...r,
                                    selectedCommands: new Set(
                                      r.selectedCommands.has(commandName)
                                        ? Array.from(r.selectedCommands).filter(n => n !== commandName)
                                        : [...r.selectedCommands, commandName]
                                    ),
                                  }
                                : r
                            )
                          )
                        }}
                      />
                    ))
                  )}
                </div>
              </ScrollArea>
            </div>
          </div>
        </div>

        <div className="px-6 py-4 border-t flex justify-between items-center shrink-0">
          <p className="text-sm text-muted-foreground">
            {tAgent('creator.resources.dialog.selectedCount', { count: selectedResources.length })}
          </p>
          <Button onClick={() => onOpenChange(false)}>
            {tAgent('creator.resources.dialog.done')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// Sub-Components
// ============================================================================

interface ScheduleCardProps {
  icon: React.ReactNode
  label: string
  description: string
  active: boolean
  onClick: () => void
  isMobile?: boolean
}

function ScheduleCard({ icon, label, description, active, onClick, isMobile = false }: ScheduleCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex flex-col items-center rounded-xl border-2 transition-all",
        isMobile ? "gap-3 p-4" : "gap-2 p-3",
        active
          ? "border-primary bg-primary/5"
          : "border-transparent hover:border-muted-foreground/30 hover:bg-muted/30"
      )}
    >
      <div className={cn("rounded-lg", active ? "bg-primary/10" : "bg-muted/50", isMobile ? "p-2" : "p-1.5")}>
        {icon}
      </div>
      <div className="text-center">
        <div className={cn("font-medium", active ? "text-foreground" : "text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{label}</div>
        <div className={cn("text-muted-foreground", isMobile ? "text-xs" : "text-[10px]")}>{description}</div>
      </div>
    </button>
  )
}

interface RecommendationCardProps {
  recommendation: ResourceRecommendation
  selected: boolean
  onClick: () => void
}

function RecommendationCard({ recommendation, selected, onClick }: RecommendationCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 px-3 py-2 rounded-lg border text-left whitespace-nowrap transition-colors min-w-0",
        selected ? "border-primary bg-primary/5" : "border-border hover:bg-muted/50"
      )}
    >
      <div className={cn(
        "p-1 rounded",
        selected ? "bg-primary text-primary-foreground" : "bg-muted"
      )}>
        {recommendation.type === 'extension' ? <Puzzle className="h-3 w-3" /> : <Target className="h-3 w-3" />}
      </div>
      <div className="min-w-0">
        <div className="text-sm font-medium truncate">{recommendation.name}</div>
        <div className="text-xs text-muted-foreground truncate">{recommendation.reason}</div>
      </div>
      {selected && <Check className="h-4 w-4 text-primary ml-1 shrink-0" />}
    </button>
  )
}

interface AvailableResource {
  id: string
  name: string
  type: 'device' | 'extension'
  deviceType?: string
  metrics: MetricInfo[]
  commands: CommandInfo[]
}

interface ResourceListItemProps {
  resource: AvailableResource
  selected: boolean
  onClick: () => void
  isMobile?: boolean
}

function ResourceListItem({ resource, selected, onClick, isMobile = false }: ResourceListItemProps) {
  const metricCount = resource.metrics.length
  const commandCount = resource.commands.length

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "w-full rounded-lg text-left transition-colors flex items-center justify-between group",
        isMobile ? "px-4 py-3" : "px-3 py-2.5",
        selected ? "bg-primary/10 border border-primary/30" : "hover:bg-muted/40 border border-transparent"
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <div className={cn(
          "rounded",
          isMobile ? "p-2" : "p-1.5",
          selected ? "bg-primary text-primary-foreground" : "bg-muted"
        )}>
          {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-3.5 w-3.5")} />}
        </div>
        <div className="min-w-0">
          <div className={cn("font-medium truncate", isMobile ? "text-base" : "text-sm")}>{resource.name}</div>
          <div className={cn("text-muted-foreground", isMobile ? "text-xs" : "text-xs")}>
            {metricCount > 0 && `${metricCount} metric${metricCount > 1 ? 's' : ''}`}
            {metricCount > 0 && commandCount > 0 && ' • '}
            {commandCount > 0 && `${commandCount} command${commandCount > 1 ? 's' : ''}`}
          </div>
        </div>
      </div>
      {selected && <Check className={cn("text-primary shrink-0", isMobile ? "h-5 w-5" : "h-4 w-4")} />}
    </button>
  )
}

interface SelectedResourceItemProps {
  resource: SelectedResource
  setSelectedResources: React.Dispatch<React.SetStateAction<SelectedResource[]>>
  onRemove: () => void
  onToggleMetric: (resourceId: string, metricName: string) => void
  onToggleCommand: (resourceId: string, commandName: string) => void
  isMobile?: boolean
}

function SelectedResourceItem({ resource, setSelectedResources, onRemove, onToggleMetric, onToggleCommand, isMobile = false }: SelectedResourceItemProps) {
  const [expanded, setExpanded] = useState(false)
  const selectedMetricCount = resource.selectedMetrics.size
  const selectedCommandCount = resource.selectedCommands.size
  const allMetricCount = resource.allMetrics.length
  const allCommandCount = resource.allCommands.length

  const hasMetrics = resource.allMetrics.length > 0
  const hasCommands = resource.allCommands.length > 0

  return (
    <div className={cn("rounded-lg bg-background border group", isMobile ? "px-4 py-3" : "px-3 py-2")}>
      {/* Header */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 min-w-0 flex-1"
        >
          <ChevronRight
            className={cn(
              "text-muted-foreground transition-transform",
              isMobile ? "h-5 w-5" : "h-4 w-4",
              expanded && "rotate-90"
            )}
          />
          <div className={cn(
            "rounded",
            isMobile ? "p-2" : "p-1",
            resource.type === 'extension' ? "bg-purple-100 text-purple-600 dark:bg-purple-900/30 dark:text-purple-300" : "bg-blue-100 text-blue-600 dark:bg-blue-900/30 dark:text-blue-300"
          )}>
            {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-3 w-3")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-3 w-3")} />}
          </div>
          <span className={cn("font-medium truncate", isMobile ? "text-base" : "text-sm")}>{resource.name}</span>
          {(hasMetrics || hasCommands) && (
            <Badge variant="secondary" className={cn(isMobile ? "text-xs" : "text-xs")}>
              {selectedMetricCount}/{allMetricCount} • {selectedCommandCount}/{allCommandCount}
            </Badge>
          )}
        </button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className={cn("transition-opacity", isMobile ? "h-9 w-9" : "h-6 w-6", isMobile ? "opacity-100" : "opacity-0 group-hover:opacity-100")}
          onClick={onRemove}
        >
          <X className={cn(isMobile ? "h-4 w-4" : "h-3 w-3")} />
        </Button>
      </div>

      {/* Expandable Metrics/Commands */}
      {expanded && (hasMetrics || hasCommands) && (
        <div className={cn("space-y-2", isMobile ? "mt-3 pl-7" : "mt-2 pl-6")}>
          {/* Metrics */}
          {hasMetrics && (
            <div className="space-y-2">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Metrics</span>
                <button
                  type="button"
                  onClick={() => {
                    // Toggle all metrics
                    const selectAll = selectedMetricCount < allMetricCount
                    setSelectedResources((prev: SelectedResource[]) =>
                      prev.map(r =>
                        r.id === resource.id
                          ? {
                              ...r,
                              selectedMetrics: selectAll
                                ? new Set(resource.allMetrics.map(m => m.name))
                                : new Set(),
                            }
                          : r
                      )
                    )
                  }}
                  className="text-primary hover:underline"
                >
                  {selectedMetricCount === allMetricCount ? 'Deselect All' : selectedMetricCount > 0 ? 'Select All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {resource.allMetrics.map((metric) => (
                  <label
                    key={metric.name}
                    className={cn(
                      "flex items-center gap-2 rounded cursor-pointer transition-colors",
                      isMobile
                        ? "px-3 py-2.5 text-sm"
                        : "px-2 py-1 text-xs",
                      resource.selectedMetrics.has(metric.name)
                        ? "bg-primary/10 text-primary"
                        : "hover:bg-muted/50"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={resource.selectedMetrics.has(metric.name)}
                      onChange={() => onToggleMetric(resource.id, metric.name)}
                      className={cn("rounded", isMobile ? "h-4 w-4" : "h-3 w-3")}
                    />
                    <span className="truncate">{metric.display_name}</span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {/* Commands */}
          {hasCommands && (
            <div className="space-y-2">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Commands</span>
                <button
                  type="button"
                  onClick={() => {
                    // Toggle all commands
                    const selectAll = selectedCommandCount < allCommandCount
                    setSelectedResources((prev: SelectedResource[]) =>
                      prev.map(r =>
                        r.id === resource.id
                          ? {
                              ...r,
                              selectedCommands: selectAll
                                ? new Set(resource.allCommands.map(c => c.name))
                                : new Set(),
                            }
                          : r
                      )
                    )
                  }}
                  className="text-primary hover:underline"
                >
                  {selectedCommandCount === allCommandCount ? 'Deselect All' : selectedCommandCount > 0 ? 'Select All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {resource.allCommands.map((command) => (
                  <label
                    key={command.name}
                    className={cn(
                      "flex items-center gap-2 rounded cursor-pointer transition-colors",
                      isMobile
                        ? "px-3 py-2.5 text-sm"
                        : "px-2 py-1 text-xs",
                      resource.selectedCommands.has(command.name)
                        ? "bg-primary/10 text-primary"
                        : "hover:bg-muted/50"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={resource.selectedCommands.has(command.name)}
                      onChange={() => onToggleCommand(resource.id, command.name)}
                      className={cn("rounded", isMobile ? "h-4 w-4" : "h-3 w-3")}
                    />
                    <span className="truncate">{command.display_name}</span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {!hasMetrics && !hasCommands && (
            <p className={cn("text-muted-foreground italic", isMobile ? "text-xs" : "text-xs")}>No metrics or commands available</p>
          )}
        </div>
      )}
    </div>
  )
}

// Export as default
export default AgentEditorFullScreen
