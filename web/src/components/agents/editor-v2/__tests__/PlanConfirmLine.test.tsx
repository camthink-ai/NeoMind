import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { PlanConfirmLine } from '../PlanConfirmLine'
import { seedDailyBrief } from '../templates'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: Record<string, unknown>) => {
        const map: Record<string, string> = {
          'editorV2.planLine.label': 'Will run:',
          'editorV2.planLine.sep': ' · ',
          'editorV2.planLine.dailyAt': `daily at ${opts?.time ?? ''}`,
          'editorV2.planLine.everyMinutes': `every ${opts?.count ?? ''} min`,
          'editorV2.planLine.onEvent': 'on event',
          'editorV2.planLine.manual': 'manual only',
          'editorV2.planLine.reads': `${opts?.count ?? 0} sources`,
          'editorV2.planLine.toolsOff': 'tools off',
          'editorV2.planLine.toolsOn': 'tools open',
          'editorV2.planLine.toolsScoped': 'tools allowlist',
          'editorV2.planLine.notifyComplete': 'notify on complete',
          'editorV2.planLine.notifyFailure': 'notify on failure',
          'editorV2.planLine.notifySilent': 'silent',
        }
        return map[key] ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

describe('PlanConfirmLine', () => {
  it('renders a one-line confirmation for daily brief seed', () => {
    render(<PlanConfirmLine draft={seedDailyBrief()} />)
    const el = screen.getByTestId('plan-confirm-line')
    expect(el.textContent).toContain('Will run:')
    expect(el.textContent).toContain('daily at 09:00')
    expect(el.textContent).toContain('tools off')
    expect(el.textContent).toContain('0 sources')
  })
})
