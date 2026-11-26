/**
 * Task templates seed safe defaults (tools off + schedule).
 */
import { describe, expect, it } from 'vitest'
import {
  seedVision,
  seedDailyBrief,
  seedWeekly,
  seedScheduledPatrol,
  seedMeterInspect,
  seedEventReview,
  seedQa,
  inferTemplateId,
  TASK_TEMPLATES,
} from '../templates'
import { emptyDraft } from '../defaults'

describe('task templates', () => {
  it('exports working-only presets including vision', () => {
    expect(TASK_TEMPLATES.map((t) => t.id)).toEqual([
      'vision',
      'daily_brief',
      'meter_inspect',
      'event_review',
      'weekly',
      'shift',
      'patrol',
      'qa',
      'custom',
    ])
  })

  it('vision seeds 5m interval + alert notify + published structured output', () => {
    const d = seedVision()
    expect(d.trigger).toEqual({ kind: 'interval', every_secs: 5 * 60 })
    expect(d.notify?.on_alert).toBe(true)
    expect(d.output?.publish).toBe(true)
    expect(d.output?.fields.map((f) => f.name)).toEqual(['status', 'summary'])
  })

  it('qa seeds scoped query_data tool policy', () => {
    const d = seedQa()
    expect(d.trigger.kind).toBe('manual')
    expect(d.tool_policy).toEqual({
      scoped: { allowed_tools: ['query_data'], allowed_commands: [] },
    })
  })

  it('daily brief and weekly report seed cron + unrestricted tools + complete notify', () => {
    const d = seedDailyBrief()
    expect(d.trigger).toEqual({
      kind: 'cron',
      expr: '0 9 * * *',
      timezone: 'Asia/Shanghai',
    })
    expect(d.tool_policy).toBe('unrestricted')
    expect(d.notify?.on_complete).toBe(true)
    expect(d.preset).toBe('daily_brief')

    const w = seedWeekly()
    expect(w.trigger).toEqual({
      kind: 'cron',
      expr: '0 9 * * 1',
      timezone: 'Asia/Shanghai',
    })
    expect(w.tool_policy).toBe('unrestricted')
    expect(w.notify?.on_complete).toBe(true)
  })

  it('meter inspect seeds interval + tools disabled', () => {
    const d = seedMeterInspect()
    expect(d.trigger).toEqual({ kind: 'interval', every_secs: 15 * 60 })
    expect(d.tool_policy).toBe('disabled')
  })

  it('scheduled patrol alias seeds hourly interval + tools disabled', () => {
    const d = seedScheduledPatrol()
    expect(d.trigger).toEqual({ kind: 'interval', every_secs: 3600 })
    expect(d.tool_policy).toBe('disabled')
  })

  it('event review seeds event trigger', () => {
    const d = seedEventReview()
    expect(d.trigger.kind).toBe('event')
    expect(d.tool_policy).toBe('disabled')
  })

  it('empty draft starts blank, manual, and with default scoped shell + web tools', () => {
    const d = emptyDraft()
    expect(d.name).toBe('')
    expect(d.trigger.kind).toBe('manual')
    expect(d.tool_policy).toEqual({
      scoped: {
        allowed_tools: ['shell', 'web_search', 'web_fetch'],
        allowed_commands: [],
      },
    })
  })

  it('infers template from trigger kind', () => {
    expect(inferTemplateId({ kind: 'cron', expr: '0 8 * * *', timezone: 'Asia/Shanghai' })).toBe(
      'daily_brief',
    )
    expect(inferTemplateId({ kind: 'interval', every_secs: 600 })).toBe('meter_inspect')
    expect(inferTemplateId({ kind: 'event', filter: {} })).toBe('event_review')
  })
})
