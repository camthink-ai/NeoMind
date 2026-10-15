import { describe, it, expect } from 'vitest'
import { normalizeTimestampMs, formatTimestamp } from '../utils/format'
import { mergeMessagesForDisplay } from '../messageUtils'
import type { Message } from '@/types'

describe('normalizeTimestampMs', () => {
  it('converts server seconds to ms', () => {
    // 2026-10-15T02:50:00Z in SECONDS — the server history unit.
    const seconds = 1792047000
    expect(normalizeTimestampMs(seconds)).toBe(seconds * 1000)
  })

  it('keeps ms timestamps as-is', () => {
    const ms = 1792047000123
    expect(normalizeTimestampMs(ms)).toBe(ms)
  })

  it('parses ISO strings', () => {
    const ms = normalizeTimestampMs('2026-10-15T02:50:00Z')
    expect(ms).toBe(Date.parse('2026-10-15T02:50:00Z'))
  })

  it('returns undefined for null/garbage', () => {
    expect(normalizeTimestampMs(undefined)).toBeUndefined()
    expect(normalizeTimestampMs('not-a-date')).toBeUndefined()
    expect(normalizeTimestampMs(NaN)).toBeUndefined()
  })
})

describe('the 497289h regression', () => {
  it('the interrupted-path arithmetic stays sane on a seconds timestamp', () => {
    // The incident: user message from SERVER history (seconds), subtracted
    // from Date.now() (ms) → ~1.79e12 ms ≈ 497,289 hours.
    // A user turn that started 30 s ago, as the server would timestamp it.
    const serverSecondsTs = Math.floor(Date.now() / 1000) - 30
    const buggy = Date.now() - serverSecondsTs
    expect(buggy).toBeGreaterThan(1e12) // the bug shape

    const fixed = Date.now() - (normalizeTimestampMs(serverSecondsTs) as number)
    expect(fixed).toBeGreaterThan(0)
    expect(fixed).toBeLessThan(24 * 3600 * 1000) // a sane turn duration
  })

  it('mergeMessagesForDisplay canonicalizes history timestamps to ms', () => {
    const userMsg = {
      id: 'u1', role: 'user', content: 'hi', timestamp: 1792047000,
    } as unknown as Message
    const out = mergeMessagesForDisplay([userMsg])
    expect(out[0].timestamp).toBe(1792047000 * 1000)
  })

  it('formatTimestamp still renders both units identically', () => {
    const seconds = 1792047000
    expect(formatTimestamp(seconds, false)).toBe(formatTimestamp(seconds * 1000, false))
  })
})
