/// Regression tests for the connect-livelock fix in ChatWebSocket:
/// 1. a second connect() while a socket to the SAME URL is still CONNECTING
///    must not kill it (the old code closed it → onclose → reconnect →
///    close the next… none ever reached OPEN, ~1s spin forever);
/// 2. close/message events from a REPLACED (stale) socket must be ignored —
///    only the current socket drives connection state.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

class FakeWebSocket {
  static instances: FakeWebSocket[] = []
  static OPEN = 1
  static CONNECTING = 0
  static CLOSING = 2
  static CLOSED = 3
  url: string
  readyState = 0 // CONNECTING until the test advances it
  onopen: (() => void) | null = null
  onclose: ((ev: { code: number; reason: string; target: unknown }) => void) | null = null
  onerror: ((ev: { target: unknown }) => void) | null = null
  onmessage: ((ev: { data: string; target: unknown }) => void) | null = null
  closedWith: number | null = null
  constructor(url: string) {
    this.url = url
    FakeWebSocket.instances.push(this)
  }
  close(code = 1000) {
    this.readyState = 3
    this.closedWith = code
  }
  send() {}
  // test helpers
  simulateOpen() {
    this.readyState = 1
    this.onopen?.()
  }
  simulateClose(code = 1006) {
    this.readyState = 3
    this.onclose?.({ code, reason: '', target: this })
  }
}

describe('ChatWebSocket connect race', () => {
  let ChatWebSocket: typeof import('../websocket').ChatWebSocket

  beforeEach(async () => {
    FakeWebSocket.instances = []
    vi.stubGlobal('WebSocket', FakeWebSocket as unknown as typeof WebSocket)
    localStorage.setItem('neomind_token', 'test-token')
    ;({ ChatWebSocket } = await import('../websocket'))
  })
  afterEach(() => {
    vi.unstubAllGlobals()
    localStorage.clear()
    vi.resetModules()
  })

  it('second connect() with same URL does not kill the CONNECTING socket', () => {
    const cws = new ChatWebSocket()
    cws.connect()
    expect(FakeWebSocket.instances).toHaveLength(1)

    // The StrictMode double-mount scenario: connect() again while the first
    // socket is still mid-handshake.
    cws.connect()
    expect(FakeWebSocket.instances).toHaveLength(1) // no new socket
    expect(FakeWebSocket.instances[0].closedWith).toBeNull() // first one NOT closed

    // And the first socket can still reach OPEN — no livelock.
    FakeWebSocket.instances[0].simulateOpen()
    cws.connect() // now OPEN — plain no-op
    expect(FakeWebSocket.instances).toHaveLength(1)
  })

  it('replaced socket: close events are ignored (no reconnect churn)', () => {
    const cws = new ChatWebSocket()
    cws.connect()
    const first = FakeWebSocket.instances[0]

    // Force the replacement path: a connect to a different URL tears the old
    // socket down (handlers detached by the fix) and opens a new one.
    localStorage.setItem('neomind_token', 'different-token')
    // token-change path needs an OPEN socket; use manual replacement instead:
    first.simulateOpen()
    cws.connect()
    expect(FakeWebSocket.instances).toHaveLength(1)

    // Stale close from a socket that is no longer `this.ws` must be a no-op:
    // detach by simulating what the fix does — replace the socket directly.
    const stale = FakeWebSocket.instances[0]
    // Simulate the replacement (what connect() does on a URL change)
    const replacement = new FakeWebSocket('ws://x/api/chat?token=t2')
    ;(cws as unknown as { ws: FakeWebSocket }).ws = replacement
    replacement.simulateOpen()

    const states: string[] = []
    void cws.onStateChange((s) => states.push(s.status))
    const before = states.length
    stale.simulateClose(1006) // stale socket closes late
    expect(states.length).toBe(before) // ignored — no disconnect state pushed
  })

  it('auth rejection (4001) on the current socket still surfaces', () => {
    const cws = new ChatWebSocket()
    cws.connect()
    const sock = FakeWebSocket.instances[0]
    sock.simulateOpen()

    const states: string[] = []
    void cws.onStateChange((s) => states.push(s.status))
    sock.simulateClose(4001) // CURRENT socket — must NOT be ignored
    expect(states).toContain('disconnected')
  })
})
