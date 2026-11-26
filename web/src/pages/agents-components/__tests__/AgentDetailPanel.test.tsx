import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { AgentDetailPanel } from '../AgentDetailPanel'
import type { AiAgentDetail } from '@/types'

const mockGetAgentExecutions = vi.fn()
const mockGetAgentMemory = vi.fn()
const mockExplainLastAgent = vi.fn()
const mockListLlmBackends = vi.fn()

vi.mock('@/lib/api', () => ({
  api: {
    getAgentExecutions: (...args: unknown[]) => mockGetAgentExecutions(...args),
    getAgentMemory: (...args: unknown[]) => mockGetAgentMemory(...args),
    explainLastAgent: (...args: unknown[]) => mockExplainLastAgent(...args),
    listLlmBackends: (...args: unknown[]) => mockListLlmBackends(...args),
    getAgent: vi.fn(),
  },
}))

vi.mock('@/hooks/useEvents', () => ({
  useEvents: () => {},
}))

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
}))

vi.mock('@/hooks/useErrorHandler', () => ({
  useErrorHandler: () => ({ handleError: vi.fn() }),
}))

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: Record<string, unknown>) => {
        if (opts && typeof opts.defaultValue === 'string') return opts.defaultValue
        if (opts && 'version' in opts) return `${key}:${opts.version}`
        if (opts && 'secs' in opts) return `${key}:${opts.secs}`
        if (opts && 'mins' in opts) return `${key}:${opts.mins}`
        if (opts && 'hours' in opts) return `${key}:${opts.hours}`
        if (opts && 'deps' in opts) return `${key}:${opts.deps}`
        return key
      },
      i18n: { language: 'en' },
    }),
  }
})

function makeSpecAgent(overrides: Partial<AiAgentDetail> = {}): AiAgentDetail {
  return {
    id: 'agent-1',
    name: 'Cold Room Monitor',
    status: 'Active',
    user_prompt: 'Inspect cold room temperatures every morning.',
    created_at: 1764000000000,
    updated_at: 1764003600000,
    last_execution_at: 1764003500000,
    execution_count: 12,
    success_count: 10,
    error_count: 1,
    avg_duration_ms: 850,
    config_version: 3,
    llm_backend_id: 'backend-1',
    trigger: {
      kind: 'cron',
      expr: '0 9 * * *',
      timezone: 'Asia/Shanghai',
    },
    input: {
      requirements: [
        {
          key: 'temp_reading',
          source: { kind: 'device_metric', device_id: 'sensor-01', field: 'temperature' },
          timeout_secs: 10,
          result_type: 'metric',
          freshness: { kind: 'latest_within', max_age_secs: 300 },
          on_missing: 'undetermined',
        },
      ],
    },
    output: {
      publish: true,
      fields: [
        { name: 'risk_level', description: 'Cold room risk', depends_on: ['temp_reading'] },
      ],
    },
    tool_policy: {
      scoped: {
        allowed_tools: ['shell'],
        allowed_commands: ['device.restart'],
      },
    },
    notify: {
      channels: ['IM', 'webhook:ops'],
      on_complete: false,
      on_alert: true,
      on_failure: true,
      pending_confirmation: false,
    },
    pacing: {
      burst_merge_secs: 30,
      min_interval_secs: 60,
      max_runs_per_day: 24,
      failure_threshold: 3,
    },
    controlled_actions: [
      { name: 'Restart Fan', destination: 'device:fan-01:restart', idempotent: true },
    ],
    effective_plan: {
      agent_id: 'agent-1',
      config_version: 3,
      trigger_summary: 'cron(0 9 * * * @ Asia/Shanghai)',
      timezone: 'Asia/Shanghai',
      authorized_reads: [
        { key: 'temp_reading', source_id: 'sensor-01:temperature', capability_class: 'device_metric' },
      ],
      tool_policy: { scoped: { allowed_tools: ['shell'], allowed_commands: ['device.restart'] } },
      notify: { channels: ['IM', 'webhook:ops'], on_alert: true, on_failure: true },
      controlled_actions: [
        { name: 'Restart Fan', destination: 'device:fan-01:restart', idempotent: true },
      ],
      warnings: ['Daily cap is close to cron cadence'],
      external_read_counts: { weather: 0, tariff: 0, other: 0 },
    },
    stats: {
      total_runs: 12,
      successful_runs: 10,
      failed_runs: 1,
      partial_runs: 1,
      runs_today: 4,
      avg_duration_ms: 850,
    },
    latest_output: { risk_level: 'normal' },
    ...overrides,
  }
}

describe('AgentDetailPanel (AgentSpec contract)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockGetAgentExecutions.mockResolvedValue({ executions: [], count: 0 })
    mockGetAgentMemory.mockResolvedValue({
      journal: { records: [], max_records: 10 },
      knowledge_files: [],
      updated_at: '2026-11-26T00:00:00Z',
    })
    mockExplainLastAgent.mockResolvedValue({
      agent_id: 'agent-1',
      explanation: 'Run succeeded on fresh sensor-01 temperature reading.',
    })
    mockListLlmBackends.mockResolvedValue({
      backends: [{ id: 'backend-1', name: 'Primary LLM', model: 'qwen3:8b' }],
    })
  })

  it('renders full AgentSpec without legacy schedule/resources fields and loads explain-last', async () => {
    const agent = makeSpecAgent()
    // Ensure legacy fields are absent to verify zero crash on new payloads
    expect(agent.schedule).toBeUndefined()
    expect(agent.resources).toBeUndefined()
    expect(agent.output_schema).toBeUndefined()

    render(
      <AgentDetailPanel
        agent={agent}
        onEdit={vi.fn()}
        onExecute={vi.fn()}
        onViewExecutionDetail={vi.fn()}
        onRefresh={vi.fn()}
        inlineMode
      />,
    )

    // Trigger (cron + timezone)
    expect(screen.getByText('0 9 * * *')).toBeTruthy()
    expect(screen.getByText('Asia/Shanghai')).toBeTruthy()

    // Output contract
    expect(screen.getByText('risk_level')).toBeTruthy()
    expect(screen.getByText('normal')).toBeTruthy()

    // Input requirements
    expect(screen.getByText('temp_reading')).toBeTruthy()
    expect(screen.getByText('sensor-01 / temperature')).toBeTruthy()

    // Tool policy (scoped)
    expect(screen.getByText('shell')).toBeTruthy()
    expect(screen.getByText('device.restart')).toBeTruthy()

    // Notification channels
    expect(screen.getByText('IM, webhook:ops')).toBeTruthy()

    // Pacing
    expect(screen.getByText('30s')).toBeTruthy()
    expect(screen.getByText('60s')).toBeTruthy()
    expect(screen.getByText('24')).toBeTruthy()

    // Controlled actions & effective plan warning
    expect(screen.getByText('Restart Fan')).toBeTruthy()
    expect(screen.getByText('Daily cap is close to cron cadence')).toBeTruthy()

    // Async explain-last & LLM backend name
    await waitFor(() => {
      expect(screen.getByText('Run succeeded on fresh sensor-01 temperature reading.')).toBeTruthy()
      expect(screen.getByText('Primary LLM')).toBeTruthy()
    })
  })

  it('handles manual trigger, disabled tools, and empty inputs cleanly', async () => {
    mockExplainLastAgent.mockRejectedValue(new Error('not found'))
    const agent = makeSpecAgent({
      trigger: { kind: 'manual' },
      input: { requirements: [] },
      output: { fields: [], publish: false },
      tool_policy: 'disabled',
      notify: { channels: [], on_complete: false, on_alert: false, on_failure: false },
      pacing: undefined,
      controlled_actions: [],
      effective_plan: undefined,
    })

    render(
      <AgentDetailPanel
        agent={agent}
        onEdit={vi.fn()}
        onExecute={vi.fn()}
        onViewExecutionDetail={vi.fn()}
        onRefresh={vi.fn()}
        inlineMode
      />,
    )

    expect(screen.getByText('agents:detail.manualOnly')).toBeTruthy()
    expect(screen.getByText('agents:detail.noInputs')).toBeTruthy()
    expect(screen.getByText('agents:detail.notifySilent')).toBeTruthy()
  })
})
