import { describe, it, expect } from 'vitest'
import { AGENT_PRESETS, FEATURED_PRESETS, nextStepFor, type PresetNextStep } from '../presets'
import { deriveExecutionMode } from '../derivation'
import en from '@/i18n/locales/en/agents.json'
import zh from '@/i18n/locales/zh/agents.json'

describe('AGENT_PRESETS × deriveExecutionMode', () => {
  it('every preset derives the mode it was designed for (resources unbound)', () => {
    const expectMode: Record<string, Parameters<typeof deriveExecutionMode>[0] & { mode: string }> = {
      image: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: true, mode: 'structured' },
      monitor: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: true, mode: 'structured' },
      event: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: true, mode: 'structured' },
      smart: { hasDeviceCommands: false, canActAutonomously: true, hasOutputContract: false, mode: 'free' },
      rootCause: { hasDeviceCommands: false, canActAutonomously: true, hasOutputContract: true, mode: 'free' },
      scheduled: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: false, mode: 'focused' },
      weekly: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: false, mode: 'focused' },
      shift: { hasDeviceCommands: false, canActAutonomously: false, hasOutputContract: false, mode: 'focused' },
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

  it('rootCause is the one preset that both acts and publishes — S3', () => {
    // `smart` reaches `free`, but its conclusion is prose nothing downstream
    // can bind to; `monitor` publishes but cannot investigate. Only this
    // combination produces a conclusion that is also a data source.
    const rc = AGENT_PRESETS.find((p) => p.key === 'rootCause')!
    expect(rc.fill.autonomy).toBe(true)
    expect((rc.fill.outputSchema?.((k) => k) ?? []).length).toBeGreaterThan(0)
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
      event: 'tool',
      shift: 'tool',
      smart: 'assistant',
      rootCause: 'assistant',
      scheduled: 'assistant',
      weekly: 'assistant',
      advanced: 'assistant',
    }
    for (const preset of AGENT_PRESETS) {
      expect(preset.fill.memoryMode, `preset ${preset.key}`).toBe(memory[preset.key])
    }
  })
})

describe('the rail carries a few, the dialog carries all', () => {
  it('keeps the rail to the four a new agent most likely wants', () => {
    expect(FEATURED_PRESETS.map((p) => p.key)).toEqual(['image', 'monitor', 'smart', 'scheduled'])
    // The split only earns its extra click while it is a real split.
    expect(FEATURED_PRESETS.length).toBeLessThan(AGENT_PRESETS.length)
  })

  it('every preset is reachable from the dialog, featured or not', () => {
    for (const p of AGENT_PRESETS) expect(p.icon, `preset ${p.key} has an icon`).toBeTruthy()
  })

  it('every preset key resolves in both locales', () => {
    // The labels are built by interpolation (`creator.preset.${key}.name`), so
    // a key that does not exist renders as itself — visible, but only to
    // whoever opens that language.
    for (const p of AGENT_PRESETS) {
      for (const [loc, bundle] of [['en', en], ['zh', zh]] as const) {
        const entry = (bundle as Record<string, unknown> & {
          creator: { preset: Record<string, { name?: string; desc?: string; prompt?: string }> }
        }).creator.preset[p.key]
        expect(entry?.name, `${loc}: creator.preset.${p.key}.name`).toBeTruthy()
        expect(entry?.desc, `${loc}: creator.preset.${p.key}.desc`).toBeTruthy()
        expect(entry?.prompt, `${loc}: creator.preset.${p.key}.prompt`).toBeTruthy()
      }
    }
  })
})

describe('notification routing the presets do set', () => {
  it('only the two whose conclusion is the point speak for themselves', () => {
    // `judgment` asserts the task will use its own tools. Everywhere else the
    // preset stays out of it and the API's floor applies.
    for (const p of AGENT_PRESETS) {
      if (p.key === 'smart' || p.key === 'rootCause') {
        expect(p.fill.notify, `preset ${p.key}`).toEqual({ channels: [], on: 'judgment' })
      } else {
        expect(p.fill.notify, `preset ${p.key} must leave routing alone`).toBeUndefined()
      }
    }
  })

  it('the weekly preset names a weekday, not the editor default', () => {
    const weekly = AGENT_PRESETS.find((p) => p.key === 'weekly')!
    expect(weekly.fill.schedule.subType).toBe('weekly')
    expect(weekly.fill.schedule.weekday).toBeTypeOf('number')
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

  it('every field-bearing preset names ASCII fields — they become data source ids', () => {
    for (const p of AGENT_PRESETS) {
      for (const f of p.fill.outputSchema?.((k) => k) ?? []) {
        expect(f.name, `preset ${p.key} field ${f.name}`).toMatch(/^[a-z][a-z0-9_]*$/)
      }
    }
  })
})

describe('the next step a preset leaves open', () => {
  it('names what the form will not save without', () => {
    // Two of these block the save button and one does not, so getting the
    // mapping wrong means telling the user to do something unnecessary — or
    // saying nothing while Save stays grey.
    const expected: Record<string, PresetNextStep> = {
      image: 'sources',
      monitor: 'sources',
      event: 'sources',
      smart: 'trigger',
      rootCause: 'trigger',
      scheduled: 'optional',
      weekly: 'optional',
      shift: 'optional',
      advanced: 'optional',
    }
    for (const p of AGENT_PRESETS) {
      expect(nextStepFor(p), `preset ${p.key}`).toBe(expected[p.key])
    }
  })

  it('event reads as sources, not trigger — one binding covers both rules', () => {
    // Reactive AND structured. Bound resources satisfy the structured rule and
    // the reactive one, so naming only the trigger would send the user to
    // half of what they need.
    const event = AGENT_PRESETS.find((p) => p.key === 'event')!
    expect(event.fill.schedule.type).toBe('reactive')
    expect(nextStepFor(event)).toBe('sources')
  })

  it('every step resolves in both locales', () => {
    const steps: PresetNextStep[] = ['sources', 'trigger', 'optional']
    for (const step of steps) {
      for (const [loc, bundle] of [['en', en], ['zh', zh]] as const) {
        expect(bundle.creator.preset.next[step], `${loc}: creator.preset.next.${step}`).toBeTruthy()
      }
    }
  })
})
