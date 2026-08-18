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
 *
 * Using unified FullScreenDialog components with glassmorphism style.
 */

import { getPortalRoot } from '@/lib/portal'
import React, { useState, useEffect, useCallback, useMemo, useRef } from "react"
import { useTranslation } from "react-i18next"
import { createPortal } from "react-dom"
import { api } from "@/lib/api"
import { validateRequired, validateLength } from "@/lib/form-validation"
import { useToast } from "@/hooks/use-toast"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { showErrorToast } from "@/lib/error-messages"
import { useIsMobile, useSafeAreaInsets } from "@/hooks/useMobile"
import { useBodyScrollLock } from "@/hooks/useBodyScrollLock"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

import { Checkbox } from "@/components/ui/checkbox"
import { Slider } from "@/components/ui/slider"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Switch } from "@/components/ui/switch"
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
  Eye,
  Brain,
  Wrench,
  MessageSquare,
  Database,
  Workflow,
  MousePointerClick,
  GitBranch,
  Settings,
} from "lucide-react"
import type {
  AiAgentDetail,
  AgentSchedule,
  CreateAgentRequest,
  Device,
  DeviceType,
  Extension,
  ExtensionDataSourceInfo,
  ExtensionCommandDescriptor,
  LlmBackendInstance,
  ResourceRequest,
  UnifiedDataSourceInfo,
} from "@/types"
// Unified dialog components
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogMain,
} from '@/components/automation/dialog'

// Shared className for inline link-styled buttons (not IconButton — these are
// text links rendered as <button> for accessibility).
const inlineLinkBtn = 'text-xs text-primary hover:underline'

interface AgentEditorFullScreenProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agent: AiAgentDetail | undefined
  devices: Device[]
  deviceTypes: DeviceType[]
  extensions?: Extension[]
  extensionDataSources?: ExtensionDataSourceInfo[]
  unifiedDataSources?: UnifiedDataSourceInfo[]
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

interface DataCollectionConfig {
  time_range_minutes: number
  include_history: boolean
  include_trend: boolean
  include_baseline: boolean
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
  // Data collection config for Focused Mode
  config?: {
    data_collection?: DataCollectionConfig
  }
}

interface ResourceRecommendation {
  id: string
  name: string
  type: 'device' | 'extension'
  reason: string
  metrics?: MetricInfo[]
  commands?: CommandInfo[]
}

type ScheduleType = 'timer' | 'reactive' | 'on-demand'
type TimerSubType = 'interval' | 'daily' | 'weekly'

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
    icon: <Activity className="h-4 w-4" />,
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
    icon: <Zap className="h-4 w-4" />,
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
    icon: <BarChart3 className="h-4 w-4" />,
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
    icon: <Bell className="h-4 w-4" />,
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
  unifiedDataSources = [],
  onSave,
}: AgentEditorFullScreenProps) {
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()
  const isMobile = useIsMobile()

  // ========================================================================
  // State
  // ========================================================================

  // Ref for auto-focusing name input on create
  const nameInputRef = useRef<HTMLInputElement>(null)

  // Basic info
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [userPrompt, setUserPrompt] = useState("")
  const [llmBackendId, setLlmBackendId] = useState<string | null>(null)

  // Schedule state
  const [scheduleType, setScheduleType] = useState<ScheduleType>('timer')
  const [timerSubType, setTimerSubType] = useState<TimerSubType>('interval')
  const [intervalValue, setIntervalValue] = useState(5)
  const [scheduleHour, setScheduleHour] = useState(9)
  const [scheduleMinute, setScheduleMinute] = useState(0)
  const [selectedWeekdays, setSelectedWeekdays] = useState<number[]>([1])

  // Trigger sources for reactive mode
  // When field is undefined → match all fields from this source
  // When field is specified → match only that field
  const [triggerSources, setTriggerSources] = useState<Array<{ type: string; id: string; name: string; field?: string }>>([])
  const [activeTriggerEntity, setActiveTriggerEntity] = useState<{ type: string; id: string } | null>(null)
  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [resourceDialogOpen, setResourceDialogOpen] = useState(false)
  const [freeModeResourcesExpanded, setFreeModeResourcesExpanded] = useState(false)
  const [searchQuery, setSearchQuery] = useState("")
  const [recommendations, setRecommendations] = useState<ResourceRecommendation[]>([])
  const [generatingRecommendations, setGeneratingRecommendations] = useState(false)

  // UI state
  const [saving, setSaving] = useState(false)

  // LLM backends
  const [llmBackends, setLlmBackends] = useState<LlmBackendInstance[]>([])
  const [activeBackendId, setActiveBackendId] = useState<string | null>(null)

  // Advanced configuration state
  const [executionMode, setExecutionMode] = useState<'focused' | 'free'>('focused')
  const [priority, setPriority] = useState(5)
  const [contextWindowSize, setContextWindowSize] = useState(10)
  const [maxChainDepth, setMaxChainDepth] = useState(5)

  // LLM validation state
  const [llmValidating, setLlmValidating] = useState(false)
  const [llmValid, setLlmValid] = useState<boolean | null>(null)
  const [llmValidationError, setLlmValidationError] = useState<string | null>(null)

  // Form field validation errors
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  // ========================================================================
  // Mode Helpers
  // ========================================================================

  const isFocusedMode = executionMode === 'focused'
  const isFreeMode = executionMode === 'free'

  // Helper: get metrics for a device (from deviceTypes)
  const getDeviceMetrics = useCallback((deviceId: string): Array<{ name: string; display_name: string }> => {
    const device = devices.find(d => d.id === deviceId)
    if (!device) return []
    const dt = deviceTypes.find(t => t.device_type === device.device_type)
    return dt?.metrics?.map(m => ({ name: m.name, display_name: m.display_name })) || []
  }, [devices, deviceTypes])

  // Helper: get metrics for an extension
  const getExtensionMetrics = useCallback((extId: string): Array<{ name: string; display_name: string }> => {
    const ext = (extensions || []).find(e => e.id === extId)
    return ext?.metrics?.map(m => ({ name: m.name, display_name: m.display_name })) || []
  }, [extensions])

  // Build unified trigger entities from UnifiedDataSourceInfo
  const triggerEntities = useMemo(() => {
    const entityMap = new Map<string, {
      type: string
      id: string
      name: string
      metrics: Array<{ name: string; display_name: string }>
    }>()

    for (const ds of unifiedDataSources) {
      const key = `${ds.source_type}:${ds.source_name}`
      if (!entityMap.has(key)) {
        entityMap.set(key, {
          type: ds.source_type,
          id: ds.source_name,
          name: ds.source_display_name || ds.source_name,
          metrics: [],
        })
      }
      entityMap.get(key)!.metrics.push({
        name: ds.field,
        display_name: ds.field_display_name || ds.field,
      })
    }

    // Sort: device first, then extension, then ai, then others
    const typeOrder = ['device', 'extension', 'ai', 'transform', 'system']
    return Array.from(entityMap.values()).sort((a, b) => {
      const ai = typeOrder.indexOf(a.type) ?? 99
      const bi = typeOrder.indexOf(b.type) ?? 99
      return ai - bi
    })
  }, [unifiedDataSources])

  // Icon helper for source type
  const getSourceIcon = (type: string, className: string) => {
    switch (type) {
      case 'device': return <Database className={className} />
      case 'extension': return <Puzzle className={className} />
      case 'ai': return <Brain className={className} />
      case 'transform': return <GitBranch className={className} />
      default: return <Database className={className} />
    }
  }

  // Map frontend source type to backend event_filter source type for saving
  const mapToBackendSourceType = (frontendType: string, id: string): { type: string; id: string } => {
    switch (frontendType) {
      case 'device': return { type: 'device', id }
      case 'extension': return { type: 'extension', id }
      case 'ai': return { type: 'extension', id: `ai:${id}` }
      default: return { type: frontendType, id }
    }
  }

  // Restore frontend source type from saved event_filter source
  const restoreFromBackendSourceType = (s: { type: string; id: string; name?: string; field?: string }) => {
    // Detect AI sources stored as type=extension, id=ai:xxx
    if (s.type === 'extension' && s.id.startsWith('ai:')) {
      return {
        type: 'ai',
        id: s.id.replace('ai:', ''),
        name: s.name || s.id.replace('ai:', ''),
        ...(s.field ? { field: s.field } : {}),
      }
    }
    return {
      type: s.type,
      id: s.id,
      name: s.name || s.id,
      ...(s.field ? { field: s.field } : {}),
    }
  }

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
    setFieldErrors({})
    if (open) {
      if (agent) {
        // Edit mode
        setName(agent.name || '')
        setDescription(agent.description || '')
        setUserPrompt(agent.user_prompt || '')
        setLlmBackendId(agent.llm_backend_id || null)
        // Load advanced config from agent
        setExecutionMode(agent.execution_mode === 'free' ? 'free' : 'focused')
        setPriority(agent.priority ?? 5)
        setContextWindowSize(agent.context_window_size ?? 10)
        setMaxChainDepth(agent.max_chain_depth ?? 5)
        parseSchedule(agent.schedule)
        loadAgentResources(agent)
      } else {
        // Create mode - reset
        setName("")
        setDescription("")
        setUserPrompt("")
        setLlmBackendId(null)
        // Reset to defaults
        setExecutionMode('focused')
        setPriority(5)
        setContextWindowSize(10)
        setMaxChainDepth(5)
        setScheduleType('timer')
        setTimerSubType('interval')
        setIntervalValue(5)
        setScheduleHour(9)
        setScheduleMinute(0)
        setSelectedWeekdays([1])
        setTriggerSources([])
        setSelectedResources([])
        setRecommendations([])
        setSearchQuery("")
        setLlmValid(null)
        setLlmValidationError(null)
      }
    }
  }, [agent, open])

  // Reload resources when devices/deviceTypes become available (may be empty on first render)
  useEffect(() => {
    if (open && agent && devices.length > 0) {
      loadAgentResources(agent)
    }
  }, [devices, deviceTypes, open])

  // Auto-focus name input when creating a new agent
  useEffect(() => {
    if (open && !agent) {
      // Use requestAnimationFrame to ensure the DOM is ready
      const raf = requestAnimationFrame(() => {
        nameInputRef.current?.focus()
      })
      return () => cancelAnimationFrame(raf)
    }
  }, [open, agent])

  // Generate recommendations when prompt changes or dialog opens
  useEffect(() => {
    if ((userPrompt.length > 20 || resourceDialogOpen) && !agent) {
      generateRecommendations()
    }
  }, [userPrompt, resourceDialogOpen])

  // ========================================================================
  // Helpers
  // ========================================================================

  const parseSchedule = (schedule: AgentSchedule) => {
    if (!schedule) return
    if (schedule.schedule_type === 'interval') {
      if (schedule.interval_seconds === 0) {
        // interval_seconds=0 means on-demand (no auto-trigger)
        setScheduleType('on-demand')
      } else {
        setScheduleType('timer')
        setTimerSubType('interval')
        if (schedule.interval_seconds) {
          setIntervalValue(Math.floor(schedule.interval_seconds / 60))
        }
      }
    } else if (schedule.schedule_type === 'cron') {
      setScheduleType('timer')
      if (schedule.cron_expression) {
        const parts = schedule.cron_expression.split(' ')
        if (parts.length === 5) {
          setScheduleMinute(parseInt(parts[0]) || 0)
          setScheduleHour(parseInt(parts[1]) || 9)
          if (parts[4] !== '*') {
            setTimerSubType('weekly')
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
            setTimerSubType('daily')
          }
        } else if (parts.length >= 6) {
          setScheduleMinute(parseInt(parts[1]) || 0)
          setScheduleHour(parseInt(parts[2]) || 9)
          if (parts[5] !== '*') {
            setTimerSubType('weekly')
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
            setTimerSubType('daily')
          }
        }
      }
    } else if (schedule.schedule_type === 'event') {
      setScheduleType('reactive')
      // Parse trigger sources from event_filter
      try {
        const filter = JSON.parse(schedule.event_filter || '{}')
        if (filter.sources && Array.isArray(filter.sources)) {
          const sources = filter.sources.map((s: Record<string, string>) =>
            restoreFromBackendSourceType({
              type: s.type || 'device',
              id: s.id || '',
              name: s.name || s.id || '',
              ...(s.field ? { field: s.field } : {}),
            })
          ).filter((s: { id: string }) => s.id)
          setTriggerSources(sources)
        }
      } catch {
        // Legacy event_filter format - ignore
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

    // Store data collection config per resource (first encountered metric's config will be used for the resource)
    const resourceDataCollectionConfigs = new Map<string, DataCollectionConfig>()

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
        // Store data collection config from the first metric
        if (res.config?.data_collection && !resourceDataCollectionConfigs.has(deviceId)) {
          const dc = res.config.data_collection as { time_range_minutes?: number; include_history?: boolean; include_trend?: boolean; include_baseline?: boolean }
          resourceDataCollectionConfigs.set(deviceId, {
            time_range_minutes: dc.time_range_minutes ?? 60,
            include_history: dc.include_history ?? false,
            include_trend: dc.include_trend ?? false,
            include_baseline: dc.include_baseline ?? false,
          })
        }
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
          // Store data collection config from the first metric
          if (res.config?.data_collection && !resourceDataCollectionConfigs.has(extId)) {
            const dc = res.config.data_collection as { time_range_minutes?: number; include_history?: boolean; include_trend?: boolean; include_baseline?: boolean }
            resourceDataCollectionConfigs.set(extId, {
              time_range_minutes: dc.time_range_minutes ?? 60,
              include_history: dc.include_history ?? false,
              include_trend: dc.include_trend ?? false,
              include_baseline: dc.include_baseline ?? false,
            })
          }
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
        config: {
          data_collection: resourceDataCollectionConfigs.get(deviceId),
        },
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
        config: {
          data_collection: resourceDataCollectionConfigs.get(extKey),
        },
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
        config: {
          data_collection: resourceDataCollectionConfigs.get(extKey),
        },
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
    if (scheduleType === 'timer') {
      if (timerSubType === 'interval') parts.push(`runs every ${intervalValue} minutes`)
      else if (timerSubType === 'daily') parts.push(`runs daily at ${scheduleHour}:${scheduleMinute.toString().padStart(2, '0')}`)
      else if (timerSubType === 'weekly') parts.push(`runs weekly on ${selectedWeekdays.map(d => ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][d]).join(', ')} at ${scheduleHour}:${scheduleMinute.toString().padStart(2, '0')}`)
    }
    if (scheduleType === 'reactive') {
      if (triggerSources.length > 0) {
        parts.push(`triggers on ${triggerSources.map(s => s.name).join(', ')} data updates`)
      } else {
        parts.push('triggers on data updates (no sources configured)')
      }
    }
    if (scheduleType === 'on-demand') parts.push('runs on demand only')

    const selectedDeviceCount = selectedResources.filter(r => r.type === 'device').length
    const selectedExtCount = selectedResources.filter(r => r.type === 'extension').length
    const metricCount = selectedResources.reduce((sum, r) => sum + r.selectedMetrics.size, 0)
    const commandCount = selectedResources.reduce((sum, r) => sum + r.selectedCommands.size, 0)

    if (selectedDeviceCount > 0 || selectedExtCount > 0) {
      parts.push(`monitors ${selectedDeviceCount} device(s) and ${selectedExtCount} extension(s)`)
      parts.push(`tracks ${metricCount} metric(s) and ${commandCount} command(s)`)
    }

    return parts.join(', ')
  }, [name, scheduleType, timerSubType, intervalValue, scheduleHour, scheduleMinute, selectedWeekdays, triggerSources, selectedResources])

  // Validation - name and prompt are required
  // Metric selection is optional for event-triggered agents (device-level deduplication prevents loops)
  const nameError = fieldErrors.name ??
    (validateRequired(name, 'Name') || validateLength(name, 'Name', 1, 100))
  const promptError = fieldErrors.prompt ??
    (validateRequired(userPrompt, 'Prompt') || validateLength(userPrompt, 'Prompt', 1, 5000))
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
      let eventFilter: string | undefined = undefined

      if (scheduleType === 'timer') {
        if (timerSubType === 'interval') {
          intervalSeconds = intervalValue * 60
        } else if (timerSubType === 'daily') {
          cronExpression = `0 ${scheduleMinute} ${scheduleHour} * * *`
          finalScheduleType = 'cron'
        } else { // weekly
          const sortedDays = [...selectedWeekdays].sort((a, b) => a - b)
          cronExpression = `0 ${scheduleMinute} ${scheduleHour} * * ${sortedDays.join(',')}`
          finalScheduleType = 'cron'
        }
      } else if (scheduleType === 'reactive') {
        finalScheduleType = 'event'
        // Save trigger sources to event_filter with backend type mapping
        const eventFilterObj: { sources: Array<{ type: string; id: string; name: string; field?: string }> } = {
          sources: triggerSources.map(s => {
            const mapped = mapToBackendSourceType(s.type, s.id)
            return { type: mapped.type, id: mapped.id, name: s.name, ...(s.field ? { field: s.field } : {}) }
          }),
        }
        eventFilter = JSON.stringify(eventFilterObj)
      } else { // on-demand
        finalScheduleType = 'interval'
        intervalSeconds = 0  // 0 = no auto-scheduling
      }

      // Build resources array in the new format that supports both devices and extensions
      const resources = selectedResources.flatMap(r => {
        const result: ResourceRequest[] = []

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
                // Include data collection config for Focused Mode
                ...(r.config?.data_collection && { data_collection: r.config.data_collection }),
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
                // Include data collection config for Focused Mode
                ...(r.config?.data_collection && { data_collection: r.config.data_collection }),
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
        priority: priority !== 5 ? priority : undefined,
        context_window_size: contextWindowSize !== 10 ? contextWindowSize : undefined,
        max_chain_depth: maxChainDepth !== 5 ? maxChainDepth : undefined,
        execution_mode: isFocusedMode ? 'focused' : 'free',
      }

      await onSave(data)
      onOpenChange(false)
      toast({ title: tCommon('success'), description: agent ? tAgent('agentUpdated') : tAgent('agentCreated') })
    } catch (error) {
      handleError(error, { operation: 'Save agent', showToast: false })
      showErrorToast(toast, error, tCommon('failed'))
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

  return (
    <>
    <FullScreenDialog
      open={open}
      onOpenChange={onOpenChange}
    >
      {/* Header */}
      <FullScreenDialogHeader
        icon={<Sparkles className="h-5 w-5" />}
        iconBg="bg-accent-purple-light"
        iconColor="text-accent-purple"
        title={agent ? tAgent('editAgent') : tAgent('createAgent')}
        onClose={() => onOpenChange(false)}
      />

      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-hidden">
          <div className="h-full overflow-y-auto">
              <div className={cn(
                "space-y-6",
                isMobile ? "px-4 py-6" : "px-4 py-6"
              )}>
            {/* Execution Mode */}
            <div className="space-y-2">
              <Label className="text-sm font-medium flex items-center gap-2">
                <Brain className="h-4 w-4 text-muted-foreground" />
                {tAgent('creator.advanced.executionMode')}
              </Label>
              <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                <button
                  type="button"
                  onClick={() => setExecutionMode('focused')}
                  className={cn(
                    "relative flex flex-col items-start gap-1.5 rounded-lg border-2 p-3 text-left transition-all",
                    isFocusedMode
                      ? "border-primary bg-muted shadow-sm"
                      : "border-border hover:border-border"
                  )}
                >
                  <div className="flex items-center gap-2 w-full">
                    <div className={cn(
                      "h-8 w-8 rounded-lg flex items-center justify-center shrink-0",
                      isFocusedMode ? "bg-primary text-primary-foreground" : "bg-muted"
                    )}>
                      <Target className="h-4 w-4" />
                    </div>
                    <div className="flex items-center gap-1.5 flex-1 min-w-0">
                      <span className="text-sm font-medium">{tAgent('creator.advanced.focusedMode')}</span>
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground pl-10">
                    {tAgent('creator.advanced.focusedModeDescription')}
                  </p>
                </button>

                <button
                  type="button"
                  onClick={() => { setExecutionMode('free'); setSelectedResources([]) }}
                  className={cn(
                    "relative flex flex-col items-start gap-1.5 rounded-lg border-2 p-3 text-left transition-all",
                    isFreeMode
                      ? "border-primary bg-muted shadow-sm"
                      : "border-border hover:border-border"
                  )}
                >
                  <div className="flex items-center gap-2 w-full">
                    <div className={cn(
                      "h-8 w-8 rounded-lg flex items-center justify-center shrink-0",
                      isFreeMode ? "bg-primary text-primary-foreground" : "bg-muted"
                    )}>
                      <Zap className="h-4 w-4" />
                    </div>
                    <div className="flex items-center gap-1.5 flex-1 min-w-0">
                      <span className="text-sm font-medium">{tAgent('creator.advanced.freeMode')}</span>
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground pl-10">
                    {tAgent('creator.advanced.freeModeDescription')}
                  </p>
                </button>
              </div>
            </div>

            {/* Name */}
            <div className="space-y-2">
              <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                {tAgent('creator.basicInfo.name')} <span className="text-error">*</span>
              </Label>
              <Input
                ref={nameInputRef}
                value={name}
                onChange={(e) => {
                  setName(e.target.value)
                  if (fieldErrors.name) setFieldErrors(prev => { const next = { ...prev }; delete next.name; return next })
                }}
                onBlur={() => {
                  const err = validateRequired(name, 'Name') || validateLength(name, 'Name', 1, 100)
                  if (err) setFieldErrors(prev => ({ ...prev, name: err }))
                }}
                placeholder={tAgent('creator.basicInfo.namePlaceholder')}
                className={cn(isMobile ? "h-12 text-base" : "h-10", fieldErrors.name && "border-error")}
              />
              {fieldErrors.name && (
                <p className="text-sm text-error mt-1">{fieldErrors.name}</p>
              )}
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
                {tAgent('creator.basicInfo.requirement')} <span className="text-error">*</span>
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
                onChange={(e) => {
                  setUserPrompt(e.target.value)
                  if (fieldErrors.prompt) setFieldErrors(prev => { const next = { ...prev }; delete next.prompt; return next })
                }}
                onBlur={() => {
                  const err = validateRequired(userPrompt, 'Prompt') || validateLength(userPrompt, 'Prompt', 1, 5000)
                  if (err) setFieldErrors(prev => ({ ...prev, prompt: err }))
                }}
                placeholder={tAgent('creator.basicInfo.promptPlaceholder')}
                className={cn("min-h-[140px] resize-y text-sm leading-relaxed", fieldErrors.prompt && "border-error")}
              />
              {fieldErrors.prompt && (
                <p className="text-sm text-error mt-1">{fieldErrors.prompt}</p>
              )}

              {/* AI Helper Tip */}
              <div className="flex items-start gap-2 p-3 rounded-lg border border-border">
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
                    <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                  ) : llmValid === true ? (
                    <Check className="h-4 w-4 mr-1 text-success" />
                  ) : llmValid === false ? (
                    <span className="text-error">!</span>
                  ) : null}
                  {llmValidating ? 'Checking...' : llmValid === true ? 'OK' : llmValid === false ? 'Failed' : 'Test'}
                </Button>
              </div>
              <Select value={llmBackendId ?? activeBackendId ?? ''} onValueChange={setLlmBackendId}>
                <SelectTrigger className="h-10">
                  {(() => {
                    const selectedId = llmBackendId ?? activeBackendId ?? '';
                    if (!selectedId || selectedId === 'default') {
                      const activeName = llmBackends.find((b) => b.id === activeBackendId)?.name;
                      return (
                        <span className="flex items-center gap-2 min-w-0 truncate">
                          <span className="truncate">{tAgent('creator.basicInfo.useActiveBackend')}</span>
                          {activeName && (
                            <span className="text-xs text-muted-foreground shrink-0 truncate">({activeName})</span>
                          )}
                        </span>
                      );
                    }
                    const backend = llmBackends.find((b) => b.id === selectedId);
                    if (!backend) {
                      return <span className="text-muted-foreground truncate">{tAgent('creator.basicInfo.useActiveBackend')}</span>;
                    }
                    return (
                      <span className="flex items-center gap-2 min-w-0">
                        <span className="truncate min-w-0">{backend.name}</span>
                        <span className="text-muted-foreground shrink-0 truncate">{backend.model}</span>
                      </span>
                    );
                  })()}
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default" textValue={tAgent('creator.basicInfo.useActiveBackend')}>
                    <span className="flex items-center gap-2">
                      <span>{tAgent('creator.basicInfo.useActiveBackend')}</span>
                      {activeBackendId && (
                        <span className="text-xs text-muted-foreground">
                          ({tAgent('creator.basicInfo.active')})
                        </span>
                      )}
                    </span>
                  </SelectItem>
                  {llmBackends.map((backend) => (
                    <SelectItem key={backend.id} value={backend.id} textValue={backend.name}>
                      <span className="flex flex-1 min-w-0 items-center gap-2">
                        <span className="truncate min-w-0">{backend.name}</span>
                        <span className="text-muted-foreground shrink-0 truncate">{backend.model}</span>
                        <span className="ml-auto flex items-center gap-1 shrink-0">
                          {backend.capabilities?.supports_multimodal && (
                            <span title={tAgent('creator.basicInfo.supportsVision')} className="inline-flex items-center px-1.5 h-5 rounded font-medium bg-muted-30 text-muted-foreground">{tAgent('creator.capability.vision', { defaultValue: 'Vision' })}</span>
                          )}
                          {backend.capabilities?.supports_tools && (
                            <span title={tAgent('creator.basicInfo.supportsTools')} className="inline-flex items-center px-1.5 h-5 rounded font-medium bg-muted-30 text-muted-foreground">{tAgent('creator.capability.tools', { defaultValue: 'Tools' })}</span>
                          )}
                          {backend.capabilities?.supports_thinking && (
                            <span title={tAgent('creator.basicInfo.supportsThinking')} className="inline-flex items-center px-1.5 h-5 rounded font-medium bg-muted-30 text-muted-foreground">{tAgent('creator.capability.thinking', { defaultValue: 'Thinking' })}</span>
                          )}
                        </span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {llmValidationError && (
                <p className="text-xs text-error">{llmValidationError}</p>
              )}
            </div>

            {/* Agent Priority */}
            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium">{tAgent('creator.advanced.agentPriority', 'Agent Priority')}</Label>
                <span className="text-sm font-medium tabular-nums">{priority}</span>
              </div>
              <Slider
                min={1}
                max={10}
                step={1}
                value={[priority]}
                onValueChange={([v]) => setPriority(v)}
              />
              <p className="text-xs text-muted-foreground">
                {tAgent('creator.advanced.priorityHint', 'Execution priority (1=lowest, 10=highest)')}
              </p>
            </div>

            {/* Max Chain Depth */}
            <div className="space-y-2">
              <Label className="text-sm font-medium">{tAgent('creator.advanced.chainDepth', 'Max Chain Depth')}</Label>
              <Slider
                min={1}
                max={20}
                step={1}
                value={[maxChainDepth]}
                onValueChange={([v]) => setMaxChainDepth(v)}
              />
              <p className="text-xs text-muted-foreground">
                {tAgent('creator.advanced.chainDepthHint', 'Maximum tool-calling rounds per execution (1=single-pass, higher=more capable)')}
              </p>
            </div>

            {/* Conversation History Depth */}
            <div className="space-y-2">
              <Label className="text-sm font-medium">{tAgent('creator.advanced.historyDepth', 'Conversation History Depth')}</Label>
              <Select value={contextWindowSize.toString()} onValueChange={(v) => setContextWindowSize(parseInt(v))}>
                <SelectTrigger className="h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="5">5</SelectItem>
                  <SelectItem value="10">10</SelectItem>
                  <SelectItem value="20">20</SelectItem>
                  <SelectItem value="30">30</SelectItem>
                  <SelectItem value="50">50</SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {tAgent('creator.advanced.contextHint', 'Number of recent conversation turns to include as context')}
              </p>
            </div>

            {/* Execution Schedule */}
            <div className="space-y-3">
              <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>{tAgent('creator.basicInfo.scheduleLabel')}</Label>

              {/* Strategy Cards - 3 modes */}
              <div className={cn(
                "gap-2",
                isMobile ? "grid grid-cols-1" : "grid grid-cols-3"
              )}>
                <ScheduleCard
                  icon={<Clock className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.timer')}
                  description={scheduleType === 'timer' ? (
                    timerSubType === 'interval'
                      ? tAgent('creator.schedule.interval.preview', { value: intervalValue, unit: tAgent('creator.schedule.interval.minutes') })
                      : timerSubType === 'daily'
                        ? tAgent('creator.schedule.daily.preview', { hour: scheduleHour, minute: scheduleMinute })
                        : tAgent('creator.schedule.weekly.preview', { day: selectedWeekdays.length > 0 ? selectedWeekdays[0] : 1, hour: scheduleHour, minute: scheduleMinute })
                  ) : tAgent('creator.schedule.timer.description')}
                  active={scheduleType === 'timer'}
                  onClick={() => setScheduleType('timer')}
                  isMobile={isMobile}
                />
                <ScheduleCard
                  icon={<Activity className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.reactive')}
                  description={scheduleType === 'reactive' && triggerSources.length > 0
                    ? tAgent('creator.schedule.reactive.preview', {
                        names: (() => {
                          const unique = [...new Set(triggerSources.map(s => s.name))]
                          return unique.slice(0, 3).join(', ') + (unique.length > 3 ? '…' : '')
                        })(),
                      })
                    : tAgent('creator.schedule.reactive.description')}
                  active={scheduleType === 'reactive'}
                  onClick={() => setScheduleType('reactive')}
                  isMobile={isMobile}
                />
                <ScheduleCard
                  icon={<MousePointerClick className="h-5 w-5" />}
                  label={tAgent('creator.schedule.strategies.onDemand')}
                  description={tAgent('creator.schedule.onDemand.description')}
                  active={scheduleType === 'on-demand'}
                  onClick={() => setScheduleType('on-demand')}
                  isMobile={isMobile}
                />
              </div>

              {/* Schedule Configuration */}
              <div className={cn("border rounded-lg p-4")}>
                {scheduleType === 'timer' && (
                  <div className="space-y-3">
                    {/* Timer sub-type tabs */}
                    <div className={cn(
                      "flex gap-1",
                      isMobile ? "flex-wrap gap-2" : ""
                    )}>
                      {([
                        { key: 'interval' as TimerSubType, label: tAgent('creator.schedule.timer.subTypes.interval') },
                        { key: 'daily' as TimerSubType, label: tAgent('creator.schedule.timer.subTypes.daily') },
                        { key: 'weekly' as TimerSubType, label: tAgent('creator.schedule.timer.subTypes.weekly') },
                      ]).map(({ key, label }) => (
                        <button
                          key={key}
                          type="button"
                          onClick={() => setTimerSubType(key)}
                          className={cn(
                            "rounded-lg font-medium transition-colors",
                            isMobile
                              ? "px-4 py-2.5 text-sm flex-1"
                              : "px-3 py-1.5 text-sm",
                            timerSubType === key
                              ? "bg-primary text-primary-foreground"
                              : "bg-background hover:bg-muted"
                          )}
                        >
                          {label}
                        </button>
                      ))}
                    </div>

                    {/* Interval config */}
                    {timerSubType === 'interval' && (
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

                    {/* Daily config */}
                    {timerSubType === 'daily' && (
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

                    {/* Weekly config */}
                    {timerSubType === 'weekly' && (
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
                  </div>
                )}

                {scheduleType === 'reactive' && (() => {
                  // Build entity list: all entities from unifiedDataSources,
                  // supplemented with devices/extensions from props for richer metric info
                  const unifiedEntityMap = new Map<string, {
                    type: string; id: string; name: string
                    metrics: Array<{ name: string; display_name: string }>
                  }>()
                  for (const e of triggerEntities) {
                    unifiedEntityMap.set(`${e.type}:${e.id}`, e)
                  }

                  // Start with props-based entities (devices/extensions with full metric info)
                  const propsEntities = [
                    ...devices.map(d => ({
                      type: 'device' as const,
                      id: d.id,
                      name: d.name,
                      metrics: getDeviceMetrics(d.id),
                    })),
                    ...(extensions || []).map(ext => ({
                      type: 'extension' as const,
                      id: ext.id,
                      name: ext.name || ext.id,
                      metrics: getExtensionMetrics(ext.id),
                    })),
                  ]

                  // Merge: props entities first (they have richer info), then unified-only entities
                  const propsKeys = new Set(propsEntities.map(e => `${e.type}:${e.id}`))
                  const extraEntities = triggerEntities.filter(e => !propsKeys.has(`${e.type}:${e.id}`))
                  const entities = [...propsEntities, ...extraEntities]

                  const active = activeTriggerEntity
                    ? entities.find(e => e.type === activeTriggerEntity.type && e.id === activeTriggerEntity.id)
                    : null

                  const activeSources = active
                    ? triggerSources.filter(s => s.type === active.type && s.id === active.id)
                    : []
                  const activeAllSelected = activeSources.some(s => s.field === undefined)
                  const activeSelectedFields = activeSources
                    .filter(s => s.field !== undefined)
                    .map(s => s.field!)

                  const isEntityActive = (type: string, id: string) =>
                    triggerSources.some(s => s.type === type && s.id === id)

                  const getEntityFieldCount = (type: string, id: string) =>
                    triggerSources.filter(s => s.type === type && s.id === id).length

                  const toggleEntity = (type: string, id: string, name: string) => {
                    const wasActive = isEntityActive(type, id)
                    let newSources: typeof triggerSources
                    setTriggerSources(prev => {
                      const filtered = prev.filter(s => !(s.type === type && s.id === id))
                      if (wasActive) { newSources = filtered; return filtered }
                      newSources = [...filtered, { type, id, name }]
                      return newSources
                    })
                    syncResourceMetrics(type, id, name, newSources!, entities)
                  }

                  const toggleMetric = (type: string, id: string, name: string, field: string) => {
                    let newSources: typeof triggerSources
                    setTriggerSources(prev => {
                      const entitySources = prev.filter(s => s.type === type && s.id === id)
                      const selectedFields = entitySources.filter(s => s.field !== undefined).map(s => s.field!)
                      const filtered = prev.filter(s => !(s.type === type && s.id === id))
                      if (selectedFields.includes(field)) {
                        const remaining = selectedFields.filter(f => f !== field)
                        if (remaining.length === 0) { newSources = filtered; return filtered }
                        newSources = [...filtered, ...remaining.map(f => ({ type, id, name, field: f }))]
                        return newSources
                      }
                      newSources = [...filtered, ...selectedFields.map(f => ({ type, id, name, field: f })), { type, id, name, field }]
                      return newSources
                    })
                    syncResourceMetrics(type, id, name, newSources!, entities)
                  }

                  // Sync trigger selections to resources: create if missing, update selectedMetrics
                  const syncResourceMetrics = (
                    type: string, id: string, name: string,
                    sources: typeof triggerSources,
                    ents: typeof entities
                  ) => {
                    if (type !== 'device' && type !== 'extension') return
                    setSelectedResources(prev => {
                      const resourceKey = type === 'extension' ? `extension:${id}` : id
                      const hasAnyTrigger = sources.some(s => s.type === type && s.id === id)
                      const existing = prev.find(r => r.id === resourceKey)

                      // No trigger source for this entity → remove the resource
                      if (!hasAnyTrigger && existing) {
                        return prev.filter(r => r.id !== resourceKey)
                      }

                      const allSelected = sources.some(s => s.type === type && s.id === id && s.field === undefined)
                      const selectedFields = sources
                        .filter(s => s.type === type && s.id === id && s.field !== undefined)
                        .map(s => s.field!)

                      if (existing) {
                        const newMetrics = allSelected
                          ? new Set(existing.allMetrics.map(m => m.name))
                          : new Set(selectedFields)
                        if (setsEqual(newMetrics, existing.selectedMetrics)) return prev
                        return prev.map(r => r.id === resourceKey ? { ...r, selectedMetrics: newMetrics } : r)
                      }

                      // No trigger → don't create resource
                      if (!hasAnyTrigger) return prev

                      const entity = ents.find(e => e.type === type && e.id === id)
                      const allMetrics: MetricInfo[] = (entity?.metrics || []).map(m => ({
                        name: m.name,
                        display_name: m.display_name,
                        source: type as 'device' | 'extension',
                        ...(type === 'extension' ? { extensionId: id } : {}),
                      }))
                      const initialMetrics = allSelected
                        ? new Set(allMetrics.map(m => m.name))
                        : new Set(selectedFields)
                      return [...prev, {
                        id: resourceKey,
                        name,
                        type: type as 'device' | 'extension',
                        allMetrics,
                        allCommands: [],
                        selectedMetrics: initialMetrics,
                        selectedCommands: new Set<string>(),
                      }]
                    })
                  }

                  const setsEqual = (a: Set<string>, b: Set<string>) => {
                    if (a.size !== b.size) return false
                    for (const v of a) if (!b.has(v)) return false
                    return true
                  }

                  // Remove a specific field from trigger sources and sync resources
                  const removeTriggerField = (type: string, id: string, field?: string) => {
                    let newSources: typeof triggerSources
                    setTriggerSources(prev => {
                      if (field === undefined) {
                        newSources = prev.filter(s => !(s.type === type && s.id === id && s.field === undefined))
                      } else {
                        newSources = prev.filter(s => !(s.type === type && s.id === id && s.field === field))
                      }
                      return newSources
                    })
                    const entityName = (entities.find(e => e.type === type && e.id === id))?.name || id
                    syncResourceMetrics(type, id, entityName, newSources!, entities)
                  }

                  return (
                  <div className="space-y-3">
                    <div className="flex items-start gap-2 text-sm text-muted-foreground">
                      <Info className="h-4 w-4 mt-0.5 shrink-0" />
                      <p>{tAgent('creator.schedule.reactive.hint')}</p>
                    </div>

                    {/* Two-panel selector */}
                    <div className={cn(
                      "border rounded-lg",
                      isMobile ? "flex flex-col" : "flex",
                      isMobile ? "" : "h-[240px]"
                    )}>
                      {/* Left: entity list */}
                      <div className={cn(
                        "overflow-y-auto shrink-0",
                        isMobile ? "w-full border-b max-h-[120px]" : "w-[180px] border-r"
                      )}>
                        {entities.length === 0 ? (
                          <div className="p-3 text-xs text-muted-foreground text-center">
                            {tAgent('creator.schedule.reactive.noDevices')}
                          </div>
                        ) : (
                          entities.map(e => {
                            const isViewing = activeTriggerEntity?.type === e.type && activeTriggerEntity?.id === e.id
                            const hasTrigger = isEntityActive(e.type, e.id)
                            const fieldCount = getEntityFieldCount(e.type, e.id)
                            return (
                              <button
                                key={`${e.type}-${e.id}`}
                                type="button"
                                onClick={() => setActiveTriggerEntity({ type: e.type, id: e.id })}
                                className={cn(
                                  "w-full flex items-center gap-2 text-left transition-colors relative",
                                  isMobile ? "px-3 py-2 text-sm" : "px-2 py-1.5 text-xs",
                                  hasTrigger && "border-l-2 border-primary",
                                  !hasTrigger && "border-l-2 border-transparent",
                                  isViewing && hasTrigger && "bg-muted",
                                  isViewing && !hasTrigger && "bg-muted-30",
                                  "hover:bg-muted"
                                )}
                              >
                                {getSourceIcon(e.type, "h-4 w-4 shrink-0 text-muted-foreground")}
                                <span className="truncate flex-1">{e.name}</span>
                                {hasTrigger && (
                                  <Badge variant="secondary" className={cn("h-4 min-w-[18px]", textNano, "px-1 rounded-full")}>
                                    {activeAllSelected && isViewing ? tAgent('creator.schedule.reactive.allMetrics') : fieldCount}
                                  </Badge>
                                )}
                              </button>
                            )
                          })
                        )}
                      </div>

                      {/* Right: metric chips */}
                      <div className="flex-1 overflow-y-auto p-2.5">
                        {!active ? (
                          <div className="flex items-center justify-center h-full text-xs text-muted-foreground">
                            {tAgent('creator.schedule.reactive.selectSource')}
                          </div>
                        ) : active.metrics.length === 0 ? (
                          <div className="flex items-center justify-center h-full text-xs text-muted-foreground">
                            {tAgent('creator.schedule.reactive.noMetrics')}
                          </div>
                        ) : (
                          <div className="flex flex-wrap gap-1.5 content-start">
                            {/* "All" chip */}
                            <button
                              type="button"
                              onClick={() => toggleEntity(active.type, active.id, active.name)}
                              className={cn(
                                "inline-flex items-center rounded-md font-medium transition-colors",
                                isMobile ? "px-3 py-1.5 text-sm" : "px-2.5 py-1 text-xs",
                                activeAllSelected
                                  ? "bg-primary text-primary-foreground"
                                  : "bg-muted hover:bg-muted text-muted-foreground"
                              )}
                            >
                              {tAgent('creator.schedule.reactive.allMetrics')}
                            </button>
                            {/* Individual metric chips */}
                            {!activeAllSelected && active.metrics.map(m => {
                              const isSelected = activeSelectedFields.includes(m.name)
                              return (
                                <button
                                  key={m.name}
                                  type="button"
                                  onClick={() => toggleMetric(active.type, active.id, active.name, m.name)}
                                  className={cn(
                                    "inline-flex items-center rounded-md transition-colors",
                                    isMobile ? "px-3 py-1.5 text-sm" : "px-2.5 py-1 text-xs",
                                    isSelected
                                      ? "bg-muted text-primary font-medium ring-1 ring-primary"
                                      : "bg-muted hover:bg-muted text-muted-foreground"
                                  )}
                                >
                                  {m.display_name}
                                </button>
                              )
                            })}
                          </div>
                        )}
                      </div>
                    </div>

                    {/* Selected trigger sources summary */}
                    {triggerSources.length > 0 && (() => {
                      const grouped = new Map<string, { type: string; id: string; name: string; fields: (string | undefined)[] }>()
                      for (const s of triggerSources) {
                        const key = `${s.type}:${s.id}`
                        if (!grouped.has(key)) grouped.set(key, { type: s.type, id: s.id, name: s.name, fields: [] })
                        grouped.get(key)!.fields.push(s.field)
                      }
                      return (
                        <div className="space-y-1">
                          {[...grouped.values()].map(g => {
                            const hasAll = g.fields.includes(undefined)
                            const specificFields = g.fields.filter((f): f is string => f !== undefined)
                            const removeEntity = () => {
                              setTriggerSources(prev => prev.filter(s => !(s.type === g.type && s.id === g.id)))
                              // Remove corresponding resource
                              if (g.type === 'device' || g.type === 'extension') {
                                const resourceKey = g.type === 'extension' ? `extension:${g.id}` : g.id
                                setSelectedResources(prev => prev.filter(r => r.id !== resourceKey))
                              }
                            }
                            return (
                              <div key={`${g.type}-${g.id}`} className="flex items-center gap-1.5 flex-wrap">
                                {getSourceIcon(g.type, "h-4 w-4 shrink-0 text-muted-foreground")}
                                <span className="text-xs font-medium truncate max-w-[100px]">{g.name}</span>
                                {hasAll ? (
                                  <Badge
                                    variant="secondary"
                                    className={cn(textNano, "h-5 px-1.5 gap-0.5 cursor-pointer hover:bg-muted transition-colors")}
                                    onClick={removeEntity}
                                  >
                                    {tAgent('creator.schedule.reactive.allMetrics')}
                                    <X className="h-2.5 w-2.5" />
                                  </Badge>
                                ) : (
                                  specificFields.map(f => (
                                    <Badge
                                      key={f}
                                      variant="outline"
                                      className={cn(textNano, "h-5 px-1.5 gap-0.5 font-normal cursor-pointer hover:bg-muted transition-colors")}
                                      onClick={() => removeTriggerField(g.type, g.id, f)}
                                    >
                                      {f}
                                      <X className="h-2.5 w-2.5" />
                                    </Badge>
                                  ))
                                )}
                              </div>
                            )
                          })}
                        </div>
                      )
                    })()}

                    {/* Warning when no sources selected */}
                    {triggerSources.length === 0 && (
                      <div className="flex items-start gap-2 text-sm text-warning">
                        <Info className="h-4 w-4 mt-0.5 shrink-0" />
                        <p>{selectedResources.length > 0
                          ? tAgent('creator.schedule.reactive.fallbackToResources')
                          : tAgent('creator.schedule.reactive.emptyWarning')
                        }</p>
                      </div>
                    )}
                  </div>
                  )
                })()}

                {scheduleType === 'on-demand' && (
                  <div className="space-y-2">
                    <p className={cn("text-muted-foreground", isMobile ? "text-sm" : "text-sm")}>
                      {tAgent('creator.schedule.onDemand.hint')}
                    </p>
                  </div>
                )}
              </div>
            </div>

            {/* Resources Section */}
            {(() => {
              // Determine Resources section style based on schedule type + execution mode
              // Priority: scheduleType first (Reactive always supplemental), then executionMode
              const isReactive = scheduleType === 'reactive'
              const isProminent = !isReactive && isFocusedMode
              // isCollapsed = Free mode OR Reactive mode

              const sectionTitle = isReactive
                ? tAgent('creator.resources.supplementalTitle')
                : isProminent
                  ? tAgent('creator.resources.title')
                  : tAgent('creator.resources.preloadTitle')

              const sectionHint = isReactive
                ? tAgent('creator.resources.hintReactive')
                : isProminent
                  ? tAgent('creator.resources.hintFocused')
                  : tAgent('creator.resources.hintFree')

              if (isProminent) {
                return (
                  <div className="space-y-3 rounded-lg p-3 -mx-3 border border-border">
                    <div className={cn(
                      "flex items-center justify-between",
                      isMobile ? "flex-col items-start gap-3" : ""
                    )}>
                      <div className="flex items-center gap-2">
                        <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>{sectionTitle}</Label>
                        {selectedResources.length > 0 && (
                          <Badge variant="secondary" className="text-xs h-5">
                            {selectedResources.length}
                          </Badge>
                        )}
                      </div>
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
                    <p className="text-xs text-muted-foreground">{sectionHint}</p>
                    {selectedResources.length === 0 ? (
                      <div className="flex flex-col items-center justify-center py-6 border rounded-lg">
                        <Target className="h-6 w-6 text-muted-foreground mb-2" />
                        <p className="text-xs text-muted-foreground">
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
                            onRemove={() => setSelectedResources(prev => prev.filter(r => r.id !== resource.id))}
                            onToggleMetric={(resourceId, metricName) => {
                              setSelectedResources((prev) => prev.map(r => r.id === resourceId ? { ...r, selectedMetrics: new Set(r.selectedMetrics.has(metricName) ? Array.from(r.selectedMetrics).filter(n => n !== metricName) : [...r.selectedMetrics, metricName]) } : r))
                            }}
                            onToggleCommand={(resourceId, commandName) => {
                              setSelectedResources((prev) => prev.map(r => r.id === resourceId ? { ...r, selectedCommands: new Set(r.selectedCommands.has(commandName) ? Array.from(r.selectedCommands).filter(n => n !== commandName) : [...r.selectedCommands, commandName]) } : r))
                            }}
                            isMobile={isMobile}
                                                      />
                        ))}
                      </div>
                    )}
                  </div>
                )
              }

              // Collapsed style (Reactive / Free / On-demand)
              return (
                <div className="rounded-lg -mx-3 border overflow-hidden">
                  <button
                    type="button"
                    onClick={() => setFreeModeResourcesExpanded(!freeModeResourcesExpanded)}
                    className="w-full flex items-center justify-between p-3 text-left hover:bg-muted-30 transition-colors"
                  >
                    <div className="flex items-center gap-2">
                      <ChevronRight className={cn("h-4 w-4 transition-transform", freeModeResourcesExpanded && "rotate-90")} />
                      <Label className="text-sm font-medium">{sectionTitle}</Label>
                      {selectedResources.length > 0 && (
                        <Badge variant="secondary" className="text-xs h-5">
                          {selectedResources.length}
                        </Badge>
                      )}
                      <Badge variant="outline" className="text-xs h-5">
                        {tAgent('creator.resources.optional')}
                      </Badge>
                    </div>
                    <Plus
                      className="h-4 w-4 text-muted-foreground cursor-pointer hover:text-foreground"
                      onClick={(e) => { e.stopPropagation(); setResourceDialogOpen(true) }}
                    />
                  </button>
                  {freeModeResourcesExpanded && (
                    <div className="px-3 pb-3 pt-0 space-y-2 border-t">
                      <p className="text-xs text-muted-foreground pt-2">{sectionHint}</p>
                      {selectedResources.length === 0 ? (
                        <div className="flex flex-col items-center justify-center py-4 border rounded-lg">
                          <Target className="h-6 w-6 text-muted-foreground mb-1" />
                          <p className="text-xs text-muted-foreground">
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
                              onRemove={() => setSelectedResources(prev => prev.filter(r => r.id !== resource.id))}
                              onToggleMetric={(resourceId, metricName) => {
                                setSelectedResources((prev) => prev.map(r => r.id === resourceId ? { ...r, selectedMetrics: new Set(r.selectedMetrics.has(metricName) ? Array.from(r.selectedMetrics).filter(n => n !== metricName) : [...r.selectedMetrics, metricName]) } : r))
                              }}
                              onToggleCommand={(resourceId, commandName) => {
                                setSelectedResources((prev) => prev.map(r => r.id === resourceId ? { ...r, selectedCommands: new Set(r.selectedCommands.has(commandName) ? Array.from(r.selectedCommands).filter(n => n !== commandName) : [...r.selectedCommands, commandName]) } : r))
                              }}
                              isMobile={isMobile}
                            />
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )
            })()}
          </div>
        </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>

      {/* Footer with action buttons and summary */}
      <FullScreenDialogFooter className="flex-col items-stretch">
        <div className={cn(
          "flex gap-2",
          isMobile ? "justify-end" : "justify-end"
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
            isMobile ? "text-xs" : "text-xs"
          )}>
            <Info className={cn(isMobile ? "h-4 w-4" : "h-4 w-4", "shrink-0")} />
            <span className="line-clamp-1">{agentSummary}</span>
          </div>
        )}
      </FullScreenDialogFooter>

    </FullScreenDialog>

      {/* Resource Selection Dialog — sibling of FullScreenDialog to avoid Radix focus conflict (double-click issue) */}
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
    </>
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
          "fixed inset-0 z-[100] bg-background flex flex-col",
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
            <div className="px-4 py-3 border-b shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />}
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
                <Target className="h-8 w-8 text-muted-foreground mb-2" />
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
      </div>, getPortalRoot()
    )
  }

  // Desktop Dialog
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="z-[110] sm:max-w-3xl sm:max-h-[80vh] flex flex-col p-0 sm:p-0 gap-0 m-0">
        <DialogHeader className="px-5 py-3 border-b">
          <DialogTitle>{tAgent('creator.resources.dialog.title')}</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-hidden flex flex-col min-h-0">
          {/* Recommendations */}
          {recommendations.length > 0 && (
            <div className="px-5 py-3 border-b shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />}
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
          <div className="flex-1 flex gap-4 min-h-0 p-4 overflow-hidden">
            {/* Available Resources */}
            <div className="flex-1 flex flex-col rounded-lg overflow-hidden">
              <div className="p-3 border-b bg-bg-50">
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
            <div className="flex-1 flex flex-col rounded-lg overflow-hidden border">
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
                      <Target className="h-8 w-8 text-muted-foreground mb-2" />
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

        <div className="px-5 py-3 border-t flex justify-between items-center shrink-0">
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
        "flex flex-col items-center rounded-lg border-2 transition-all",
        isMobile ? "gap-3 p-4" : "gap-2 p-3",
        active
          ? "border-primary bg-muted"
          : "border-transparent hover:border-border"
      )}
    >
      <div className={cn("rounded-lg", active ? "bg-muted" : "", isMobile ? "p-2" : "p-1.5")}>
        {icon}
      </div>
      <div className="text-center">
        <div className={cn("font-medium", active ? "text-foreground" : "text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{label}</div>
        <div className={cn("text-muted-foreground", isMobile ? "text-xs" : textNano)}>{description}</div>
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
        selected ? "border-primary bg-muted" : "border-border hover:bg-muted-30"
      )}
    >
      <div className={cn(
        "p-1 rounded",
        selected ? "bg-primary text-primary-foreground" : "bg-muted"
      )}>
        {recommendation.type === 'extension' ? <Puzzle className="h-4 w-4" /> : <Target className="h-4 w-4" />}
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
        selected ? "bg-muted border border-border" : "hover:bg-muted border border-transparent"
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <div className={cn(
          "rounded",
          isMobile ? "p-2" : "p-1.5",
          selected ? "bg-primary text-primary-foreground" : "bg-muted"
        )}>
          {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />}
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
  const [showAllMetrics, setShowAllMetrics] = useState(false)
  const [showAllCommands, setShowAllCommands] = useState(false)
  const { t: tAgent } = useTranslation('agents')
  const selectedMetricCount = resource.selectedMetrics.size
  const selectedCommandCount = resource.selectedCommands.size
  const allMetricCount = resource.allMetrics.length
  const allCommandCount = resource.allCommands.length

  const hasMetrics = resource.allMetrics.length > 0
  const hasCommands = resource.allCommands.length > 0

  return (
    <div className={cn("rounded-lg border group", isMobile ? "px-4 py-3" : "px-3 py-2")}>
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
            resource.type === 'extension' ? "bg-accent-purple-light text-accent-purple" : "bg-info-light text-info"
          )}>
            {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />}
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
          <X className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />
        </Button>
      </div>

      {/* Collapsed state summary */}
      {!expanded && (selectedMetricCount > 0 || selectedCommandCount > 0) && (
        <div className={cn("mt-1", isMobile ? "pl-7" : "pl-6")}>
          <p className="text-xs text-muted-foreground truncate">
            {resource.allMetrics
              .filter(m => resource.selectedMetrics.has(m.name))
              .map(m => m.display_name)
              .slice(0, 3)
              .join(', ')}
            {selectedMetricCount > 3 && ` +${selectedMetricCount - 3} more`}
            {selectedMetricCount > 0 && selectedCommandCount > 0 && ' \u00B7 '}
            {selectedCommandCount > 0 && `${selectedCommandCount} cmd${selectedCommandCount > 1 ? 's' : ''}`}
          </p>
        </div>
      )}

      {/* Expandable Metrics/Commands — partition folding: show only selected by default */}
      {expanded && (hasMetrics || hasCommands) && (
        <div className={cn("space-y-2", isMobile ? "mt-3 pl-7" : "mt-2 pl-6")}>
          {/* Metrics */}
          {hasMetrics && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Metrics ({selectedMetricCount}/{allMetricCount})</span>
                <button
                  type="button"
                  onClick={() => {
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
                  {selectedMetricCount === allMetricCount ? 'Deselect All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {(showAllMetrics || selectedMetricCount === 0 ? resource.allMetrics : resource.allMetrics.filter(m => resource.selectedMetrics.has(m.name)))
                  .map((metric) => (
                  <div key={metric.name} className="contents">
                    <div
                      className={cn(
                        "flex items-center justify-between rounded transition-colors",
                        isMobile
                          ? "px-2 py-1 text-sm"
                          : "px-1.5 py-0.5 text-xs",
                        resource.selectedMetrics.has(metric.name)
                          ? "bg-muted text-primary"
                          : "hover:bg-muted-30"
                      )}
                    >
                      <div
                        className="flex items-center gap-2 cursor-pointer min-w-0"
                        onClick={() => onToggleMetric(resource.id, metric.name)}
                      >
                        <Checkbox
                          checked={resource.selectedMetrics.has(metric.name)}
                          className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")}
                        />
                        <span className="truncate">{metric.display_name}</span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
              {!showAllMetrics && selectedMetricCount < allMetricCount && (
                <button type="button" onClick={() => setShowAllMetrics(true)} className={inlineLinkBtn}>
                  Show All ({allMetricCount})
                </button>
              )}
              {showAllMetrics && selectedMetricCount < allMetricCount && (
                <button type="button" onClick={() => setShowAllMetrics(false)} className={inlineLinkBtn}>
                  Show Selected Only
                </button>
              )}
            </div>
          )}

          {/* Commands */}
          {hasCommands && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Commands ({selectedCommandCount}/{allCommandCount})</span>
                <button
                  type="button"
                  onClick={() => {
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
                  {selectedCommandCount === allCommandCount ? 'Deselect All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {(showAllCommands || selectedCommandCount === 0 ? resource.allCommands : resource.allCommands.filter(c => resource.selectedCommands.has(c.name)))
                  .map((command) => (
                  <div
                    key={command.name}
                    className={cn(
                      "flex items-center gap-2 rounded cursor-pointer transition-colors",
                      isMobile
                        ? "px-2 py-1 text-sm"
                        : "px-1.5 py-0.5 text-xs",
                      resource.selectedCommands.has(command.name)
                        ? "bg-muted text-primary"
                        : "hover:bg-muted-30"
                    )}
                    onClick={() => onToggleCommand(resource.id, command.name)}
                  >
                    <Checkbox
                      checked={resource.selectedCommands.has(command.name)}
                      className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")}
                    />
                    <span className="truncate">{command.display_name}</span>
                  </div>
                ))}
              </div>
              {!showAllCommands && selectedCommandCount < allCommandCount && (
                <button type="button" onClick={() => setShowAllCommands(true)} className={inlineLinkBtn}>
                  Show All ({allCommandCount})
                </button>
              )}
              {showAllCommands && selectedCommandCount < allCommandCount && (
                <button type="button" onClick={() => setShowAllCommands(false)} className={inlineLinkBtn}>
                  Show Selected Only
                </button>
              )}
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
