/**
 * M2 temporary form ↔ AgentSpec v2 translation layer.
 *
 * Bridges form/editor state against the clean-slate DTO
 * until M3 rewrites the editor. Delete this module (and its re-exports from
 * agents.ts) when that lands.
 *
 * Round-trip invariant: fields the old editor cannot fully express
 * (input.requirements, tool_policy, notify channels/flags, trigger.timezone,
 * controlled_actions, pacing, preset) are stashed in `form.v2Carry` and
 * restored when the form's projected values are unchanged — so a name-only
 * edit does not wipe them.
 */
import type {
  AgentEditorFormState,
  AgentNotify,
  AgentNotifyOn,
  AgentScheduleRequest,
  AgentSpec,
  AgentSpecDraft,
  AgentSpecV2Carry,
  AgentToolConfig,
  InputContract,
  InputRequirement,
  NotifyPolicy,
  ResourceRequest,
  ToolPolicy,
  TriggerSpec,
} from '@/types'

function stableStringify(value: unknown): string {
  return JSON.stringify(value ?? null)
}

function sameJson(a: unknown, b: unknown): boolean {
  return stableStringify(a) === stableStringify(b)
}

/** Accept either a persisted AgentSpec or a create draft (fixture). */
export type AgentSpecLike = AgentSpecDraft | AgentSpec

function asCarry(spec: AgentSpecLike): AgentSpecV2Carry {
  return {
    preset: spec.preset,
    trigger: spec.trigger,
    pacing: spec.pacing,
    input: spec.input ?? { requirements: [] },
    tool_policy: spec.tool_policy,
    output: spec.output,
    notify: spec.notify,
    controlled_actions: spec.controlled_actions,
  }
}

// ---------------------------------------------------------------------------
// Trigger ↔ schedule
// ---------------------------------------------------------------------------

export function triggerToSchedule(trigger: TriggerSpec): AgentScheduleRequest {
  switch (trigger.kind) {
    case 'cron':
      return {
        schedule_type: 'cron',
        cron_expression: trigger.expr,
        timezone: trigger.timezone,
      }
    case 'interval':
      return {
        schedule_type: 'interval',
        interval_seconds: trigger.every_secs,
      }
    case 'event':
      return {
        schedule_type: 'event',
        event_filter:
          trigger.filter === undefined || trigger.filter === null
            ? undefined
            : typeof trigger.filter === 'string'
              ? trigger.filter
              : JSON.stringify(trigger.filter),
      }
    case 'manual':
      return { schedule_type: 'manual' }
  }
}

/**
 * Build a TriggerSpec from the legacy schedule. When `previous` is a cron
 * trigger and the form omits timezone, keep the previous timezone so a
 * partial form save cannot silently drop Asia/Shanghai.
 */
export function scheduleToTrigger(
  schedule: AgentScheduleRequest,
  previous?: TriggerSpec,
): TriggerSpec {
  switch (schedule.schedule_type) {
    case 'cron': {
      const prevTz =
        previous?.kind === 'cron' ? previous.timezone : undefined
      return {
        kind: 'cron',
        expr: schedule.cron_expression ?? (previous?.kind === 'cron' ? previous.expr : ''),
        timezone: schedule.timezone ?? prevTz ?? 'UTC',
      }
    }
    case 'interval':
      return {
        kind: 'interval',
        every_secs: schedule.interval_seconds ?? 60,
      }
    case 'event': {
      let filter: unknown = previous?.kind === 'event' ? previous.filter : undefined
      if (schedule.event_filter !== undefined) {
        try {
          filter = JSON.parse(schedule.event_filter)
        } catch {
          filter = schedule.event_filter
        }
      }
      return { kind: 'event', filter: filter ?? {} }
    }
    case 'manual':
      return { kind: 'manual' }
  }
}

// ---------------------------------------------------------------------------
// ToolPolicy ↔ tool_config
// ---------------------------------------------------------------------------

export function toolPolicyToConfig(policy: ToolPolicy): AgentToolConfig {
  if (policy === 'disabled') {
    return { enabled: false, allowed_tools: [] }
  }
  if (policy === 'unrestricted') {
    return { enabled: true, allowed_tools: [] }
  }
  return {
    enabled: true,
    allowed_tools: [...(policy.scoped.allowed_tools ?? [])],
  }
}

export function toolConfigToPolicy(
  config: AgentToolConfig | undefined,
  previous?: ToolPolicy,
): ToolPolicy {
  if (!config) return previous ?? 'unrestricted'
  if (!config.enabled) return 'disabled'
  if (!config.allowed_tools || config.allowed_tools.length === 0) {
    // Empty allow-list = all tools. Prefer previous unrestricted/disabled
    // distinction only when previous was already unrestricted.
    if (previous === 'disabled') return 'unrestricted'
    if (previous && typeof previous === 'object' && 'scoped' in previous) {
      // Form cleared the allow-list → unrestricted
      return 'unrestricted'
    }
    return previous ?? 'unrestricted'
  }
  const prevCommands =
    previous && typeof previous === 'object' && 'scoped' in previous
      ? previous.scoped.allowed_commands ?? []
      : []
  return {
    scoped: {
      allowed_tools: [...config.allowed_tools],
      allowed_commands: prevCommands,
    },
  }
}

// ---------------------------------------------------------------------------
// NotifyPolicy ↔ legacy AgentNotify
// ---------------------------------------------------------------------------

function legacyOnFromPolicy(notify: NotifyPolicy): AgentNotifyOn {
  if (notify.pending_confirmation) return 'judgment'
  if (notify.on_complete && notify.on_failure && !notify.on_alert) return 'always'
  if (notify.on_alert && !notify.on_complete) return 'failure' // closest; alert path ≠ failure
  if (notify.on_failure && !notify.on_complete && !notify.on_alert) return 'failure'
  if (notify.on_complete) return 'always'
  return 'failure'
}

export function notifyPolicyToLegacy(notify: NotifyPolicy | undefined): AgentNotify | undefined {
  if (!notify) return undefined
  return {
    channels: [...(notify.channels ?? [])],
    on: legacyOnFromPolicy(notify),
  }
}

/**
 * Map legacy notify onto v2. When the form's legacy projection matches the
 * previous policy's projection, callers should keep `previous` instead —
 * this helper is for when the form did change notify.
 */
export function legacyNotifyToPolicy(
  legacy: AgentNotify | undefined,
  previous?: NotifyPolicy,
): NotifyPolicy {
  if (!legacy) {
    return (
      previous ?? {
        channels: [],
        on_complete: false,
        on_alert: false,
        alert_depends_on: [],
        on_failure: true,
        pending_confirmation: false,
      }
    )
  }
  const channels = [...(legacy.channels ?? [])]
  const base = previous ?? {
    channels: [],
    on_complete: false,
    on_alert: false,
    alert_depends_on: [],
    on_failure: true,
    pending_confirmation: false,
  }
  switch (legacy.on) {
    case 'always':
      return {
        ...base,
        channels,
        on_complete: true,
        on_failure: true,
        on_alert: base.on_alert ?? false,
        pending_confirmation: false,
      }
    case 'judgment':
      return {
        ...base,
        channels,
        on_complete: false,
        on_alert: false,
        on_failure: false,
        pending_confirmation: true,
      }
    case 'failure':
    default:
      return {
        ...base,
        channels,
        on_complete: false,
        on_failure: true,
        // Preserve alert wiring the legacy form cannot express.
        on_alert: base.on_alert ?? false,
        alert_depends_on: base.alert_depends_on ?? [],
        pending_confirmation: false,
      }
  }
}

// ---------------------------------------------------------------------------
// Input ↔ resources (lossy; carry wins when unchanged)
// ---------------------------------------------------------------------------

export function inputToResources(input: InputContract | undefined): ResourceRequest[] {
  if (!input?.requirements?.length) return []
  const out: ResourceRequest[] = []
  for (const req of input.requirements) {
    const mapped = requirementToResource(req)
    if (mapped) out.push(mapped)
  }
  return out
}

function requirementToResource(req: InputRequirement): ResourceRequest | null {
  const src = req.source
  switch (src.kind) {
    case 'device_metric':
      return {
        resource_id: `${src.device_id}:${src.field}`,
        resource_type: 'metric',
        name: req.key,
        config: {
          device_id: src.device_id,
          field: src.field,
          input_key: req.key,
        },
      }
    case 'whole_device':
      return {
        resource_id: src.device_id,
        resource_type: 'device',
        name: req.key,
        config: { input_key: req.key },
      }
    case 'extension_instance':
      return {
        resource_id: src.id,
        resource_type: 'extension_metric',
        name: req.key,
        config: {
          input_key: req.key,
          operation: req.operation,
          params: req.params,
        },
      }
    case 'upstream_ai':
      return {
        resource_id: `${src.agent_id}:${src.field}`,
        resource_type: 'data_stream',
        name: req.key,
        config: {
          agent_id: src.agent_id,
          field: src.field,
          input_key: req.key,
        },
      }
    case 'caller_payload':
      return null
  }
}

// ---------------------------------------------------------------------------
// Public adapters
// ---------------------------------------------------------------------------

/**
 * Project a v2 AgentSpec/Draft into the legacy editor form, stashing
 * unexpressible fields in `v2Carry` for a lossless round-trip.
 */
export function v2SpecToFormState(spec: AgentSpecLike): AgentEditorFormState {
  const carry = asCarry(spec)
  return {
    name: spec.name,
    description: spec.description,
    user_prompt: spec.user_prompt,
    llm_backend_id: spec.llm_backend_id,
    schedule: triggerToSchedule(spec.trigger),
    resources: inputToResources(spec.input),
    notify: notifyPolicyToLegacy(spec.notify),
    tool_config: toolPolicyToConfig(spec.tool_policy),
    output_schema: (spec.output?.fields ?? []).map((f) => ({
      name: f.name,
      field_type: { type: 'text' as const },
      description: f.description,
    })),
    v2Carry: carry,
  }
}

/**
 * Build an AgentSpecDraft from the legacy form.
 *
 * When `previous` (or `form.v2Carry`) is present, fields whose form projection
 * still matches the carried projection are restored verbatim — including
 * timezone, tool_policy, notify channels/flags, and input.requirements.
 */
export function formStateToV2Draft(
  form: AgentEditorFormState,
  previous?: AgentSpecLike,
): AgentSpecDraft {
  const carry: AgentSpecV2Carry | undefined = form.v2Carry ?? (previous ? asCarry(previous) : undefined)

  const trigger = resolveTrigger(form, carry)
  const input = resolveInput(form, carry)
  const tool_policy = resolveToolPolicy(form, carry)
  const notify = resolveNotify(form, carry)
  const output = resolveOutput(form, carry)

  return {
    name: form.name,
    description: form.description ?? previous?.description,
    user_prompt: form.user_prompt,
    llm_backend_id: form.llm_backend_id ?? previous?.llm_backend_id,
    preset: carry?.preset ?? previous?.preset,
    trigger,
    pacing: carry?.pacing ?? previous?.pacing,
    input,
    tool_policy,
    output,
    notify,
    controlled_actions: carry?.controlled_actions ?? previous?.controlled_actions ?? [],
  }
}

function resolveTrigger(form: AgentEditorFormState, carry?: AgentSpecV2Carry): TriggerSpec {
  if (carry?.trigger) {
    const projected = triggerToSchedule(carry.trigger)
    if (sameJson(form.schedule, projected)) {
      return carry.trigger
    }
  }
  return scheduleToTrigger(form.schedule, carry?.trigger)
}

function resolveInput(form: AgentEditorFormState, carry?: AgentSpecV2Carry): InputContract {
  if (carry?.input) {
    const projected = inputToResources(carry.input)
    if (sameJson(form.resources ?? [], projected)) {
      return carry.input
    }
  }
  // Resources changed (or no carry): build a best-effort InputContract so
  // Chat/analyst create stubs can reach POST /agents/v2 without a silent empty input.
  if (form.resources?.length) {
    return resourcesToInput(form.resources)
  }
  if (carry?.input) return carry.input
  return { requirements: [] }
}

/** Best-effort ResourceRequest[] → InputContract for V2 create/update stubs. */
export function resourcesToInput(resources: ResourceRequest[] | undefined): InputContract {
  if (!resources?.length) return { requirements: [] }
  const requirements: InputRequirement[] = []
  for (const [i, r] of resources.entries()) {
    const req = resourceToRequirement(r, i)
    if (req) requirements.push(req)
  }
  return { requirements }
}

function resourceToRequirement(r: ResourceRequest, index: number): InputRequirement | null {
  const key = r.name || `r${index}`
  const base = {
    key,
    params: (r.config as Record<string, unknown>) ?? {},
    timeout_secs: 10,
    result_type: 'structured_json' as const,
    freshness: { kind: 'any' as const },
    on_missing: 'report_stale' as const,
  }
  switch (r.resource_type) {
    case 'device':
      return { ...base, source: { kind: 'whole_device', device_id: r.resource_id } }
    case 'metric': {
      const [device_id, field] = r.resource_id.split(/:(.+)/)
      return {
        ...base,
        source: {
          kind: 'device_metric',
          device_id: device_id || r.resource_id,
          field: field || 'value',
        },
      }
    }
    case 'extension_metric':
    case 'extension_tool': {
      const id =
        typeof r.config?.extension_id === 'string'
          ? r.config.extension_id
          : r.resource_id.replace(/^extension:/, '').split(':')[0]
      return {
        ...base,
        source: { kind: 'extension_instance', id },
        operation:
          typeof r.config?.command_name === 'string'
            ? r.config.command_name
            : typeof r.config?.operation === 'string'
              ? r.config.operation
              : undefined,
      }
    }
    case 'data_stream': {
      const [agent_id, field] = r.resource_id.split(/:(.+)/)
      return {
        ...base,
        source: {
          kind: 'upstream_ai',
          agent_id: agent_id || r.resource_id,
          field: field || 'value',
        },
      }
    }
    case 'command':
      // Commands become controlled_actions / tool allowlists, not input reads.
      return null
    default:
      return { ...base, source: { kind: 'whole_device', device_id: r.resource_id } }
  }
}

function resolveToolPolicy(form: AgentEditorFormState, carry?: AgentSpecV2Carry): ToolPolicy {
  if (carry?.tool_policy !== undefined) {
    const projected = toolPolicyToConfig(carry.tool_policy)
    if (sameJson(form.tool_config, projected)) {
      return carry.tool_policy
    }
  }
  return toolConfigToPolicy(form.tool_config, carry?.tool_policy)
}

function resolveNotify(form: AgentEditorFormState, carry?: AgentSpecV2Carry): NotifyPolicy {
  if (carry?.notify) {
    const projected = notifyPolicyToLegacy(carry.notify)
    if (sameJson(form.notify, projected)) {
      return carry.notify
    }
  }
  return legacyNotifyToPolicy(form.notify, carry?.notify)
}

function resolveOutput(
  form: AgentEditorFormState,
  carry?: AgentSpecV2Carry,
): AgentSpecDraft['output'] {
  if (carry?.output) {
    const projectedNames = (carry.output.fields ?? []).map((f) => f.name).join('\0')
    const formNames = (form.output_schema ?? []).map((f) => f.name).join('\0')
    if (projectedNames === formNames) {
      return carry.output
    }
  }
  if (carry?.output) return carry.output
  if (!form.output_schema?.length) return { fields: [], publish: false }
  return {
    fields: form.output_schema.map((f) => ({
      name: f.name,
      depends_on: [],
      description: f.description,
    })),
    publish: false,
  }
}

/**
 * Apply a name-only (or other sparse) form patch onto a previous spec and
 * return a draft suitable for PUT. Convenience for tests and Chat updates.
 */
export function applyFormPatchToV2Draft(
  previous: AgentSpecLike,
  patch: Partial<Pick<AgentEditorFormState, 'name' | 'description' | 'user_prompt' | 'llm_backend_id'>>,
): AgentSpecDraft {
  const form = v2SpecToFormState(previous)
  if (patch.name !== undefined) form.name = patch.name
  if (patch.description !== undefined) form.description = patch.description
  if (patch.user_prompt !== undefined) form.user_prompt = patch.user_prompt
  if (patch.llm_backend_id !== undefined) form.llm_backend_id = patch.llm_backend_id
  return formStateToV2Draft(form, previous)
}
