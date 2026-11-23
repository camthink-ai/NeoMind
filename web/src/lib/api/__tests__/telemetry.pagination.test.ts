import { describe, it, expect } from 'vitest'
import { buildDeviceTelemetryQuery, resolveTelemetryPageTotal } from '../telemetry'

describe('device telemetry pagination query', () => {
  it('sends offset for page jumps when no cursor', () => {
    const q = buildDeviceTelemetryQuery({
      metric: 'image_data',
      limit: 10,
      offset: 110,
    })
    expect(q.get('offset')).toBe('110')
    expect(q.get('cursor')).toBeNull()
  })

  it('prefers cursor over offset for sequential next', () => {
    const q = buildDeviceTelemetryQuery({
      metric: 'image_data',
      limit: 10,
      offset: 110,
      cursor: 1_700_000_500,
    })
    expect(q.get('cursor')).toBe('1700000500')
    expect(q.get('offset')).toBeNull()
  })
})

describe('device telemetry page total', () => {
  const pageSize = 10

  it('keeps the whole-window total on offset pages (page 1 included)', () => {
    expect(resolveTelemetryPageTotal({ offset: 0, limit: 10, total: 100 }, 1, pageSize)).toBe(100)
    expect(resolveTelemetryPageTotal({ offset: 30, limit: 10, total: 100 }, 4, pageSize)).toBe(100)
  })

  it('adds back the already-paged points on cursor pages', () => {
    // 100 points; "next" from page 1 lands on a cursor page that reports
    // only the 90 points behind the cursor.
    expect(
      resolveTelemetryPageTotal({ offset: 0, limit: 10, total: 90, next_cursor: 1_700_000_100 }, 2, pageSize),
    ).toBe(100)
    // Last full page: the pager must keep all 10 pages, not collapse to one.
    expect(resolveTelemetryPageTotal({ offset: 0, limit: 10, total: 10, next_cursor: null }, 10, pageSize)).toBe(100)
    // Last partial page of a 95-point series.
    expect(resolveTelemetryPageTotal({ offset: 0, limit: 10, total: 5, next_cursor: null }, 10, pageSize)).toBe(95)
  })

  it('is undefined without a pagination block', () => {
    expect(resolveTelemetryPageTotal(undefined, 2, pageSize)).toBeUndefined()
  })
})
