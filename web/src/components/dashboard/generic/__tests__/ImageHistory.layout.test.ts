import { describe, it, expect } from 'vitest'
import {
  IMAGE_HISTORY_DEFAULT_FIT,
  imageHistoryFrameClass,
  imageHistoryObjectFitClass,
  normalizeDataSourceForImages,
} from '../ImageHistory'
import type { DataSource } from '@/types/dashboard'

describe('ImageHistory layout / fit helpers', () => {
  it('defaults to contain (not fill) for new widgets', () => {
    expect(IMAGE_HISTORY_DEFAULT_FIT).toBe('contain')
    expect(imageHistoryObjectFitClass()).toBe('object-contain')
    expect(imageHistoryObjectFitClass(undefined)).toBe('object-contain')
  })

  it('honors explicit fill for stored PIR configs without remapping', () => {
    expect(imageHistoryObjectFitClass('fill')).toBe('object-fill')
    expect(imageHistoryObjectFitClass('cover')).toBe('object-cover')
  })

  it('uses min-height + flex fill instead of fixed h-[180px]', () => {
    const md = imageHistoryFrameClass('md')
    expect(md).toContain('min-h-[180px]')
    expect(md).toContain('flex-1')
    expect(md).not.toMatch(/\bh-\[180px\]\b/)
    expect(imageHistoryFrameClass('sm')).toContain('min-h-[120px]')
    expect(imageHistoryFrameClass('lg')).toContain('min-h-[240px]')
  })
})

describe('normalizeDataSourceForImages', () => {
  it('clears persisted last_1hour and applies widget limit/timeRange', () => {
    const ds = {
      type: 'telemetry',
      sourceId: '200366',
      metricId: 'image_data',
      timeRange: 1,
      limit: 50,
      timeWindow: { type: 'last_1hour' },
      params: { includeRawPoints: true },
      transform: 'raw',
    } as DataSource

    const norm = normalizeDataSourceForImages(ds, 100, 48)
    expect(norm?.timeWindow).toBeUndefined()
    expect(norm?.limit).toBe(100)
    expect(norm?.timeRange).toBe(48)
    expect(norm?.params?.isImage).toBe(true)
    expect(norm?.params?.includeRawPoints).toBe(true)
  })

  it('does not invent a 1h/3-point quick window (regression)', () => {
    const ds = {
      type: 'telemetry',
      sourceId: '200366',
      metricId: 'image_data',
    } as DataSource
    const norm = normalizeDataSourceForImages(ds, 100, 48)
    expect(norm?.limit).toBe(100)
    expect(norm?.timeRange).toBe(48)
    expect(norm?.limit).not.toBe(3)
    expect(norm?.timeRange).not.toBe(1)
  })
})
