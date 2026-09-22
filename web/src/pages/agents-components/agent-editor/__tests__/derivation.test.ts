import { describe, it, expect } from 'vitest'
import { deriveExecutionMode } from '../derivation'

describe('deriveExecutionMode', () => {
  it('binds device commands → the agent must act, so it reasons across rounds', () => {
    expect(
      deriveExecutionMode({
        hasDeviceCommands: true,
        canActAutonomously: false,
        hasOutputContract: false,
      }),
    ).toBe('free')
  })

  it('asks to work it out itself → same reasoning, even with nothing bound', () => {
    // This is the case the four cards used to cover and resources never could:
    // "investigate" and "summarise" look identical on the form until the user
    // says the agent may try things.
    expect(
      deriveExecutionMode({
        hasDeviceCommands: false,
        canActAutonomously: true,
        hasOutputContract: false,
      }),
    ).toBe('free')
  })

  it('an output contract and no autonomy → a fixed-shape read', () => {
    expect(
      deriveExecutionMode({
        hasDeviceCommands: false,
        canActAutonomously: false,
        hasOutputContract: true,
      }),
    ).toBe('structured')
  })

  it('nothing at all → just look and answer', () => {
    expect(
      deriveExecutionMode({
        hasDeviceCommands: false,
        canActAutonomously: false,
        hasOutputContract: false,
      }),
    ).toBe('focused')
  })

  it('autonomy wins over the contract — the fields ride along after the run', () => {
    expect(
      deriveExecutionMode({
        hasDeviceCommands: false,
        canActAutonomously: true,
        hasOutputContract: true,
      }),
    ).toBe('free')
  })
})
