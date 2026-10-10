/// Tests for the shared elapsed-time formatter (chat reply stats,
/// AI-analyst bubbles) — adaptive units, never a bare "185.0s".
import { describe, it, expect } from 'vitest'
import { formatElapsedMs } from '../format'

describe('formatElapsedMs', () => {
  it('sub-second times render as ms', () => {
    expect(formatElapsedMs(400)).toBe('400ms')
    expect(formatElapsedMs(999)).toBe('999ms')
  })

  it('under a minute renders as seconds with one decimal', () => {
    expect(formatElapsedMs(1000)).toBe('1.0s')
    expect(formatElapsedMs(12345)).toBe('12.3s')
    expect(formatElapsedMs(59900)).toBe('59.9s')
  })

  it('never renders "60.0s" — rolls into minutes at the boundary', () => {
    expect(formatElapsedMs(59950)).toBe('1m 00s')
    expect(formatElapsedMs(60000)).toBe('1m 00s')
  })

  it('minutes keep zero-padded seconds', () => {
    expect(formatElapsedMs(125000)).toBe('2m 05s')
    expect(formatElapsedMs(3599000)).toBe('59m 59s')
  })

  it('hours drop seconds and omit zero minutes', () => {
    expect(formatElapsedMs(3600000)).toBe('1h')
    expect(formatElapsedMs(4980000)).toBe('1h 23m')
  })

  it('non-positive or invalid input renders a dash', () => {
    expect(formatElapsedMs(0)).toBe('-')
    expect(formatElapsedMs(-5)).toBe('-')
    expect(formatElapsedMs(NaN)).toBe('-')
    expect(formatElapsedMs(Infinity)).toBe('-')
  })
})
