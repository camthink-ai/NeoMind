/**
 * Simple "when it runs" picker — daily / interval / event / manual.
 * Raw cron + timezone stay under Advanced.
 */
import { useTranslation } from 'react-i18next'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import type { TriggerSpec } from '@/types'

export type SimpleWhenMode = 'daily' | 'interval' | 'event' | 'manual'

export interface SimpleWhenSectionProps {
  value: TriggerSpec
  onChange: (next: TriggerSpec) => void
  className?: string
}

export function modeFromTrigger(trigger: TriggerSpec): SimpleWhenMode {
  if (trigger.kind === 'cron') return 'daily'
  if (trigger.kind === 'interval') return 'interval'
  if (trigger.kind === 'event') return 'event'
  return 'manual'
}

function hourFromCron(expr: string): number {
  const parts = expr.trim().split(/\s+/)
  const h = Number(parts[1])
  return Number.isFinite(h) && h >= 0 && h <= 23 ? h : 9
}

function intervalMinutes(everySecs: number): number {
  return Math.max(1, Math.round(everySecs / 60))
}

export function SimpleWhenSection({ value, onChange, className }: SimpleWhenSectionProps) {
  const { t } = useTranslation('agents')
  const mode = modeFromTrigger(value)

  const setMode = (next: SimpleWhenMode) => {
    switch (next) {
      case 'daily':
        onChange({
          kind: 'cron',
          expr: value.kind === 'cron' ? value.expr : '0 9 * * *',
          timezone: value.kind === 'cron' ? value.timezone : 'Asia/Shanghai',
        })
        break
      case 'interval':
        onChange({
          kind: 'interval',
          every_secs: value.kind === 'interval' ? value.every_secs : 3600,
        })
        break
      case 'event':
        onChange({ kind: 'event', filter: value.kind === 'event' ? value.filter : {} })
        break
      case 'manual':
        onChange({ kind: 'manual' })
        break
    }
  }

  return (
    <div className={cn('space-y-4', className)} data-section="when-simple">
      <FormField label={t('editorV2.when.mode')}>
        <Select value={mode} onValueChange={(v) => setMode(v as SimpleWhenMode)}>
          <SelectTrigger className="h-10">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="daily">{t('editorV2.when.daily')}</SelectItem>
            <SelectItem value="interval">{t('editorV2.when.interval')}</SelectItem>
            <SelectItem value="event">{t('editorV2.when.event')}</SelectItem>
            <SelectItem value="manual">{t('editorV2.when.manual')}</SelectItem>
          </SelectContent>
        </Select>
      </FormField>

      {mode === 'daily' && value.kind === 'cron' && (
        <FormField label={t('editorV2.when.hour')}>
          <Select
            value={String(hourFromCron(value.expr))}
            onValueChange={(h) =>
              onChange({
                kind: 'cron',
                expr: `0 ${h} * * *`,
                timezone: value.timezone || 'Asia/Shanghai',
              })
            }
          >
            <SelectTrigger className="h-10 max-w-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {Array.from({ length: 24 }, (_, i) => (
                <SelectItem key={i} value={String(i)}>
                  {t('editorV2.when.hourOption', { hour: String(i).padStart(2, '0') })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
      )}

      {mode === 'interval' && value.kind === 'interval' && (
        <FormField label={t('editorV2.when.everyMinutes')}>
          <Input
            type="number"
            min={1}
            className="max-w-xs"
            value={intervalMinutes(value.every_secs)}
            onChange={(e) =>
              onChange({
                kind: 'interval',
                every_secs: Math.max(1, Number(e.target.value) || 1) * 60,
              })
            }
          />
        </FormField>
      )}
    </div>
  )
}
