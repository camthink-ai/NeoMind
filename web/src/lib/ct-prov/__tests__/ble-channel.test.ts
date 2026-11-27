import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CT_PROV_D2C_UUID, CT_PROV_SERVICE_UUID } from '../constants'

const blecMock = vi.hoisted(() => ({
  startScan: vi.fn(),
  stopScan: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  subscribe: vi.fn(),
  unsubscribe: vi.fn(),
  send: vi.fn(),
  read: vi.fn(),
}))

vi.mock('@mnlphlp/plugin-blec', () => blecMock)

import { openCtGattChannel, scanCtDevices, stopBleScan } from '../ble-channel'

describe('Tauri BLE channel lifecycle coordination', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    ;(globalThis as unknown as { window: Record<string, unknown> }).window = {
      __TAURI_INTERNALS__: {},
    }
    blecMock.startScan.mockResolvedValue(undefined)
    blecMock.stopScan.mockResolvedValue(undefined)
    blecMock.connect.mockResolvedValue(undefined)
    blecMock.disconnect.mockResolvedValue(undefined)
    blecMock.subscribe.mockResolvedValue(undefined)
    blecMock.unsubscribe.mockResolvedValue(undefined)
  })

  afterEach(async () => {
    await stopBleScan()
    vi.useRealTimers()
  })

  it('stops an active Tauri scan before connecting and does not stopScan twice', async () => {
    vi.useFakeTimers()
    const events: string[] = []
    let markScanStarted!: () => void
    const scanStarted = new Promise<void>((r) => {
      markScanStarted = r
    })

    blecMock.startScan.mockImplementation(async (cb: (found: unknown[]) => void) => {
      events.push('startScan')
      cb([
        {
          address: 'AA:BB:CC:DD:EE:FF',
          name: 'NE101-A1B2C3',
          rssi: -52,
          services: [CT_PROV_SERVICE_UUID],
        },
      ])
      markScanStarted()
    })
    blecMock.stopScan.mockImplementation(async () => {
      events.push('stopScan')
    })
    blecMock.connect.mockImplementation(async () => {
      events.push('connect')
    })

    const onDevices = vi.fn()
    const scanPromise = scanCtDevices({ durationMs: 30_000, onDevices })
    await scanStarted
    expect(onDevices).toHaveBeenCalledTimes(1)

    const ch = await openCtGattChannel('AA:BB:CC:DD:EE:FF')
    await scanPromise

    expect(events).toEqual(['startScan', 'stopScan', 'connect'])
    // Advancing past the original 30s scan window must not call stopScan again.
    await vi.advanceTimersByTimeAsync(35_000)
    expect(blecMock.stopScan).toHaveBeenCalledTimes(1)
    // Cancelled scan should not re-fire final onDevices after connect started.
    expect(onDevices).toHaveBeenCalledTimes(1)

    await ch.disconnect()
  })

  it('serializes pending Tauri disconnect before the next connect or scan', async () => {
    const events: string[] = []
    let releaseDisconnect!: () => void
    let markDisconnectStarted!: () => void
    const disconnectStarted = new Promise<void>((r) => {
      markDisconnectStarted = r
    })

    blecMock.connect.mockImplementation(async () => {
      events.push('connect')
    })
    blecMock.subscribe.mockImplementation(async () => {
      events.push('subscribe')
    })
    blecMock.unsubscribe.mockImplementation(async (uuid: string) => {
      events.push(`unsubscribe:${uuid}`)
    })
    blecMock.disconnect.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          events.push('disconnect:start')
          markDisconnectStarted()
          releaseDisconnect = () => {
            events.push('disconnect:end')
            resolve()
          }
        }),
    )

    const ch1 = await openCtGattChannel('DEV-1')
    await ch1.subscribeD2c(() => {})

    // Trigger disconnect without awaiting (simulating sync UI retry button)
    const d1 = ch1.disconnect()
    // Calling disconnect twice on the same channel is idempotent
    const d2 = ch1.disconnect()
    expect(d2).toBe(d1)

    // Immediately start a new connect while disconnect is still in-flight
    let ch2Connected = false
    const ch2Promise = openCtGattChannel('DEV-1').then((c) => {
      ch2Connected = true
      return c
    })

    await disconnectStarted
    expect(ch2Connected).toBe(false)
    expect(events).toEqual([
      'connect',
      'subscribe',
      `unsubscribe:${CT_PROV_D2C_UUID}`,
      'disconnect:start',
    ])

    releaseDisconnect()
    const ch2 = await ch2Promise
    expect(ch2Connected).toBe(true)
    expect(events).toEqual([
      'connect',
      'subscribe',
      `unsubscribe:${CT_PROV_D2C_UUID}`,
      'disconnect:start',
      'disconnect:end',
      'connect',
    ])

    // ch2 was never subscribed — disconnect should skip unsubscribe
    blecMock.disconnect.mockResolvedValue(undefined)
    await ch2.disconnect()
    expect(blecMock.unsubscribe).toHaveBeenCalledTimes(1)
  })
})
