import { describe, it, expect } from 'vitest'
import { hasOutputContract } from '../validation'
import type { OperatorField } from '@/types'

const field = (name: string): OperatorField => ({ name, field_type: { type: 'number' } })

describe('hasOutputContract', () => {
  it('is false with no fields — the executor rejects every run without one', () => {
    expect(hasOutputContract([])).toBe(false)
  })

  it('is false when every field is still unnamed', () => {
    expect(hasOutputContract([{ name: '', field_type: { type: 'number' } }])).toBe(false)
  })

  it('ignores blank rows mixed in with named ones', () => {
    expect(hasOutputContract([field('missing_count'), { name: '  ', field_type: { type: 'text' } }])).toBe(true)
  })

  it('is true once one field is named', () => {
    expect(hasOutputContract([field('missing_count')])).toBe(true)
  })
})
