/**
 * Seed drafts for the v2 editor — empty create canvas + V2 spec projection.
 * Task templates live in templates.ts (user mental model).
 *
 * Edit load MUST use GET /agents/v2/:id → specToDraft. Never seed save from
 * detailToV2Draft (legacy detail lacks requirements / controlled_actions /
 * output.depends_on and would wipe them on PUT).
 */
import type {
  AgentSpec,
  AgentSpecDraft,
  AgentSpecPatch,
  AiAgentDetail,
  NotifyPolicy,
} from '@/types'
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

/**
 * Strip persistence metadata so a loaded AgentSpec seeds the editor draft.
 * Preserves input.requirements, output deps, controlled_actions, notify, …
 */
export function specToDraft(spec: AgentSpec): AgentSpecDraft {
  const {
    id: _id,
    config_version: _configVersion,
    created_at: _createdAt,
    updated_at: _updatedAt,
    ...draft
  } = spec
  return draft
}

/**
 * Full-contract PUT body matching AgentsPage save (omit id / config_version).
 * Callers that loaded via specToDraft can rename-only without wiping V2 fields.
 */
export function draftToUpdatePatch(draft: AgentSpecDraft): AgentSpecPatch {
  const {
    name,
    description,
    user_prompt,
    llm_backend_id,
    preset,
    trigger,
    pacing,
    input,
    tool_policy,
    output,
    notify,
    controlled_actions,
  } = draft
  return {
    name,
    description,
    user_prompt,
    llm_backend_id,
    preset,
    trigger,
    pacing,
    input,
    tool_policy,
    output,
    notify,
    controlled_actions,
  }
}

/**
 * @deprecated Legacy detail → draft. Empties requirements / controlled_actions
 * and clears output.depends_on. Do not use for edit→save; use getAgentV2 + specToDraft.
 */
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
