/**
 * Page-link: AgentsPage edit load (GET /agents/v2) → rename-only → PUT patch
 * must leave every other contract field intact for a subsequent re-GET.
 *
 * Guards the regression where getAgent + detailToV2Draft emptied
 * input.requirements / controlled_actions and cleared output.depends_on.
 */
import { describe, expect, it } from 'vitest'
import type { AgentSpec, AgentSpecDraft, AiAgentDetail } from '@/types'
import {
  detailToV2Draft,
  draftToUpdatePatch,
  specToDraft,
} from '@/components/agents/editor-v2/defaults'

/** Embedded from tests/fixtures/agent-contract/v2/daily-brief.json `draft`. */
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

const CONTRACT_KEYS = [
  'description',
  'user_prompt',
  'llm_backend_id',
  'preset',
  'trigger',
  'pacing',
  'input',
  'tool_policy',
  'output',
  'notify',
  'controlled_actions',
] as const

describe('AgentsPage edit V2 page-link', () => {
  it('rename-only → re-GET keeps all other contract fields', () => {
    const loaded: AgentSpec = {
      ...dailyBriefDraft,
      id: 'agent-daily-brief',
      config_version: 3,
      created_at: 1_700_000_000,
      updated_at: 1_700_000_100,
    }

    // Page edit load: GET /agents/v2/:id → seed editor (not detailToV2Draft).
    const draft = specToDraft(loaded)
    draft.name = 'daily-brief-renamed'

    // Page save: same patch body AgentsPage sends via updateAgentV2.
    const patch = draftToUpdatePatch(draft)

    // Server-side merge for a full-contract PUT, then re-GET shape.
    const afterGet: AgentSpec = {
      ...loaded,
      name: patch.name!,
      description: patch.description ?? loaded.description,
      user_prompt: patch.user_prompt ?? loaded.user_prompt,
      llm_backend_id: patch.llm_backend_id ?? loaded.llm_backend_id,
      preset: patch.preset ?? loaded.preset,
      trigger: patch.trigger ?? loaded.trigger,
      pacing: patch.pacing ?? loaded.pacing,
      input: patch.input ?? loaded.input,
      tool_policy: patch.tool_policy ?? loaded.tool_policy,
      output: patch.output ?? loaded.output,
      notify: patch.notify ?? loaded.notify,
      controlled_actions: patch.controlled_actions ?? loaded.controlled_actions,
      config_version: loaded.config_version + 1,
      updated_at: loaded.updated_at + 1,
    }

    expect(afterGet.name).toBe('daily-brief-renamed')
    expect(afterGet.id).toBe(loaded.id)

    for (const key of CONTRACT_KEYS) {
      expect(afterGet[key]).toEqual(loaded[key])
    }

    // Fixture must_roundtrip fields (timezone, requirements, tool_policy, channels).
    expect(afterGet.trigger).toEqual({
      kind: 'cron',
      expr: '0 9 * * *',
      timezone: 'Asia/Shanghai',
    })
    expect(afterGet.input.requirements).toHaveLength(2)
    expect(afterGet.input.requirements.map((r) => r.key)).toEqual([
      'weather_now',
      'tariff_today',
    ])
    expect(afterGet.tool_policy).toBe('disabled')
    expect(afterGet.notify?.channels).toEqual(['test-channel'])
    expect(afterGet.output?.fields[0]?.depends_on).toEqual([
      'weather_now',
      'tariff_today',
    ])
  })

  it('legacy detailToV2Draft still empties contract fields (do not use for edit)', () => {
    const legacy = {
      id: 'legacy-1',
      name: 'legacy',
      description: 'd',
      user_prompt: 'do something',
      schedule: { type: 'Cron', cron: '0 9 * * *' },
      tool_config: { enabled: false },
      output_schema: [{ name: 'brief', description: 'x' }],
    } as unknown as AiAgentDetail

    const wiped = detailToV2Draft(legacy)
    expect(wiped.input.requirements).toEqual([])
    expect(wiped.controlled_actions).toEqual([])
    expect(wiped.output?.fields[0]?.depends_on).toEqual([])
  })
})
