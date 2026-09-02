/// Tests for classifyDeviceBindings — device-binding health for dashboard widgets
import { describe, it, expect } from 'vitest'
import { classifyDeviceBindings } from '../shared/useDeviceBindingStatus'
import type { Device } from '@/types'
import type { DataSource } from '@/types/dashboard'

function device(overrides: Partial<Device> & { id: string }): Device {
  return {
    name: overrides.id,
    online: false,
    transport_connected: false,
    last_seen: '',
    ...overrides,
  } as Device
}

const metricBinding: DataSource = {
  type: 'metric',
  source: 'device',
  id: 'dev-1',
  field: 'temperature',
  mode: 'latest',
}

describe('classifyDeviceBindings', () => {
  it('returns NO_BINDING shape when the widget has no device binding', () => {
    const status = classifyDeviceBindings([], [device({ id: 'dev-1' })])
    expect(status.hasDeviceBinding).toBe(false)
    expect(status.allDangling).toBe(false)
    expect(status.staleDeviceIds).toEqual([])
  })

  it('ignores info and command bindings — they read the device record itself', () => {
    const status = classifyDeviceBindings(
      [
        { ...metricBinding, mode: 'info', field: 'name' },
        { ...metricBinding, mode: 'command', field: 'reboot' },
      ],
      [],
    )
    expect(status.hasDeviceBinding).toBe(false)
  })

  it('marks a connected-but-idle device as stale (value is the last one)', () => {
    const devices = [device({ id: 'dev-1', name: '温度传感器', transport_connected: true, last_seen: new Date().toISOString() })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.hasDeviceBinding).toBe(true)
    expect(status.allDangling).toBe(false)
    expect(status.staleDeviceIds).toEqual(['dev-1'])
    // Idle stale carries its state (calm rendering) plus the last report time
    expect(status.staleDevices).toHaveLength(1)
    expect(status.staleDevices[0].state).toBe('connectedIdle')
    expect(status.staleDevices[0].name).toBe('温度传感器')
    expect(status.staleDevices[0].lastSeen).toBeGreaterThan(0)
  })

  it('marks a device that reported before and went offline as stale', () => {
    const devices = [device({ id: 'dev-1', transport_connected: false, last_seen: '2026-08-26T10:00:00Z' })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.staleDeviceIds).toEqual(['dev-1'])
    // Offline stale is distinguishable from idle — it gets urgent rendering
    expect(status.staleDevices[0].state).toBe('offline')
    expect(status.staleDevices[0].lastSeen).toBe(Date.parse('2026-08-26T10:00:00Z'))
  })

  it('classifies mixed stale bindings per device state', () => {
    const devices = [
      device({ id: 'dev-1', transport_connected: true, last_seen: new Date().toISOString() }),
      device({ id: 'dev-2', transport_connected: false, last_seen: '2026-08-26T10:00:00Z' }),
    ]
    const status = classifyDeviceBindings(
      [{ ...metricBinding, id: 'dev-1' }, { ...metricBinding, id: 'dev-2' }],
      devices,
    )
    expect(status.staleDeviceIds).toEqual(['dev-1', 'dev-2'])
    expect(status.staleDevices.map((d) => d.state)).toEqual(['connectedIdle', 'offline'])
  })

  it('carries null lastSeen for a stale device with an unparseable timestamp', () => {
    const devices = [device({ id: 'dev-1', transport_connected: true, last_seen: 'not-a-date' })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.staleDevices).toHaveLength(1)
    expect(status.staleDevices[0].lastSeen).toBeNull()
  })

  it('does not mark a never-reported device as stale — the card shows no value anyway', () => {
    const devices = [device({ id: 'dev-1', transport_connected: false, last_seen: '' })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.staleDeviceIds).toEqual([])
  })

  it('does not mark an online device as stale', () => {
    const devices = [device({ id: 'dev-1', online: true, transport_connected: true, last_seen: new Date().toISOString() })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.staleDeviceIds).toEqual([])
  })

  it('flags allDangling when the bound device is missing from a non-empty registry', () => {
    const devices = [device({ id: 'dev-other' })]
    const status = classifyDeviceBindings([metricBinding], devices)
    expect(status.allDangling).toBe(true)
    expect(status.danglingDeviceIds).toEqual(['dev-1'])
  })

  it('does not flag dangling while the registry is still empty (bootstrap)', () => {
    const status = classifyDeviceBindings([metricBinding], [])
    expect(status.allDangling).toBe(false)
    expect(status.danglingDeviceIds).toEqual([])
  })

  it('is not allDangling when only some bound devices are missing', () => {
    const devices = [device({ id: 'dev-1', online: true, transport_connected: true, last_seen: new Date().toISOString() })]
    const status = classifyDeviceBindings(
      [metricBinding, { ...metricBinding, id: 'dev-gone' }],
      devices,
    )
    expect(status.allDangling).toBe(false)
    expect(status.danglingDeviceIds).toEqual(['dev-gone'])
  })

  it('accepts multi-source lists and aggregates per-device state', () => {
    const devices = [
      device({ id: 'dev-1', online: true, transport_connected: true, last_seen: new Date().toISOString() }),
      device({ id: 'dev-2', transport_connected: true, last_seen: new Date().toISOString() }),
    ]
    const status = classifyDeviceBindings(
      [
        { ...metricBinding, id: 'dev-1' },
        { ...metricBinding, id: 'dev-2' },
      ],
      devices,
    )
    expect(status.staleDeviceIds).toEqual(['dev-2'])
  })
})
