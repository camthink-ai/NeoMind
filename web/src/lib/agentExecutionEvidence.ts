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
  /** True when an action/delivery was skipped by an alert gate (non-failure). */
  skipped?: boolean
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

function getDeliveryStateName(d: ActionDeliveryRecord | null | undefined): string | undefined {
  return d && d.state && typeof d.state === 'object' ? d.state.state : undefined
}

function getDeliveryStateReason(d: ActionDeliveryRecord | null | undefined): string {
  if (!d || !d.state || typeof d.state !== 'object') return ''
  return 'reason' in d.state && typeof d.state.reason === 'string' ? d.state.reason : ''
}

function isAlertGatedDelivery(d: ActionDeliveryRecord | null | undefined): boolean {
  if (!d || getDeliveryStateName(d) !== 'rejected') return false
  const reason = getDeliveryStateReason(d).toLowerCase()
  const fp = typeof d.action_fingerprint === 'string' ? d.action_fingerprint : ''
  return reason.includes('alert criteria not met') || fp.endsWith(':gated')
}

function isAlertGatedAction(a: { description?: string; result?: string | null }): boolean {
  const resultLower = (a.result ?? '').toLowerCase()
  const desc = a.description ?? ''
  return resultLower.includes('alert criteria not met') || desc.endsWith(':gated')
}

function mapDeliveryToAction(d: ActionDeliveryRecord): ActionEvidenceRow {
  const stateName = getDeliveryStateName(d)
  const reason = getDeliveryStateReason(d)
  if (isAlertGatedDelivery(d)) {
    return {
      description: d.action_fingerprint || 'delivery',
      target: d.destination || '—',
      success: true,
      skipped: true,
      result: reason ? `skipped: ${reason}` : 'skipped',
      resultState: 'known',
    }
  }
  const success = stateName === 'accepted' || stateName === 'confirmed'
  let result: string | null = stateName ?? null
  let resultState: 'known' | 'unknown' = stateName ? 'known' : 'unknown'
  if (stateName === 'rejected') {
    result = reason ? `rejected: ${reason}` : 'rejected'
    resultState = 'known'
  } else if (stateName === 'outcome_unknown') {
    result = reason ? `outcome_unknown: ${reason}` : null
    resultState = reason ? 'known' : 'unknown'
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
  const label = tc.name || 'tool'
  return {
    description:
      typeof tc.round === 'number' && Number.isFinite(tc.round)
        ? `#${tc.round + 1} ${label}`
        : label,
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

  const failed = (detail.result?.actions_executed ?? []).filter(
    (a) => !a.success && !isAlertGatedAction(a),
  )
  if (failed.length > 0) {
    const first = failed[0]
    const detailText = [first.description, first.result].filter(Boolean).join(' — ')
    return {
      kind: 'failed_action',
      detail: detailText || first.target || null,
      state: 'known',
    }
  }

  const rejectedDelivery = (detail.deliveries ?? []).find((d) => {
    const stateName = getDeliveryStateName(d)
    if (stateName === 'outcome_unknown') return true
    if (stateName === 'rejected') return !isAlertGatedDelivery(d)
    return false
  })
  if (rejectedDelivery) {
    const reason = getDeliveryStateReason(rejectedDelivery)
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

  const actions: ActionEvidenceRow[] = []
  const hasActionSource =
    Boolean(detail.result) ||
    Array.isArray(detail.deliveries) ||
    Array.isArray(detail.tool_calls)

  const representedDeliveryKeys = new Set<string>()
  const v2DeliveryDestinations = new Set<string>(
    (detail.deliveries ?? [])
      .map((d) => d?.destination)
      .filter((dest): dest is string => typeof dest === 'string' && dest.length > 0),
  )

  if (detail.result) {
    for (const a of detail.result.actions_executed ?? []) {
      const gated = isAlertGatedAction(a)
      const trimmedResult = a.result?.trim() ? a.result.trim() : null
      const result = gated ? trimmedResult || 'skipped' : trimmedResult
      const description = a.description || a.action_type || '—'
      const target = a.target || '—'
      actions.push({
        description,
        target,
        success: gated ? true : !!a.success,
        ...(gated ? { skipped: true } : {}),
        result,
        resultState: result ? 'known' : 'unknown',
      })
      if (a.target) {
        representedDeliveryKeys.add(`${a.target}\0${a.description ?? ''}`)
      }
    }

    for (const n of detail.result.notifications_sent ?? []) {
      const target = n.channel || n.recipient || '—'
      // If V2 deliveries already carry a receipt for this notification destination,
      // prefer the V2 delivery record (preserves fingerprint, state, and alert gating).
      if (v2DeliveryDestinations.has(target)) continue
      const msg = n.message?.trim()
      actions.push({
        description: msg ? `notify: ${msg.slice(0, 80)}` : 'notify',
        target,
        success: Boolean(n.success),
        result: n.success ? 'sent' : 'failed',
        resultState: 'known',
      })
    }
  }

  if (Array.isArray(detail.deliveries)) {
    for (const d of detail.deliveries) {
      if (!d || typeof d !== 'object') continue
      const key = `${d.destination ?? ''}\0${d.action_fingerprint ?? ''}`
      if (representedDeliveryKeys.has(key)) continue
      actions.push(mapDeliveryToAction(d))
    }
  }

  if (Array.isArray(detail.tool_calls)) {
    for (const tc of detail.tool_calls) {
      if (!tc || typeof tc !== 'object') continue
      actions.push(mapToolCallToAction(tc))
    }
  }

  const actionsPresence: EvidencePresence = !hasActionSource
    ? 'unknown'
    : actions.length > 0
      ? 'known'
      : 'empty'

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
