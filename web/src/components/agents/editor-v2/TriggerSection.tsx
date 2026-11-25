/**
 * Trigger section — cron expression + timezone (and other trigger kinds).
 * Speaks AgentSpec v2 TriggerSpec directly.
 */
import { useTranslation } from 'react-i18next'
import { Clock } from '@/design-system/icons'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cardBase } from '@/design-system/tokens/size'
import { cn } from '@/lib/utils'
import type { TriggerSpec } from '@/types'

export interface TriggerSectionProps {
  value: TriggerSpec
  onChange: (next: TriggerSpec) => void
  className?: string
}

type TriggerKind = TriggerSpec['kind']

export function TriggerSection({ value, onChange, className }: TriggerSectionProps) {
  const { t } = useTranslation('agents')

  const setKind = (kind: TriggerKind) => {
    switch (kind) {
      case 'cron':
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
    <section className={cn(cardBase, 'p-4 space-y-4', className)} data-section="trigger">
      <header className="flex items-center gap-2">
        <Clock className="h-4 w-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">
          {t('editorV2.trigger.title')}
        </h3>
      </header>

      <FormField label={t('editorV2.trigger.kind')}>
        <Select value={value.kind} onValueChange={(k) => setKind(k as TriggerKind)}>
          <SelectTrigger className="h-10">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="cron">{t('editorV2.trigger.cron')}</SelectItem>
            <SelectItem value="interval">{t('editorV2.trigger.interval')}</SelectItem>
            <SelectItem value="event">{t('editorV2.trigger.event')}</SelectItem>
            <SelectItem value="manual">{t('editorV2.trigger.manual')}</SelectItem>
          </SelectContent>
        </Select>
      </FormField>

      {value.kind === 'cron' && (
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField
            label={t('editorV2.trigger.expr')}
            helpText={t('editorV2.trigger.exprHint')}
          >
            <Input
              value={value.expr}
              onChange={(e) => onChange({ ...value, expr: e.target.value })}
              placeholder="0 9 * * *"
              className="font-mono text-sm"
            />
          </FormField>
          <FormField
            label={t('editorV2.trigger.timezone')}
            helpText={t('editorV2.trigger.timezoneHint')}
          >
            <Input
              value={value.timezone}
              onChange={(e) => onChange({ ...value, timezone: e.target.value })}
              placeholder="Asia/Shanghai"
            />
          </FormField>
        </div>
      )}

      {value.kind === 'interval' && (
        <FormField label={t('editorV2.trigger.everySecs')}>
          <Input
            type="number"
            min={1}
            value={value.every_secs}
            onChange={(e) =>
              onChange({
                kind: 'interval',
                every_secs: Math.max(1, Number(e.target.value) || 1),
              })
            }
          />
        </FormField>
      )}

      {value.kind === 'manual' && (
        <p className="text-sm text-muted-foreground">{t('editorV2.trigger.manualHint')}</p>
      )}

      {value.kind === 'event' && (
        <p className="text-sm text-muted-foreground">{t('editorV2.trigger.eventHint')}</p>
      )}
    </section>
  )
}
