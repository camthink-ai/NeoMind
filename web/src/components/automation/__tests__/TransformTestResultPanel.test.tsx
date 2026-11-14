import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { TransformTestResultPanel } from '../TransformTestResultPanel'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, fallback?: string | { defaultValue?: string }) =>
        typeof fallback === 'string'
          ? fallback
          : (fallback?.defaultValue ?? key),
      i18n: { language: 'en' },
    }),
  }
})

describe('TransformTestResultPanel', () => {
  it('does not use green success when unverified is non-empty', () => {
    const { container } = render(
      <TransformTestResultPanel
        count={1}
        metrics={[
          {
            device_id: 'd1',
            metric: 'avg',
            value: 1,
            timestamp: 1_700_000_000,
            quality: null,
          },
        ]}
        unverified={['extension invocation blocked in dry-run']}
        warnings={[]}
      />,
    )
    expect(container.querySelector('[data-verdict="unverified"]')).toBeTruthy()
    expect(screen.getByText(/Dry-run blocked/i)).toBeTruthy()
    expect(screen.getByText(/Unverified dependencies/i)).toBeTruthy()
    // Metrics still visible alongside unverified
    expect(screen.getByText('avg')).toBeTruthy()
  })

  it('marks verified_success only for clean metric runs', () => {
    const { container } = render(
      <TransformTestResultPanel
        count={1}
        metrics={[
          {
            device_id: 'd1',
            metric: 'v',
            value: 2,
            timestamp: 1_700_000_000,
            quality: 1,
          },
        ]}
        unverified={[]}
        warnings={[]}
      />,
    )
    expect(container.querySelector('[data-verdict="verified_success"]')).toBeTruthy()
  })

  it('surfaces partial_success with expandable warnings', () => {
    const { container } = render(
      <TransformTestResultPanel
        count={2}
        metrics={[
          {
            device_id: 'd1',
            metric: 'a',
            value: 1,
            timestamp: 1,
            quality: null,
          },
        ]}
        warnings={['sub-operation failed: boom']}
        unverified={[]}
      />,
    )
    expect(container.querySelector('[data-verdict="partial_success"]')).toBeTruthy()
    expect(screen.getByText(/sub-operation failed/)).toBeTruthy()
  })
})
