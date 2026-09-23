import { describe, it, expect } from 'vitest'
import {
  parseTriggerFilter,
  buildTriggerFilter,
  reasonTriggerFilterInvalid,
} from '../triggerFilter'

const cam = { type: 'device', id: 'cam-01', name: '车位相机', field: 'occupied' }
const gate = { type: 'extension', id: 'booking', name: '预约系统', field: 'reserved' }

describe('parseTriggerFilter', () => {
  it('reads the legacy `sources` shape as an any-group', () => {
    const f = parseTriggerFilter(JSON.stringify({ sources: [cam] }))
    expect(f.mode).toBe('any')
    expect(f.sources).toEqual([cam])
    expect(f.withinSecs).toBeNull()
  })

  it('reads an all-group together with its window', () => {
    const f = parseTriggerFilter(
      JSON.stringify({ all: [cam, gate], within_secs: 1200 }),
    )
    expect(f.mode).toBe('all')
    expect(f.sources).toEqual([cam, gate])
    expect(f.withinSecs).toBe(1200)
  })

  it('keeps a stored all-group that has no window readable', () => {
    // Faithful, not "helpfully" repaired: the editor has to show the user that
    // this rule cannot fire, rather than quietly changing its meaning on save.
    const f = parseTriggerFilter(JSON.stringify({ all: [cam, gate] }))
    expect(f.mode).toBe('all')
    expect(f.withinSecs).toBeNull()
  })

  it('falls back to an empty any-group for a filter it cannot read', () => {
    for (const raw of [undefined, '', 'not json', JSON.stringify({ foo: 1 })]) {
      expect(parseTriggerFilter(raw)).toEqual({
        mode: 'any',
        sources: [],
        withinSecs: null,
      })
    }
  })
})

describe('buildTriggerFilter', () => {
  it('emits the canonical any shape', () => {
    const json = buildTriggerFilter({ mode: 'any', sources: [cam], withinSecs: null })
    expect(JSON.parse(json)).toEqual({ any: [cam] })
  })

  it('emits an all-group with its window', () => {
    const json = buildTriggerFilter({ mode: 'all', sources: [cam, gate], withinSecs: 1200 })
    expect(JSON.parse(json)).toEqual({ all: [cam, gate], within_secs: 1200 })
  })
})

describe('reasonTriggerFilterInvalid', () => {
  it('refuses an all-group with no window', () => {
    // The backend reads a missing window as "one single event must satisfy
    // every source" — i.e. an agent that effectively never fires. This is the
    // silent-failure shape, so it must not be saveable.
    expect(
      reasonTriggerFilterInvalid(
        { mode: 'all', sources: [cam, gate], withinSecs: null },
        { hasBoundResources: true },
      ),
    ).toBe('window_required')
  })

  it('refuses an event agent with nothing to trigger on', () => {
    expect(
      reasonTriggerFilterInvalid(
        { mode: 'any', sources: [], withinSecs: null },
        { hasBoundResources: false },
      ),
    ).toBe('sources_required')
  })

  it('accepts an event agent whose bound resources are the trigger', () => {
    // No explicit sources is fine when resources are bound: the backend falls
    // back to them, so this is not the never-fires case.
    expect(
      reasonTriggerFilterInvalid(
        { mode: 'any', sources: [], withinSecs: null },
        { hasBoundResources: true },
      ),
    ).toBeNull()
  })

  it('accepts the ordinary shapes', () => {
    expect(
      reasonTriggerFilterInvalid(
        { mode: 'any', sources: [cam], withinSecs: null },
        { hasBoundResources: false },
      ),
    ).toBeNull()
    expect(
      reasonTriggerFilterInvalid(
        { mode: 'all', sources: [cam, gate], withinSecs: 600 },
        { hasBoundResources: false },
      ),
    ).toBeNull()
  })
})
