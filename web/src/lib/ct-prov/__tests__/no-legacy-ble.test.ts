import { describe, expect, it } from 'vitest'

const LEGACY_MARKERS = [
  '0000fffe-0000-1000-8000-00805f9b34fb',
  '0000ff53-0000-1000-8000-00805f9b34fb',
  '0000ff50-0000-1000-8000-00805f9b34fb',
  'BLE_CHAR_MQTT',
  'ble-protocol',
  'useBleProvision',
]

// Eager raw sources under web/src (excludes this file's own marker strings via path filter).
const rawSources = import.meta.glob('../../**/*.{ts,tsx}', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

describe('hard cut: no legacy JSON-on-GATT / ESP BLE provision entry points', () => {
  it('web/src has no legacy BLE UUID or old hook imports', () => {
    const hits: string[] = []
    for (const [path, text] of Object.entries(rawSources)) {
      if (path.includes('no-legacy-ble.test')) continue
      for (const marker of LEGACY_MARKERS) {
        if (text.includes(marker)) {
          hits.push(`${path}: ${marker}`)
        }
      }
    }
    expect(hits, hits.join('\n')).toEqual([])
  })
})
