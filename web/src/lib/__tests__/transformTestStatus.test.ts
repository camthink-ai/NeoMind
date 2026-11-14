import { describe, expect, it } from 'vitest'
import { deriveTransformTestStatus } from '../transformTestStatus'

describe('deriveTransformTestStatus', () => {
  it('verified_success only when metrics and no warnings/unverified', () => {
    const s = deriveTransformTestStatus({ count: 2, warnings: [], unverified: [] })
    expect(s.verdict).toBe('verified_success')
    expect(s.allowGreenSuccess).toBe(true)
  })

  it('unverified beats green even when metrics exist', () => {
    const s = deriveTransformTestStatus({
      count: 1,
      warnings: [],
      unverified: ['extension invocation blocked in dry-run'],
    })
    expect(s.verdict).toBe('unverified')
    expect(s.allowGreenSuccess).toBe(false)
  })

  it('partial_success when metrics + warnings', () => {
    const s = deriveTransformTestStatus({
      count: 2,
      warnings: ['sub-operation failed: boom'],
      unverified: [],
    })
    expect(s.verdict).toBe('partial_success')
    expect(s.allowGreenSuccess).toBe(false)
  })

  it('failed when JS throw is reported via warnings and zero metrics', () => {
    const s = deriveTransformTestStatus({
      count: 0,
      warnings: ["Transform 'x' failed: Error: boom"],
      unverified: [],
    })
    expect(s.verdict).toBe('failed')
  })

  it('no_output when zero metrics without failure language', () => {
    const s = deriveTransformTestStatus({
      count: 0,
      warnings: [],
      unverified: [],
    })
    expect(s.verdict).toBe('no_output')
    expect(s.allowGreenSuccess).toBe(false)
  })

  it('codeSuccess false is hard failed', () => {
    const s = deriveTransformTestStatus({
      count: 3,
      codeSuccess: false,
      warnings: [],
      unverified: [],
    })
    expect(s.verdict).toBe('failed')
    expect(s.allowGreenSuccess).toBe(false)
  })

  it('unverified still wins over codeSuccess true with metrics', () => {
    const s = deriveTransformTestStatus({
      count: 1,
      codeSuccess: true,
      unverified: ['blocked'],
      warnings: [],
    })
    expect(s.verdict).toBe('unverified')
  })
})
