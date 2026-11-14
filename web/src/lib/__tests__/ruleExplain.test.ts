import { describe, expect, it } from 'vitest'
import {
  deriveRuleBlockingReasons,
  flattenConditionLeaves,
} from '../ruleExplain'
import type { RuleExplanation } from '@/types/rule'

function base(over: Partial<RuleExplanation> = {}): RuleExplanation {
  return {
    rule_id: 'r1',
    rule_name: 'demo',
    enabled: true,
    snapshot_at: '2026-11-14T00:00:00Z',
    dsl_preview: 'x > 1',
    condition_met: 'true',
    cooldown: { configured_secs: 60, in_cooldown: false },
    trigger_count: 0,
    actions: [],
    notes: [],
    ...over,
  }
}

describe('deriveRuleBlockingReasons', () => {
  it('primary disabled before other gates', () => {
    const s = deriveRuleBlockingReasons(
      base({
        enabled: false,
        cooldown: { configured_secs: 60, in_cooldown: true, remaining_secs: 12 },
        condition_met: 'false',
      }),
    )
    expect(s.primary.code).toBe('disabled')
    expect(s.wouldEvaluateTrue).toBe(false)
    expect(s.reasons.map((r) => r.code)).toEqual([
      'disabled',
      'cooldown',
      'condition_false',
    ])
  })

  it('cooldown blocks when condition is true', () => {
    const s = deriveRuleBlockingReasons(
      base({
        cooldown: { configured_secs: 60, in_cooldown: true, remaining_secs: 9 },
      }),
    )
    expect(s.primary.code).toBe('cooldown')
    expect(s.primary.detail).toBe('9')
    expect(s.wouldEvaluateTrue).toBe(false)
  })

  it('condition_false when measured false', () => {
    const s = deriveRuleBlockingReasons(base({ condition_met: 'false' }))
    expect(s.primary.code).toBe('condition_false')
    expect(s.wouldEvaluateTrue).toBe(false)
  })

  it('condition_unknown carries leaf reason', () => {
    const s = deriveRuleBlockingReasons(
      base({
        condition_met: 'unknown',
        condition: {
          kind: 'comparison',
          result: 'unknown',
          source: 'device:s1:temp',
          unknown_reason: 'no_value_cached',
        },
      }),
    )
    expect(s.primary.code).toBe('condition_unknown')
    expect(s.primary.detail).toBe('no_value_cached')
  })

  it('for_duration blocks when not satisfied', () => {
    const s = deriveRuleBlockingReasons(
      base({
        for_duration: {
          configured_secs: 30,
          elapsed_secs: 10,
          satisfied: false,
        },
      }),
    )
    expect(s.primary.code).toBe('for_duration')
    expect(s.primary.detail).toBe('10/30')
    expect(s.wouldEvaluateTrue).toBe(false)
  })

  it('ready when all gates clear (no condition counts as met)', () => {
    const s = deriveRuleBlockingReasons(
      base({ condition_met: undefined, for_duration: undefined }),
    )
    expect(s.primary.code).toBe('ready')
    expect(s.wouldEvaluateTrue).toBe(true)
    expect(s.reasons).toHaveLength(1)
  })

  it('ready when condition true and duration satisfied', () => {
    const s = deriveRuleBlockingReasons(
      base({
        for_duration: {
          configured_secs: 30,
          elapsed_secs: 30,
          satisfied: true,
        },
      }),
    )
    expect(s.primary.code).toBe('ready')
    expect(s.wouldEvaluateTrue).toBe(true)
  })
})

describe('flattenConditionLeaves', () => {
  it('flattens logical children', () => {
    const leaves = flattenConditionLeaves({
      kind: 'logical',
      result: 'false',
      conditions: [
        { kind: 'comparison', result: 'true', source: 'a:b:c', value: 1 },
        {
          kind: 'logical',
          result: 'unknown',
          conditions: [
            {
              kind: 'range',
              result: 'unknown',
              source: 'd:e:f',
              unknown_reason: 'no_value_cached',
            },
          ],
        },
      ],
    })
    expect(leaves).toHaveLength(2)
    expect(leaves[0].source).toBe('a:b:c')
    expect(leaves[1].unknown_reason).toBe('no_value_cached')
  })
})
