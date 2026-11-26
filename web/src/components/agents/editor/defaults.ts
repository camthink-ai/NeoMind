/**
 * Seed drafts for the agent editor — empty create canvas + spec projection.
 * Task templates live in templates.ts (user mental model).
 *
 * Edit load uses GET /agents/:id → specToDraft, preserving input.requirements,
 * controlled_actions, and output.depends_on cleanly across saves.
 */
import type {
  AgentSpec,
  AgentSpecDraft,
  AgentSpecPatch,
  NotifyPolicy,
} from '@/types'

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
 * Project a loaded AgentSpec (from GET /agents/:id) into a clean AgentSpecDraft.
 * Explicitly picks only draft fields so runtime fields (status, stats,
 * effective_plan, etc.) never leak into the edit state.
 */
export function specToDraft(spec: AgentSpec): AgentSpecDraft {
  return {
    name: spec.name,
    description: spec.description,
    user_prompt: spec.user_prompt,
    llm_backend_id: spec.llm_backend_id,
    preset: spec.preset,
    trigger: spec.trigger,
    pacing: spec.pacing,
    input: spec.input ?? { requirements: [] },
    tool_policy: spec.tool_policy,
    output: spec.output,
    notify: spec.notify,
    controlled_actions: spec.controlled_actions ?? [],
  }
}

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

/**
 * AgentSpecPatch for PUT /agents/:id (omit id / config_version).
 * With `previous` (loaded seed), only changed fields are sent — rename-only
 * becomes `{ name }` so a full old snapshot cannot clobber the contract.
 * Without `previous`, sends the full draft.
 */
export function draftToUpdatePatch(
  draft: AgentSpecDraft,
  previous?: AgentSpecDraft,
): AgentSpecPatch {
  if (!previous) {
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

  const patch: AgentSpecPatch = {}
  if (draft.name !== previous.name) patch.name = draft.name
  if (draft.description !== previous.description) {
    patch.description = draft.description ?? null
  }
  if (draft.user_prompt !== previous.user_prompt) patch.user_prompt = draft.user_prompt
  if (draft.llm_backend_id !== previous.llm_backend_id) {
    patch.llm_backend_id = draft.llm_backend_id ?? null
  }
  if (draft.preset !== previous.preset) patch.preset = draft.preset ?? null
  if (!sameJson(draft.trigger, previous.trigger)) patch.trigger = draft.trigger
  if (!sameJson(draft.pacing, previous.pacing)) patch.pacing = draft.pacing
  if (!sameJson(draft.input, previous.input)) patch.input = draft.input
  if (!sameJson(draft.tool_policy, previous.tool_policy)) {
    patch.tool_policy = draft.tool_policy
  }
  if (!sameJson(draft.output, previous.output)) patch.output = draft.output
  if (!sameJson(draft.notify, previous.notify)) patch.notify = draft.notify
  if (!sameJson(draft.controlled_actions, previous.controlled_actions)) {
    patch.controlled_actions = draft.controlled_actions
  }
  return patch
}
