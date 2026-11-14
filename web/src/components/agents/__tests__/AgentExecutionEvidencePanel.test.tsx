import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AgentExecutionEvidencePanel } from '../AgentExecutionEvidencePanel'
import type { AgentExecutionDetail } from '@/types/agent'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: { count?: number; defaultValue?: string }) => {
        const k = key.replace(/^agents:/, '')
        const map: Record<string, string> = {
          'evidence.title': 'Execution evidence',
          'evidence.inputs': 'Inputs',
          'evidence.output': 'Output',
          'evidence.actions': 'Action results',
          'evidence.timeUnknown': 'Time unknown',
          'evidence.resultUnknown': 'Result unknown',
          'evidence.unknownDetail': 'Failure detail unknown',
          'evidence.inputsEmpty': 'No inputs recorded',
          'evidence.outputEmpty': 'No output recorded',
          'evidence.actionsEmpty': 'No actions recorded',
          'evidence.presence.known': 'Known',
          'evidence.presence.empty': 'Empty',
          'evidence.presence.unknown': 'Unknown',
          'evidence.failure.none': 'No failure node',
          'evidence.failure.error': 'Failed — execution error',
          'evidence.failure.unknown': 'Failure node unknown',
          'evidence.failure.failed_action': 'Failed — action error',
        }
        if (k === 'evidence.tokens' && opts?.count != null) return `${opts.count} tokens`
        return map[k] ?? opts?.defaultValue ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

vi.mock('@/lib/utils/format', () => ({
  formatTimestamp: (ms: number) => `ts:${ms}`,
}))

function detail(over: Partial<AgentExecutionDetail> = {}): AgentExecutionDetail {
  return {
    id: 'e1',
    agent_id: 'a1',
    timestamp: '2026-11-14T12:00:00Z',
    trigger_type: 'manual',
    status: 'Completed',
    duration_ms: 10,
    ...over,
  }
}

describe('AgentExecutionEvidencePanel', () => {
  it('shows unknown input time without inventing a clock', () => {
    const { container } = render(
      <AgentExecutionEvidencePanel
        detail={detail({
          decision_process: {
            situation_analysis: '',
            data_collected: [
              { source: 'device:x:y', data_type: 'metric', values: {}, timestamp: 0 },
            ],
            reasoning_steps: [],
            decisions: [],
            conclusion: 'ok',
          },
        })}
      />,
    )
    expect(container.querySelector('[data-inputs="known"]')).toBeTruthy()
    expect(screen.getByText('Time unknown')).toBeTruthy()
    expect(screen.getByText('No failure node')).toBeTruthy()
  })

  it('marks failure unknown when status failed without detail', () => {
    const { container } = render(
      <AgentExecutionEvidencePanel
        detail={detail({
          status: 'Failed',
          decision_process: {
            situation_analysis: '',
            data_collected: [],
            reasoning_steps: [],
            decisions: [],
            conclusion: '',
          },
        })}
      />,
    )
    expect(container.querySelector('[data-failure="unknown"]')).toBeTruthy()
    expect(screen.getByText('Failure node unknown')).toBeTruthy()
  })
})
