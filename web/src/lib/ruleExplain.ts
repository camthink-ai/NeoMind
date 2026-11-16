/**
 * Derive "why not triggered" blockers from a RuleExplanation snapshot
 * (1.3 iter D). Ordering matches the engine gate: enabled → cooldown →
 * condition → for_duration. UI must not invent a green "would fire" when
 * any gate fails; `ready` only means gates are clear — actual fire still
 * depends on the trigger path (data_change / schedule / manual).
 */
import type {
  ConditionExplanation,
  EvalOutcome,
  RuleExplanation,
} from '@/types/rule'

export type RuleBlockCode =
  | 'disabled'
  | 'cooldown'
  | 'condition_false'
  | 'condition_unknown'
  | 'for_duration'
  | 'ready'

export interface RuleBlockReason {
  code: RuleBlockCode
  /** Optional machine detail (remaining secs, unknown_reason, progress). */
  detail?: string
}

export interface RuleBlockingSummary {
  /** True when every engine gate would pass right now. */
  wouldEvaluateTrue: boolean
  primary: RuleBlockReason
  reasons: RuleBlockReason[]
}

/** Flatten comparison/range leaves (skip logical wrappers). */
export function flattenConditionLeaves(
  node: ConditionExplanation | undefined | null,
): ConditionExplanation[] {
  if (!node) return []
  if (node.kind === 'logical' && node.conditions?.length) {
    return node.conditions.flatMap((c) => flattenConditionLeaves(c))
  }
  return [node]
}

function firstUnknownReason(
  node: ConditionExplanation | undefined,
): string | undefined {
  const leaf = flattenConditionLeaves(node).find((l) => l.result === 'unknown')
  return leaf?.unknown_reason
}

function conditionBlocks(
  met: EvalOutcome | undefined,
  condition: ConditionExplanation | undefined,
): RuleBlockReason[] {
  if (met === 'false') return [{ code: 'condition_false' }]
  if (met === 'unknown') {
    return [
      {
        code: 'condition_unknown',
        detail: firstUnknownReason(condition),
      },
    ]
  }
  return []
}

/**
 * Collect blockers in engine order. When none apply, primary is `ready`.
 */
export function deriveRuleBlockingReasons(
  exp: RuleExplanation,
): RuleBlockingSummary {
  const reasons: RuleBlockReason[] = []

  if (!exp.enabled) {
    reasons.push({ code: 'disabled' })
  }

  if (exp.cooldown.in_cooldown) {
    reasons.push({
      code: 'cooldown',
      detail:
        exp.cooldown.remaining_secs != null
          ? String(exp.cooldown.remaining_secs)
          : undefined,
    })
  }

  reasons.push(...conditionBlocks(exp.condition_met, exp.condition))

  // Manual triggers skip the hold gate — ignore non-applicable for_duration.
  const forDurationEnforced =
    !!exp.for_duration && exp.for_duration.applicable !== false
  if (forDurationEnforced && !exp.for_duration!.satisfied) {
    const elapsed = exp.for_duration!.elapsed_secs
    const configured = exp.for_duration!.configured_secs
    reasons.push({
      code: 'for_duration',
      detail:
        elapsed != null ? `${elapsed}/${configured}` : String(configured),
    })
  }

  const wouldEvaluateTrue =
    exp.enabled &&
    !exp.cooldown.in_cooldown &&
    (exp.condition_met === undefined || exp.condition_met === 'true') &&
    (!forDurationEnforced || exp.for_duration!.satisfied)

  if (reasons.length === 0) {
    reasons.push({ code: 'ready' })
  }

  return {
    wouldEvaluateTrue,
    primary: reasons[0],
    reasons,
  }
}
