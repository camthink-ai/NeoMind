/// Tests for session-history pagination in the session slice: newest-page
/// load on switch, backward paging via the raw-index cursor, and the guards
/// (no double load, stop at history start).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { create } from 'zustand'
import { createSessionSlice, type SessionSlice } from '../slices/sessionSlice'
import type { SessionHistoryResponse } from '@/types'

const getSessionHistory = vi.fn()
vi.mock('@/lib/api', () => ({
  api: {
    getSessionHistory: (...args: unknown[]) => getSessionHistory(...args),
    createSession: vi.fn(),
    listSessions: vi.fn().mockResolvedValue([]),
    bulkDeleteSessions: vi.fn(),
  },
}))
vi.mock('@/lib/websocket', () => ({
  ws: { setSessionId: vi.fn(), sendMessage: vi.fn() },
}))

// Raw backend history: an assistant turn is 3 adjacent records the slice
// merges for display. 5 turns = 20 raw records.
function rawTurns(n: number) {
  const msgs = []
  for (let i = 0; i < n; i++) {
    msgs.push(
      { id: `q${i}`, role: 'user', content: `q${i}`, timestamp: i },
      { id: `a${i}t`, role: 'assistant', content: '', tool_calls: [{ id: `t${i}`, name: 'shell', arguments: {} }], timestamp: i },
      { id: `a${i}c`, role: 'assistant', content: `answer-${i}`, timestamp: i },
    )
  }
  return msgs
}

function page(msgs: unknown[], total: number, hasMore: boolean): SessionHistoryResponse {
  return { messages: msgs as never[], count: msgs.length, total, has_more: hasMore }
}

const makeStore = () => create<SessionSlice>()(createSessionSlice)

describe('session slice history pagination', () => {
  beforeEach(() => {
    getSessionHistory.mockReset()
  })

  it('switchSession loads only the newest page and records the cursor', async () => {
    const store = makeStore()
    const all = rawTurns(5) // 15 records
    getSessionHistory.mockResolvedValueOnce(page(all.slice(7), 15, true)) // newest 8

    await store.getState().switchSession('s1')

    // Requested with a page size limit
    const [id, query] = getSessionHistory.mock.calls[0]
    expect(id).toBe('s1')
    expect(query).toEqual({ limit: 200 })
    // Merged display messages (8 raw -> 5 assistant turns have 3+2... slice at
    // a user boundary keeps whole turns: q1..q4 = 4 user + 4 assistant = 8 display)
    expect(store.getState().messages.length).toBeGreaterThan(0)
    expect(store.getState().hasEarlierHistory).toBe(true)
    expect(store.getState().earlierCursor).toBe(7)
  })

  it('loadEarlierHistory pages backwards with before=<cursor> and prepends', async () => {
    const store = makeStore()
    const all = rawTurns(5)
    getSessionHistory.mockResolvedValueOnce(page(all.slice(7), 15, true))
    await store.getState().switchSession('s1')
    const beforeCount = store.getState().messages.length

    getSessionHistory.mockResolvedValueOnce(page(all.slice(0, 7), 15, false))
    const loaded = await store.getState().loadEarlierHistory()

    expect(loaded).toBe(true)
    const [, query] = getSessionHistory.mock.calls[1]
    expect(query).toEqual({ limit: 200, before: 7 })
    const after = store.getState().messages
    expect(after.length).toBeGreaterThan(beforeCount) // prepended older turns
    // cursor exhausted
    expect(store.getState().hasEarlierHistory).toBe(false)
    expect(store.getState().earlierCursor).toBe(0)
  })

  it('loadEarlierHistory is a no-op when no earlier history exists', async () => {
    const store = makeStore()
    getSessionHistory.mockResolvedValueOnce(page(rawTurns(1), 3, false))
    await store.getState().switchSession('s1')

    const loaded = await store.getState().loadEarlierHistory()
    expect(loaded).toBe(false)
    expect(getSessionHistory).toHaveBeenCalledTimes(1) // no extra request
  })

  it('unpaginated responses (no has_more) load everything', async () => {
    const store = makeStore()
    const all = rawTurns(2)
    getSessionHistory.mockResolvedValueOnce(page(all, 6, false))
    await store.getState().switchSession('s1')
    expect(store.getState().hasEarlierHistory).toBe(false)
    expect(store.getState().earlierCursor).toBe(0)
  })
})

describe('loadEarlierHistory concurrency & failure', () => {
  beforeEach(() => {
    getSessionHistory.mockReset()
  })

  it('rejects concurrent calls without a second request (store-level guard)', async () => {
    const store = makeStore()
    const all = rawTurns(5)
    getSessionHistory.mockResolvedValueOnce(page(all.slice(7), 15, true))
    await store.getState().switchSession('s1')

    // First call never resolves until we allow it
    let resolveFirst!: (v: SessionHistoryResponse) => void
    getSessionHistory.mockReturnValueOnce(new Promise((r) => { resolveFirst = r }))
    const first = store.getState().loadEarlierHistory()
    const second = await store.getState().loadEarlierHistory() // resolves immediately (guarded)

    expect(second).toBe(false)
    expect(getSessionHistory).toHaveBeenCalledTimes(2) // initial + first earlier

    resolveFirst(page(all.slice(0, 7), 15, false))
    expect(await first).toBe(true)
    expect(store.getState().messages.length).toBeGreaterThan(0)
  })

  it('a failed page load leaves paging available for retry', async () => {
    const store = makeStore()
    const all = rawTurns(5)
    getSessionHistory.mockResolvedValueOnce(page(all.slice(7), 15, true))
    await store.getState().switchSession('s1')

    getSessionHistory.mockRejectedValueOnce(new Error('network'))
    const failed = await store.getState().loadEarlierHistory()
    expect(failed).toBe(false)
    // Cursor untouched -> still page-able
    expect(store.getState().hasEarlierHistory).toBe(true)
    expect(store.getState().earlierCursor).toBe(7)

    // Retry succeeds
    getSessionHistory.mockResolvedValueOnce(page(all.slice(0, 7), 15, false))
    expect(await store.getState().loadEarlierHistory()).toBe(true)
  })
})
