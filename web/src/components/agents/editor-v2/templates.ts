/**
 * Task-type templates for AgentEditorV2 create — user mental model, not engineer contracts.
 * Prefills trigger + tools=disabled + freshness defaults; sources stay user-picked.
 */
import { Calendar, Radar, ShieldCheck } from 'lucide-react'
import type { ComponentType, CSSProperties } from 'react'
import type {
  AgentPresetId,
  AgentSpecDraft,
  NotifyPolicy,
  TriggerSpec,
} from '@/types'
import { emptyDraft } from './defaults'

type Icon = ComponentType<{ className?: string; style?: CSSProperties }>

export type TaskTemplateId = 'daily_brief' | 'scheduled_patrol' | 'event_review'

export interface TaskTemplate {
  id: TaskTemplateId
  /** Maps to AgentSpecDraft.preset when the backend knows it. */
  preset: AgentPresetId
  icon: Icon
  /** i18n key under editorV2.templates.<id> */
  nameKey: string
  descKey: string
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
    ...partial,
  }
}

/** Daily brief — 09:00 Asia/Shanghai cron, tools off, complete notify. */
export function seedDailyBrief(): AgentSpecDraft {
  return baseSeed({
    name: '',
    description: '',
    user_prompt: '',
    preset: 'daily_brief',
    trigger: { kind: 'cron', expr: '0 9 * * *', timezone: 'Asia/Shanghai' },
    notify: COMPLETE_NOTIFY,
    pacing: { failure_threshold: 3 },
  })
}

/** Scheduled patrol — every hour, tools off, failure notify only. */
export function seedScheduledPatrol(): AgentSpecDraft {
  return baseSeed({
    name: '',
    description: '',
    user_prompt: '',
    preset: 'custom',
    trigger: { kind: 'interval', every_secs: 3600 },
    notify: QUIET_NOTIFY,
    pacing: { failure_threshold: 3 },
  })
}

/** Event review — event trigger, tools off. */
export function seedEventReview(): AgentSpecDraft {
  return baseSeed({
    name: '',
    description: '',
    user_prompt: '',
    preset: 'event_vision',
    trigger: { kind: 'event', filter: {} },
    notify: QUIET_NOTIFY,
    pacing: { failure_threshold: 3 },
  })
}

export const TASK_TEMPLATES: TaskTemplate[] = [
  {
    id: 'daily_brief',
    preset: 'daily_brief',
    icon: Calendar,
    nameKey: 'editorV2.templates.daily_brief.name',
    descKey: 'editorV2.templates.daily_brief.desc',
    seed: seedDailyBrief,
  },
  {
    id: 'scheduled_patrol',
    preset: 'custom',
    icon: Radar,
    nameKey: 'editorV2.templates.scheduled_patrol.name',
    descKey: 'editorV2.templates.scheduled_patrol.desc',
    seed: seedScheduledPatrol,
  },
  {
    id: 'event_review',
    preset: 'event_vision',
    icon: ShieldCheck,
    nameKey: 'editorV2.templates.event_review.name',
    descKey: 'editorV2.templates.event_review.desc',
    seed: seedEventReview,
  },
]

export function templateById(id: TaskTemplateId | null | undefined): TaskTemplate | undefined {
  if (!id) return undefined
  return TASK_TEMPLATES.find((t) => t.id === id)
}

/** Infer which template best matches an existing trigger (edit / re-open). */
export function inferTemplateId(
  trigger: TriggerSpec,
  preset?: AgentPresetId,
): TaskTemplateId | null {
  if (preset === 'daily_brief') return 'daily_brief'
  if (preset === 'event_vision') return 'event_review'
  if (trigger.kind === 'cron') return 'daily_brief'
  if (trigger.kind === 'interval') return 'scheduled_patrol'
  if (trigger.kind === 'event') return 'event_review'
  return null
}
