import { describe, it, expect } from 'vitest'
import { normalizeAllowedTypes } from '../categories'

describe('normalizeAllowedTypes', () => {
  it('keeps ai when the widget explicitly allows it', () => {
    const out = normalizeAllowedTypes(['device-metric', 'transform', 'ai'])
    expect(out).toContain('ai')
  })

  it('does not invent ai when the widget did not allow it', () => {
    const out = normalizeAllowedTypes(['device-metric'])
    expect(out).not.toContain('ai')
  })
})
