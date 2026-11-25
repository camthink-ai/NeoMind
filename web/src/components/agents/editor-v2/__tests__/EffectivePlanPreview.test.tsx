/**
 * EffectivePlanPreview — renders mocked plan-preview response without network.
 */
import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { EffectivePlanPreview } from '../EffectivePlanPreview'
import { defaultDailyBriefDraft } from '../defaults'
import type { EffectivePlan } from '@/types'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => {
        const map: Record<string, string> = {
          'editorV2.planPreview.title': 'Effective plan',
          'editorV2.planPreview.refresh': 'Refresh',
          'editorV2.planPreview.hint': 'Static preview — zero external reads',
          'editorV2.planPreview.error': 'Preview failed',
          'editorV2.planPreview.triggerSummary': 'Trigger',
          'editorV2.planPreview.timezone': 'Timezone',
          'editorV2.planPreview.toolPolicy': 'Tools',
          'editorV2.planPreview.reads': 'Authorized reads',
          'editorV2.planPreview.noReads': 'None',
          'editorV2.planPreview.externalReads': 'External read counts',
          'editorV2.planPreview.warnings': 'Warnings',
          'editorV2.planPreview.empty': 'No preview yet',
          'editorV2.tools.disabled': 'Disabled',
          'editorV2.tools.unrestricted': 'Unrestricted',
          'editorV2.tools.scoped': 'Scoped',
        }
        return map[key] ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

const mockPlan: EffectivePlan = {
  agent_id: 'preview',
  config_version: 0,
  trigger_summary: 'cron 0 9 * * *',
  timezone: 'Asia/Shanghai',
  authorized_reads: [
    {
      key: 'weather_now',
      source_id: 'weather_cn',
      operation: 'current',
      auth_scope: 'extension:weather:read',
      capability_class: 'extension_read',
    },
    {
      key: 'tariff_today',
      source_id: 'tariff_tou',
      operation: 'tou_today',
      auth_scope: 'extension:tariff:read',
      capability_class: 'extension_read',
    },
  ],
  tool_policy: 'disabled',
  notify: {
    channels: ['test-channel'],
    on_complete: true,
    on_alert: false,
    on_failure: true,
  },
  controlled_actions: [],
  external_read_counts: { weather: 0, tariff: 0, other: 0 },
}

describe('EffectivePlanPreview', () => {
  it('calls previewFn and shows timezone + authorized reads', async () => {
    const previewFn = vi.fn(async () => ({
      effective_plan: mockPlan,
      dry_run: false,
    }))

    render(
      <EffectivePlanPreview
        draft={defaultDailyBriefDraft()}
        previewFn={previewFn}
        debounceMs={1}
      />,
    )

    await waitFor(() => {
      expect(screen.getByTestId('plan-preview-body')).toBeTruthy()
    })
    expect(previewFn).toHaveBeenCalled()
    expect(screen.getByText('Asia/Shanghai')).toBeTruthy()
    expect(screen.getByText(/weather_now/)).toBeTruthy()
    expect(screen.getByText(/tariff_today/)).toBeTruthy()
    expect(screen.getByText(/weather=0/)).toBeTruthy()
  })

  it('surfaces preview errors', async () => {
    const previewFn = vi.fn(async () => {
      throw new Error('boom')
    })

    render(
      <EffectivePlanPreview
        draft={defaultDailyBriefDraft()}
        previewFn={previewFn}
        debounceMs={1}
      />,
    )

    await waitFor(() => {
      expect(screen.getByTestId('plan-preview-error')).toBeTruthy()
    })
    expect(screen.getByText('boom')).toBeTruthy()
  })
})
