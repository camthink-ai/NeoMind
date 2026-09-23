// Starting points, not forks. A preset fills the form and everything it fills
// stays visible and editable; it never sets the execution mode directly (the
// derivation does its job on the filled form) and never touches resources —
// binding your own devices, cameras included, is your part of the work.

import type { OperatorField } from '@/types'
import {
  Activity,
  AlertTriangle,
  Calendar,
  Camera,
  Clock,
  Gauge,
  Radar,
  Settings2,
  Zap,
} from '@/design-system/icons'

/** Localiser — the editor hands its `tAgent` in at apply time. */
type T = (key: string) => string

/** The design system's icon component shape. */
type Icon = React.ComponentType<{ className?: string; style?: React.CSSProperties }>

const ENUM = (t: T, key: string): OperatorField['field_type'] => ({
  type: 'enum',
  values: t(key).split(',').map((v) => v.trim()).filter(Boolean),
})

export interface PresetSchedule {
  type: 'timer' | 'reactive' | 'on-demand'
  subType?: 'interval' | 'daily' | 'weekly'
  intervalMinutes?: number
  hour?: number
  /** `weekly` only — 0 = Sunday, matching `Date.getDay()`. */
  weekday?: number
}

/**
 * What a preset writes into the notify card.
 *
 * `channels: []` is not "unset". The storage layer reads an empty list as a
 * config that is present and routes nowhere, which is exactly what `judgment`
 * means: the task speaks through its own tools and the machinery stays out of
 * the conversation. Spelling it out also switches off the legacy keyword
 * sniffing, which is the whole reason to say it rather than say nothing.
 */
export interface PresetNotify {
  channels: string[]
  on: 'failure' | 'always' | 'judgment'
}

export interface AgentPreset {
  key:
    | 'image'
    | 'monitor'
    | 'smart'
    | 'scheduled'
    | 'event'
    | 'rootCause'
    | 'weekly'
    | 'shift'
    | 'advanced'
  icon: Icon
  /**
   * Shown in the create rail. The rest sit behind "All templates": nine
   * two-line cards would push the name and model fields past the fold of a
   * 440px column that scrolls.
   */
  featured?: boolean
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
    /** Omitted = leave routing alone; the API's floor applies instead. */
    notify?: PresetNotify
  }
}

/**
 * A task that reads its own data and decides whether it is worth speaking.
 * Both presets that carry it end in a conclusion an operator acts on rather
 * than a reading they merely glance at.
 */
const SPEAKS_FOR_ITSELF: PresetNotify = { channels: [], on: 'judgment' }

export const AGENT_PRESETS: AgentPreset[] = [
  // ── In the rail ────────────────────────────────────────────────────────
  {
    // One-shot image analysis WITH an output contract — S1. The structured
    // inference attaches the bound camera's frame as a multimodal part, so
    // fields come from pixels the model actually saw, not from a base64
    // blob truncated to fit the prompt.
    key: 'image',
    icon: Camera,
    featured: true,
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
          field_type: ENUM(t, 'creator.preset.image.fStatusValues'),
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
    icon: Activity,
    featured: true,
    fill: {
      autonomy: false,
      memoryMode: 'tool',
      outputSchema: (t) => [
        {
          name: 'status',
          field_type: ENUM(t, 'creator.preset.monitor.fStatusValues'),
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
    // `judgment` because an agent that acts is the one case where "tell me
    // every round" and "tell me nothing" are both wrong.
    key: 'smart',
    icon: Zap,
    featured: true,
    fill: {
      autonomy: true,
      memoryMode: 'assistant',
      schedule: { type: 'reactive' },
      notify: SPEAKS_FOR_ITSELF,
    },
  },
  {
    // Periodic summary. Carries history on purpose: "compare with yesterday"
    // is the whole point of a report.
    key: 'scheduled',
    icon: Clock,
    featured: true,
    fill: {
      autonomy: false,
      memoryMode: 'assistant',
      schedule: { type: 'timer', subType: 'daily', hour: 9 },
    },
  },

  // ── Behind "All templates" ─────────────────────────────────────────────
  {
    // Event-driven constrained inference: the trigger is a change, not a
    // clock. Fields, because the whole point is to record a verdict per
    // change — prose would have to be read once and thrown away.
    key: 'event',
    icon: AlertTriangle,
    fill: {
      autonomy: false,
      memoryMode: 'tool',
      outputSchema: (t) => [
        {
          name: 'status',
          field_type: ENUM(t, 'creator.preset.event.fStatusValues'),
          description: t('creator.preset.event.fStatusDesc'),
        },
        {
          name: 'detail',
          field_type: { type: 'text' },
          description: t('creator.preset.event.fDetailDesc'),
        },
      ],
      schedule: { type: 'reactive' },
    },
  },
  {
    // S3: several sources, several rounds, and a conclusion that outlives the
    // run. Autonomy wins over the contract in the derivation, so this is a
    // `free` agent that publishes — the shape `smart` cannot reach, because
    // its conclusion is prose nothing downstream can bind to.
    key: 'rootCause',
    icon: Radar,
    fill: {
      autonomy: true,
      memoryMode: 'assistant',
      outputSchema: (t) => [
        {
          name: 'root_cause',
          field_type: { type: 'text' },
          description: t('creator.preset.rootCause.fCauseDesc'),
        },
        {
          name: 'confidence',
          field_type: ENUM(t, 'creator.preset.rootCause.fConfValues'),
          description: t('creator.preset.rootCause.fConfDesc'),
        },
        {
          name: 'suggestion',
          field_type: { type: 'text' },
          description: t('creator.preset.rootCause.fSuggestDesc'),
        },
      ],
      schedule: { type: 'reactive' },
      notify: SPEAKS_FOR_ITSELF,
    },
  },
  {
    // The same report shape as `scheduled` over a week instead of a day, which
    // is a different document: recurrences and the comparison to last week are
    // only visible at this horizon.
    key: 'weekly',
    icon: Calendar,
    fill: {
      autonomy: false,
      memoryMode: 'assistant',
      schedule: { type: 'timer', subType: 'weekly', weekday: 1, hour: 9 },
    },
  },
  {
    // Short-cycle inspection in prose. Distinct from `monitor` (same cadence,
    // but a field per metric) and from `scheduled` (same prose, but the
    // question is "what changed", not "summarise the day"). Judged fresh: a
    // shift hand-over is about this round, not about yesterday.
    key: 'shift',
    icon: Gauge,
    fill: {
      autonomy: false,
      memoryMode: 'tool',
      schedule: { type: 'timer', subType: 'interval', intervalMinutes: 30 },
    },
  },
  {
    // High freedom: switches set to permissive, the words are yours.
    key: 'advanced',
    icon: Settings2,
    fill: {
      autonomy: true,
      memoryMode: 'assistant',
      schedule: { type: 'on-demand' },
    },
  },
]

/** The few that stay in the rail; the rest live in the picker dialog. */
export const FEATURED_PRESETS = AGENT_PRESETS.filter((p) => p.featured)
