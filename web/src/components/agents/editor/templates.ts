/**
 * Working V2 presets only — zero-tool + authorized reads + cron/interval/event/manual.
 * Vision / multi-tool autonomy presets are omitted (not shown disabled).
 */
import {
  Activity,
  AlertTriangle,
  Calendar,
  Clock,
  Gauge,
  Radar,
  Search,
  Settings2,
} from 'lucide-react'
import type { ComponentType, CSSProperties } from 'react'
import type {
  AgentPresetId,
  AgentSpecDraft,
  NotifyPolicy,
  OutputContract,
  TriggerSpec,
} from '@/types'
import { emptyDraft } from './defaults'

type Icon = ComponentType<{ className?: string; style?: CSSProperties }>

export type TaskTemplateId =
  | 'daily_brief'
  | 'meter_inspect'
  | 'event_review'
  | 'weekly'
  | 'shift'
  | 'patrol'
  | 'qa'
  | 'custom'

export interface TaskTemplate {
  id: TaskTemplateId
  preset: AgentPresetId
  icon: Icon
  nameKey: string
  seed: () => AgentSpecDraft
}

const QUIET_NOTIFY: NotifyPolicy = {
  channels: [],
  on_complete: false,
  on_alert: false,
  alert_depends_on: [],
  on_failure: true,
  pending_confirmation: false,
}

const COMPLETE_NOTIFY: NotifyPolicy = {
  ...QUIET_NOTIFY,
  on_complete: true,
}

function baseSeed(
  partial: Partial<AgentSpecDraft> &
    Pick<AgentSpecDraft, 'trigger' | 'preset' | 'name' | 'user_prompt'>,
): AgentSpecDraft {
  return {
    ...emptyDraft(),
    tool_policy: 'disabled',
    notify: QUIET_NOTIFY,
    output: { fields: [], publish: false },
    ...partial,
  }
}

function fields(...items: { name: string; description: string }[]): OutputContract {
  return {
    fields: items.map((f) => ({ name: f.name, description: f.description, depends_on: [] })),
    publish: false,
  }
}

export function seedDailyBrief(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'daily_brief',
    trigger: { kind: 'cron', expr: '0 9 * * *', timezone: 'Asia/Shanghai' },
    notify: COMPLETE_NOTIFY,
    output: fields({ name: 'brief', description: 'Human-readable daily brief' }),
  })
}

/** Meter / multi-source inspect — maps to partial_meter when backend knows it. */
export function seedMeterInspect(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'partial_meter',
    trigger: { kind: 'interval', every_secs: 15 * 60 },
    output: fields(
      { name: 'status', description: 'Overall status' },
      { name: 'anomaly_count', description: 'Number of anomalies detected' },
    ),
  })
}

/** @deprecated alias — prefer seedMeterInspect / seedPatrol */
export function seedScheduledPatrol(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'interval', every_secs: 3600 },
  })
}

export function seedEventReview(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'event_vision',
    trigger: { kind: 'event', filter: {} },
    output: fields(
      { name: 'status', description: 'Event verdict' },
      { name: 'detail', description: 'Short explanation' },
    ),
  })
}

export function seedWeekly(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'cron', expr: '0 9 * * 1', timezone: 'Asia/Shanghai' },
    notify: COMPLETE_NOTIFY,
    output: fields({ name: 'report', description: 'Weekly summary' }),
  })
}

export function seedShift(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'interval', every_secs: 30 * 60 },
    output: fields({ name: 'delta', description: 'What changed since last look' }),
  })
}

export function seedPatrol(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'interval', every_secs: 30 * 60 },
    output: fields(
      { name: 'status', description: 'Patrol status' },
      { name: 'findings', description: 'Notable findings' },
    ),
  })
}

export function seedQa(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'manual' },
  })
}

export function seedCustom(): AgentSpecDraft {
  return baseSeed({
    name: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'manual' },
  })
}

export const TASK_TEMPLATES: TaskTemplate[] = [
  {
    id: 'daily_brief',
    preset: 'daily_brief',
    icon: Calendar,
    nameKey: 'editorV2.templates.daily_brief.name',
    seed: seedDailyBrief,
  },
  {
    id: 'meter_inspect',
    preset: 'partial_meter',
    icon: Activity,
    nameKey: 'editorV2.templates.meter_inspect.name',
    seed: seedMeterInspect,
  },
  {
    id: 'event_review',
    preset: 'event_vision',
    icon: AlertTriangle,
    nameKey: 'editorV2.templates.event_review.name',
    seed: seedEventReview,
  },
  {
    id: 'weekly',
    preset: 'custom',
    icon: Clock,
    nameKey: 'editorV2.templates.weekly.name',
    seed: seedWeekly,
  },
  {
    id: 'shift',
    preset: 'custom',
    icon: Gauge,
    nameKey: 'editorV2.templates.shift.name',
    seed: seedShift,
  },
  {
    id: 'patrol',
    preset: 'custom',
    icon: Radar,
    nameKey: 'editorV2.templates.patrol.name',
    seed: seedPatrol,
  },
  {
    id: 'qa',
    preset: 'custom',
    icon: Search,
    nameKey: 'editorV2.templates.qa.name',
    seed: seedQa,
  },
  {
    id: 'custom',
    preset: 'custom',
    icon: Settings2,
    nameKey: 'editorV2.templates.custom.name',
    seed: seedCustom,
  },
]

export function templateById(id: TaskTemplateId | null | undefined): TaskTemplate | undefined {
  if (!id) return undefined
  return TASK_TEMPLATES.find((t) => t.id === id)
}

export function inferTemplateId(
  trigger?: TriggerSpec | null,
  preset?: AgentPresetId,
): TaskTemplateId | null {
  if (preset === 'daily_brief') return 'daily_brief'
  if (preset === 'partial_meter') return 'meter_inspect'
  if (preset === 'event_vision') return 'event_review'
  if (!trigger || typeof trigger !== 'object' || !('kind' in trigger)) return 'custom'
  if (trigger.kind === 'cron') {
    const parts = (trigger.expr ?? '').trim().split(/\s+/)
    if (parts[4] && parts[4] !== '*') return 'weekly'
    return 'daily_brief'
  }
  if (trigger.kind === 'interval') {
    if (trigger.every_secs <= 20 * 60) return 'meter_inspect'
    if (trigger.every_secs <= 40 * 60) return 'shift'
    return 'patrol'
  }
  if (trigger.kind === 'event') return 'event_review'
  if (trigger.kind === 'manual') return 'qa'
  return 'custom'
}
