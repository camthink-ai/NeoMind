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
      description: '#2 shell',
      target: 'neomind device list',
      success: true,
      result: '2 devices',
      resultState: 'known',
    })
  })

  it('combines result with V2 tool_calls (0-based round -> 1-based #1/#2) and unrepresented deliveries', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Completed',
        result: {
          actions_executed: [
            {
              action_type: 'device_command',
              description: 'restart',
              target: 'device:fan-01',
              success: true,
              result: 'Confirmed',
            },
          ],
          notifications_sent: [
            {
              channel: 'ops-webhook',
              recipient: 'ops-webhook',
              message: 'done',
              sent_at: 1_700_000_000,
              success: true,
            },
          ],
          summary: 'Checked devices and restarted fan',
          success_rate: 1,
        },
        deliveries: [
          {
            execution_id: 'e1',
            destination: 'device:fan-01',
            action_fingerprint: 'restart',
            idempotent: true,
            state: { state: 'confirmed' },
          },
          {
            execution_id: 'e1',
            destination: 'ops-webhook',
            action_fingerprint: 'notify:complete',
            idempotent: true,
            state: { state: 'accepted' },
          },
        ],
        tool_calls: [
          {
            round: 0,
            name: 'shell',
            arguments: { command: 'neomind device list' },
            success: true,
            result_preview: '1 device',
            duration_ms: 15,
          },
          {
            round: 1,
            name: 'shell',
            arguments: { command: 'neomind device status fan-01' },
            success: true,
            result_preview: 'ok',
            duration_ms: 20,
          },
        ],
      }),
    )

    expect(e.actionsPresence).toBe('known')
    // 1 action_executed + 1 notification delivery (fan-01 deduped) + 2 tool_calls
    expect(e.actions).toHaveLength(4)
    expect(e.actions[0]).toMatchObject({
      description: 'restart',
      target: 'device:fan-01',
      success: true,
    })
    expect(e.actions[1]).toMatchObject({
      description: 'notify:complete',
      target: 'ops-webhook',
      success: true,
      result: 'accepted',
    })
    expect(e.actions[2]).toMatchObject({
      description: '#1 shell',
      target: 'neomind device list',
      success: true,
    })
    expect(e.actions[3]).toMatchObject({
      description: '#2 shell',
      target: 'neomind device status fan-01',
      success: true,
    })
    expect(e.failure.kind).toBe('none')
  })

  it('treats alert-gated deliveries and actions as skipped (not failed_action)', () => {
    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Completed',
        result: {
          actions_executed: [
            {
              action_type: 'device_command',
              description: 'sound_alarm:gated',
              target: 'device:siren-01',
              success: false,
              result: 'Rejected: alert criteria not met',
            },
          ],
          notifications_sent: [],
          summary: 'Normal conditions',
          success_rate: 0,
        },
        deliveries: [
          {
            execution_id: 'e1',
            destination: 'device:siren-01',
            action_fingerprint: 'sound_alarm:gated',
            idempotent: true,
            state: { state: 'rejected', reason: 'alert criteria not met' },
          },
          {
            execution_id: 'e1',
            destination: 'ops-channel',
            action_fingerprint: 'notify_alert',
            idempotent: true,
            state: { state: 'rejected', reason: 'alert criteria not met' },
          },
        ],
      }),
    )

    expect(e.failure.kind).toBe('none')
    expect(e.actions).toHaveLength(2)
    expect(e.actions[0]).toMatchObject({
      description: 'sound_alarm:gated',
      success: true,
      skipped: true,
    })
    expect(e.actions[1]).toMatchObject({
      description: 'notify_alert',
      success: true,
      skipped: true,
      result: 'skipped: alert criteria not met',
    })
  })

  it('handles malformed persisted delivery states defensively without throwing', () => {
    const malformed = {
      execution_id: 'e1',
      destination: 'device:broken',
      action_fingerprint: 'noop',
      idempotent: true,
      state: null,
    } as unknown as NonNullable<AgentExecutionDetail['deliveries']>[number]

    const e = deriveAgentExecutionEvidence(
      base({
        status: 'Completed',
        deliveries: [malformed],
      }),
    )

    expect(e.actionsPresence).toBe('known')
    expect(e.actions[0]).toMatchObject({
      description: 'noop',
      target: 'device:broken',
      success: false,
      resultState: 'unknown',
    })
    expect(e.failure.kind).toBe('none')
  })
})
