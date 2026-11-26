/**
 * SimpleOutputSection — toggle writes draft.output.fields.
 */
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { SimpleOutputSection } from '../SimpleOutputSection'
import type { OutputContract } from '@/types'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => {
        const map: Record<string, string> = {
          'editorV2.output.enabled': 'JSON fields',
          'editorV2.output.fields': 'Field names',
          'editorV2.output.add': 'Add',
          'editorV2.output.remove': 'Remove field',
          'editorV2.output.namePlaceholder': 'e.g. status',
        }
        return map[key] ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

describe('SimpleOutputSection', () => {
  it('enables with a default field name', () => {
    const onChange = vi.fn()
    render(<SimpleOutputSection value={{ fields: [], publish: false }} onChange={onChange} />)
    fireEvent.click(screen.getByLabelText('JSON fields'))
    expect(onChange).toHaveBeenCalledOnce()
    const next = onChange.mock.calls[0][0] as OutputContract
    expect(next.fields).toHaveLength(1)
    expect(next.fields[0].name).toBe('result')
  })

  it('disables by clearing fields', () => {
    const onChange = vi.fn()
    render(
      <SimpleOutputSection
        value={{ fields: [{ name: 'brief', depends_on: [] }], publish: false }}
        onChange={onChange}
      />,
    )
    fireEvent.click(screen.getByLabelText('JSON fields'))
    expect(onChange).toHaveBeenCalledOnce()
    expect((onChange.mock.calls[0][0] as OutputContract).fields).toEqual([])
  })
})
