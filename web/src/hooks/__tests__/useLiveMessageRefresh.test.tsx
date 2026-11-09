/// Tests for useLiveMessageRefresh — the messages page's live-update seam.
/// Exercises the REAL useEvents wiring (dynamic-imported events lib →
/// connection.onEvent → client-side type filter → dedup) with a mocked
/// connection, plus the coalescing window: a burst of MessageCreated events
/// must produce exactly one refetch, foreign event types none, and a
/// disabled/unmounted page must cancel its pending fetch.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, cleanup } from '@testing-library/react'
import { useLiveMessageRefresh } from '../useLiveMessageRefresh'
import type { NeoMindEvent } from '@/lib/events'

// ── events lib mock: capture the subscriber the real useEvents registers ──
type EventHandler = (event: NeoMindEvent) => void
let handlers: EventHandler[] = []
const connect = vi.fn()
const disconnect = vi.fn()
vi.mock('@/lib/events', () => ({
  getEventsConnection: () => ({
    connect,
    disconnect,
    onEvent: (cb: EventHandler) => {
      handlers.push(cb)
      return () => {
        handlers = handlers.filter((h) => h !== cb)
      }
    },
    onConnection: () => () => {},
    onError: () => () => {},
  }),
}))

let seq = 0
const messageCreated = (): NeoMindEvent =>
  ({ id: `e${seq++}`, type: 'MessageCreated', timestamp: 0, source: 'test', data: {} }) as NeoMindEvent
const otherEvent = (): NeoMindEvent =>
  ({ id: `e${seq++}`, type: 'DeviceMetric', timestamp: 0, source: 'test', data: {} }) as NeoMindEvent

const emit = (event: NeoMindEvent) => {
  act(() => {
    handlers.forEach((h) => h(event))
  })
}

describe('useLiveMessageRefresh', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    handlers = []
    seq = 0
    connect.mockClear()
    disconnect.mockClear()
  })
  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  it('coalesces a MessageCreated burst into exactly one refetch', async () => {
    const refetch = vi.fn()
    renderHook(() => useLiveMessageRefresh(true, refetch))
    await act(async () => {}) // let useEvents' dynamic import resolve

    emit(messageCreated())
    emit(messageCreated())
    emit(messageCreated())
    expect(refetch).not.toHaveBeenCalled()

    await act(async () => {
      vi.advanceTimersByTime(1500)
    })
    expect(refetch).toHaveBeenCalledTimes(1)

    // The window re-arms: a later event refreshes again.
    emit(messageCreated())
    await act(async () => {
      vi.advanceTimersByTime(1500)
    })
    expect(refetch).toHaveBeenCalledTimes(2)
  })

  it('ignores non-message events', async () => {
    const refetch = vi.fn()
    renderHook(() => useLiveMessageRefresh(true, refetch))
    await act(async () => {})

    emit(otherEvent())
    emit(otherEvent())
    await act(async () => {
      vi.advanceTimersByTime(3000)
    })
    expect(refetch).not.toHaveBeenCalled()
  })

  it('does not subscribe while disabled', async () => {
    const refetch = vi.fn()
    const { rerender } = renderHook(({ enabled }) => useLiveMessageRefresh(enabled, refetch), {
      initialProps: { enabled: false },
    })
    await act(async () => {})
    expect(handlers).toHaveLength(0)

    // Enabling subscribes; disabling cancels a pending refetch.
    rerender({ enabled: true })
    await act(async () => {})
    expect(handlers).toHaveLength(1)
    emit(messageCreated())
    rerender({ enabled: false })
    await act(async () => {
      vi.advanceTimersByTime(3000)
    })
    expect(refetch).not.toHaveBeenCalled()
  })

  it('cancels a pending refetch on unmount', async () => {
    const refetch = vi.fn()
    const { unmount } = renderHook(() => useLiveMessageRefresh(true, refetch))
    await act(async () => {})
    emit(messageCreated())
    unmount()
    await act(async () => {
      vi.advanceTimersByTime(3000)
    })
    expect(refetch).not.toHaveBeenCalled()
  })
})
