import { describe, it, expect } from 'vitest'
import { selectImageRawForCache } from '../fetch'

describe('selectImageRawForCache', () => {
  it('keeps full URL history (PIR image_data) — not truncated to 5', () => {
    const points = Array.from({ length: 100 }, (_, i) => ({
      timestamp: 1_700_000_000 + i,
      value: `/api/images/200366/image_data/${i}.jpg`,
    }))
    expect(selectImageRawForCache(points)).toHaveLength(100)
  })

  it('truncates heavy base64 payloads to newest 5', () => {
    const heavy = 'data:image/jpeg;base64,' + 'A'.repeat(10_000)
    const points = Array.from({ length: 20 }, (_, i) => ({
      timestamp: 1_700_000_000 + i,
      value: heavy,
    }))
    expect(selectImageRawForCache(points)).toHaveLength(5)
  })
})
