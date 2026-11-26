import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { ExecutionDetailDialog } from '../ExecutionDetailDialog'
import type { AgentExecutionDetail } from '@/types'

const mockGetExecution = vi.fn()

vi.mock('@/lib/api', () => ({
  api: {
    getExecution: (...args: unknown[]) => mockGetExecution(...args),
  },
}))

vi.mock('@/hooks/useErrorHandler', () => ({
  useErrorHandler: () => ({ handleError: vi.fn() }),
}))

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

vi.mock('@/components/chat/MarkdownMessage', () => ({
  MarkdownMessage: ({ content }: { content: string }) => <div>{content}</div>,
}))

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: string | Record<string, unknown>) => {
        if (typeof opts === 'string') return opts
        if (opts && typeof opts === 'object') {
          if ('version' in opts) return `${key}:${opts.version}`
          if (typeof opts.defaultValue === 'string') return opts.defaultValue
        }
        return key
      },
      i18n: { language: 'en' },
    }),
  }
})

describe('ExecutionDetailDialog (new execution fields)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders config_version, dry_run, stale_config, alert_verdict, structured_result, and deliveries', async () => {
    const exec: AgentExecutionDetail = {
      id: 'exec-123456',
      agent_id: 'agent-1',
      timestamp: '2026-11-26T09:00:00Z',
      trigger_type: 'cron',
      status: 'Partial',
      duration_ms: 420,
      config_version: 5,
      dry_run: true,
      stale_config: true,
      alert_verdict: 'alert_raised',
      structured_result: {
        status: 'abnormal',
        anomaly_count: 2,
      },
      deliveries: [
        {
          execution_id: 'exec-123456',
          destination: 'channel:IM',
          action_fingerprint: 'fp-1',
          idempotent: true,
          state: { state: 'confirmed' },
        },
        {
          execution_id: 'exec-123456',
          destination: 'webhook:ops',
          action_fingerprint: 'fp-2',
          idempotent: false,
          state: { state: 'rejected', reason: 'Channel disabled' },
        },
      ],
      decision_process: {
        situation_analysis: '',
        data_collected: [],
        reasoning_steps: [],
        decisions: [],
        conclusion: 'Detected 2 anomalies in cold room.',
      },
    }
    mockGetExecution.mockResolvedValue(exec)

    render(
      <ExecutionDetailDialog
        open
        onOpenChange={vi.fn()}
        agentId="agent-1"
        executionId="exec-123456"
      />,
    )

    await waitFor(() => {
      expect(screen.getByText('agents:execution.configVersion:5')).toBeTruthy()
    })

    expect(screen.getByText('agents:execution.dryRun')).toBeTruthy()
    expect(screen.getByText('agents:execution.staleConfig')).toBeTruthy()
    expect(screen.getByText('agents:execution.alertVerdict: alert_raised')).toBeTruthy()

    // Structured result fields
    expect(screen.getByText('status')).toBeTruthy()
    expect(screen.getByText('abnormal')).toBeTruthy()
    expect(screen.getByText('anomaly_count')).toBeTruthy()
    expect(screen.getByText('2')).toBeTruthy()

    // Delivery receipts
    expect(screen.getByText('channel:IM')).toBeTruthy()
    expect(screen.getByText('agents:execution.deliveryState.confirmed')).toBeTruthy()
    expect(screen.getByText('webhook:ops')).toBeTruthy()
    expect(screen.getByText('agents:execution.deliveryState.rejected')).toBeTruthy()
    expect(screen.getByText('Channel disabled')).toBeTruthy()
  })
})
