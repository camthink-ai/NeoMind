/**
 * Seed drafts for the agent editor — empty create canvas + spec projection.
 * Task templates live in templates.ts (user mental model).
 *
 * Edit load uses GET /agents/:id → specToDraft, preserving input.requirements,
 * controlled_actions, and output.depends_on cleanly across saves.
 */
import type {
  AgentNotify,
  AgentResource,
  AgentSchedule,
  AgentSpec,
  AgentSpecDraft,
  AgentSpecPatch,
  AgentToolConfig,
  InputContract,
  InputRequirement,
  NotifyPolicy,
  OperatorField,
  OutputContract,
  ToolPolicy,
  TriggerSpec,
} from '@/types'
import { normalizeNotify } from './NotifyPolicySection'

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
    tool_policy: {
      scoped: {
        allowed_tools: ['web_search', 'web_fetch'],
        allowed_commands: [],
      },
    },
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

/** Normalize a possibly missing, partial, or legacy schedule into a valid TriggerSpec. */
export function normalizeTrigger(
  trigger?: TriggerSpec | null,
  legacySchedule?: AgentSchedule | null,
): TriggerSpec {
  if (trigger && typeof trigger === 'object' && 'kind' in trigger) {
    switch (trigger.kind) {
      case 'cron':
        return {
          kind: 'cron',
          expr: typeof trigger.expr === 'string' && trigger.expr.trim() ? trigger.expr : '0 9 * * *',
          timezone:
            typeof trigger.timezone === 'string' && trigger.timezone.trim()
              ? trigger.timezone
              : 'Asia/Shanghai',
        }
      case 'interval':
        return {
          kind: 'interval',
          every_secs:
            typeof trigger.every_secs === 'number' && Number.isFinite(trigger.every_secs) && trigger.every_secs > 0
              ? trigger.every_secs
              : 300,
        }
      case 'event':
        return {
          kind: 'event',
          filter: trigger.filter ?? {},
        }
      case 'manual':
        return { kind: 'manual' }
    }
  }
  if (legacySchedule && typeof legacySchedule === 'object') {
    switch (legacySchedule.schedule_type) {
      case 'cron':
        return {
          kind: 'cron',
          expr: legacySchedule.cron_expression?.trim() || '0 9 * * *',
          timezone: legacySchedule.timezone?.trim() || 'Asia/Shanghai',
        }
      case 'interval':
        return {
          kind: 'interval',
          every_secs:
            typeof legacySchedule.interval_seconds === 'number' && legacySchedule.interval_seconds > 0
              ? legacySchedule.interval_seconds
              : 300,
        }
      case 'event': {
        let filter: unknown = {}
        if (legacySchedule.event_filter) {
          try {
            filter = JSON.parse(legacySchedule.event_filter)
          } catch {
            filter = legacySchedule.event_filter
          }
        }
        return { kind: 'event', filter }
      }
      case 'manual':
        return { kind: 'manual' }
    }
  }
  return { kind: 'manual' }
}

/** Normalize a possibly missing or legacy tool policy. */
export function normalizeToolPolicy(
  policy?: ToolPolicy | null,
  legacyToolConfig?: AgentToolConfig | null,
): ToolPolicy {
  if (policy === 'disabled' || policy === 'unrestricted') return policy
  if (policy && typeof policy === 'object' && 'scoped' in policy && policy.scoped) {
    return {
      scoped: {
        allowed_tools: Array.isArray(policy.scoped.allowed_tools)
          ? [...policy.scoped.allowed_tools]
          : [],
        allowed_commands: Array.isArray(policy.scoped.allowed_commands)
          ? [...policy.scoped.allowed_commands]
          : [],
      },
    }
  }
  if (legacyToolConfig && typeof legacyToolConfig === 'object') {
    if (!legacyToolConfig.enabled) return 'disabled'
    if (Array.isArray(legacyToolConfig.allowed_tools) && legacyToolConfig.allowed_tools.length > 0) {
      return {
        scoped: {
          allowed_tools: [...legacyToolConfig.allowed_tools],
          allowed_commands: [],
        },
      }
    }
    return 'unrestricted'
  }
  return 'disabled'
}

function legacyResourcesToInput(resources?: AgentResource[] | null): InputContract {
  if (!Array.isArray(resources) || resources.length === 0) {
    return { requirements: [] }
  }
  const requirements: InputRequirement[] = []
  resources.forEach((r, idx) => {
    if (!r || typeof r.resource_id !== 'string') return
    if (r.resource_type === 'command') return
    const key = `res_${idx + 1}_${r.resource_id}`.replace(/[^a-zA-Z0-9_]/g, '_')
    if (r.resource_type === 'metric' && r.resource_id.includes(':')) {
      const [deviceId, field] = r.resource_id.split(':', 2)
      requirements.push({
        key,
        source: { kind: 'device_metric', device_id: deviceId, field },
        timeout_secs: 10,
        result_type: 'metric',
        freshness: { kind: 'any' },
        on_missing: 'report_stale',
      })
    } else if (r.resource_type === 'extension_tool' || r.resource_type === 'extension_metric') {
      const id = r.resource_id.replace(/^extension:/, '').split(':')[0] || r.resource_id
      requirements.push({
        key,
        source: { kind: 'extension_instance', id },
        timeout_secs: 10,
        result_type: 'structured_json',
        freshness: { kind: 'any' },
        on_missing: 'report_stale',
      })
    } else {
      requirements.push({
        key,
        source: { kind: 'whole_device', device_id: r.resource_id },
        timeout_secs: 10,
        result_type: 'structured_json',
        freshness: { kind: 'any' },
        on_missing: 'report_stale',
      })
    }
  })
  return { requirements }
}

function normalizeOutput(
  output?: OutputContract | null,
  legacySchema?: OperatorField[] | null,
): OutputContract {
  if (output && typeof output === 'object' && Array.isArray(output.fields)) {
    return {
      fields: output.fields.map((f) => ({
        name: typeof f?.name === 'string' ? f.name : '',
        description: f?.description,
        depends_on: Array.isArray(f?.depends_on) ? [...f.depends_on] : [],
      })),
      publish: Boolean(output.publish),
    }
  }
  if (Array.isArray(legacySchema) && legacySchema.length > 0) {
    return {
      fields: legacySchema.map((f) => ({
        name: f.name,
        description: f.description,
        depends_on: [],
      })),
      publish: true,
    }
  }
  return { fields: [], publish: false }
}

type SpecLike = Partial<AgentSpec> & {
  agent?: Partial<AgentSpec>
  schedule?: AgentSchedule
  resources?: AgentResource[]
  output_schema?: OperatorField[]
  tool_config?: AgentToolConfig
  notify?: NotifyPolicy | AgentNotify
}

/**
 * Project a loaded AgentSpec (from GET /agents/:id) into a clean AgentSpecDraft.
 * Explicitly picks only draft fields so runtime fields (status, stats,
 * effective_plan, etc.) never leak into the edit state, and defensively
 * normalizes missing or legacy-shaped fields so editing an old agent never crashes.
 */
export function specToDraft(raw: AgentSpec | SpecLike): AgentSpecDraft {
  const spec: SpecLike =
    raw && typeof raw === 'object' && 'agent' in raw && raw.agent && typeof raw.agent === 'object'
      ? (raw.agent as SpecLike)
      : ((raw ?? {}) as SpecLike)

  const input: InputContract =
    spec.input && Array.isArray(spec.input.requirements)
      ? { requirements: spec.input.requirements }
      : legacyResourcesToInput(spec.resources)

  return {
    name: typeof spec.name === 'string' ? spec.name : '',
    description: typeof spec.description === 'string' ? spec.description : '',
    user_prompt: typeof spec.user_prompt === 'string' ? spec.user_prompt : '',
    llm_backend_id: spec.llm_backend_id ?? undefined,
    preset: spec.preset ?? undefined,
    trigger: normalizeTrigger(spec.trigger, spec.schedule),
    pacing: spec.pacing ?? { failure_threshold: 3 },
    input,
    tool_policy: normalizeToolPolicy(spec.tool_policy, spec.tool_config),
    output: normalizeOutput(spec.output, spec.output_schema),
    notify: normalizeNotify(spec.notify),
    controlled_actions: Array.isArray(spec.controlled_actions) ? spec.controlled_actions : [],
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
