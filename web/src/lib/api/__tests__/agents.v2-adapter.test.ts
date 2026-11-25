/**
 * M2 temporary adapter: name-only edit must preserve v2 fields the legacy
 * editor cannot express (timezone, tool_policy, notify.channels, inputs).
 *
 * Critical fields embedded from tests/fixtures/agent-contract/v2/daily-brief.json
 * (must_roundtrip: trigger.timezone, input.requirements, tool_policy, notify.channels).
 */
import { describe, expect, it } from 'vitest'
import type { AgentSpecDraft } from '@/types'
import {
  applyFormPatchToV2Draft,
  formStateToV2Draft,
  v2SpecToFormState,
} from '../agentV2Adapter'

/** Snapshot of daily-brief.json `draft` — keep in sync with the shared fixture. */
const dailyBriefDraft: AgentSpecDraft = {
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

describe('M2 agent v2 form adapter', () => {
  it('name-only patch keeps timezone, tool_policy, channels, and inputs', () => {
    const draft = dailyBriefDraft

    expect(draft.trigger).toMatchObject({
      kind: 'cron',
      expr: '0 9 * * *',
      timezone: 'Asia/Shanghai',
    })
    expect(draft.tool_policy).toBe('disabled')
    expect(draft.notify?.channels).toEqual(['test-channel'])
    expect(draft.input.requirements).toHaveLength(2)

    const form = v2SpecToFormState(draft)
    form.name = 'daily-brief-renamed'

    const next = formStateToV2Draft(form, draft)

    expect(next.name).toBe('daily-brief-renamed')
    expect(next.trigger).toEqual(draft.trigger)
    if (next.trigger.kind === 'cron') {
      expect(next.trigger.timezone).toBe('Asia/Shanghai')
    }
    expect(next.tool_policy).toBe('disabled')
    expect(next.notify?.channels).toEqual(['test-channel'])
    expect(next.notify?.on_complete).toBe(true)
    expect(next.notify?.on_failure).toBe(true)
    expect(next.input.requirements).toEqual(draft.input.requirements)
    expect(next.input.requirements.map((r) => r.key)).toEqual([
      'weather_now',
      'tariff_today',
    ])
    expect(next.preset).toBe('daily_brief')
    expect(next.output).toEqual(draft.output)
  })

  it('applyFormPatchToV2Draft is equivalent for a name-only edit', () => {
    const draft = dailyBriefDraft
    const next = applyFormPatchToV2Draft(draft, { name: 'only-name-changed' })

    expect(next.name).toBe('only-name-changed')
    expect(next.trigger).toEqual(draft.trigger)
    expect(next.tool_policy).toBe(draft.tool_policy)
    expect(next.notify).toEqual(draft.notify)
    expect(next.input).toEqual(draft.input)
    expect(next.user_prompt).toBe(draft.user_prompt)
  })

  it('round-trips unchanged form back to an equal draft payload', () => {
    const draft = dailyBriefDraft
    const form = v2SpecToFormState(draft)
    const next = formStateToV2Draft(form, draft)

    expect(next).toMatchObject({
      name: draft.name,
      description: draft.description,
      user_prompt: draft.user_prompt,
      preset: draft.preset,
      trigger: draft.trigger,
      tool_policy: draft.tool_policy,
      input: draft.input,
      notify: draft.notify,
      output: draft.output,
      controlled_actions: draft.controlled_actions ?? [],
    })
  })
})
