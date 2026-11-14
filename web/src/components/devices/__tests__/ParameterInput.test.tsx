/**
 * ParameterInput interaction — Length caps must use Unicode scalars, not
 * native maxLength (UTF-16), so five emoji with max=5 are accepted.
 */
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import type { ParameterDefinition } from '@/types/device'
import { ParameterInput } from '../ParameterInput'
import { unicodeScalarLength } from '../parameterOptions'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k, i18n: {} }),
}))

describe('ParameterInput Length (Unicode scalars)', () => {
  it('accepts five emoji under max=5 (native maxlength would stop at two)', () => {
    const onChange = vi.fn()
    const param: ParameterDefinition = {
      name: 'label',
      data_type: 'string',
      validation: [{ type: 'length', min: 1, max: 5, error_message: 'max 5' }],
    }
    render(<ParameterInput param={param} value="" onChange={onChange} />)
    const input = screen.getByRole('textbox') as HTMLInputElement
    // No scalar-as-UTF-16 maxlength attribute (would block at 2 emoji).
    expect(input.getAttribute('maxLength')).toBeNull()
    const fiveEmoji = '😀😀😀😀😀'
    expect(unicodeScalarLength(fiveEmoji)).toBe(5)
    expect(fiveEmoji.length).toBe(10) // UTF-16 units
    fireEvent.change(input, { target: { value: fiveEmoji } })
    expect(onChange).toHaveBeenCalledWith(fiveEmoji)
  })

  it('truncates a sixth emoji by Unicode scalar count', () => {
    const onChange = vi.fn()
    const param: ParameterDefinition = {
      name: 'label',
      data_type: 'string',
      validation: [{ type: 'length', min: 1, max: 5, error_message: 'max 5' }],
    }
    render(<ParameterInput param={param} value="" onChange={onChange} />)
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '😀😀😀😀😀😀' },
    })
    expect(onChange).toHaveBeenCalledWith('😀😀😀😀😀')
  })
})
