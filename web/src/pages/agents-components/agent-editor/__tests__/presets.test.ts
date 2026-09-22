import { describe, it, expect } from 'vitest'
import { AGENT_PRESETS } from '../presets'
import { deriveExecutionMode } from '../derivation'

describe('AGENT_PRESETS × deriveExecutionMode', () => {
  it('every preset derives the mode it was designed for (resources unbound)', () => {
    const expectMode: Record<string, Parameters<typeof deriveExecutionMode>[0] & { mode: string }> = {
      image: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: true, mode: 'structured' },
      monitor: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: true, mode: 'structured' },
      smart: { hasDeviceCommands: false, canActAutonomously: true, hasOutputContract: false, mode: 'free' },
      scheduled: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: false, mode: 'focused' },
      advanced: { hasDeviceCommands: false, canActAutonomously: true, hasOutputContract: false, mode: 'free' },
    }
    for (const preset of AGENT_PRESETS) {
      const spec = expectMode[preset.key]
      expect(spec, `preset ${preset.key} has a spec`).toBeTruthy()
      const schema = preset.fill.outputSchema?.((k) => k) ?? []
      const derived = deriveExecutionMode({
        hasDeviceCommands: spec.hasDeviceCommands,
        canActAutonomously: preset.fill.autonomy,
        hasOutputContract: schema.length > 0,
      })
      expect(derived, `preset ${preset.key} should derive ${spec.mode}`).toBe(spec.mode)
    }
  })

  it('the image preset carries an output contract — S1 is closed, fields come from pixels', () => {
    // Until the structured inference attached images as multimodal parts,
    // a field-bearing image agent derived to a text-only path that truncated
    // the base64 into garbage. The gap closed; the preset now ships fields.
    const image = AGENT_PRESETS.find((p) => p.key === 'image')
    expect((image?.fill.outputSchema?.((k) => k) ?? []).length).toBeGreaterThan(0)
  })

  it('scanner presets judge fresh (tool memory), linkage and report presets carry history', () => {
    const memory: Record<string, 'tool' | 'assistant' | null> = {
      image: 'tool',
      monitor: 'tool',
      smart: 'assistant',
      scheduled: 'assistant',
      advanced: 'assistant',
    }
    for (const preset of AGENT_PRESETS) {
      expect(preset.fill.memoryMode, `preset ${preset.key}`).toBe(memory[preset.key])
    }
  })
})

describe('preset field localisation', () => {
  it('monitor enum values and units resolve through the localiser, not hardcoded', () => {
    const monitor = AGENT_PRESETS.find((p) => p.key === 'monitor')
    const fields = monitor!.fill.outputSchema!((k) => (k === 'creator.preset.monitor.fStatusValues' ? 'ok, bad' : k))
    const status = fields[0]
    expect(status.field_type).toEqual({ type: 'enum', values: ['ok', 'bad'] })
    expect(fields[1].unit).toBe('creator.preset.monitor.fCountUnit')
  })
})
