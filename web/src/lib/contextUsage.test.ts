import { describe, it, expect } from 'vitest'
import { contextUsageSnapshot } from './contextUsage'

describe('request context snapshot', () => {
  it('keeps the real total and hides inconsistent first-request component estimates', () => {
    const value = contextUsageSnapshot({ promptTokens: 7138, systemPromptTokens: 2986, toolTokens: 6398, maxContextTokens: 32768 }, 32768, 1)
    expect(value.used).toBe(7138)
    expect(value.system).toBeUndefined()
    expect(value.tools).toBeUndefined()
    expect(value.history).toBeUndefined()
  })

  it('keeps measured request usage while long ReAct history accumulates', () => {
    const snapshot = contextUsageSnapshot({ promptTokens: 16000, maxContextTokens: 32768 }, 128000, 80)
    expect(snapshot.used).toBe(16000)
    expect(snapshot.max).toBe(32768)
    expect(snapshot.estimated).toBe(false)
  })
  it('first streaming turn waits for a measurement instead of claiming transcript occupancy', () => {
    expect(contextUsageSnapshot(null, 32768, 1)).toMatchObject({ pending: true, used: 0, estimated: true })
  })
  it('uses a newer, lower snapshot after context is reduced', () => {
    expect(contextUsageSnapshot({ promptTokens: 7000, maxContextTokens: 32768 }, 128000, 80).used).toBe(7000)
  })
  it('rejects invalid capacity and does not hide a genuine measured overrun', () => {
    expect(contextUsageSnapshot({ promptTokens: 9000, maxContextTokens: 0 }, 8192, 1)).toMatchObject({ used: 9000, max: 8192 })
  })
})
