import { describe, expect, it } from 'vitest'
import { deriveAgentExecutionEvidence } from '../agentExecutionEvidence'
import type { AgentExecutionDetail } from '@/types/agent'

function base(over: Partial<AgentExecutionDetail> = {}): AgentExecutionDetail {
  return {
    id: 'e1',
    agent_id: 'a1',
    timestamp: '2026-11-14T12:00:00Z',
    trigger_type: 'manual',
    status: 'Completed',
    duration_ms: 100,
    ...over,
  }
}

describe('deriveAgentExecutionEvidence', () => {
  it('null detail → all unknown', () => {
    const e = deriveAgentExecutionEvidence(null)
    expect(e.inputsPresence).toBe('unknown')
    expect(e.outputPresence).toBe('unknown')
    expect(e.actionsPresence).toBe('unknown')
    expect(e.failure.kind).toBe('unknown')
  })

  it('marks input time unknown when timestamp is 0', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        decision_process: {
          situation_analysis: '',
          data_collected: [
            { source: 'device:s1:temp', data_type: 'metric', values: { v: 1 }, timestamp: 0 },
          ],
          reasoning_steps: [],
          decisions: [],
          conclusion: 'ok',
        },
      }),
    )
    expect(e.inputsPresence).toBe('known')
    expect(e.inputs[0].timeState).toBe('unknown')
    expect(e.outputPresence).toBe('known')
    expect(e.failure.kind).toBe('none')
  })

  it('keeps known input times (seconds)', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        decision_process: {
          situation_analysis: '',
          data_collected: [
            {
              source: 'device:cam:people',
              data_type: 'metric',
              values: { n: 3 },
              timestamp: 1_700_000_000,
            },
          ],
          reasoning_steps: [],
          decisions: [],
          conclusion: 'crowded',
        },
      }),
    )
    expect(e.inputs[0].timeState).toBe('known')
    expect(e.inputs[0].timestampMs).toBe(1_700_000_000_000)
  })

  it('surfaces failed action as failure node; result unknown when missing', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Completed',
        decision_process: {
          situation_analysis: '',
          data_collected: [],
          reasoning_steps: [],
          decisions: [],
          conclusion: 'tried',
        },
        result: {
          actions_executed: [
            {
              action_type: 'execute',
              description: 'relay on',
              target: 'device:act',
              success: false,
            },
          ],
          notifications_sent: [],
          summary: 'done',
          success_rate: 0,
        },
      }),
    )
    expect(e.failure.kind).toBe('failed_action')
    expect(e.actions[0].resultState).toBe('unknown')
  })

  it('error field is primary failure node', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Failed',
        error: 'LLM timeout',
        decision_process: {
          situation_analysis: '',
          data_collected: [],
          reasoning_steps: [],
          decisions: [],
          conclusion: '',
          stop_reason: 'timeout',
        },
      }),
    )
    expect(e.failure.kind).toBe('error')
    expect(e.failure.detail).toBe('LLM timeout')
    expect(e.stopReasonPresence).toBe('known')
  })

  it('Failed status without detail → failure unknown', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Failed',
        decision_process: {
          situation_analysis: '',
          data_collected: [],
          reasoning_steps: [],
          decisions: [],
          conclusion: '',
        },
      }),
    )
    expect(e.failure.kind).toBe('unknown')
    expect(e.failure.state).toBe('unknown')
  })

  it('empty decision_process inputs → empty not unknown', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        decision_process: {
          situation_analysis: 'x',
          data_collected: [],
          reasoning_steps: [],
          decisions: [],
          conclusion: 'y',
        },
        result: {
          actions_executed: [],
          notifications_sent: [],
          summary: 'y',
          success_rate: 1,
        },
      }),
    )
    expect(e.inputsPresence).toBe('empty')
    expect(e.actionsPresence).toBe('empty')
  })

  it('derives actions from V2 deliveries and tool_calls when result is absent', () => {
    const emptyV2 = deriveAgentExecutionEvidence(
      base({
        deliveries: [],
        tool_calls: [],
      }),
    )
    expect(emptyV2.actionsPresence).toBe('empty')
    expect(emptyV2.actions).toHaveLength(0)

    const populatedV2 = deriveAgentExecutionEvidence(
      base({
        deliveries: [
          {
            execution_id: 'e1',
            destination: 'device:fan-01',
            action_fingerprint: 'restart',
            idempotent: true,
            state: { state: 'confirmed' },
          },
        ],
        tool_calls: [
          {
            round: 1,
            name: 'shell',
            arguments: { command: 'neomind device list' },
            success: true,
            result_preview: '2 devices',
            duration_ms: 42,
          },
        ],
      }),
    )
    expect(populatedV2.actionsPresence).toBe('known')
    expect(populatedV2.actions).toHaveLength(2)
    expect(populatedV2.actions[0]).toMatchObject({
      description: 'restart',
      target: 'device:fan-01',
      success: true,
      result: 'confirmed',
      resultState: 'known',
    })
    expect(populatedV2.actions[1]).toMatchObject({
      description: '#1 shell',
      target: 'neomind device list',
      success: true,
      result: '2 devices',
      resultState: 'known',
    })
  })
})
