import { describe, expect, it } from 'vitest'
import { deriveCommandDeliveryLadder, parseReachedKind } from '../commandDeliveryLadder'

describe('parseReachedKind', () => {
  it('parses struct adapter_accepted', () => {
    expect(parseReachedKind({ adapter_accepted: { adapters: 2 } })).toEqual({
      kind: 'adapter_accepted',
      adapters: 2,
    })
  })

  it('parses string unit variants', () => {
    expect(parseReachedKind('extension_accepted').kind).toBe('extension_accepted')
    expect(parseReachedKind('device_acknowledged').kind).toBe('device_acknowledged')
    expect(parseReachedKind('physical_effect_verified').kind).toBe(
      'physical_effect_verified',
    )
  })

  it('rejects unknown shapes', () => {
    expect(parseReachedKind({ foo: 1 }).kind).toBeNull()
    expect(parseReachedKind(null).kind).toBeNull()
  })
})

describe('deriveCommandDeliveryLadder', () => {
  it('adapter_accepted reaches only adapter; never green verified', () => {
    const s = deriveCommandDeliveryLadder({
      sent: true,
      delivery: { adapter_accepted: { adapters: 1 } },
      delivery_note: 'broker publish; device acknowledgment: none',
    })
    expect(s.tiers.adapter_received).toBe('reached')
    expect(s.tiers.extension_received).toBe('not_applicable')
    expect(s.tiers.device_ack).toBe('not_reached')
    expect(s.tiers.physical_effect).toBe('not_reached')
    expect(s.allowVerifiedSuccess).toBe(false)
    expect(s.adapters).toBe(1)
  })

  it('extension_accepted is not a device ACK', () => {
    const s = deriveCommandDeliveryLadder({
      sent: true,
      delivery: 'extension_accepted',
    })
    expect(s.tiers.extension_received).toBe('reached')
    expect(s.tiers.adapter_received).toBe('not_applicable')
    expect(s.tiers.device_ack).toBe('not_reached')
    expect(s.allowVerifiedSuccess).toBe(false)
  })

  it('sent without delivery → all unknown, no elevation', () => {
    const s = deriveCommandDeliveryLadder({ sent: true, delivery: null })
    expect(s.tiers.adapter_received).toBe('unknown')
    expect(s.tiers.device_ack).toBe('unknown')
    expect(s.allowVerifiedSuccess).toBe(false)
    expect(s.dispatched).toBe(true)
  })

  it('null / missing → unknown, not dispatched', () => {
    const s = deriveCommandDeliveryLadder(null)
    expect(s.dispatched).toBe(false)
    expect(s.tiers.physical_effect).toBe('unknown')
  })

  it('physical_effect_verified alone allows verified success', () => {
    const s = deriveCommandDeliveryLadder({
      sent: true,
      delivery: 'physical_effect_verified',
    })
    expect(s.tiers.physical_effect).toBe('reached')
    expect(s.allowVerifiedSuccess).toBe(true)
  })

  it('never elevates device_ack from adapter proof', () => {
    const s = deriveCommandDeliveryLadder({
      adapter_accepted: { adapters: 3 },
    })
    expect(s.tiers.device_ack).toBe('not_reached')
    expect(s.tiers.physical_effect).toBe('not_reached')
  })
})
