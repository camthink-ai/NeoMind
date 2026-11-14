import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { RuleWhyNotTriggeredPanel } from '../RuleWhyNotTriggeredPanel'
import type { RuleExplanation } from '@/types/rule'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: string | { defaultValue?: string; reason?: string; secs?: string | number; progress?: string; time?: string; elapsed?: number; configured?: number; mins?: number; hours?: number }) => {
        if (typeof opts === 'string') return opts
        if (opts?.defaultValue) return opts.defaultValue
        const k = key.replace(/^automation:/, '')
        if (k === 'ruleExplain.block.disabled') return 'Rule is disabled'
        if (k === 'ruleExplain.block.condition_unknown' && opts?.reason) {
          return `Condition unknown (${opts.reason})`
        }
        if (k === 'ruleExplain.unknownReason.no_value_cached') return 'No value cached'
        if (k === 'ruleExplain.block.ready') return 'Ready to fire on next qualifying trigger'
        if (k === 'ruleExplain.wouldFireHint') {
          return 'Gates are clear now; actual fire still depends on the trigger path'
        }
        if (k === 'ruleExplain.loading') return 'Loading current state…'
        if (k === 'ruleExplain.loadFailed') return 'Could not load explanation'
        if (k === 'ruleExplain.title') return 'Why not triggered'
        if (k === 'ruleExplain.conditionTree') return 'Live condition'
        if (k === 'ruleExplain.source') return 'Source'
        if (k === 'ruleExplain.value') return 'Value'
        if (k === 'ruleExplain.age') return 'Age'
        if (k === 'ruleExplain.result') return 'Result'
        if (k === 'ruleExplain.outcome.unknown') return 'Unknown'
        if (k === 'ruleExplain.outcome.true') return 'True'
        if (k === 'ruleExplain.ageSecs' && opts?.secs != null) return `${opts.secs}s`
        if (k === 'ruleExplain.cooldown') return 'Cooldown'
        if (k === 'ruleExplain.notInCooldown') return 'Not in cooldown'
        if (k === 'ruleExplain.lastExecution') return 'Last execution'
        if (k === 'ruleExplain.noLastExecution') return 'No persisted execution yet'
        if (k === 'ruleExplain.snapshotAt' && opts?.time) return `Snapshot at ${opts.time}`
        if (k === 'ruleExplain.conditionMet') return 'Overall'
        return key
      },
      i18n: { language: 'en' },
    }),
  }
})

function expl(over: Partial<RuleExplanation> = {}): RuleExplanation {
  return {
    rule_id: 'r1',
    rule_name: 'demo',
    enabled: true,
    snapshot_at: '2026-11-14T12:00:00.000Z',
    dsl_preview: 'temp > 30',
    condition_met: 'true',
    cooldown: { configured_secs: 60, in_cooldown: false },
    trigger_count: 0,
    actions: [],
    notes: [],
    ...over,
  }
}

describe('RuleWhyNotTriggeredPanel', () => {
  it('shows loading state', () => {
    render(<RuleWhyNotTriggeredPanel explanation={null} status="loading" />)
    expect(screen.getByText('Loading current state…')).toBeTruthy()
  })

  it('surfaces disabled as primary blocker', () => {
    const { container } = render(
      <RuleWhyNotTriggeredPanel
        status="ready"
        explanation={expl({ enabled: false, condition_met: 'true' })}
      />,
    )
    expect(container.querySelector('[data-block="disabled"]')).toBeTruthy()
    expect(screen.getByText('Rule is disabled')).toBeTruthy()
  })

  it('shows unknown leaf with reason and never claims would-evaluate', () => {
    const { container } = render(
      <RuleWhyNotTriggeredPanel
        status="ready"
        explanation={expl({
          condition_met: 'unknown',
          condition: {
            kind: 'comparison',
            result: 'unknown',
            source: 'device:cam1:people',
            unknown_reason: 'no_value_cached',
          },
        })}
      />,
    )
    expect(container.querySelector('[data-block="condition_unknown"]')).toBeTruthy()
    expect(container.querySelector('[data-would-evaluate="false"]')).toBeTruthy()
    expect(screen.getByText('device:cam1:people')).toBeTruthy()
    expect(screen.getByText('No value cached')).toBeTruthy()
  })

  it('ready shows trigger-path hint, not silent green fire claim', () => {
    const { container } = render(
      <RuleWhyNotTriggeredPanel status="ready" explanation={expl()} />,
    )
    expect(container.querySelector('[data-block="ready"]')).toBeTruthy()
    expect(container.querySelector('[data-would-evaluate="true"]')).toBeTruthy()
    expect(screen.getByText(/trigger path/i)).toBeTruthy()
  })
})
