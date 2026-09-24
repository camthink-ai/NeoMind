/**
 * Agent Editor Full Screen (Single Page Layout)
 *
 * All configuration in one page:
 * - Left: Basic info (name, model)
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

import { Collapsible, CollapsibleTrigger, CollapsibleContent } from '@/components/ui/collapsible'
import { useState, useEffect, useCallback, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import { validateRequired, validateLength } from '@/lib/form-validation'
import { useToast } from '@/hooks/use-toast'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { showErrorToast } from '@/lib/error-messages'
import { useIsMobile } from '@/hooks/useMobile'
import { cn } from '@/lib/utils'
import { textNano } from '@/design-system/tokens/typography'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Badge } from '@/components/ui/badge'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { } from '@/components/ui/dialog'
import {
  Loader2,
  Clock,
  Check,
  Activity,
  X,
  Sparkles,
  Puzzle,
  Plus,
  Info,
  Database,
  Brain,
  MousePointerClick,
  GitBranch,
  ArrowRight,
} from 'lucide-react'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { Link } from 'react-router-dom'
import { channelsApi } from '@/lib/api/channels'
import type { AlertChannel } from '@/types/message'
import type {
  AiAgentDetail,
  AgentSchedule,
  CreateAgentRequest,
  AgentExecutionMode,
  AgentMemoryMode,
  AgentNotify,
  OperatorField,
  OperatorConfig,
  DryRunResult,
  Device,
  DeviceType,
  Extension,
  ExtensionDataSourceInfo,
  LlmBackendInstance,
  ResourceRequest,
  UnifiedDataSourceInfo,
} from '@/types'
import { BuilderShell } from '@/components/automation/dialog/BuilderShell'
import {
  ResourceSelectionDialog,
  PresetPickerDialog,
  ScheduleCard,
  INTERVALS,
  HOURS,
  deriveExecutionMode,
  hasOutputContract,
  FEATURED_PRESETS,
  nextStepFor,
  presetByKey,
  DEFAULT_LOOKBACK_MINUTES,
  DEFAULT_MAX_CHAIN_DEPTH,
  DEFAULT_NOTIFY_CHANNEL,
  DEFAULT_NOTIFY_ON,
  parseTriggerFilter,
  buildTriggerFilter,
  reasonTriggerFilterInvalid,
  type AgentPreset,
  type TriggerFilter,
  type TriggerSource,
} from './agent-editor'
import type {
  MetricInfo,
  CommandInfo,
  DataCollectionConfig,
  SelectedResource,
  ResourceRecommendation,
  ScheduleType,
  TimerSubType,
  AvailableResource,
} from './agent-editor'


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


// ============================================================================
// Main Component
// ============================================================================

/**
 * The single way this dialog explains itself: a small info icon whose text
 * appears on hover. Explanations stay off the surface, so the form reads as
 * labels and controls rather than as prose.
 */
function InfoHint({ text }: { text: string }) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <Info className="h-3.5 w-3.5 shrink-0 cursor-help text-muted-foreground" />
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-xs">
          <p>{text}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

/**
 * One section of the editor. Every section uses the same shell, so the dialog
 * reads as a designed form: the header carries the title, the body carries only
 * controls. No step numbers — this is a single-page form, not a wizard.
 */
function EditorSection({
  title,
  hint,
  children,
}: {
  title: React.ReactNode
  hint?: string
  children: React.ReactNode
}) {
  return (
    <section className="overflow-hidden rounded-lg border border-border">
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <Label className="text-sm font-medium">{title}</Label>
        {hint ? <InfoHint text={hint} /> : null}
      </header>
      <div className="space-y-3 p-4">{children}</div>
    </section>
  )
}

export function AgentEditorFullScreen({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  extensions = [],
  extensionDataSources: _extensionDataSources = [],
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
  const [triggerSources, setTriggerSources] = useState<TriggerSource[]>([])
  // How the selected trigger sources combine (M2). `any` fires on the first
  // match; `all` waits for every source inside `triggerWindowSecs`.
  const [triggerMode, setTriggerMode] = useState<TriggerFilter['mode']>('any')
  // null = the stored filter carries no window, which the backend reads as
  // "one single event must satisfy every source". Kept null (not repaired to a
  // default) so the editor can show the user the problem instead of quietly
  // changing what their rule means.
  const [triggerWindowSecs, setTriggerWindowSecs] = useState<number | null>(null)
  const [activeTriggerEntity, setActiveTriggerEntity] = useState<{ type: string; id: string } | null>(null)
  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [resourceDialogOpen, setResourceDialogOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState("")
  const [recommendations, setRecommendations] = useState<ResourceRecommendation[]>([])
  const [generatingRecommendations, setGeneratingRecommendations] = useState(false)

  // UI state
  const [saving, setSaving] = useState(false)

  // LLM backends
  const [llmBackends, setLlmBackends] = useState<LlmBackendInstance[]>([])
  const [activeBackendId, setActiveBackendId] = useState<string | null>(null)

  // How much latitude the agent gets. It is the input the derivation needs to
  // tell "investigate" from "summarise" — no resource or field can.
  const [canActAutonomously, setCanActAutonomously] = useState(false)
  // The chosen starting point, or null while the picker is showing. Create-only:
  // an existing agent already has its content.
  const [appliedPreset, setAppliedPreset] = useState<AgentPreset['key'] | null>(null)
  const [presetDialogOpen, setPresetDialogOpen] = useState(false)
  const [memoryMode, setMemoryMode] = useState<AgentMemoryMode | null>(null)
  // Notification routing: null = 未配置(走旧的关键词行为)。
  // Seeded with the API's floor rather than null: `create_agent` applies that
  // floor to any request that omits `notify`, so an empty card here would show
  // "nothing configured" while the agent that gets created routes to IM on
  // failure. What the card shows is what will be saved.
  const [notify, setNotify] = useState<AgentNotify | null>({
    channels: [DEFAULT_NOTIFY_CHANNEL],
    on: DEFAULT_NOTIFY_ON,
  })
  const [channels, setChannels] = useState<AlertChannel[]>([])
  const [outputSchema, setOutputSchema] = useState<OperatorField[]>([])
  const [operatorConfig, setOperatorConfig] = useState<OperatorConfig>({
    debounce_secs: 30,
    timeout_secs: 60,
    consecutive_failure_threshold: 3,
  })
  const [priority, setPriority] = useState(128)
  const [contextWindowSize, setContextWindowSize] = useState(10)
  const [maxChainDepth, setMaxChainDepth] = useState(DEFAULT_MAX_CHAIN_DEPTH)
  // Advanced knobs collapsed by default — defaults suit most agents

  // LLM validation state
  const [llmValidating, setLlmValidating] = useState(false)
  const [llmValid, setLlmValid] = useState<boolean | null>(null)
  const [llmValidationError, setLlmValidationError] = useState<string | null>(null)

  // Form field validation errors
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  // ========================================================================
  // Mode Helpers
  // ========================================================================

  // ── Derived execution mode ──────────────────────────────────────────────
  // The user answers four questions — what it should do, what it watches, what
  // it records, how often — and the mode follows from the answers.
  const hasDeviceCommands = useMemo(
    () => selectedResources.some((r) => r.selectedCommands.size > 0),
    [selectedResources],
  )
  const hasContract = useMemo(() => hasOutputContract(outputSchema), [outputSchema])
  const executionMode: AgentExecutionMode = deriveExecutionMode({
    hasDeviceCommands,
    canActAutonomously,
    hasOutputContract: hasContract,
  })
  const isFocusedMode = executionMode === 'focused'
  const isStructuredMode = executionMode === 'structured'
  // Mirrors `MemoryMode::derived_for` on the server: a scanner carries no
  // history, anything that reasons across turns does.
  const effectiveMemoryMode: AgentMemoryMode = memoryMode ?? (isStructuredMode ? 'tool' : 'assistant')
  // Resources say different things depending on the run: the only input for a
  // single-pass agent, a supplement when an event triggers it, an optional
  // preload when the agent fetches its own data.
  // What the binding strip means depends on the run; the title is fixed, the
  // role is the hint's job.
  const resourcesHint = scheduleType === 'reactive'
    ? tAgent('creator.resources.hintReactive')
    : isFocusedMode
      ? tAgent('creator.resources.hintFocused')
      : tAgent('creator.resources.hintFree')

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
      channelsApi.listMessageChannels().then((r) => setChannels(r.channels)).catch(() => {})
    }
  }, [open, loadBackends])

  // Reset/Load form when dialog opens
  useEffect(() => {
    setFieldErrors({})
    if (open) {
      if (agent) {
        // Edit mode
        setName(agent.name || '')
        setUserPrompt(agent.user_prompt || '')
        setLlmBackendId(agent.llm_backend_id || null)
        // Seeded, not derived: re-saving must not silently change how an
        // existing agent runs. A stored `free` means it was allowed latitude.
        setCanActAutonomously(agent.execution_mode === 'free')
        setMemoryMode(agent.memory_mode ?? null)
        setNotify(agent.notify ?? null)
        setNotify(agent.notify ?? null)
        setOutputSchema(agent.output_schema ?? [])
        setOperatorConfig(agent.operator_config ?? {
          debounce_secs: 30,
          timeout_secs: 60,
          consecutive_failure_threshold: 3,
        })
        setPriority(agent.priority ?? 128)
        setContextWindowSize(agent.context_window_size ?? 10)
        setMaxChainDepth(agent.max_chain_depth ?? DEFAULT_MAX_CHAIN_DEPTH)
        parseSchedule(agent.schedule)
        loadAgentResources(agent)
      } else {
        // Create mode - reset
        setName("")
        setUserPrompt("")
        setLlmBackendId(null)
        // Reset to defaults
        setCanActAutonomously(false)
        setMemoryMode(null)
        setNotify({ channels: [DEFAULT_NOTIFY_CHANNEL], on: DEFAULT_NOTIFY_ON })
        setAppliedPreset(null)
        setOutputSchema([])
        setOperatorConfig({ debounce_secs: 30, timeout_secs: 60, consecutive_failure_threshold: 3 })
        setPriority(5)
        setContextWindowSize(10)
        setMaxChainDepth(DEFAULT_MAX_CHAIN_DEPTH)
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
    if (schedule.schedule_type === 'manual') {
      // First-class manual-only form (repeatable via invoke)
      setScheduleType('on-demand')
    } else if (schedule.schedule_type === 'interval') {
      if (schedule.interval_seconds === 0) {
        // Legacy encoding of on-demand (pre-Once rows) — still readable
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
      // Trigger sources from event_filter — `any` (the legacy `sources`
      // shape included) or `all` with its window.
      const parsedFilter = parseTriggerFilter(schedule.event_filter)
      setTriggerMode(parsedFilter.mode)
      setTriggerWindowSecs(parsedFilter.withinSecs)
      setTriggerSources(
        parsedFilter.sources
          .map(s =>
            restoreFromBackendSourceType({
              type: s.type || 'device',
              id: s.id || '',
              name: s.name || s.id || '',
              ...(s.field ? { field: s.field } : {}),
            })
          )
          .filter((s: { id: string }) => s.id),
      )
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
            time_range_minutes: dc.time_range_minutes ?? DEFAULT_LOOKBACK_MINUTES,
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
              time_range_minutes: dc.time_range_minutes ?? DEFAULT_LOOKBACK_MINUTES,
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
          // The edited value wins: the lookback field writes onto the selected
          // resource, and the map only carries what was loaded from the server.
          data_collection:
            selectedResources.find(r => r.id === deviceId)?.config?.data_collection ??
            resourceDataCollectionConfigs.get(deviceId),
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
      const _motionKeywords = ['motion', 'move', 'movement', '移动', '人', 'person']
      const _alertKeywords = ['alert', 'notify', '告警', '通知', 'send', 'push']

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

  const outputContractOk = !isStructuredMode || hasOutputContract(outputSchema)
  // A Structured agent whose bound sources are all silent *refuses to run* —
  // `execute_structured` errors rather than publish a guess. So an agent saved
  // with no resources is one that can never succeed, and the editor is the only
  // place that can say so before the user waits for a schedule to fire.
  //
  // Focused and Free are NOT held to this: Focused keeps a tool loop and can
  // gather its own inputs, Free takes none by design. Blocking those would
  // reject agents that work.
  const resourceOk = !isStructuredMode || selectedResources.length > 0
  const triggerFilter: TriggerFilter = {
    mode: triggerMode,
    sources: triggerSources,
    withinSecs: triggerMode === 'all' ? triggerWindowSecs : null,
  }
  // Both shapes this catches would silently never fire, so neither is
  // saveable — see `reasonTriggerFilterInvalid`.
  const triggerFilterIssue =
    scheduleType === 'reactive'
      ? reasonTriggerFilterInvalid(triggerFilter, {
          hasBoundResources: selectedResources.length > 0,
        })
      : null

  const isValid: boolean =
    name.trim().length > 0 &&
    userPrompt.trim().length > 0 &&
    outputContractOk &&
    resourceOk &&
    !triggerFilterIssue

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

  /** Resource requests in the new unified format — shared by save and dry-run. */
  const buildResourceRequests = useCallback((): ResourceRequest[] => {
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
    return resources
  }, [selectedResources])

  const [dryRunResult, setDryRunResult] = useState<DryRunResult | null>(null)
  const [dryRunError, setDryRunError] = useState<string | null>(null)
  const [dryRunning, setDryRunning] = useState(false)

  /** 保存前试跑：transient dry-run via /api/agents/test-preview — nothing persisted. */
  const handleDryRun = async () => {
    if (!userPrompt.trim() || dryRunning) return
    setDryRunning(true)
    setDryRunError(null)
    setDryRunResult(null)
    try {
      const result = await api.testAgentPreview({
        user_prompt: userPrompt,
        execution_mode: executionMode,
        resources: buildResourceRequests(),
        output_schema: outputSchema.filter((f) => f.name.trim() !== ''),
        operator_config: operatorConfig,
        llm_backend_id: llmBackendId ?? undefined,
      })
      setDryRunResult(result)
    } catch (error) {
      handleError(error, { operation: 'Dry-run structured agent', showToast: false })
      setDryRunError(error instanceof Error ? error.message : String(error))
    } finally {
      setDryRunning(false)
    }
  }

  const handleSave = async () => {
    if (!isValid) return

    setSaving(true)
    try {
      let cronExpression: string | undefined = undefined
      let intervalSeconds: number | undefined = undefined
      let finalScheduleType: 'interval' | 'cron' | 'event' | 'manual' = 'interval'
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
        // Backend source-type mapping first, then the any/all envelope.
        const mappedSources: TriggerSource[] = triggerSources.map(s => {
          const mapped = mapToBackendSourceType(s.type, s.id)
          return { type: mapped.type, id: mapped.id, name: s.name, ...(s.field ? { field: s.field } : {}) }
        })
        eventFilter = buildTriggerFilter({ ...triggerFilter, sources: mappedSources })
      } else { // on-demand
        finalScheduleType = 'manual'
        intervalSeconds = undefined  // Manual needs no interval — never auto-scheduled
      }

      // Build resources array in the new format that supports both devices and extensions
      const resources = buildResourceRequests()

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
        priority: priority !== 128 ? priority : undefined,
        context_window_size: contextWindowSize !== 10 ? contextWindowSize : undefined,
        max_chain_depth:
          maxChainDepth !== DEFAULT_MAX_CHAIN_DEPTH ? maxChainDepth : undefined,
        execution_mode: executionMode,
        memory_mode: memoryMode ?? undefined,
        notify: notify ?? undefined,
        // The output contract is no longer structured-only: any mode may
        // publish fields. A reasoning agent does it in a post-run step.
        ...(hasContract
          ? {
              output_schema: outputSchema.filter((f) => f.name.trim() !== ''),
              operator_config: operatorConfig,
            }
          : {}),
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

  // One header treatment for every workspace section, numbered to match the
  // four questions the form asks — so the dialog reads as a flow rather than a
  // stack of unrelated blocks.
  const applyPreset = (preset: AgentPreset) => {
    // A matching name to go with the template — only when the field is still
    // empty, so a name the user typed is never clobbered.
    if (!name.trim()) setName(tAgent(`creator.preset.${preset.key}.name`))
    setUserPrompt(tAgent(`creator.preset.${preset.key}.prompt`))
    // Clicking the preset blurs the focused field BEFORE the fill lands, so
    // onBlur validation can fire on the still-empty value — and the error
    // only self-clears through onChange, which a programmatic fill never
    // triggers. Clear both here or "Name is required" sticks beside a
    // name the template just wrote.
    setFieldErrors((prev) => {
      const next = { ...prev }
      delete next.name
      delete next.prompt
      return next
    })
    setCanActAutonomously(preset.fill.autonomy)
    setOutputSchema(preset.fill.outputSchema ? preset.fill.outputSchema(tAgent).map((f) => ({ ...f })) : [])
    setMemoryMode(preset.fill.memoryMode)
    // A preset that says nothing about routing leaves `notify` null, and the
    // API's floor applies. A preset that does say — the two whose conclusions
    // are the point — routes nowhere on purpose: `judgment` means the task
    // speaks through its own tools, not through the machinery.
    setNotify(
      preset.fill.notify
        ? { channels: [...preset.fill.notify.channels], on: preset.fill.notify.on }
        : null,
    )
    const sched = preset.fill.schedule
    setScheduleType(sched.type)
    if (sched.type === 'timer') {
      setTimerSubType(sched.subType ?? 'interval')
      if (sched.intervalMinutes) setIntervalValue(sched.intervalMinutes)
      if (sched.hour !== undefined) {
        setScheduleHour(sched.hour)
        setScheduleMinute(0)
      }
      if (sched.weekday !== undefined) setSelectedWeekdays([sched.weekday])
    }
    setAppliedPreset(preset.key)
    setPresetDialogOpen(false)
  }

  const rail: Record<string, React.ReactNode> = {}
  const canvas: Record<string, React.ReactNode> = {}

  // The card naming the applied template is also the one place that can say
  // what the template left open. The line clears itself once the binding
  // exists — a hint that outlives its own condition is noise.
  const appliedPresetDef = appliedPreset ? presetByKey(appliedPreset) : undefined
  const presetNextStep = (() => {
    if (agent || !appliedPresetDef) return null
    const step = nextStepFor(appliedPresetDef)
    const met =
      step === 'trigger'
        ? triggerSources.length > 0 || selectedResources.length > 0
        : selectedResources.length > 0
    return met ? null : step
  })()

  rail['preset'] = (
    <>
            {/* Create-only: a starting point for the blank prompt. It fills the
                form and nothing gets hidden; picking another overwrites the
                same fields. Resources are never touched — cameras and devices
                are bound by the user in ②. */}
            {!agent && (
              appliedPreset ? (
                <div className="rounded-lg border border-border bg-muted-30 px-3 py-2">
                  <div className="flex items-center justify-between gap-2">
                    <span className="min-w-0 truncate text-xs text-muted-foreground">
                      {tAgent('creator.preset.appliedFrom', {
                        name: tAgent(`creator.preset.${appliedPreset}.name`),
                      })}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      onClick={() => setAppliedPreset(null)}
                    >
                      {tAgent('creator.preset.repick')}
                    </Button>
                  </div>
                  {presetNextStep && (
                    // A tinted strip rather than coloured text: this is the one
                    // thing on the card the user has to act on, and `bg-primary-light`
                    // bakes its alpha in at the variable level — `bg-primary/10`
                    // would silently render nothing.
                    <div className="mt-1.5 flex items-start gap-1.5 rounded-md bg-primary-light px-2 py-1.5 text-xs text-primary">
                      <ArrowRight className="mt-0.5 h-3 w-3 shrink-0" />
                      <span>{tAgent(`creator.preset.next.${presetNextStep}`)}</span>
                    </div>
                  )}
                </div>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center justify-between gap-2">
                    <Label className="text-sm font-medium">{tAgent('creator.preset.title')}</Label>
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="text-muted-foreground"
                      onClick={() => setPresetDialogOpen(true)}
                    >
                      {tAgent('creator.preset.all')}
                      <ArrowRight className="ml-1 h-3.5 w-3.5" />
                    </Button>
                  </div>
                  <div className="grid grid-cols-1 gap-1">
                    {FEATURED_PRESETS.map((preset) => {
                      const Icon = preset.icon
                      return (
                        <button
                          key={preset.key}
                          type="button"
                          onClick={() => applyPreset(preset)}
                          className="flex items-center gap-2 rounded-lg border border-border px-3 py-1.5 text-left transition-colors hover:border-muted-foreground hover:bg-muted"
                        >
                          <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                          <span className="shrink-0 text-sm font-medium">
                            {tAgent(`creator.preset.${preset.key}.name`)}
                          </span>
                          <span className="min-w-0 truncate text-xs text-muted-foreground">
                            {tAgent(`creator.preset.${preset.key}.desc`)}
                          </span>
                        </button>
                      )
                    })}
                  </div>
                </div>
              )
            )}
    </>
  )

  rail['name'] = (
    <>
            {/* Name */}
            <div className="space-y-2 min-w-0">
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
    </>
  )

  rail['model'] = (
    <>
            {/* Model Selection */}
            <div className="space-y-2 min-w-0">
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

            {/* Advanced knobs — rarely changed; collapsed so the required
                fields (mode / name / requirements) keep the visual focus. */}
    </>
  )

  canvas['prompt'] = (
    <>
            {/* Prompt */}
            <div className="space-y-3">

              <div className="flex items-center gap-2">
                <Label className="text-sm font-medium">
                  {tAgent('creator.basicInfo.requirement')} <span className="text-error">*</span>
                </Label>
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

              {/* 数据与指令 — the things the words above refer to, right under
                  the words. Deep selection (which metric of which device) stays
                  in the dialog; this strip is summary, add and remove. */}
              <div className="space-y-2 pt-1">
                <div className="flex items-center gap-2">
                  <Label className="text-sm font-medium">
                    {tAgent('creator.resources.title')}
                  </Label>
                  <InfoHint text={resourcesHint} />
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    className="ml-auto"
                    onClick={() => setResourceDialogOpen(true)}
                  >
                    <Plus className="mr-1 h-3.5 w-3.5" />
                    {tAgent('creator.resources.addResources')}
                  </Button>
                </div>
                {/* A zone with its own default height: the layout never jumps
                    between "empty hint" and "first chip", and the area reads as
                    a place you put things — dashed until it has some. */}
                <div
                  className={cn(
                    'flex min-h-[3rem] flex-wrap gap-1.5 rounded-md border bg-muted-30 p-2',
                    selectedResources.length === 0
                      ? 'items-center justify-center border-dashed border-border'
                      : 'content-start border-border',
                  )}
                >
                {selectedResources.length === 0 ? (
                  <p className="text-xs text-muted-foreground">
                    {tAgent('creator.resources.dialog.noResourcesHint')}
                  </p>
                ) : (
                  selectedResources.map((r) => (
                      <span
                        key={r.id}
                        className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-background px-2 text-xs"
                      >
                        {getSourceIcon(r.type, 'h-3.5 w-3.5 shrink-0 text-muted-foreground')}
                        <span className="max-w-[12rem] truncate font-medium">{r.name}</span>
                        {(r.selectedMetrics.size > 0 || r.selectedCommands.size > 0) && (
                          <span className="whitespace-nowrap text-muted-foreground">
                            {r.selectedMetrics.size > 0 &&
                              tAgent('creator.resources.metricCount', { count: r.selectedMetrics.size })}
                            {r.selectedMetrics.size > 0 && r.selectedCommands.size > 0 && ' · '}
                            {r.selectedCommands.size > 0 &&
                              tAgent('creator.resources.commandCount', { count: r.selectedCommands.size })}
                          </span>
                        )}
                        <button
                          type="button"
                          aria-label={tAgent('creator.resources.removeOne', { name: r.name })}
                          onClick={() =>
                            setSelectedResources((prev) => prev.filter((x) => x.id !== r.id))
                          }
                          className="ml-0.5 flex h-4 w-4 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground"
                        >
                          <X className="h-2.5 w-2.5" />
                        </button>
                      </span>
                    ))
                )}
                </div>
                {/* Where the choice is made, not two cards down: a Structured
                    agent's whole input is its bound sources and it refuses to
                    run without one, so this is the only place the requirement
                    can be acted on. */}
                {!resourceOk && (
                  <p className="text-sm text-error">{tAgent('creator.validation.resourceRequired')}</p>
                )}
              </div>

              {/* The one input nothing else can express: whether the AI may
                  take multiple rounds. It separates "investigate" from
                  "summarise" — each option says what happens, not what is
                  permitted. */}
              <div className="space-y-1.5 pt-1">
                <Label className="text-sm font-medium">
                  {tAgent('creator.workstyle.title')}
                </Label>
                <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-2">
                  {([
                    {
                      key: 'bound' as const,
                      active: !canActAutonomously,
                      apply: () => setCanActAutonomously(false),
                    },
                    {
                      key: 'auto' as const,
                      active: canActAutonomously,
                      apply: () => setCanActAutonomously(true),
                    },
                  ]).map(({ key, active, apply }) => (
                    <button
                      key={key}
                      type="button"
                      onClick={apply}
                      className={cn(
                        'flex flex-col items-start rounded-md border p-2.5 text-left transition-colors',
                        active
                          ? 'border-primary bg-muted'
                          : 'border-border hover:border-muted-foreground',
                      )}
                    >
                      <span className="text-sm font-medium">
                        {tAgent(`creator.workstyle.${key}`)}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {tAgent(`creator.workstyle.${key}Desc`)}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              {/* Rounds only mean anything once the AI may take them. Same
                  row shape as priority: label + hint left, exact number right —
                  a slider over 20 steps cannot express "exactly 3". */}
              {canActAutonomously && (
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <Label htmlFor="agent-chain-depth" className="text-sm font-medium">
                      {tAgent('creator.advanced.chainDepth', 'Rounds per run')}
                    </Label>
                    <InfoHint
                      text={tAgent(
                        'creator.advanced.chainDepthHint',
                        'How many rounds the AI may act for on its own. More rounds, more capable — and slower.',
                      )}
                    />
                  </div>
                  <Input
                    id="agent-chain-depth"
                    type="number"
                    min={1}
                    max={20}
                    step={1}
                    value={maxChainDepth}
                    onChange={(e) => {
                      const v = Number(e.target.value)
                      if (e.target.value !== '' && Number.isFinite(v)) {
                        setMaxChainDepth(Math.max(1, Math.min(20, Math.round(v))))
                      }
                    }}
                    className="h-9 w-24 text-right tabular-nums"
                  />
                </div>
              )}
            </div>
    </>
  )

  canvas['structured'] = (
    <>
            {/* The output contract — every mode may publish fields. A
                structured agent produces them directly; a reasoning agent gets
                them from a post-run step, which costs one extra call. */}
            <div className="space-y-2">
                {/* The user only needs to know the price on the modes that pay
                    it: a constrained read IS the output, a reasoning agent has
                    to be asked again at the end. */}
                {!isStructuredMode && hasContract && (
                  <p className="text-xs text-muted-foreground">
                    {tAgent('creator.structured.multiRoundCost')}
                  </p>
                )}
                {!outputContractOk && (
                  <p className="text-sm text-error">{tAgent('creator.validation.outputFieldRequired')}</p>
                )}

                {outputSchema.length > 0 && (
                  // Column identity lives in this header, not in the input
                  // placeholders — a placeholder disappears as soon as the
                  // field has a value, which is exactly when you need it.
                  <div className="hidden gap-2 px-2 md:grid md:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,0.7fr)_minmax(0,1.4fr)_1.75rem]">
                    {['colName', 'colType', 'colUnit', 'colDesc'].map((k) => (
                      <span key={k} className="text-xs text-muted-foreground">
                        {tAgent(`creator.structured.${k}`)}
                      </span>
                    ))}
                    <span />
                  </div>
                )}

                {outputSchema.map((field, idx) => (
                  <div key={idx} className="space-y-2 rounded-md border border-border p-2">
                    <div className="grid grid-cols-1 items-center gap-2 md:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,0.7fr)_minmax(0,1.4fr)_1.75rem]">
                      <Input
                        className="font-mono"
                        placeholder={tAgent('creator.structured.fieldName')}
                        value={field.name}
                        onChange={(e) => {
                          const next = [...outputSchema]
                          next[idx] = { ...field, name: e.target.value }
                          setOutputSchema(next)
                        }}
                      />
                      <Select
                        value={field.field_type.type}
                        onValueChange={(v) => {
                          const next = [...outputSchema]
                          next[idx] = {
                            ...field,
                            field_type:
                              v === 'enum'
                                ? { type: 'enum', values: [] }
                                : ({ type: v } as OperatorField['field_type']),
                          }
                          setOutputSchema(next)
                        }}
                      >
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="number">{tAgent('creator.structured.typeNumber')}</SelectItem>
                          <SelectItem value="text">{tAgent('creator.structured.typeText')}</SelectItem>
                          <SelectItem value="boolean">{tAgent('creator.structured.typeBoolean')}</SelectItem>
                          <SelectItem value="enum">{tAgent('creator.structured.typeEnum')}</SelectItem>
                        </SelectContent>
                      </Select>
                      <Input
                        placeholder={tAgent('creator.structured.fieldUnit')}
                        value={field.unit ?? ''}
                        onChange={(e) => {
                          const next = [...outputSchema]
                          next[idx] = { ...field, unit: e.target.value || undefined }
                          setOutputSchema(next)
                        }}
                      />
                      <Input
                        placeholder={tAgent('creator.structured.fieldDescription')}
                        value={field.description ?? ''}
                        onChange={(e) => {
                          const next = [...outputSchema]
                          next[idx] = { ...field, description: e.target.value || undefined }
                          setOutputSchema(next)
                        }}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        className="text-muted-foreground hover:text-error"
                        aria-label={tAgent('creator.structured.removeField')}
                        title={tAgent('creator.structured.removeField')}
                        onClick={() => setOutputSchema(outputSchema.filter((_, i) => i !== idx))}
                      >
                        <X className="h-4 w-4" />
                      </Button>
                    </div>
                    {field.field_type.type === 'enum' && (
                      <Input
                        placeholder={tAgent('creator.structured.enumValues')}
                        value={field.field_type.type === 'enum' ? field.field_type.values.join(', ') : ''}
                        onChange={(e) => {
                          const next = [...outputSchema]
                          next[idx] = {
                            ...field,
                            field_type: {
                              type: 'enum',
                              values: e.target.value.split(',').map((v) => v.trim()).filter(Boolean),
                            },
                          }
                          setOutputSchema(next)
                        }}
                      />
                    )}
                  </div>
                ))}

                <button
                  type="button"
                  onClick={() =>
                    setOutputSchema([...outputSchema, { name: '', field_type: { type: 'number' } }])
                  }
                  className="flex w-full items-center justify-center gap-1.5 rounded-md border border-dashed border-border py-1.5 text-xs text-muted-foreground transition-colors hover:border-muted-foreground hover:text-foreground"
                >
                  <Plus className="h-3.5 w-3.5" />
                  {tAgent('creator.structured.addField')}
                </button>

                <Collapsible>
                  <CollapsibleTrigger className="flex w-full items-center justify-between py-1 text-left text-xs text-muted-foreground">
                    <span>
                      {isStructuredMode
                        ? tAgent('creator.structured.guardrailSummary', {
                            debounce: operatorConfig.debounce_secs ?? 30,
                            timeout: operatorConfig.timeout_secs ?? 60,
                            threshold: operatorConfig.consecutive_failure_threshold ?? 3,
                            cap: operatorConfig.max_calls_per_day ?? '∞',
                          })
                        : tAgent('creator.structured.guardrailSummaryEveryRun', {
                            timeout: operatorConfig.timeout_secs ?? 60,
                            cap: operatorConfig.max_calls_per_day ?? '∞',
                          })}
                    </span>
                    <span className="text-primary">{tAgent('creator.structured.guardrailToggle')}</span>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                <div className={cn("grid grid-cols-1 gap-2 pt-1", isStructuredMode ? "md:grid-cols-4" : "md:grid-cols-2")}>
                  {/* Debounce and the breaker are L0 semantics — merging input
                      bursts into one inference, and tripping on repeated
                      inference failures. A reasoning agent has neither, so it
                      gets the two that apply to every run: timeout and cap. */}
                  {isStructuredMode && (
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{tAgent('creator.structured.debounce')}</Label>
                    <Input
                      type="number"
                      value={operatorConfig.debounce_secs ?? 30}
                      onChange={(e) =>
                        setOperatorConfig({ ...operatorConfig, debounce_secs: Number(e.target.value) || 30 })
                      }
                    />
                  </div>
                  )}
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{tAgent('creator.structured.timeout')}</Label>
                    <Input
                      type="number"
                      value={operatorConfig.timeout_secs ?? 60}
                      onChange={(e) =>
                        setOperatorConfig({ ...operatorConfig, timeout_secs: Number(e.target.value) || 60 })
                      }
                    />
                  </div>
                  {isStructuredMode && (
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{tAgent('creator.structured.threshold')}</Label>
                    <Input
                      type="number"
                      value={operatorConfig.consecutive_failure_threshold ?? 3}
                      onChange={(e) =>
                        setOperatorConfig({
                          ...operatorConfig,
                          consecutive_failure_threshold: Number(e.target.value) || 3,
                        })
                      }
                    />
                  </div>
                  )}
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{tAgent('creator.structured.dailyCap')}</Label>
                    <Input
                      type="number"
                      placeholder="—"
                      value={operatorConfig.max_calls_per_day ?? ''}
                      onChange={(e) =>
                        setOperatorConfig({
                          ...operatorConfig,
                          max_calls_per_day: e.target.value ? Number(e.target.value) : undefined,
                        })
                      }
                    />
                  </div>
                </div>
                  </CollapsibleContent>
                </Collapsible>
              </div>
            
            {/* Dry-run (试跑) result — capability zone feedback */}
            {dryRunResult && (
              <div className="space-y-2 rounded-lg border border-success bg-card p-3">
                <div className="flex items-center gap-2 text-sm font-medium text-success">
                  <Check className="h-4 w-4" />
                  {tAgent('creator.structured.dryRunResultTitle')}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {Object.entries(dryRunResult.fields).map(([k, v]) => (
                    <span key={k} className="rounded border border-border px-1.5 py-0.5 font-mono text-xs">
                      {k} = {String(v)}
                    </span>
                  ))}
                </div>
                <p className="line-clamp-3 rounded bg-muted p-2 font-mono text-xs text-muted-foreground">
                  {dryRunResult.raw_text}
                </p>
                <p className="text-xs text-muted-foreground">
                  {tAgent('creator.structured.dryRunMeta', {
                    sources: dryRunResult.data_sources,
                    attempts: dryRunResult.attempts,
                  })}
                </p>
              </div>
            )}
            {dryRunError && (
              <div className="rounded-lg border border-error bg-error-light p-3 text-sm text-error">
                {dryRunError}
              </div>
            )}
    </>
  )

  canvas['memory'] = (
    <>
            {/* What it remembers. The standards always apply (they are what the
                judgement is made against); only past events are optional. */}
            <div className="space-y-2">
              {/* Sub-block of the output contract: the contract is what it
                  records, this is how much of the past it carries alongside. */}
              <div className="flex items-center gap-2">
                <Label className="text-sm font-medium text-muted-foreground">
                  {tAgent('creator.memory.title')}
                </Label>
                <InfoHint
                  text={`${
                    effectiveMemoryMode === 'tool'
                      ? tAgent('creator.memory.statelessHint')
                      : tAgent('creator.memory.historyHint')
                  } ${tAgent('creator.memory.standardsKept')}`}
                />
              </div>
              <div className="flex flex-wrap gap-1">
                {([
                  { key: null, label: tAgent('creator.memory.auto') },
                  { key: 'tool' as const, label: tAgent('creator.memory.stateless') },
                  { key: 'assistant' as const, label: tAgent('creator.memory.history') },
                ]).map(({ key, label }) => (
                  <button
                    key={String(key)}
                    type="button"
                    onClick={() => setMemoryMode(key)}
                    className={cn(
                      "rounded-lg border px-3 py-1.5 text-sm font-medium transition-colors",
                      memoryMode === key
                        ? "border-primary bg-primary text-primary-foreground"
                        : "border-border bg-background hover:bg-muted"
                    )}
                  >
                    {label}
                  </button>
                ))}
              </div>
              {/* Depth only means anything once history is actually carried —
                  and only appears when the user picked that, not when the
                  derivation happens to land on it under "auto". */}
              {memoryMode === 'assistant' && (
                <div className="space-y-2">
                  <Label className="text-sm font-medium">
                    {tAgent('creator.advanced.historyDepth', 'Conversation History Depth')}
                  </Label>
                  <Select
                    value={contextWindowSize.toString()}
                    onValueChange={(v) => setContextWindowSize(parseInt(v))}
                  >
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
              )}

            </div>
    </>
  )


  canvas['notify'] = (
    <>
            {/* Notification routing — the channels and the moment, chosen
                explicitly instead of sniffed from the conclusion text. */}
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Label className="text-sm font-medium text-muted-foreground">
                  {tAgent('creator.notify.title')}
                </Label>
                <InfoHint text={tAgent('creator.notify.hint')} />
              </div>
              {channels.length === 0 ? (
                <p className="text-xs text-muted-foreground">
                  {tAgent('creator.notify.noChannels')}{' '}
                  {/* A way forward, not just a note: the old text pointed at
                      the Messages page in prose and left it there. */}
                  <Link to="/messages" className="text-primary hover:underline">
                    {tAgent('creator.notify.configureChannels')}
                  </Link>
                </p>
              ) : (
                <>
                  <div className="flex flex-wrap gap-1.5">
                    {channels.map((ch) => {
                      const on = notify?.channels.includes(ch.name) ?? false
                      return (
                        <Button
                          key={ch.name}
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() =>
                            setNotify((prev) => {
                              const base = prev ?? {
                                channels: [],
                                on: DEFAULT_NOTIFY_ON,
                              }
                              const next = on
                                ? base.channels.filter((c) => c !== ch.name)
                                : [...base.channels, ch.name]
                              // An empty list is not "unset": the storage layer
                              // reads it as a config that is present and routes
                              // nowhere, which is what unchecking every channel
                              // means. Returning `null` would let the API's
                              // floor put IM back — the user's wish, undone by
                              // a default they cannot see.
                              return { ...base, channels: next }
                            })
                          }
                          className={cn(
                            'gap-1.5',
                            on
                              ? 'border-primary bg-muted text-foreground'
                              : 'text-muted-foreground hover:border-muted-foreground',
                            !ch.enabled && 'opacity-50',
                          )}
                          title={ch.channel_type + (ch.enabled ? '' : ' (disabled)')}
                        >
                          {on && <Check className="h-4 w-4" />}
                          <span className="max-w-[10rem] truncate">{ch.name}</span>
                        </Button>
                      )
                    })}
                  </div>
                  {notify && (
                    <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-3">
                      {(['failure', 'always', 'judgment'] as const).map((k) => (
                        <button
                          key={k}
                          type="button"
                          onClick={() => setNotify((prev) => prev && { ...prev, on: k })}
                          className={cn(
                            'flex flex-col items-start rounded-md border p-2 text-left transition-colors',
                            notify.on === k
                              ? 'border-primary bg-muted'
                              : 'border-border hover:border-muted-foreground',
                          )}
                        >
                          <span className="text-sm font-medium">
                            {tAgent(`creator.notify.on${k[0].toUpperCase()}${k.slice(1)}`)}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            {tAgent(
                              `creator.notify.on${k[0].toUpperCase()}${k.slice(1)}Desc`,
                            )}
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                </>
              )}
            </div>
    </>
  )

  canvas['schedule'] = (
    <>
            {/* Execution Schedule */}
            <div className="space-y-3">

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
              <div className={cn("border rounded-lg p-3")}>
                {scheduleType === 'timer' && (
                  <div className="space-y-2">
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
                            "rounded-lg border font-medium transition-colors",
                            isMobile
                              ? "px-4 py-2.5 text-sm flex-1"
                              : "px-3 py-1.5 text-sm",
                            timerSubType === key
                              ? "border-primary bg-primary text-primary-foreground"
                              : "border-border bg-background hover:bg-muted"
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
                                "rounded-lg border font-medium transition-colors",
                                isMobile
                                  ? "px-4 py-3 text-base min-w-[60px]"
                                  : "px-3 py-1.5 text-sm",
                                intervalValue === mins
                                  ? "border-primary bg-primary text-primary-foreground"
                                  : "border-border bg-background hover:bg-muted"
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
                        <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                          <SelectTrigger className={cn(isMobile ? "w-28 h-11 text-base" : "w-24 h-9")}>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {HOURS.map((h) => (
                              <SelectItem key={h} value={h.toString()}>{h.toString().padStart(2, '0')}:00</SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
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
                                "rounded-lg border font-medium transition-colors",
                                isMobile
                                  ? "w-12 h-12 text-base"
                                  : "w-10 h-10 text-sm",
                                selectedWeekdays.includes(d)
                                  ? "border-primary bg-primary text-primary-foreground"
                                  : "border-border bg-background hover:bg-muted"
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
                            <SelectTrigger className={cn(isMobile ? "w-28 h-11 text-base" : "w-24 h-9")}>
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
                        // `overflow-x-hidden` as well as `overflow-y-auto`:
                        // a row wider than the column would otherwise scroll
                        // sideways, and the row background paints across the
                        // whole scroll area — which is what made the hover and
                        // selected blocks look like they overflowed the column.
                        "overflow-y-auto overflow-x-hidden shrink-0",
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
                                {/* `min-w-0` is what actually lets a flex item
                                    shrink below its text width. Without it
                                    `truncate` never engages on a long name and
                                    the row grows past the column. */}
                                <span className="min-w-0 flex-1 truncate">{e.name}</span>
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
                                  : "bg-muted text-muted-foreground hover:text-foreground"
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
                                      : "bg-muted text-muted-foreground hover:text-foreground"
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

                    {/* How the selected sources combine (M2). "any" fires on
                        the first match — what this editor has always meant.
                        "all" waits for every source inside a window, which is
                        what "occupancy AND not booked" needs.

                        Cards rather than a segmented strip: this is the same
                        "pick one, and here is what it means" choice as the
                        schedule cards above, so it speaks the same language. */}
                    {(triggerSources.length > 1 || triggerMode === 'all') && (
                      <div className="space-y-2.5">
                        <div className={cn('grid gap-2', isMobile ? 'grid-cols-1' : 'grid-cols-2')}>
                          {(['any', 'all'] as const).map(mode => {
                            const selected = triggerMode === mode
                            return (
                              <button
                                key={mode}
                                type="button"
                                aria-pressed={selected}
                                onClick={() => {
                                  setTriggerMode(mode)
                                  // An `all` group with no window never fires,
                                  // so offer a starting point rather than
                                  // leaving the field empty.
                                  if (mode === 'all' && triggerWindowSecs === null) {
                                    setTriggerWindowSecs(600)
                                  }
                                }}
                                className={cn(
                                  'flex items-start gap-2 rounded-lg border p-2.5 text-left transition-colors',
                                  selected
                                    ? 'border-primary bg-muted'
                                    : 'border-border hover:border-muted-foreground'
                                )}
                              >
                                <span className="mt-0.5 shrink-0">
                                  {mode === 'any'
                                    ? <Sparkles className="h-4 w-4" />
                                    : <GitBranch className="h-4 w-4" />}
                                </span>
                                <span className="min-w-0">
                                  <span className={cn(
                                    'block text-sm font-medium',
                                    selected ? 'text-foreground' : 'text-muted-foreground'
                                  )}>
                                    {tAgent(`creator.schedule.reactive.mode.${mode}`)}
                                  </span>
                                  <span className="block text-xs text-muted-foreground">
                                    {tAgent(`creator.schedule.reactive.modeHint.${mode}`)}
                                  </span>
                                </span>
                              </button>
                            )
                          })}
                        </div>

                        {/* The window phrased as a sentence, with the number in
                            it — "within [10] minutes" as a bare field read like
                            an unfinished form. */}
                        {triggerMode === 'all' && (
                          <div className="flex flex-wrap items-center gap-1.5 text-sm text-muted-foreground">
                            <span>{tAgent('creator.schedule.reactive.windowBefore')}</span>
                            <Input
                              type="number"
                              min={1}
                              aria-label={tAgent('creator.schedule.reactive.windowLabel')}
                              value={triggerWindowSecs === null ? '' : Math.round(triggerWindowSecs / 60)}
                              onChange={e => {
                                const minutes = Number(e.target.value)
                                setTriggerWindowSecs(minutes > 0 ? minutes * 60 : null)
                              }}
                              className="h-8 w-16 text-center"
                            />
                            <span>{tAgent('creator.schedule.reactive.windowAfter')}</span>
                          </div>
                        )}

                        {triggerFilterIssue && (
                          <div className="flex items-start gap-2 text-sm text-warning">
                            <Info className="h-4 w-4 mt-0.5 shrink-0" />
                            <p>{tAgent(`creator.schedule.reactive.issue.${triggerFilterIssue}`)}</p>
                          </div>
                        )}
                      </div>
                    )}

                    {/* What the picker above produced. Same `border rounded-lg`
                        surface as the picker, and one row per source, so this
                        reads as the result of that box rather than a loose list
                        floating under it. */}
                    {triggerSources.length > 0 && (() => {
                      const grouped = new Map<string, { type: string; id: string; name: string; fields: (string | undefined)[] }>()
                      for (const s of triggerSources) {
                        const key = `${s.type}:${s.id}`
                        if (!grouped.has(key)) grouped.set(key, { type: s.type, id: s.id, name: s.name, fields: [] })
                        grouped.get(key)!.fields.push(s.field)
                      }
                      return (
                        <div className="space-y-1.5">
                          <span className="block text-xs font-medium text-muted-foreground">
                            {tAgent('creator.schedule.reactive.selectedSources')}
                          </span>
                          <div className="rounded-lg border divide-y">
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
                              // A real button, not a clickable Badge: the old chips
                              // were status pills pretending to be controls, which
                              // said nothing about being removable until you hovered.
                              const chip = (key: string, label: string, onClick: () => void, aria: string) => (
                                <button
                                  key={key}
                                  type="button"
                                  onClick={onClick}
                                  aria-label={aria}
                                  className={cn(
                                    textNano,
                                    "inline-flex h-5 items-center gap-1 rounded-md border border-border bg-muted-30 px-1.5",
                                    "text-muted-foreground transition-colors hover:border-muted-foreground hover:text-foreground"
                                  )}
                                >
                                  {label}
                                  <X className="h-2.5 w-2.5 opacity-60" />
                                </button>
                              )
                              return (
                                <div
                                  key={`${g.type}-${g.id}`}
                                  className="flex flex-wrap items-center gap-x-2 gap-y-2 px-2.5 py-2"
                                >
                                  <span className="shrink-0">
                                    {getSourceIcon(g.type, "h-4 w-4 text-muted-foreground")}
                                  </span>
                                  {/* A floor, not just `flex-1`: many chips make
                                      a wide block beside the name, and a bare
                                      `flex-1` on a zero basis lets that block
                                      squeeze the name to nothing — which is how
                                      it disappeared once the chips ran past two
                                      lines. The row wraps instead, so the name
                                      keeps its line and the chips fall below. */}
                                  <span className="min-w-[8rem] flex-1 truncate text-xs font-medium">{g.name}</span>
                                  <span className="flex flex-wrap items-center gap-1">
                                    {hasAll
                                      ? chip(
                                          'all',
                                          tAgent('creator.schedule.reactive.allMetrics'),
                                          removeEntity,
                                          tAgent('creator.schedule.reactive.removeAllMetrics', { name: g.name }),
                                        )
                                      : specificFields.map(f =>
                                          chip(
                                            f,
                                            f,
                                            () => removeTriggerField(g.type, g.id, f),
                                            tAgent('creator.schedule.reactive.removeField', { field: f, name: g.name }),
                                          ),
                                        )}
                                  </span>
                                </div>
                              )
                            })}
                          </div>
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

              {/* Scheduling order — same axis as this section: when runs happen.
                  A number, not a slider: priority is a value you may want
                  exactly (128 is the default every untouched agent carries,
                  so "+1 to go first" is a real intent a drag cannot express). */}
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  <Label htmlFor="agent-priority" className="text-sm font-medium">
                    {tAgent('creator.advanced.agentPriority', 'Agent Priority')}
                  </Label>
                  <InfoHint
                    text={tAgent(
                      'creator.advanced.priorityHint',
                      'Higher runs first when agents compete for a slot (128 = default)',
                    )}
                  />
                </div>
                <Input
                  id="agent-priority"
                  type="number"
                  min={0}
                  max={255}
                  step={1}
                  value={priority}
                  onChange={(e) => {
                    const v = Number(e.target.value)
                    if (e.target.value !== '' && Number.isFinite(v)) {
                      setPriority(Math.max(0, Math.min(255, Math.round(v))))
                    }
                  }}
                  className="h-9 w-24 text-right tabular-nums"
                />
              </div>
            </div>
    </>
  )

  const footerNode = (
    <div className="flex w-full flex-col gap-2">
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
          {/* 试跑 open to every mode: structured previews the constrained
              extraction; reasoning modes preview through the same endpoint. */}
          <Button
            variant="outline"
            size={isMobile ? "default" : "sm"}
            onClick={handleDryRun}
              disabled={!userPrompt.trim() || !outputContractOk || dryRunning || saving}
              className={isMobile ? "min-w-[100px] h-12" : ""}
            >
              {dryRunning ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : null}
              {dryRunning ? tAgent('creator.structured.dryRunRunning') : tAgent('creator.structured.dryRun')}
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
    </div>
  )


  return (
    <>
    <BuilderShell
      open={open}
      onOpenChange={onOpenChange}
      accent="indigo"
      title={agent ? tAgent('editAgent') : tAgent('createAgent')}
      icon={<Sparkles className="h-5 w-5" />}
      config={<div className="space-y-5">{rail.preset}{rail.name}{rail.model}</div>}
      workspace={
        <div className="space-y-4">
          <EditorSection
            title={
              <>
                {tAgent('creator.section.what')} <span className="text-error">*</span>
              </>
            }
            hint={tAgent('creator.basicInfo.promptTip')}
          >
            {canvas.prompt}
          </EditorSection>
          <EditorSection
            title={tAgent('creator.section.record')}
            hint={tAgent('creator.structured.hint')}
          >
            {canvas.structured}
            {canvas.memory}
            {canvas.notify}
          </EditorSection>
          <EditorSection title={tAgent('creator.section.when')}>
            {canvas.schedule}
          </EditorSection>
        </div>
      }
      footer={footerNode}
      mobileConfigLabel={tAgent('creator.steps.basic')}
    />



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

      {/* Sibling of the editor dialog for the same reason as the one above. */}
      <PresetPickerDialog
        open={presetDialogOpen}
        onOpenChange={setPresetDialogOpen}
        onSelect={applyPreset}
      />
    </>
  )
}

// ============================================================================
// Resource Selection Dialog
// ============================================================================

export default AgentEditorFullScreen
