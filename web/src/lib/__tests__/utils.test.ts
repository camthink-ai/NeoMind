/// Tests for utility functions
import { describe, it, expect } from 'vitest'
import { cn } from '../utils'

describe('cn utility function', () => {
  it('should merge class names correctly', () => {
    expect(cn('foo', 'bar')).toBe('foo bar')
  })

  it('should handle empty inputs', () => {
    expect(cn()).toBe('')
  })

  it('should handle conditional classes', () => {
    expect(cn('base', false && 'hidden', 'active')).toBe('base active')
  })

  it('should merge conflicting Tailwind classes', () => {
    // tailwind-merge ensures later classes override earlier ones
    expect(cn('p-4', 'p-2')).toBe('p-2')
  })

  it('should handle arrays of classes', () => {
    expect(cn(['foo', 'bar'], 'baz')).toBe('foo bar baz')
  })

  it('should handle undefined and null values', () => {
    expect(cn('foo', undefined, null, 'bar')).toBe('foo bar')
  })

  // REGRESSION GUARD: tailwind-merge used to treat the custom fontSize
  // utilities (text-micro/nano/mini/code/body/heading) as textColor classes
  // and silently dropped the SIZE when a color class was present — every
  // paired label app-wide rendered at the inherited default size. If this
  // test fails, the extendTailwindMerge font-size registration in
  // lib/utils.ts broke.
  describe('custom font-size tokens coexist with color classes', () => {
    const sizes = ['text-micro', 'text-nano', 'text-mini', 'text-code', 'text-body', 'text-heading']
    for (const size of sizes) {
      it(`keeps ${size} next to a text color`, () => {
        expect(cn(size, 'text-muted-foreground')).toContain(size)
        expect(cn(size, 'text-muted-foreground')).toContain('text-muted-foreground')
      })
    }
    it('latest size wins on conflict (standard tailwind-merge semantics)', () => {
      expect(cn('text-mini', 'text-nano')).toBe('text-nano')
    })
  })
})
