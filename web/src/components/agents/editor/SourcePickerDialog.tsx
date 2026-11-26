/**
 * Unified Input Sources section + sibling SourcePickerDialog.
 *
 * Supports all AgentSpec InputSourceRef variants:
 *   - whole_device / device_metric (grouped by device, metrics from DeviceType)
 *   - extension_instance (+ operation picker)
 *   - upstream_ai (published output_fields from other agents)
 *
 * Each bound requirement carries inline freshness & on_missing controls.
 */
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  Bot,
  Check,
  ChevronLeft,
  ChevronRight,
  Cpu,
  Database,
  Plus,
  Puzzle,
  Search,
  Server,
  Trash2,
  X,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { useIsMobile } from '@/hooks/useMobile'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { textMicro, textNano } from '@/design-system/tokens/typography'
import type {
  AiAgent,
  Device,
  DeviceType,
  Extension,
  FreshnessPolicy,
  InputContract,
  InputRequirement,
  InputSourceRef,
  MissingPolicy,
} from '@/types'

// ============================================================================
// Pure helpers
// ============================================================================

function sanitizeKey(raw: string): string {
  return raw.replace(/[^a-zA-Z0-9_]/g, '_').replace(/_+/g, '_').replace(/^_|_$/g, '') || 'input'
}

function uniqueKey(base: string, existing: InputRequirement[]): string {
  const clean = sanitizeKey(base)
  const used = new Set(existing.map((r) => r.key))
  if (!used.has(clean)) return clean
  let i = 2
  while (used.has(`${clean}_${i}`)) i += 1
  return `${clean}_${i}`
}

function describeSource(
  source: InputSourceRef,
  devices: Device[],
  extensions: Extension[],
  agents: AiAgent[],
): { badge: string; label: string } {
  switch (source.kind) {
    case 'whole_device': {
      const dev = devices.find((d) => (d.device_id || d.id) === source.device_id)
      return { badge: 'device', label: dev?.name ? `${dev.name} (${source.device_id})` : source.device_id }
    }
    case 'device_metric': {
      const dev = devices.find((d) => (d.device_id || d.id) === source.device_id)
      return {
        badge: 'metric',
        label: `${dev?.name || source.device_id} · ${source.field}`,
      }
    }
    case 'extension_instance': {
      const ext = extensions.find((e) => e.id === source.id)
      return { badge: 'extension', label: ext?.name || source.id }
    }
    case 'upstream_ai': {
      const ag = agents.find((a) => a.id === source.agent_id)
      return { badge: 'ai', label: `${ag?.name || source.agent_id} · ${source.field}` }
    }
    case 'caller_payload':
      return { badge: 'caller', label: 'caller_payload' }
  }
}

// ============================================================================
// Inline Bound Inputs List (inside Section 1 of BuilderShell workspace)
// ============================================================================

export interface UnifiedInputsSectionProps {
  value: InputContract
  onChange: (next: InputContract) => void
  devices: Device[]
  extensions: Extension[]
  agents: AiAgent[]
  onOpenPicker: () => void
}

export function UnifiedInputsSection({
  value,
  onChange,
  devices,
  extensions,
  agents,
  onOpenPicker,
}: UnifiedInputsSectionProps) {
  const { t } = useTranslation('agents')
  const requirements = value?.requirements ?? []

  const updateAt = (index: number, patch: Partial<InputRequirement>) => {
    const next = requirements.map((r, i) => (i === index ? { ...r, ...patch } : r))
    onChange({ requirements: next })
  }

  const removeAt = (index: number) => {
    onChange({ requirements: requirements.filter((_, i) => i !== index) })
  }

  const setFreshnessKind = (index: number, kind: FreshnessPolicy['kind']) => {
    if (kind === 'same_report') updateAt(index, { freshness: { kind: 'same_report' } })
    else if (kind === 'any') updateAt(index, { freshness: { kind: 'any' } })
    else if (kind === 'latest_within') {
      updateAt(index, { freshness: { kind: 'latest_within', max_age_secs: 3600 } })
    } else {
      updateAt(index, {
        freshness: { kind: 'lookback_window', lookback_secs: 3600, max_points: 60 },
      })
    }
  }

  return (
    <div className="space-y-3" data-section="sources">
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium text-foreground">
          {t('editorV2.sources.title')}
          {requirements.length > 0 && (
            <span className="ml-1.5 text-xs font-normal text-muted-foreground">
              ({requirements.length})
            </span>
          )}
        </span>
        <Button type="button" variant="outline" size="sm" onClick={onOpenPicker}>
          <Plus className="mr-1.5 h-3.5 w-3.5" />
          {t('editorV2.picker.addSource')}
        </Button>
      </div>

      {requirements.length === 0 ? (
        <div className="rounded-lg border border-dashed border-border bg-muted-20 px-4 py-3 text-xs text-muted-foreground">
          {t('editorV2.picker.emptyBound')}
        </div>
      ) : (
        <ul className="space-y-2.5">
          {requirements.map((req, index) => {
            const info = describeSource(req.source, devices, extensions, agents)
            const extId =
              req.source.kind === 'extension_instance' ? req.source.id : null
            const extMatch = extId ? extensions.find((e) => e.id === extId) : undefined
            return (
              <li
                key={`${req.key}-${index}`}
                className="rounded-lg border border-border bg-muted-20 p-3 space-y-2.5"
              >
                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="h-5 shrink-0 px-1.5 text-xs font-mono">
                    {info.badge}
                  </Badge>
                  <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">
                    {info.label}
                  </span>
                  <Input
                    value={req.key}
                    onChange={(e) => updateAt(index, { key: e.target.value })}
                    aria-label={t('editorV2.input.key')}
                    className="h-7 w-36 shrink-0 font-mono text-xs"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 text-muted-foreground hover:text-error"
                    onClick={() => removeAt(index)}
                    aria-label={t('editorV2.input.remove')}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>

                {/* Inline freshness, on_missing & optional extension operation */}
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <div className="flex items-center gap-1.5">
                    <span className="text-muted-foreground">{t('editorV2.picker.freshness')}</span>
                    <Select
                      value={req.freshness?.kind ?? 'any'}
                      onValueChange={(v) => setFreshnessKind(index, v as FreshnessPolicy['kind'])}
                    >
                      <SelectTrigger className="h-7 w-36 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="latest_within">{t('editorV2.picker.freshnessLatestWithinLabel')}</SelectItem>
                        <SelectItem value="lookback_window">{t('editorV2.picker.freshnessLookbackLabel')}</SelectItem>
                        <SelectItem value="same_report">{t('detail.freshnessSameReport')}</SelectItem>
                        <SelectItem value="any">{t('detail.freshnessAny')}</SelectItem>
                      </SelectContent>
                    </Select>
                    {req.freshness?.kind === 'latest_within' && (
                      <Input
                        type="number"
                        min={1}
                        value={req.freshness.max_age_secs}
                        onChange={(e) =>
                          updateAt(index, {
                            freshness: {
                              kind: 'latest_within',
                              max_age_secs: Math.max(1, Number(e.target.value) || 60),
                            },
                          })
                        }
                        aria-label={t('editorV2.picker.maxAgeSecs')}
                        className="h-7 w-20 text-xs tabular-nums"
                      />
                    )}
                    {req.freshness?.kind === 'lookback_window' && (
                      <>
                        <Input
                          type="number"
                          min={60}
                          value={req.freshness.lookback_secs}
                          onChange={(e) =>
                            updateAt(index, {
                              freshness: {
                                kind: 'lookback_window',
                                lookback_secs: Math.max(60, Number(e.target.value) || 3600),
                                max_points:
                                  req.freshness.kind === 'lookback_window'
                                    ? req.freshness.max_points
                                    : 60,
                              },
                            })
                          }
                          aria-label={t('editorV2.picker.lookbackSecs')}
                          className="h-7 w-20 text-xs tabular-nums"
                        />
                        <Input
                          type="number"
                          min={1}
                          value={req.freshness.max_points}
                          onChange={(e) =>
                            updateAt(index, {
                              freshness: {
                                kind: 'lookback_window',
                                lookback_secs:
                                  req.freshness.kind === 'lookback_window'
                                    ? req.freshness.lookback_secs
                                    : 3600,
                                max_points: Math.max(1, Number(e.target.value) || 60),
                              },
                            })
                          }
                          aria-label={t('editorV2.picker.maxPoints')}
                          className="h-7 w-16 text-xs tabular-nums"
                        />
                      </>
                    )}
                  </div>

                  <div className="flex items-center gap-1.5">
                    <span className="text-muted-foreground">{t('editorV2.picker.onMissing')}</span>
                    <Select
                      value={req.on_missing ?? 'report_stale'}
                      onValueChange={(v) => updateAt(index, { on_missing: v as MissingPolicy })}
                    >
                      <SelectTrigger className="h-7 w-32 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="report_stale">{t('detail.missingReportStale')}</SelectItem>
                        <SelectItem value="undetermined">{t('detail.missingUndetermined')}</SelectItem>
                        <SelectItem value="skip">{t('detail.missingSkip')}</SelectItem>
                        <SelectItem value="ignore">{t('detail.missingIgnore')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  {req.source.kind === 'extension_instance' && (
                    <div className="flex items-center gap-1.5">
                      <span className="text-muted-foreground">{t('editorV2.picker.extensionOp')}</span>
                      {extMatch?.commands && extMatch.commands.length > 0 ? (
                        <Select
                          value={req.operation || '__default__'}
                          onValueChange={(v) =>
                            updateAt(index, { operation: v === '__default__' ? undefined : v })
                          }
                        >
                          <SelectTrigger className="h-7 w-36 font-mono text-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="__default__">{t('editorV2.picker.extensionDefaultOp')}</SelectItem>
                            {extMatch.commands.map((cmd) => (
                              <SelectItem key={cmd.id} value={cmd.id}>
                                {cmd.display_name || cmd.id}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          value={req.operation ?? ''}
                          onChange={(e) => updateAt(index, { operation: e.target.value || undefined })}
                          placeholder="current"
                          className="h-7 w-28 font-mono text-xs"
                        />
                      )}
                    </div>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

// ============================================================================
// Sibling Dialog: UnifiedFormDialog + Master-Detail Source Picker
// ============================================================================

export interface SourcePickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  value: InputContract
  onChange: (next: InputContract) => void
  devices: Device[]
  deviceTypes: DeviceType[]
  extensions: Extension[]
  agents: AiAgent[]
  excludeAgentId?: string
}

type PickerTab = 'devices' | 'extensions' | 'ai'

function formatMetricValue(val: unknown): string {
  if (val === null || val === undefined) return ''
  if (typeof val === 'number') return Number.isInteger(val) ? String(val) : val.toFixed(2)
  if (typeof val === 'boolean') return val ? 'true' : 'false'
  if (typeof val === 'object') return JSON.stringify(val)
  return String(val)
}

export function SourcePickerDialog({
  open,
  onOpenChange,
  value,
  onChange,
  devices,
  deviceTypes,
  extensions,
  agents,
  excludeAgentId,
}: SourcePickerDialogProps) {
  const { t } = useTranslation(['agents', 'common'])
  const isMobile = useIsMobile()
  const [tab, setTab] = useState<PickerTab>('devices')
  const [query, setQuery] = useState('')
  const [staged, setStaged] = useState<InputRequirement[]>([])
  const [activeDeviceId, setActiveDeviceId] = useState<string | null>(null)
  const [activeExtId, setActiveExtId] = useState<string | null>(null)
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null)
  const [mobileDetailOpen, setMobileDetailOpen] = useState(false)

  // Reset staged state from committed value each time dialog opens
  useEffect(() => {
    if (!open) return
    setStaged(value?.requirements ?? [])
    setQuery('')
    setMobileDetailOpen(false)
  }, [open, value])

  const deviceTypeMap = useMemo(
    () => new Map(deviceTypes.map((dt) => [dt.device_type, dt])),
    [deviceTypes],
  )

  // Merge template metrics (from DeviceType) with runtime current_values keys
  const deviceMetricsMap = useMemo(() => {
    const map = new Map<string, Array<{ name: string; display_name?: string; unit?: string; value?: unknown }>>()
    for (const d of devices) {
      const id = d.device_id || d.id
      const dt = deviceTypeMap.get(d.device_type)
      const list: Array<{ name: string; display_name?: string; unit?: string; value?: unknown }> = []
      const seen = new Set<string>()
      for (const m of dt?.metrics ?? []) {
        seen.add(m.name)
        list.push({
          name: m.name,
          display_name: m.display_name,
          unit: m.unit,
          value: d.current_values?.[m.name],
        })
      }
      if (d.current_values && typeof d.current_values === 'object') {
        for (const [k, v] of Object.entries(d.current_values)) {
          if (!seen.has(k)) {
            list.push({ name: k, display_name: k, value: v })
          }
        }
      }
      map.set(id, list)
    }
    return map
  }, [devices, deviceTypeMap])

  const q = query.trim().toLowerCase()

  const filteredDevices = useMemo(
    () =>
      devices.filter((d) => {
        if (!q) return true
        const id = (d.device_id || d.id || '').toLowerCase()
        const name = (d.name || '').toLowerCase()
        const type = (d.device_type || '').toLowerCase()
        const metrics = deviceMetricsMap.get(d.device_id || d.id) ?? []
        const metricMatch = metrics.some(
          (m) =>
            m.name.toLowerCase().includes(q) ||
            (m.display_name || '').toLowerCase().includes(q),
        )
        return id.includes(q) || name.includes(q) || type.includes(q) || metricMatch
      }),
    [devices, deviceMetricsMap, q],
  )

  const filteredExtensions = useMemo(
    () =>
      extensions.filter((e) => {
        if (!q) return true
        const cmdMatch = e.commands?.some(
          (c) =>
            c.id.toLowerCase().includes(q) ||
            (c.display_name || '').toLowerCase().includes(q),
        )
        return (
          e.id.toLowerCase().includes(q) ||
          (e.name || '').toLowerCase().includes(q) ||
          Boolean(cmdMatch)
        )
      }),
    [extensions, q],
  )

  const aiAgentsWithFields = useMemo(
    () =>
      agents.filter((a) => {
        if (a.id === excludeAgentId) return false
        if (!a.output_fields || a.output_fields.length === 0) return false
        if (!q) return true
        return (
          a.name.toLowerCase().includes(q) ||
          a.output_fields.some((f) => f.toLowerCase().includes(q))
        )
      }),
    [agents, excludeAgentId, q],
  )

  // Keep master selection valid on open / filter changes
  useEffect(() => {
    if (!filteredDevices.some((d) => (d.device_id || d.id) === activeDeviceId)) {
      const first = filteredDevices[0]
      setActiveDeviceId(first ? first.device_id || first.id : null)
    }
  }, [filteredDevices, activeDeviceId])

  useEffect(() => {
    if (!filteredExtensions.some((e) => e.id === activeExtId)) {
      setActiveExtId(filteredExtensions[0]?.id ?? null)
    }
  }, [filteredExtensions, activeExtId])

  useEffect(() => {
    if (!aiAgentsWithFields.some((a) => a.id === activeAgentId)) {
      setActiveAgentId(aiAgentsWithFields[0]?.id ?? null)
    }
  }, [aiAgentsWithFields, activeAgentId])

  const selectedDevice = useMemo(
    () => filteredDevices.find((d) => (d.device_id || d.id) === activeDeviceId) ?? null,
    [filteredDevices, activeDeviceId],
  )
  const selectedExtension = useMemo(
    () => filteredExtensions.find((e) => e.id === activeExtId) ?? null,
    [filteredExtensions, activeExtId],
  )
  const selectedAgent = useMemo(
    () => aiAgentsWithFields.find((a) => a.id === activeAgentId) ?? null,
    [aiAgentsWithFields, activeAgentId],
  )

  const hasWholeDevice = (deviceId: string) =>
    staged.some((r) => r.source.kind === 'whole_device' && r.source.device_id === deviceId)

  const hasDeviceMetric = (deviceId: string, field: string) =>
    staged.some(
      (r) =>
        r.source.kind === 'device_metric' &&
        r.source.device_id === deviceId &&
        r.source.field === field,
    )

  const hasExtension = (extId: string, operation?: string) =>
    staged.some(
      (r) =>
        r.source.kind === 'extension_instance' &&
        r.source.id === extId &&
        (operation === undefined ? !r.operation : r.operation === operation),
    )

  const hasUpstreamAi = (agentId: string, field: string) =>
    staged.some(
      (r) =>
        r.source.kind === 'upstream_ai' &&
        r.source.agent_id === agentId &&
        r.source.field === field,
    )

  const deviceSelectedCount = (deviceId: string) =>
    staged.filter(
      (r) =>
        (r.source.kind === 'whole_device' || r.source.kind === 'device_metric') &&
        r.source.device_id === deviceId,
    ).length

  const extSelectedCount = (extId: string) =>
    staged.filter((r) => r.source.kind === 'extension_instance' && r.source.id === extId).length

  const aiSelectedCount = (agentId: string) =>
    staged.filter((r) => r.source.kind === 'upstream_ai' && r.source.agent_id === agentId).length

  const tabSelectedCounts = useMemo(
    () => ({
      devices: staged.filter(
        (r) => r.source.kind === 'whole_device' || r.source.kind === 'device_metric',
      ).length,
      extensions: staged.filter((r) => r.source.kind === 'extension_instance').length,
      ai: staged.filter((r) => r.source.kind === 'upstream_ai').length,
    }),
    [staged],
  )

  const toggleWholeDevice = (device: Device) => {
    const id = device.device_id || device.id
    if (hasWholeDevice(id)) {
      setStaged((prev) =>
        prev.filter((r) => !(r.source.kind === 'whole_device' && r.source.device_id === id)),
      )
      return
    }
    setStaged((prev) => [
      ...prev,
      {
        key: uniqueKey(`device_${id}`, prev),
        source: { kind: 'whole_device', device_id: id },
        timeout_secs: 10,
        result_type: 'structured_json',
        freshness: { kind: 'latest_within', max_age_secs: 3600 },
        on_missing: 'report_stale',
      },
    ])
  }

  const toggleDeviceMetric = (device: Device, field: string) => {
    const id = device.device_id || device.id
    if (hasDeviceMetric(id, field)) {
      setStaged((prev) =>
        prev.filter(
          (r) =>
            !(
              r.source.kind === 'device_metric' &&
              r.source.device_id === id &&
              r.source.field === field
            ),
        ),
      )
      return
    }
    setStaged((prev) => [
      ...prev,
      {
        key: uniqueKey(`${id}_${field}`, prev),
        source: { kind: 'device_metric', device_id: id, field },
        timeout_secs: 10,
        result_type: 'metric',
        freshness: { kind: 'latest_within', max_age_secs: 3600 },
        on_missing: 'report_stale',
      },
    ])
  }

  const toggleExtension = (ext: Extension, operation?: string) => {
    if (hasExtension(ext.id, operation)) {
      setStaged((prev) =>
        prev.filter(
          (r) =>
            !(
              r.source.kind === 'extension_instance' &&
              r.source.id === ext.id &&
              (operation === undefined ? !r.operation : r.operation === operation)
            ),
        ),
      )
      return
    }
    setStaged((prev) => [
      ...prev,
      {
        key: uniqueKey(operation ? `ext_${ext.id}_${operation}` : `ext_${ext.id}`, prev),
        source: { kind: 'extension_instance', id: ext.id },
        operation,
        auth_scope: `extension:${ext.id}:read`,
        timeout_secs: 10,
        result_type: 'structured_json',
        freshness: { kind: 'latest_within', max_age_secs: 3600 },
        on_missing: 'report_stale',
      },
    ])
  }

  const toggleUpstreamAi = (agent: AiAgent, field: string) => {
    if (hasUpstreamAi(agent.id, field)) {
      setStaged((prev) =>
        prev.filter(
          (r) =>
            !(
              r.source.kind === 'upstream_ai' &&
              r.source.agent_id === agent.id &&
              r.source.field === field
            ),
        ),
      )
      return
    }
    setStaged((prev) => [
      ...prev,
      {
        key: uniqueKey(`ai_${field}`, prev),
        source: { kind: 'upstream_ai', agent_id: agent.id, field },
        timeout_secs: 10,
        result_type: 'structured_json',
        freshness: { kind: 'latest_within', max_age_secs: 3600 },
        on_missing: 'report_stale',
      },
    ])
  }

  const handleConfirm = () => {
    onChange({ requirements: staged })
    onOpenChange(false)
  }

  const tabs: Array<{
    id: PickerTab
    Icon: typeof Cpu
    label: string
    count: number
    selected: number
  }> = [
    {
      id: 'devices',
      Icon: Cpu,
      label: t('agents:editorV2.picker.tabDevices'),
      count: filteredDevices.length,
      selected: tabSelectedCounts.devices,
    },
    {
      id: 'extensions',
      Icon: Puzzle,
      label: t('agents:editorV2.picker.tabExtensions'),
      count: filteredExtensions.length,
      selected: tabSelectedCounts.extensions,
    },
    {
      id: 'ai',
      Icon: Bot,
      label: t('agents:editorV2.picker.tabAi'),
      count: aiAgentsWithFields.length,
      selected: tabSelectedCounts.ai,
    },
  ]

  const masterPane = (
    <div className="flex h-full flex-col">
      {/* Search inside master pane */}
      <div className="border-b border-border p-2.5">
        <div className="relative">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('agents:editorV2.picker.searchPlaceholder')}
            className="h-8 pl-8 text-xs"
          />
        </div>
      </div>

      {/* Master items */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {tab === 'devices' && (
          filteredDevices.length === 0 ? (
            <p className="p-6 text-center text-xs text-muted-foreground">
              {t('agents:editorV2.picker.noDevices')}
            </p>
          ) : (
            filteredDevices.map((d) => {
              const id = d.device_id || d.id
              const isActive = id === activeDeviceId
              const count = deviceSelectedCount(id)
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => {
                    setActiveDeviceId(id)
                    if (isMobile) setMobileDetailOpen(true)
                  }}
                  className={cn(
                    'flex w-full items-center gap-2.5 border-b border-border/50 px-3 py-2.5 text-left transition-colors',
                    isActive ? 'bg-muted' : 'hover:bg-muted-30',
                  )}
                >
                  <span
                    className={cn(
                      'h-2 w-2 shrink-0 rounded-full',
                      d.online ? 'bg-success' : 'bg-muted-foreground/40',
                    )}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-foreground">
                      {d.name || id}
                    </div>
                    <div className={cn(textNano, 'truncate font-mono text-muted-foreground')}>
                      {id}
                    </div>
                  </div>
                  {count > 0 && (
                    <Badge variant="secondary" className="h-5 shrink-0 px-1.5 text-xs">
                      {count}
                    </Badge>
                  )}
                  {isMobile && <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />}
                </button>
              )
            })
          )
        )}

        {tab === 'extensions' && (
          filteredExtensions.length === 0 ? (
            <p className="p-6 text-center text-xs text-muted-foreground">
              {t('agents:editorV2.picker.noExtensions')}
            </p>
          ) : (
            filteredExtensions.map((ext) => {
              const isActive = ext.id === activeExtId
              const count = extSelectedCount(ext.id)
              return (
                <button
                  key={ext.id}
                  type="button"
                  onClick={() => {
                    setActiveExtId(ext.id)
                    if (isMobile) setMobileDetailOpen(true)
                  }}
                  className={cn(
                    'flex w-full items-center gap-2.5 border-b border-border/50 px-3 py-2.5 text-left transition-colors',
                    isActive ? 'bg-muted' : 'hover:bg-muted-30',
                  )}
                >
                  <Puzzle className={cn('h-4 w-4 shrink-0', isActive ? 'text-primary' : 'text-muted-foreground')} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-foreground">
                      {ext.name || ext.id}
                    </div>
                    <div className={cn(textNano, 'truncate font-mono text-muted-foreground')}>
                      {ext.id}
                    </div>
                  </div>
                  {count > 0 && (
                    <Badge variant="secondary" className="h-5 shrink-0 px-1.5 text-xs">
                      {count}
                    </Badge>
                  )}
                  {isMobile && <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />}
                </button>
              )
            })
          )
        )}

        {tab === 'ai' && (
          aiAgentsWithFields.length === 0 ? (
            <p className="p-6 text-center text-xs text-muted-foreground">
              {t('agents:editorV2.picker.noAiSources')}
            </p>
          ) : (
            aiAgentsWithFields.map((ag) => {
              const isActive = ag.id === activeAgentId
              const count = aiSelectedCount(ag.id)
              return (
                <button
                  key={ag.id}
                  type="button"
                  onClick={() => {
                    setActiveAgentId(ag.id)
                    if (isMobile) setMobileDetailOpen(true)
                  }}
                  className={cn(
                    'flex w-full items-center gap-2.5 border-b border-border/50 px-3 py-2.5 text-left transition-colors',
                    isActive ? 'bg-muted' : 'hover:bg-muted-30',
                  )}
                >
                  <Bot className={cn('h-4 w-4 shrink-0', isActive ? 'text-primary' : 'text-muted-foreground')} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-foreground">{ag.name}</div>
                    <div className={cn(textNano, 'truncate text-muted-foreground')}>
                      {(ag.output_fields ?? []).length} fields
                    </div>
                  </div>
                  {count > 0 && (
                    <Badge variant="secondary" className="h-5 shrink-0 px-1.5 text-xs">
                      {count}
                    </Badge>
                  )}
                  {isMobile && <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />}
                </button>
              )
            })
          )
        )}
      </div>
    </div>
  )

  const detailPane = (
    <div className="flex h-full flex-col">
      {isMobile && (
        <div className="flex items-center gap-2 border-b border-border bg-muted-20 px-3 py-2">
          <Button
            type="button"
            variant="ghost"
            size="xs"
            onClick={() => setMobileDetailOpen(false)}
          >
            <ChevronLeft className="mr-1 h-3.5 w-3.5" />
            {t('common:back', 'Back')}
          </Button>
        </div>
      )}

      {tab === 'devices' && (
        !selectedDevice ? (
          <div className="flex flex-1 items-center justify-center p-6 text-sm text-muted-foreground">
            {t('agents:editorV2.picker.selectDeviceHint')}
          </div>
        ) : (
          (() => {
            const devId = selectedDevice.device_id || selectedDevice.id
            const wholeOn = hasWholeDevice(devId)
            const metrics = deviceMetricsMap.get(devId) ?? []
            return (
              <div className="flex min-h-0 flex-1 flex-col">
                <div className="flex items-center justify-between border-b border-border bg-muted-20 px-4 py-2.5">
                  <div className="flex min-w-0 items-center gap-2">
                    <Server className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className="truncate text-sm font-medium text-foreground">
                      {selectedDevice.name || devId}
                    </span>
                    <span className={cn(textNano, 'truncate font-mono text-muted-foreground')}>
                      {selectedDevice.device_type}
                    </span>
                  </div>
                  <Badge variant="outline" className="h-5 shrink-0 px-1.5 text-xs">
                    {metrics.length}
                  </Badge>
                </div>

                <div className="min-h-0 flex-1 overflow-y-auto p-3 space-y-2">
                  {/* Whole device binding row */}
                  <button
                    type="button"
                    onClick={() => toggleWholeDevice(selectedDevice)}
                    className={cn(
                      'flex w-full items-center gap-3 rounded-lg border p-3 text-left transition-colors',
                      wholeOn
                        ? 'border-primary bg-primary-light'
                        : 'border-border bg-card hover:bg-muted-30',
                    )}
                  >
                    <div
                      className={cn(
                        'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                        wholeOn ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                      )}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium text-foreground">
                        {wholeOn
                          ? t('agents:editorV2.picker.wholeDeviceAdded')
                          : t('agents:editorV2.picker.wholeDevice')}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {t('agents:editorV2.picker.wholeDeviceDesc')}
                      </div>
                    </div>
                    <Badge variant="outline" className="h-5 shrink-0 font-mono text-xs">
                      whole_device
                    </Badge>
                  </button>

                  {/* Metrics section */}
                  <div className="pt-1 text-xs font-medium text-muted-foreground">
                    {t('agents:editorV2.picker.metrics')}
                  </div>

                  {metrics.length === 0 ? (
                    <p className="rounded-lg border border-dashed border-border p-4 text-center text-xs text-muted-foreground">
                      {t('agents:editorV2.picker.noMetrics')}
                    </p>
                  ) : (
                    <div className="space-y-1.5">
                      {metrics.map((m) => {
                        const on = hasDeviceMetric(devId, m.name)
                        const valText = formatMetricValue(m.value)
                        return (
                          <button
                            key={m.name}
                            type="button"
                            onClick={() => toggleDeviceMetric(selectedDevice, m.name)}
                            className={cn(
                              'flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left transition-colors',
                              on
                                ? 'border-primary bg-primary-light'
                                : 'border-border bg-card hover:bg-muted-30',
                            )}
                          >
                            <div
                              className={cn(
                                'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                                on ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                              )}
                            >
                              <Check className="h-3.5 w-3.5" />
                            </div>
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span className="truncate text-sm font-medium text-foreground">
                                  {m.display_name || m.name}
                                </span>
                                <code className={cn(textMicro, 'rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground')}>
                                  {m.name}
                                </code>
                              </div>
                            </div>
                            {(valText || m.unit) && (
                              <span className="shrink-0 font-mono text-xs text-muted-foreground">
                                {valText}
                                {m.unit ? ` ${m.unit}` : ''}
                              </span>
                            )}
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>
              </div>
            )
          })()
        )
      )}

      {tab === 'extensions' && (
        !selectedExtension ? (
          <div className="flex flex-1 items-center justify-center p-6 text-sm text-muted-foreground">
            {t('agents:editorV2.picker.selectExtensionHint')}
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-center justify-between border-b border-border bg-muted-20 px-4 py-2.5">
              <div className="flex min-w-0 items-center gap-2">
                <Puzzle className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate text-sm font-medium text-foreground">
                  {selectedExtension.name || selectedExtension.id}
                </span>
              </div>
              {selectedExtension.version && (
                <Badge variant="outline" className="h-5 shrink-0 font-mono text-xs">
                  v{selectedExtension.version}
                </Badge>
              )}
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto p-3 space-y-2">
              {selectedExtension.description && (
                <p className="text-xs text-muted-foreground px-1">
                  {selectedExtension.description}
                </p>
              )}

              {/* Default extension binding */}
              {(() => {
                const defaultOn = hasExtension(selectedExtension.id, undefined)
                return (
                  <button
                    type="button"
                    onClick={() => toggleExtension(selectedExtension)}
                    className={cn(
                      'flex w-full items-center gap-3 rounded-lg border p-3 text-left transition-colors',
                      defaultOn
                        ? 'border-primary bg-primary-light'
                        : 'border-border bg-card hover:bg-muted-30',
                    )}
                  >
                    <div
                      className={cn(
                        'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                        defaultOn ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                      )}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium text-foreground">
                        {t('agents:editorV2.picker.bindExtensionDefault')}
                      </div>
                      <div className={cn(textNano, 'font-mono text-muted-foreground')}>
                        {selectedExtension.id}
                      </div>
                    </div>
                  </button>
                )
              })()}

              {/* Specific commands if available */}
              {selectedExtension.commands && selectedExtension.commands.length > 0 && (
                <>
                  <div className="pt-1 text-xs font-medium text-muted-foreground">
                    {t('agents:editorV2.picker.extensionCommands')}
                  </div>
                  <div className="space-y-1.5">
                    {selectedExtension.commands.map((cmd) => {
                      const on = hasExtension(selectedExtension.id, cmd.id)
                      return (
                        <button
                          key={cmd.id}
                          type="button"
                          onClick={() => toggleExtension(selectedExtension, cmd.id)}
                          className={cn(
                            'flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left transition-colors',
                            on
                              ? 'border-primary bg-primary-light'
                              : 'border-border bg-card hover:bg-muted-30',
                          )}
                        >
                          <div
                            className={cn(
                              'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                              on ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                            )}
                          >
                            <Check className="h-3.5 w-3.5" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="truncate text-sm font-medium text-foreground">
                                {cmd.display_name || cmd.id}
                              </span>
                              <code className={cn(textMicro, 'rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground')}>
                                {cmd.id}
                              </code>
                            </div>
                            {cmd.description && (
                              <div className="truncate text-xs text-muted-foreground">
                                {cmd.description}
                              </div>
                            )}
                          </div>
                        </button>
                      )
                    })}
                  </div>
                </>
              )}
            </div>
          </div>
        )
      )}

      {tab === 'ai' && (
        !selectedAgent ? (
          <div className="flex flex-1 items-center justify-center p-6 text-sm text-muted-foreground">
            {t('agents:editorV2.picker.selectAiHint')}
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-center justify-between border-b border-border bg-muted-20 px-4 py-2.5">
              <div className="flex min-w-0 items-center gap-2">
                <Bot className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate text-sm font-medium text-foreground">
                  {selectedAgent.name}
                </span>
              </div>
              <Badge variant="outline" className="h-5 shrink-0 px-1.5 text-xs">
                {(selectedAgent.output_fields ?? []).length}
              </Badge>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto p-3 space-y-1.5">
              {(selectedAgent.output_fields ?? []).map((field) => {
                const on = hasUpstreamAi(selectedAgent.id, field)
                return (
                  <button
                    key={field}
                    type="button"
                    onClick={() => toggleUpstreamAi(selectedAgent, field)}
                    className={cn(
                      'flex w-full items-center gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors',
                      on
                        ? 'border-primary bg-primary-light'
                        : 'border-border bg-card hover:bg-muted-30',
                    )}
                  >
                    <div
                      className={cn(
                        'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                        on ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                      )}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium text-foreground">{field}</div>
                      <code className={cn(textNano, 'font-mono text-muted-foreground')}>
                        ai:{selectedAgent.id.slice(0, 8)}:{field}
                      </code>
                    </div>
                    <Activity className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  </button>
                )
              })}
            </div>
          </div>
        )
      )}
    </div>
  )

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('agents:editorV2.picker.title')}
      description={t('agents:editorV2.picker.subtitle')}
      icon={<Database className="h-5 w-5 text-primary" />}
      width="2xl"
      className="sm:h-[74vh] z-[110]"
      contentClassName="!p-0 flex flex-col overflow-hidden"
      preventCloseOnSubmit={false}
      footer={
        <>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:cancel')}
          </Button>
          <Button type="button" onClick={handleConfirm}>
            {t('common:confirm')}
            {staged.length > 0 ? ` (${staged.length})` : ''}
          </Button>
        </>
      }
    >
      {/* Selected items summary bar */}
      {staged.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-border bg-muted-20 px-4 py-2">
          <span className="inline-flex items-center gap-1 text-xs font-medium text-primary">
            <Check className="h-3.5 w-3.5" />
            {t('agents:editorV2.picker.selectedCount', { count: staged.length })}
          </span>
          <div className="mx-1 h-3.5 w-px bg-border" />
          <div className="flex min-w-0 flex-1 flex-wrap gap-1.5">
            {staged.map((req, index) => {
              const info = describeSource(req.source, devices, extensions, agents)
              return (
                <span
                  key={`${req.key}-${index}`}
                  className="inline-flex max-w-[200px] items-center gap-1 rounded-md border border-border bg-card px-2 py-0.5 text-xs text-foreground"
                >
                  <span className="truncate">{info.label}</span>
                  <button
                    type="button"
                    onClick={() => setStaged((prev) => prev.filter((_, i) => i !== index))}
                    className="ml-0.5 shrink-0 text-muted-foreground hover:text-error"
                    aria-label={t('agents:editorV2.input.remove')}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              )
            })}
          </div>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            onClick={() => setStaged([])}
            className="shrink-0 text-muted-foreground hover:text-error"
          >
            {t('common:clear')}
          </Button>
        </div>
      )}

      {/* Underline Category Tabs (matches UnifiedDataSourceConfig / DESIGN_SPEC) */}
      <div className="flex min-w-0 shrink-0 overflow-x-auto border-b border-border px-2 scrollbar-none">
        {tabs.map(({ id, Icon, label, count, selected }) => {
          const active = tab === id
          return (
            <button
              key={id}
              type="button"
              onClick={() => {
                setTab(id)
                setMobileDetailOpen(false)
              }}
              className={cn(
                'relative flex shrink-0 items-center gap-1.5 px-3 py-2.5 text-sm font-medium transition-colors',
                'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:transition-colors',
                active
                  ? 'text-primary after:bg-primary'
                  : 'text-muted-foreground hover:bg-muted-50 hover:text-foreground',
              )}
            >
              <Icon className="h-4 w-4 shrink-0" />
              <span className="whitespace-nowrap">{label}</span>
              <span className="text-xs opacity-65">({count})</span>
              {selected > 0 && (
                <Badge variant="secondary" className="h-4 px-1 text-nano">
                  {selected}
                </Badge>
              )}
            </button>
          )
        })}
      </div>

      {/* Master-Detail Split Body */}
      {isMobile ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          {mobileDetailOpen ? detailPane : masterPane}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 overflow-hidden">
          <div className="w-60 shrink-0 overflow-hidden border-r border-border sm:w-64">
            {masterPane}
          </div>
          <div className="min-w-0 flex-1 overflow-hidden">{detailPane}</div>
        </div>
      )}
    </UnifiedFormDialog>
  )
}
