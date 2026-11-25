/**
 * Seed drafts for the v2 editor — empty create canvas + legacy projection.
 * Task templates live in templates.ts (user mental model).
 */
import type { AgentSpecDraft, AiAgentDetail, NotifyPolicy } from '@/types'
import {
  scheduleToTrigger,
  toolConfigToPolicy,
  legacyNotifyToPolicy,
} from '@/lib/api/agentV2Adapter'

const QUIET_NOTIFY: NotifyPolicy = {
  channels: [],
  on_complete: false,
  on_alert: false,
  alert_depends_on: [],
  on_failure: true,
  pending_confirmation: false,
}

/** Blank canvas before the operator picks a task template. */
export function emptyDraft(): AgentSpecDraft {
  return {
    name: '',
    description: '',
    user_prompt: '',
    trigger: { kind: 'manual' },
    pacing: { failure_threshold: 3 },
    input: { requirements: [] },
    tool_policy: 'disabled',
    output: { fields: [], publish: false },
    notify: { ...QUIET_NOTIFY },
    controlled_actions: [],
  }
}

/**
 * Filled daily-brief fixture for tests. Create UX uses templates.ts instead.
 */
export function defaultDailyBriefDraft(): AgentSpecDraft {
  return {
    name: 'daily-brief',
    description: 'Zero-device weather + tariff daily brief',
    user_prompt:
      "Summarize today's weather and time-of-use electricity price for the operator.",
    preset: 'daily_brief',
    trigger: {
      kind: 'cron',
      expr: '0 9 * * *',
      timezone: 'Asia/Shanghai',
    },
    pacing: {
      failure_threshold: 3,
    },
    input: {
      requirements: [
        {
          key: 'weather_now',
          source: { kind: 'extension_instance', id: 'weather_cn' },
          operation: 'current',
          params: { city: 'Shanghai' },
          auth_scope: 'extension:weather:read',
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'latest_within', max_age_secs: 3600 },
          on_missing: 'report_stale',
        },
        {
          key: 'tariff_today',
          source: { kind: 'extension_instance', id: 'tariff_tou' },
          operation: 'tou_today',
          params: { region: 'CN-SH' },
          auth_scope: 'extension:tariff:read',
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'any' },
          on_missing: 'report_stale',
        },
      ],
    },
    tool_policy: 'disabled',
    output: {
      fields: [
        {
          name: 'brief',
          depends_on: ['weather_now', 'tariff_today'],
          description: 'Human-readable daily brief',
        },
      ],
      publish: false,
    },
    notify: {
      channels: ['test-channel'],
      on_complete: true,
      on_alert: false,
      alert_depends_on: [],
      on_failure: true,
      pending_confirmation: false,
    },
    controlled_actions: [],
  }
}

/** Best-effort projection of a legacy detail into a v2 draft for the new editor. */
export function detailToV2Draft(detail: AiAgentDetail): AgentSpecDraft {
  return {
    name: detail.name,
    description: detail.description,
    user_prompt: detail.user_prompt,
    llm_backend_id: detail.llm_backend_id,
    trigger: scheduleToTrigger(detail.schedule),
    input: { requirements: [] },
    tool_policy: toolConfigToPolicy(detail.tool_config),
    notify: legacyNotifyToPolicy(detail.notify),
    output: detail.output_schema?.length
      ? {
          fields: detail.output_schema.map((f) => ({
            name: f.name,
            depends_on: [],
            description: f.description,
          })),
          publish: false,
        }
      : { fields: [], publish: false },
    controlled_actions: [],
  }
}
