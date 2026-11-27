/**
 * Derive a read-only Agent execution evidence summary (1.3 iter D).
 *
 * Reuses fields already on AgentExecutionDetail. Missing evidence is labelled
 * `unknown` — never invent timestamps, outputs, action results, or failure
 * nodes from status color alone.
 */
import type { ActionDeliveryRecord, AgentExecutionDetail, ToolCallTrace } from '@/types/agent'

export type EvidencePresence = 'known' | 'unknown' | 'empty'

export interface InputEvidenceRow {
  source: string
  dataType: string
  /** Unix ms when known; null when absent/zero. */
  timestampMs: number | null
  timeState: 'known' | 'unknown'
}

export interface ActionEvidenceRow {
  description: string
  target: string
  success: boolean
  result: string | null
  resultState: 'known' | 'unknown'
}

export type FailureKind =
  | 'error'
  | 'failed_action'
  | 'stop_reason'
  | 'none'
  | 'unknown'

export interface FailureEvidence {
  kind: FailureKind
  detail: string | null
  state: 'known' | 'unknown'
}

export interface AgentExecutionEvidence {
  inputs: InputEvidenceRow[]
  inputsPresence: EvidencePresence
  outputText: string | null
  outputPresence: EvidencePresence
  actions: ActionEvidenceRow[]
  actionsPresence: EvidencePresence
  failure: FailureEvidence
  stopReason: string | null
  stopReasonPresence: EvidencePresence
  tokensUsed: number | null
}

function normalizeTimestamp(ts: number | undefined | null): {
  timestampMs: number | null
  timeState: 'known' | 'unknown'
} {
  if (ts == null || !Number.isFinite(ts) || ts <= 0) {
    return { timestampMs: null, timeState: 'unknown' }
  }
  // Storage uses unix seconds for many paths; accept seconds or ms.
  const ms = ts < 1_000_000_000_000 ? ts * 1000 : ts
  return { timestampMs: ms, timeState: 'known' }
}

function pickOutput(detail: AgentExecutionDetail): {
  text: string | null
  presence: EvidencePresence
} {
  const conclusion = detail.decision_process?.conclusion?.trim() ?? ''
  const summary = detail.result?.summary?.trim() ?? ''
  const generic =
    summary === 'Completed tool execution rounds.' ||
    summary === 'LLM generation failed during tool execution.'

  if (conclusion) return { text: conclusion, presence: 'known' }
  if (summary && !generic) return { text: summary, presence: 'known' }
  if (detail.decision_process || detail.result) return { text: null, presence: 'empty' }
  return { text: null, presence: 'unknown' }
}

function mapDeliveryToAction(d: ActionDeliveryRecord): ActionEvidenceRow {
  const st = d.state
  const success = st.state === 'accepted' || st.state === 'confirmed'
  let result: string | null = st.state
  let resultState: 'known' | 'unknown' = 'known'
  if (st.state === 'rejected') {
    result = st.reason ? `rejected: ${st.reason}` : 'rejected'
  } else if (st.state === 'outcome_unknown') {
    result = st.reason ? `outcome_unknown: ${st.reason}` : null
    resultState = st.reason ? 'known' : 'unknown'
  }
  return {
    description: d.action_fingerprint || 'delivery',
    target: d.destination || '—',
    success,
    result,
    resultState,
  }
}

function mapToolCallToAction(tc: ToolCallTrace): ActionEvidenceRow {
  const err = tc.error?.trim()
  const preview = tc.result_preview?.trim()
  const result = err || preview || (tc.success ? 'ok' : null)
  const target =
    tc.arguments &&
    typeof tc.arguments === 'object' &&
    'command' in tc.arguments &&
    typeof (tc.arguments as Record<string, unknown>).command === 'string'
      ? ((tc.arguments as Record<string, unknown>).command as string)
      : 'tool'
  return {
    description: tc.round ? `#${tc.round} ${tc.name}` : tc.name || 'tool',
    target,
    success: Boolean(tc.success),
    result,
    resultState: result ? 'known' : 'unknown',
  }
}

function deriveFailure(detail: AgentExecutionDetail): FailureEvidence {
  if (detail.error?.trim()) {
    return { kind: 'error', detail: detail.error.trim(), state: 'known' }
  }

  const failed = (detail.result?.actions_executed ?? []).filter((a) => !a.success)
  if (failed.length > 0) {
    const first = failed[0]
    const detailText = [first.description, first.result].filter(Boolean).join(' — ')
    return {
      kind: 'failed_action',
      detail: detailText || first.target || null,
      state: 'known',
    }
  }

  const rejectedDelivery = (detail.deliveries ?? []).find(
    (d) => d.state.state === 'rejected' || d.state.state === 'outcome_unknown',
  )
  if (rejectedDelivery) {
    const st = rejectedDelivery.state
    const reason =
      st.state === 'rejected' || st.state === 'outcome_unknown' ? st.reason : ''
    const detailText = [rejectedDelivery.action_fingerprint, reason]
      .filter(Boolean)
      .join(' — ')
    return {
      kind: 'failed_action',
      detail: detailText || rejectedDelivery.destination || null,
      state: 'known',
    }
  }

  const stop = detail.decision_process?.stop_reason?.trim()
  if (stop) {
    // Stop reasons that clearly signal failure / abort
    const lower = stop.toLowerCase()
    const looksFailed =
      lower.includes('error') ||
      lower.includes('fail') ||
      lower.includes('timeout') ||
      lower.includes('abort') ||
      lower.includes('cancel')
    if (looksFailed || detail.status === 'Failed' || detail.status === 'Partial') {
      return { kind: 'stop_reason', detail: stop, state: 'known' }
    }
  }

  if (detail.status === 'Failed' || detail.status === 'Partial') {
    // Status says failed/undetermined but no error / failed action / stop — unknown node
    return { kind: 'unknown', detail: null, state: 'unknown' }
  }

  if (detail.decision_process || detail.result || detail.status === 'Completed') {
    return { kind: 'none', detail: null, state: 'known' }
  }

  return { kind: 'unknown', detail: null, state: 'unknown' }
}

/**
 * Build evidence rows from an execution detail payload.
 */
export function deriveAgentExecutionEvidence(
  detail: AgentExecutionDetail | null | undefined,
): AgentExecutionEvidence {
  if (!detail) {
    return {
      inputs: [],
      inputsPresence: 'unknown',
      outputText: null,
      outputPresence: 'unknown',
      actions: [],
      actionsPresence: 'unknown',
      failure: { kind: 'unknown', detail: null, state: 'unknown' },
      stopReason: null,
      stopReasonPresence: 'unknown',
      tokensUsed: null,
    }
  }

  const dp = detail.decision_process
  let inputs: InputEvidenceRow[] = []
  let inputsPresence: EvidencePresence = 'unknown'
  if (dp) {
    inputs = (dp.data_collected ?? []).map((d) => {
      const { timestampMs, timeState } = normalizeTimestamp(d.timestamp)
      return {
        source: d.source || '—',
        dataType: d.data_type || '—',
        timestampMs,
        timeState,
      }
    })
    inputsPresence = inputs.length > 0 ? 'known' : 'empty'
  }

  const output = pickOutput(detail)

  let actions: ActionEvidenceRow[] = []
  let actionsPresence: EvidencePresence = 'unknown'
  if (detail.result) {
    actions = (detail.result.actions_executed ?? []).map((a) => {
      const result = a.result?.trim() ? a.result.trim() : null
      return {
        description: a.description || a.action_type || '—',
        target: a.target || '—',
        success: !!a.success,
        result,
        resultState: result ? 'known' : 'unknown',
      }
    })
    actionsPresence = actions.length > 0 ? 'known' : 'empty'
  } else if (Array.isArray(detail.deliveries) || Array.isArray(detail.tool_calls)) {
    const deliveryActions = (detail.deliveries ?? []).map(mapDeliveryToAction)
    const toolActions = (detail.tool_calls ?? []).map(mapToolCallToAction)
    actions = [...deliveryActions, ...toolActions]
    actionsPresence = actions.length > 0 ? 'known' : 'empty'
  }

  const stopRaw = dp?.stop_reason?.trim() ?? ''
  const stopReasonPresence: EvidencePresence = !dp
    ? 'unknown'
    : stopRaw
      ? 'known'
      : 'empty'
  const tokens =
    dp && typeof dp.tokens_used === 'number' && dp.tokens_used > 0
      ? dp.tokens_used
      : null

  return {
    inputs,
    inputsPresence,
    outputText: output.text,
    outputPresence: output.presence,
    actions,
    actionsPresence,
    failure: deriveFailure(detail),
    stopReason: stopRaw || null,
    stopReasonPresence,
    tokensUsed: tokens,
  }
}
