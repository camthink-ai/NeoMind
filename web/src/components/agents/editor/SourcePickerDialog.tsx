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
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bot,
  Check,
  Cpu,
  Plus,
  Puzzle,
  Search,
  Trash2,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
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
// Sibling Dialog: Searchable, grouped source picker (Devices+Metrics / Extensions / Upstream AI)
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
  const { t } = useTranslation('agents')
  const [tab, setTab] = useState<'devices' | 'extensions' | 'ai'>('devices')
  const [query, setQuery] = useState('')
  const requirements = value?.requirements ?? []

  const deviceTypeMap = useMemo(
    () => new Map(deviceTypes.map((dt) => [dt.device_type, dt])),
    [deviceTypes],
  )

  const q = query.trim().toLowerCase()

  const filteredDevices = useMemo(
    () =>
      devices.filter((d) => {
        if (!q) return true
        const id = (d.device_id || d.id || '').toLowerCase()
        const name = (d.name || '').toLowerCase()
        const type = (d.device_type || '').toLowerCase()
        const dt = deviceTypeMap.get(d.device_type)
        const metricMatch = dt?.metrics?.some(
          (m) =>
            m.name.toLowerCase().includes(q) ||
            (m.display_name || '').toLowerCase().includes(q),
        )
        return id.includes(q) || name.includes(q) || type.includes(q) || Boolean(metricMatch)
      }),
    [devices, deviceTypeMap, q],
  )

  const filteredExtensions = useMemo(
    () =>
      extensions.filter((e) => {
        if (!q) return true
        return (
          e.id.toLowerCase().includes(q) ||
          (e.name || '').toLowerCase().includes(q)
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

  const hasWholeDevice = (deviceId: string) =>
    requirements.some(
      (r) => r.source.kind === 'whole_device' && r.source.device_id === deviceId,
    )

  const hasDeviceMetric = (deviceId: string, field: string) =>
    requirements.some(
      (r) =>
        r.source.kind === 'device_metric' &&
        r.source.device_id === deviceId &&
        r.source.field === field,
    )

  const hasExtension = (extId: string) =>
    requirements.some(
      (r) => r.source.kind === 'extension_instance' && r.source.id === extId,
    )

  const hasUpstreamAi = (agentId: string, field: string) =>
    requirements.some(
      (r) =>
        r.source.kind === 'upstream_ai' &&
        r.source.agent_id === agentId &&
        r.source.field === field,
    )

  const toggleWholeDevice = (device: Device) => {
    const id = device.device_id || device.id
    if (hasWholeDevice(id)) {
      onChange({
        requirements: requirements.filter(
          (r) => !(r.source.kind === 'whole_device' && r.source.device_id === id),
        ),
      })
      return
    }
    onChange({
      requirements: [
        ...requirements,
        {
          key: uniqueKey(`device_${id}`, requirements),
          source: { kind: 'whole_device', device_id: id },
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'latest_within', max_age_secs: 3600 },
          on_missing: 'report_stale',
        },
      ],
    })
  }

  const toggleDeviceMetric = (device: Device, field: string) => {
    const id = device.device_id || device.id
    if (hasDeviceMetric(id, field)) {
      onChange({
        requirements: requirements.filter(
          (r) =>
            !(
              r.source.kind === 'device_metric' &&
              r.source.device_id === id &&
              r.source.field === field
            ),
        ),
      })
      return
    }
    onChange({
      requirements: [
        ...requirements,
        {
          key: uniqueKey(`${id}_${field}`, requirements),
          source: { kind: 'device_metric', device_id: id, field },
          timeout_secs: 10,
          result_type: 'metric',
          freshness: { kind: 'latest_within', max_age_secs: 3600 },
          on_missing: 'report_stale',
        },
      ],
    })
  }

  const toggleExtension = (ext: Extension) => {
    if (hasExtension(ext.id)) {
      onChange({
        requirements: requirements.filter(
          (r) => !(r.source.kind === 'extension_instance' && r.source.id === ext.id),
        ),
      })
      return
    }
    const op = ext.commands?.[0]?.id
    onChange({
      requirements: [
        ...requirements,
        {
          key: uniqueKey(`ext_${ext.id}`, requirements),
          source: { kind: 'extension_instance', id: ext.id },
          operation: op || undefined,
          auth_scope: `extension:${ext.id}:read`,
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'latest_within', max_age_secs: 3600 },
          on_missing: 'report_stale',
        },
      ],
    })
  }

  const toggleUpstreamAi = (agent: AiAgent, field: string) => {
    if (hasUpstreamAi(agent.id, field)) {
      onChange({
        requirements: requirements.filter(
          (r) =>
            !(
              r.source.kind === 'upstream_ai' &&
              r.source.agent_id === agent.id &&
              r.source.field === field
            ),
        ),
      })
      return
    }
    onChange({
      requirements: [
        ...requirements,
        {
          key: uniqueKey(`ai_${field}`, requirements),
          source: { kind: 'upstream_ai', agent_id: agent.id, field },
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'latest_within', max_age_secs: 3600 },
          on_missing: 'report_stale',
        },
      ],
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col gap-3">
        <DialogHeader>
          <DialogTitle>{t('editorV2.picker.title')}</DialogTitle>
        </DialogHeader>

        {/* Search + group tabs */}
        <div className="space-y-2.5">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('editorV2.picker.searchPlaceholder')}
              className="pl-9"
            />
          </div>
          <div className="flex gap-1.5">
            {([
              { id: 'devices', Icon: Cpu, label: t('editorV2.picker.tabDevices'), count: filteredDevices.length },
              { id: 'extensions', Icon: Puzzle, label: t('editorV2.picker.tabExtensions'), count: filteredExtensions.length },
              { id: 'ai', Icon: Bot, label: t('editorV2.picker.tabAi'), count: aiAgentsWithFields.length },
            ] as const).map(({ id, Icon, label, count }) => (
              <button
                key={id}
                type="button"
                onClick={() => setTab(id)}
                className={cn(
                  'inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors',
                  tab === id
                    ? 'border-primary bg-primary-light text-primary'
                    : 'border-border bg-card text-muted-foreground hover:text-foreground',
                )}
              >
                <Icon className="h-3.5 w-3.5" />
                {label}
                <span className="opacity-70">({count})</span>
              </button>
            ))}
          </div>
        </div>

        {/* Scrollable catalog */}
        <div className="min-h-0 flex-1 overflow-y-auto space-y-2 pr-1">
          {tab === 'devices' && (
            filteredDevices.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted-foreground">
                {t('editorV2.picker.noDevices')}
              </p>
            ) : (
              filteredDevices.map((d) => {
                const id = d.device_id || d.id
                const dt = deviceTypeMap.get(d.device_type)
                const metrics = dt?.metrics ?? []
                const wholeOn = hasWholeDevice(id)
                return (
                  <div key={id} className="rounded-lg border border-border bg-card p-3 space-y-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span
                            className={cn(
                              'h-2 w-2 shrink-0 rounded-full',
                              d.online ? 'bg-success' : 'bg-muted-foreground/40',
                            )}
                          />
                          <span className="truncate text-sm font-medium text-foreground">
                            {d.name || id}
                          </span>
                          <span className="truncate font-mono text-xs text-muted-foreground">
                            {id}
                          </span>
                        </div>
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        variant={wholeOn ? 'default' : 'outline'}
                        onClick={() => toggleWholeDevice(d)}
                        className="h-7 shrink-0 text-xs"
                      >
                        {wholeOn && <Check className="mr-1 h-3 w-3" />}
                        {wholeOn ? t('editorV2.picker.wholeDeviceAdded') : t('editorV2.picker.wholeDevice')}
                      </Button>
                    </div>
                    {metrics.length > 0 && (
                      <div className="space-y-1">
                        <div className="text-xs text-muted-foreground">
                          {t('editorV2.picker.metrics')}
                        </div>
                        <div className="flex flex-wrap gap-1.5">
                          {metrics.map((m) => {
                            const on = hasDeviceMetric(id, m.name)
                            return (
                              <button
                                key={m.name}
                                type="button"
                                onClick={() => toggleDeviceMetric(d, m.name)}
                                className={cn(
                                  'inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs transition-colors',
                                  on
                                    ? 'border-primary bg-primary-light text-primary font-medium'
                                    : 'border-border bg-muted-20 text-muted-foreground hover:text-foreground',
                                )}
                              >
                                {on && <Check className="h-3 w-3" />}
                                <span>{m.display_name || m.name}</span>
                                <span className="font-mono opacity-70">{m.name}</span>
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    )}
                  </div>
                )
              })
            )
          )}

          {tab === 'extensions' && (
            filteredExtensions.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted-foreground">
                {t('editorV2.picker.noExtensions')}
              </p>
            ) : (
              filteredExtensions.map((ext) => {
                const on = hasExtension(ext.id)
                return (
                  <div
                    key={ext.id}
                    className="flex items-center justify-between gap-3 rounded-lg border border-border bg-card p-3"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-foreground">
                        {ext.name || ext.id}
                      </div>
                      <div className="truncate font-mono text-xs text-muted-foreground">
                        {ext.id}
                        {ext.commands && ext.commands.length > 0
                          ? ` · ${ext.commands.map((c) => c.id).join(', ')}`
                          : ''}
                      </div>
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant={on ? 'default' : 'outline'}
                      onClick={() => toggleExtension(ext)}
                      className="h-7 shrink-0 text-xs"
                    >
                      {on && <Check className="mr-1 h-3 w-3" />}
                      {on ? t('editorV2.picker.selected') : t('editorV2.output.add')}
                    </Button>
                  </div>
                )
              })
            )
          )}

          {tab === 'ai' && (
            aiAgentsWithFields.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted-foreground">
                {t('editorV2.picker.noAiSources')}
              </p>
            ) : (
              aiAgentsWithFields.map((ag) => (
                <div key={ag.id} className="rounded-lg border border-border bg-card p-3 space-y-2">
                  <div className="text-sm font-medium text-foreground">{ag.name}</div>
                  <div className="flex flex-wrap gap-1.5">
                    {(ag.output_fields ?? []).map((field) => {
                      const on = hasUpstreamAi(ag.id, field)
                      return (
                        <button
                          key={field}
                          type="button"
                          onClick={() => toggleUpstreamAi(ag, field)}
                          className={cn(
                            'inline-flex items-center gap-1 rounded-md border px-2 py-1 font-mono text-xs transition-colors',
                            on
                              ? 'border-primary bg-primary-light text-primary font-medium'
                              : 'border-border bg-muted-20 text-muted-foreground hover:text-foreground',
                          )}
                        >
                          {on && <Check className="h-3 w-3" />}
                          ai:{ag.id.slice(0, 8)}:{field}
                        </button>
                      )
                    })}
                  </div>
                </div>
              ))
            )
          )}
        </div>

        <DialogFooter>
          <Button type="button" onClick={() => onOpenChange(false)}>
            {t('editorV2.picker.done')} ({requirements.length})
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
