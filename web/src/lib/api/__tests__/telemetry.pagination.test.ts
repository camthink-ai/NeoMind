import { describe, it, expect } from 'vitest'
import { buildDeviceTelemetryQuery } from '../telemetry'

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
