// Starting points, not forks. A preset fills the form and everything it fills
// stays visible and editable; it never sets the execution mode directly (the
// derivation does its job on the filled form) and never touches resources —
// binding your own devices, cameras included, is your part of the work.

import type { OperatorField } from '@/types'

/** Localiser — the editor hands its `tAgent` in at apply time. */
type T = (key: string) => string

export interface PresetSchedule {
  type: 'timer' | 'reactive' | 'on-demand'
  subType?: 'interval' | 'daily' | 'weekly'
  intervalMinutes?: number
  hour?: number
}

export interface AgentPreset {
  key: 'image' | 'monitor' | 'smart' | 'scheduled' | 'advanced'
  fill: {
    /** Whether the agent may work things out itself (the autonomy switch). */
    autonomy: boolean
    /**
     * Output contract, resolved at apply time so enum values, descriptions
     * and units follow the user's language. Omitted = prose, no fields.
     * Field NAMES stay ASCII on purpose — they become `ai:{id}:{field}` keys.
     */
    outputSchema?: (t: T) => OperatorField[]
    /** null = leave the memory axis on "auto". */
    memoryMode: 'tool' | 'assistant' | null
    schedule: PresetSchedule
  }
}

export const AGENT_PRESETS: AgentPreset[] = [
  {
    // One-shot image analysis WITH an output contract — S1. The structured
    // inference attaches the bound camera's frame as a multimodal part, so
    // fields come from pixels the model actually saw, not from a base64
    // blob truncated to fit the prompt.
    key: 'image',
    fill: {
      autonomy: false,
      memoryMode: 'tool',
      outputSchema: (t) => [
        {
          name: 'description',
          field_type: { type: 'text' },
          description: t('creator.preset.image.fDescDesc'),
        },
        {
          name: 'status',
          field_type: { type: 'enum', values: t('creator.preset.image.fStatusValues').split(',').map((v) => v.trim()).filter(Boolean) },
          description: t('creator.preset.image.fStatusDesc'),
        },
      ],
      schedule: { type: 'on-demand' },
    },
  },
  {
    // Multi-source monitoring into fixed fields — the scanner. Fields are fine
    // here: telemetry renders as text the constrained inference can read.
    key: 'monitor',
    fill: {
      autonomy: false,
      memoryMode: 'tool',
      outputSchema: (t) => [
        {
          name: 'status',
          field_type: {
            type: 'enum',
            values: t('creator.preset.monitor.fStatusValues')
              .split(',')
              .map((v) => v.trim())
              .filter(Boolean),
          },
          description: t('creator.preset.monitor.fStatusDesc'),
        },
        {
          name: 'anomaly_count',
          field_type: { type: 'number' },
          unit: t('creator.preset.monitor.fCountUnit'),
          description: t('creator.preset.monitor.fCountDesc'),
        },
      ],
      schedule: { type: 'timer', subType: 'interval', intervalMinutes: 15 },
    },
  },
  {
    // Analysis linked to action: autonomy on, event-driven. Commands are bound
    // by the user in ② — the preset only says the agent may use them.
    key: 'smart',
    fill: {
      autonomy: true,
      memoryMode: 'assistant',
      schedule: { type: 'reactive' },
    },
  },
  {
    // Periodic summary. Carries history on purpose: "compare with yesterday"
    // is the whole point of a report.
    key: 'scheduled',
    fill: {
      autonomy: false,
      memoryMode: 'assistant',
      schedule: { type: 'timer', subType: 'daily', hour: 9 },
    },
  },
  {
    // High freedom: switches set to permissive, the words are yours.
    key: 'advanced',
    fill: {
      autonomy: true,
      memoryMode: 'assistant',
      schedule: { type: 'on-demand' },
    },
  },
]
