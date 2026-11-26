/**
 * Section 3 of the unified AgentEditor flow: Schedule, Pacing Guardrails & Notifications.
 *
 * Replaces BOTH the duplicated SimpleWhenSection + TriggerSection AND
 * SimpleNotifySection + NotifyPolicySection with one progressive-disclosure section.
 */
import { useState } from 'react'
import { Link, useInRouterContext } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Bell, Calendar, Check, ChevronDown, ChevronRight, Clock, MessageSquare, MousePointerClick, Zap } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import type {
  AlertChannel,
  Device,
  NotifyPolicy,
  PacingPolicy,
  TriggerSpec,
} from '@/types'
import { normalizeTrigger } from './defaults'
import { normalizeNotify } from './NotifyPolicySection'

const INTERVAL_PRESETS_MINS = [1, 5, 15, 30, 60, 360] as const

const TIMEZONE_IDS = [
  'Asia/Shanghai',
  'UTC',
  'Asia/Tokyo',
  'Asia/Seoul',
  'Asia/Singapore',
  'Australia/Sydney',
  'Europe/London',
  'Europe/Paris',
  'Europe/Berlin',
  'America/New_York',
  'America/Los_Angeles',
] as const

type CronUiMode = 'daily' | 'weekly' | 'custom'

interface EventFilterSource {
  type: string
  id: string
  field?: string
}

function parseCronParts(expr: string): {
  mode: CronUiMode
  minute: number
  hour: number
  weekdays: number[]
} {
  const parts = (expr ?? '').trim().split(/\s+/)
  // Support 5-field standard cron (m h dom mon dow)
  if (parts.length === 5) {
    const m = Number(parts[0])
    const h = Number(parts[1])
    const dom = parts[2]
    const mon = parts[3]
    const dow = parts[4]
    if (Number.isInteger(m) && m >= 0 && m <= 59 && Number.isInteger(h) && h >= 0 && h <= 23 && dom === '*' && mon === '*') {
      if (dow === '*') return { mode: 'daily', minute: m, hour: h, weekdays: [1] }
      const days = dow
        .split(',')
        .map((s) => Number(s))
        .filter((n) => Number.isInteger(n) && n >= 0 && n <= 6)
      if (days.length > 0 && days.length === dow.split(',').length) {
        return { mode: 'weekly', minute: m, hour: h, weekdays: days }
      }
    }
  }
  return { mode: 'custom', minute: 0, hour: 9, weekdays: [1] }
}

function extractEventSources(filter: unknown): EventFilterSource[] {
  if (!filter || typeof filter !== 'object') return []
  const obj = filter as { sources?: unknown }
  if (!Array.isArray(obj.sources)) return []
  return obj.sources.filter(
    (s): s is EventFilterSource =>
      Boolean(s) && typeof s === 'object' && typeof (s as EventFilterSource).id === 'string',
  )
}

export interface ScheduleNotifySectionProps {
  trigger: TriggerSpec
  onTriggerChange: (next: TriggerSpec) => void
  pacing: PacingPolicy | undefined
  onPacingChange: (next: PacingPolicy) => void
  notify: NotifyPolicy
  onNotifyChange: (next: NotifyPolicy) => void
  devices: Device[]
  channels: AlertChannel[]
  inputKeys: string[]
}

export function ScheduleNotifySection({
  trigger,
  onTriggerChange,
  pacing,
  onPacingChange,
  notify,
  onNotifyChange,
  devices,
  channels,
  inputKeys,
}: ScheduleNotifySectionProps) {
  const { t } = useTranslation('agents')
  const inRouter = useInRouterContext()
  const safeTrigger = normalizeTrigger(trigger)
  const safeNotify = normalizeNotify(notify)
  const safePacing: PacingPolicy = { failure_threshold: 3, ...pacing }
  const [showRawFilter, setShowRawFilter] = useState(false)
  const [showAlertDeps, setShowAlertDeps] = useState(
    () => (safeNotify.alert_depends_on ?? []).length > 0,
  )

  const cronParsed =
    safeTrigger.kind === 'cron'
      ? parseCronParts(safeTrigger.expr)
      : { mode: 'daily' as CronUiMode, minute: 0, hour: 9, weekdays: [1] }

  const [cronModeOverride, setCronModeOverride] = useState<CronUiMode | null>(null)
  const effectiveCronMode: CronUiMode = cronModeOverride ?? cronParsed.mode

  const setTriggerKind = (kind: TriggerSpec['kind']) => {
    setCronModeOverride(null)
    if (kind === 'interval') {
      onTriggerChange({
        kind: 'interval',
        every_secs: safeTrigger.kind === 'interval' ? safeTrigger.every_secs : 900,
      })
    } else if (kind === 'cron') {
      onTriggerChange({
        kind: 'cron',
        expr: safeTrigger.kind === 'cron' ? safeTrigger.expr : '0 9 * * *',
        timezone: safeTrigger.kind === 'cron' ? safeTrigger.timezone : 'Asia/Shanghai',
      })
    } else if (kind === 'event') {
      onTriggerChange({
        kind: 'event',
        filter: safeTrigger.kind === 'event' ? safeTrigger.filter : {},
      })
    } else {
      onTriggerChange({ kind: 'manual' })
    }
  }

  const updateCronExpr = (mode: CronUiMode, minute: number, hour: number, weekdays: number[]) => {
    const tz = safeTrigger.kind === 'cron' ? safeTrigger.timezone || 'Asia/Shanghai' : 'Asia/Shanghai'
    if (mode === 'daily') {
      onTriggerChange({ kind: 'cron', expr: `${minute} ${hour} * * *`, timezone: tz })
    } else if (mode === 'weekly') {
      const days = weekdays.length > 0 ? [...weekdays].sort((a, b) => a - b) : [1]
      onTriggerChange({ kind: 'cron', expr: `${minute} ${hour} * * ${days.join(',')}`, timezone: tz })
    }
  }

  // Event source toggles
  const eventSources = safeTrigger.kind === 'event' ? extractEventSources(safeTrigger.filter) : []
  const isEventDeviceSelected = (deviceId: string) =>
    eventSources.some((s) => s.type === 'device' && s.id === deviceId)

  const toggleEventDevice = (deviceId: string) => {
    if (safeTrigger.kind !== 'event') return
    const nextSources = isEventDeviceSelected(deviceId)
      ? eventSources.filter((s) => !(s.type === 'device' && s.id === deviceId))
      : [...eventSources.filter((s) => s.id !== 'all'), { type: 'device', id: deviceId }]
    onTriggerChange({
      kind: 'event',
      filter: nextSources.length > 0 ? { sources: nextSources } : {},
    })
  }

  const toggleEventAllDevices = () => {
    if (safeTrigger.kind !== 'event') return
    const isAll = isEventDeviceSelected('all')
    onTriggerChange({
      kind: 'event',
      filter: isAll ? {} : { sources: [{ type: 'device', id: 'all' }] },
    })
  }

  // Notify channel toggles
  const toggleChannel = (name: string) => {
    const exists = safeNotify.channels.includes(name)
    const next = exists
      ? safeNotify.channels.filter((c) => c !== name)
      : [...safeNotify.channels, name]
    onNotifyChange({ ...safeNotify, channels: next })
  }

  const toggleAlertDep = (key: string) => {
    const current = safeNotify.alert_depends_on ?? []
    const next = current.includes(key)
      ? current.filter((k) => k !== key)
      : [...current, key]
    onNotifyChange({ ...safeNotify, alert_depends_on: next })
  }

  const patchPacing = (partial: Partial<PacingPolicy>) => {
    onPacingChange({ ...safePacing, ...partial })
  }

  return (
    <div className="space-y-6">
      {/* 1. Trigger Strategy */}
      <div className="space-y-3" data-section="when">
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {([
            { kind: 'interval', Icon: Clock, label: t('editorV2.trigger.interval') },
            { kind: 'cron', Icon: Calendar, label: t('editorV2.trigger.cron') },
            { kind: 'event', Icon: Zap, label: t('editorV2.trigger.event') },
            { kind: 'manual', Icon: MousePointerClick, label: t('editorV2.trigger.manual') },
          ] as const).map(({ kind, Icon, label }) => (
            <button
              key={kind}
              type="button"
              onClick={() => setTriggerKind(kind)}
              className={cn(
                'flex items-center justify-center gap-1.5 rounded-lg border px-3 py-2 text-xs font-medium transition-colors',
                safeTrigger.kind === kind
                  ? 'border-primary bg-primary-light text-primary'
                  : 'border-border bg-card text-muted-foreground hover:text-foreground',
              )}
            >
              <Icon className="h-3.5 w-3.5 shrink-0" />
              {label}
            </button>
          ))}
        </div>

        {/* Interval config */}
        {safeTrigger.kind === 'interval' && (
          <div className="space-y-3 rounded-lg border border-border bg-muted-20 p-3.5">
            <div className="flex flex-wrap gap-1.5">
              {INTERVAL_PRESETS_MINS.map((mins) => {
                const active = safeTrigger.every_secs === mins * 60
                return (
                  <button
                    key={mins}
                    type="button"
                    onClick={() => onTriggerChange({ kind: 'interval', every_secs: mins * 60 })}
                    className={cn(
                      'rounded-md border px-3 py-1.5 text-xs font-medium transition-colors',
                      active
                        ? 'border-primary bg-primary-light text-primary'
                        : 'border-border bg-card text-muted-foreground hover:text-foreground',
                    )}
                  >
                    {mins >= 60 ? `${mins / 60}h` : `${mins}m`}
                  </button>
                )
              })}
            </div>
            <div className="flex items-center gap-2.5">
              <Label className="text-xs text-muted-foreground">{t('editorV2.when.everyMinutes')}</Label>
              <Input
                type="number"
                min={1}
                value={Math.max(1, Math.round(safeTrigger.every_secs / 60))}
                onChange={(e) =>
                  onTriggerChange({
                    kind: 'interval',
                    every_secs: Math.max(1, Number(e.target.value) || 1) * 60,
                  })
                }
                className="h-9 w-24 text-xs tabular-nums"
              />
            </div>
          </div>
        )}

        {/* Cron config (Daily / Weekly / Custom + Timezone) */}
        {safeTrigger.kind === 'cron' && (
          <div className="space-y-3.5 rounded-lg border border-border bg-muted-20 p-3.5">
            <div className="inline-flex rounded-lg border border-border bg-card p-1 gap-1">
              {([
                { id: 'daily', label: t('editorV2.schedule.cronDaily') },
                { id: 'weekly', label: t('editorV2.schedule.cronWeekly') },
                { id: 'custom', label: t('editorV2.schedule.cronCustom') },
              ] as const).map(({ id, label }) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => {
                    setCronModeOverride(id)
                    if (id !== 'custom') {
                      updateCronExpr(id, cronParsed.minute, cronParsed.hour, cronParsed.weekdays)
                    }
                  }}
                  className={cn(
                    'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                    effectiveCronMode === id
                      ? 'bg-primary-light text-primary'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {label}
                </button>
              ))}
            </div>

            {effectiveCronMode !== 'custom' ? (
              <div className="space-y-3">
                {effectiveCronMode === 'weekly' && (
                  <div className="flex flex-wrap gap-1.5">
                    {[0, 1, 2, 3, 4, 5, 6].map((day) => {
                      const on = cronParsed.weekdays.includes(day)
                      return (
                        <button
                          key={day}
                          type="button"
                          onClick={() => {
                            const nextDays = on
                              ? cronParsed.weekdays.filter((d) => d !== day)
                              : [...cronParsed.weekdays, day]
                            updateCronExpr('weekly', cronParsed.minute, cronParsed.hour, nextDays)
                          }}
                          className={cn(
                            'h-9 min-w-[2.75rem] rounded-md border px-2.5 text-xs font-medium transition-colors',
                            on
                              ? 'border-primary bg-primary-light text-primary'
                              : 'border-border bg-card text-muted-foreground hover:text-foreground',
                          )}
                        >
                          {t(`creator.weekdays.${day}`)}
                        </button>
                      )
                    })}
                  </div>
                )}

                <div className="grid gap-3 sm:grid-cols-3">
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{t('editorV2.when.hour')}</Label>
                    <Select
                      value={String(cronParsed.hour)}
                      onValueChange={(h) =>
                        updateCronExpr(effectiveCronMode, cronParsed.minute, Number(h), cronParsed.weekdays)
                      }
                    >
                      <SelectTrigger className="h-9 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {Array.from({ length: 24 }, (_, i) => (
                          <SelectItem key={i} value={String(i)}>
                            {String(i).padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{t('editorV2.schedule.minute')}</Label>
                    <Select
                      value={String(cronParsed.minute)}
                      onValueChange={(m) =>
                        updateCronExpr(effectiveCronMode, Number(m), cronParsed.hour, cronParsed.weekdays)
                      }
                    >
                      <SelectTrigger className="h-9 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {[0, 5, 10, 15, 20, 30, 45].map((m) => (
                          <SelectItem key={m} value={String(m)}>
                            {String(m).padStart(2, '0')}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1">
                    <Label className="text-xs text-muted-foreground">{t('editorV2.trigger.timezone')}</Label>
                    <Select
                      value={safeTrigger.timezone || 'Asia/Shanghai'}
                      onValueChange={(timezone) =>
                        onTriggerChange({ ...safeTrigger, timezone })
                      }
                    >
                      <SelectTrigger className="h-9 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {TIMEZONE_IDS.map((tz) => (
                          <SelectItem key={tz} value={tz}>
                            {t(`agents:creator.timezones.${tz}`)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </div>
            ) : (
              <div className="grid gap-3 sm:grid-cols-[1fr_14rem]">
                <div className="space-y-1">
                  <Label className="text-xs text-muted-foreground">{t('editorV2.trigger.expr')}</Label>
                  <Input
                    value={safeTrigger.expr}
                    onChange={(e) =>
                      onTriggerChange({ ...safeTrigger, expr: e.target.value })
                    }
                    placeholder="0 9 * * *"
                    className="h-9 font-mono text-xs"
                  />
                  <p className="text-xs text-muted-foreground">{t('editorV2.trigger.exprHint')}</p>
                </div>
                <div className="space-y-1">
                  <Label className="text-xs text-muted-foreground">{t('editorV2.trigger.timezone')}</Label>
                  <Select
                    value={safeTrigger.timezone || 'Asia/Shanghai'}
                    onValueChange={(timezone) =>
                      onTriggerChange({ ...safeTrigger, timezone })
                    }
                  >
                    <SelectTrigger className="h-9 text-xs">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {TIMEZONE_IDS.map((tz) => (
                        <SelectItem key={tz} value={tz}>
                          {t(`agents:creator.timezones.${tz}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            )}
          </div>
        )}

        {/* Event config */}
        {safeTrigger.kind === 'event' && (
          <div className="space-y-2.5 rounded-lg border border-border bg-muted-20 p-3">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-foreground">
                {t('editorV2.schedule.eventSources')}
              </span>
              <button
                type="button"
                onClick={() => setShowRawFilter((v) => !v)}
                className="text-xs text-primary hover:underline"
              >
                {t('editorV2.schedule.eventRawJson')}
              </button>
            </div>
            <div className="flex flex-wrap gap-1.5">
              <button
                type="button"
                onClick={toggleEventAllDevices}
                className={cn(
                  'inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs transition-colors',
                  isEventDeviceSelected('all')
                    ? 'border-primary bg-primary-light text-primary font-medium'
                    : 'border-border bg-card text-muted-foreground hover:text-foreground',
                )}
              >
                {isEventDeviceSelected('all') && <Check className="h-3 w-3" />}
                {t('editorV2.schedule.eventAllDevices')}
              </button>
              {devices.map((d) => {
                const id = d.device_id || d.id
                const on = isEventDeviceSelected(id)
                return (
                  <button
                    key={id}
                    type="button"
                    onClick={() => toggleEventDevice(id)}
                    className={cn(
                      'inline-flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs transition-colors',
                      on
                        ? 'border-primary bg-primary-light text-primary font-medium'
                        : 'border-border bg-card text-muted-foreground hover:text-foreground',
                    )}
                  >
                    {on && <Check className="h-3 w-3" />}
                    {d.name || id}
                  </button>
                )
              })}
            </div>
            {showRawFilter && (
              <Textarea
                rows={2}
                value={
                  typeof safeTrigger.filter === 'string'
                    ? safeTrigger.filter
                    : JSON.stringify(safeTrigger.filter ?? {})
                }
                onChange={(e) => {
                  const raw = e.target.value
                  try {
                    onTriggerChange({ kind: 'event', filter: JSON.parse(raw) })
                  } catch {
                    onTriggerChange({ kind: 'event', filter: raw })
                  }
                }}
                placeholder={t('editorV2.schedule.eventFilterPlaceholder')}
                className="font-mono text-xs"
              />
            )}
          </div>
        )}

        {/* Manual config */}
        {safeTrigger.kind === 'manual' && (
          <p className="text-xs text-muted-foreground">{t('editorV2.when.manualHint')}</p>
        )}
      </div>

      {/* 2. Pacing & Guardrails */}
      <div className="space-y-2.5" data-section="pacing">
        <Label className="text-sm font-medium">{t('editorV2.pacing.title')}</Label>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <div className="space-y-1">
            <Label className="text-xs text-muted-foreground">{t('editorV2.pacing.burstMergeSecs')}</Label>
            <Input
              type="number"
              min={0}
              value={safePacing.burst_merge_secs ?? ''}
              placeholder="0"
              onChange={(e) =>
                patchPacing({
                  burst_merge_secs: e.target.value === '' ? undefined : Math.max(0, Number(e.target.value)),
                })
              }
              className="h-8 text-xs tabular-nums"
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs text-muted-foreground">{t('editorV2.pacing.minIntervalSecs')}</Label>
            <Input
              type="number"
              min={0}
              value={safePacing.min_interval_secs ?? ''}
              placeholder="0"
              onChange={(e) =>
                patchPacing({
                  min_interval_secs: e.target.value === '' ? undefined : Math.max(0, Number(e.target.value)),
                })
              }
              className="h-8 text-xs tabular-nums"
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs text-muted-foreground">{t('editorV2.pacing.maxRunsPerDay')}</Label>
            <Input
              type="number"
              min={1}
              value={safePacing.max_runs_per_day ?? ''}
              placeholder="∞"
              onChange={(e) =>
                patchPacing({
                  max_runs_per_day: e.target.value === '' ? undefined : Math.max(1, Number(e.target.value)),
                })
              }
              className="h-8 text-xs tabular-nums"
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs text-muted-foreground">{t('editorV2.pacing.failureThreshold')}</Label>
            <Input
              type="number"
              min={1}
              value={safePacing.failure_threshold ?? 3}
              onChange={(e) =>
                patchPacing({
                  failure_threshold: Math.max(1, Number(e.target.value) || 3),
                })
              }
              className="h-8 text-xs tabular-nums"
            />
          </div>
        </div>
      </div>

      {/* 3. Three-Way Notification Policy & Channels */}
      <div className="space-y-3" data-section="notify">
        <Label className="text-sm font-medium">{t('editorV2.notify.title')}</Label>
        <div className="grid gap-2 sm:grid-cols-2">
          {([
            {
              id: 'u-notify-complete',
              field: 'on_complete' as const,
              label: t('editorV2.notify.onComplete'),
              desc: t('editorV2.notify.onCompleteDesc'),
            },
            {
              id: 'u-notify-alert',
              field: 'on_alert' as const,
              label: t('editorV2.notify.onAlert'),
              desc: t('editorV2.notify.onAlertDesc'),
            },
            {
              id: 'u-notify-failure',
              field: 'on_failure' as const,
              label: t('editorV2.notify.onFailure'),
              desc: t('editorV2.notify.onFailureDesc'),
            },
            {
              id: 'u-notify-pending',
              field: 'pending_confirmation' as const,
              label: t('editorV2.notify.pendingConfirmation'),
              desc: t('editorV2.notify.pendingConfirmationDesc'),
            },
          ]).map(({ id, field, label, desc }) => (
            <div
              key={id}
              className="flex items-start justify-between gap-2.5 rounded-lg border border-border bg-muted-20 px-3 py-2"
            >
              <div className="min-w-0">
                <Label htmlFor={id} className="text-xs font-medium text-foreground">
                  {label}
                </Label>
                <p className="text-xs text-muted-foreground">{desc}</p>
              </div>
              <Switch
                id={id}
                checked={Boolean(safeNotify[field])}
                onCheckedChange={(checked) =>
                  onNotifyChange({ ...safeNotify, [field]: checked })
                }
              />
            </div>
          ))}
        </div>

        {/* Alert depends_on when on_alert enabled and inputs exist (compact inline under Advanced) */}
        {safeNotify.on_alert && inputKeys.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              onClick={() => setShowAlertDeps((v) => !v)}
              title={t('editorV2.notifyUnified.dependsOnHint')}
              className="inline-flex h-6 items-center gap-1 rounded px-1.5 text-nano text-muted-foreground transition-colors hover:bg-muted-30 hover:text-foreground"
            >
              {showAlertDeps ? (
                <ChevronDown className="h-3 w-3" />
              ) : (
                <ChevronRight className="h-3 w-3" />
              )}
              <span>{t('editorV2.notifyUnified.advancedDependsOn')}</span>
              {(safeNotify.alert_depends_on ?? []).length > 0 && (
                <span className="rounded bg-primary-light px-1 font-mono text-nano text-primary">
                  {(safeNotify.alert_depends_on ?? []).length}
                </span>
              )}
            </button>
            {showAlertDeps &&
              inputKeys.map((k) => {
                const on = (safeNotify.alert_depends_on ?? []).includes(k)
                return (
                  <button
                    key={k}
                    type="button"
                    onClick={() => toggleAlertDep(k)}
                    className={cn(
                      'inline-flex h-6 items-center gap-1 rounded border px-1.5 font-mono text-nano transition-colors',
                      on
                        ? 'border-primary bg-primary-light text-primary font-medium'
                        : 'border-border bg-card text-muted-foreground hover:text-foreground',
                    )}
                  >
                    {on && <Check className="h-2.5 w-2.5" />}
                    {k}
                  </button>
                )
              })}
          </div>
        )}

        {/* Channels: built-in IM + configured message channels */}
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <Label className="text-xs font-medium text-muted-foreground">
              {t('editorV2.notifySimple.channels')}
            </Label>
            {inRouter ? (
              <Link to="/messages/channels" className="text-xs text-primary hover:underline">
                {t('creator.notify.configureChannels')}
              </Link>
            ) : (
              <a href="/messages/channels" className="text-xs text-primary hover:underline">
                {t('creator.notify.configureChannels')}
              </a>
            )}
          </div>
          <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
            {(() => {
              const imOn = safeNotify.channels.includes('IM')
              return (
                <button
                  type="button"
                  onClick={() => toggleChannel('IM')}
                  className={cn(
                    'flex flex-col gap-1.5 rounded-lg border p-3 text-left transition-colors',
                    imOn
                      ? 'border-primary bg-primary-light text-primary'
                      : 'border-border bg-card text-foreground hover:bg-muted-30',
                  )}
                >
                  <div className="flex w-full items-center justify-between gap-2">
                    <span className="flex min-w-0 items-center gap-2 text-xs font-medium">
                      <MessageSquare className="h-3.5 w-3.5 shrink-0 opacity-80" />
                      <span className="truncate">{t('editorV2.notifyUnified.imBuiltin')}</span>
                    </span>
                    <span
                      className={cn(
                        'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                        imOn ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                      )}
                    >
                      <Check className="h-3 w-3" />
                    </span>
                  </div>
                  <p className="line-clamp-1 text-nano text-muted-foreground">
                    {t('editorV2.notifyUnified.imHint')}
                  </p>
                </button>
              )
            })()}
            {channels
              .filter((c) => c.name !== 'IM')
              .map((ch) => {
                const on = safeNotify.channels.includes(ch.name)
                return (
                  <button
                    key={ch.name}
                    type="button"
                    onClick={() => toggleChannel(ch.name)}
                    className={cn(
                      'flex flex-col gap-1.5 rounded-lg border p-3 text-left transition-colors',
                      on
                        ? 'border-primary bg-primary-light text-primary'
                        : 'border-border bg-card text-foreground hover:bg-muted-30',
                      !ch.enabled && 'opacity-50',
                    )}
                  >
                    <div className="flex w-full items-center justify-between gap-2">
                      <span className="flex min-w-0 items-center gap-2 text-xs font-medium">
                        <Bell className="h-3.5 w-3.5 shrink-0 opacity-80" />
                        <span className="truncate">{ch.name}</span>
                      </span>
                      <span
                        className={cn(
                          'flex h-4 w-4 shrink-0 items-center justify-center rounded-sm',
                          on ? 'bg-primary text-primary-foreground' : 'bg-muted text-transparent',
                        )}
                      >
                        <Check className="h-3 w-3" />
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 text-nano text-muted-foreground">
                      <span className="font-mono">{ch.channel_type || 'channel'}</span>
                      {!ch.enabled && <span>· {t('common:disabled', 'Disabled')}</span>}
                    </div>
                  </button>
                )
              })}
          </div>
        </div>
      </div>
    </div>
  )
}
