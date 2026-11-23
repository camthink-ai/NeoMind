import { describe, it, expect } from 'vitest'
import {
  IMAGE_HISTORY_DEFAULT_FIT,
  imageHistoryFrameClass,
  imageHistoryObjectFitClass,
} from '../ImageHistory'

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
