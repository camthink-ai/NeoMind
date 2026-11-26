/**
 * Trigger section — cron expression + timezone (and other trigger kinds).
 * Speaks AgentSpec v2 TriggerSpec directly.
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
import { normalizeTrigger } from './defaults'
import { EditorSection } from './EditorSection'

export interface TriggerSectionProps {
  value?: TriggerSpec | null
  onChange: (next: TriggerSpec) => void
  className?: string
  /** First section inside a FormSurface */
  first?: boolean
}

type TriggerKind = TriggerSpec['kind']

export function TriggerSection({ value, onChange, className, first }: TriggerSectionProps) {
  const { t } = useTranslation('agents')
  const trigger = normalizeTrigger(value)

  const setKind = (kind: TriggerKind) => {
    switch (kind) {
      case 'cron':
        onChange({
          kind: 'cron',
          expr: trigger.kind === 'cron' ? trigger.expr : '0 9 * * *',
          timezone: trigger.kind === 'cron' ? trigger.timezone : 'Asia/Shanghai',
        })
        break
      case 'interval':
        onChange({
          kind: 'interval',
          every_secs: trigger.kind === 'interval' ? trigger.every_secs : 3600,
        })
        break
      case 'event':
        onChange({ kind: 'event', filter: trigger.kind === 'event' ? trigger.filter : {} })
        break
      case 'manual':
        onChange({ kind: 'manual' })
        break
    }
  }

  return (
    <EditorSection
      first={first}
      title={t('editorV2.trigger.title')}
      className={cn(className)}
      data-section="trigger"
    >
      <FormField label={t('editorV2.trigger.kind')}>
        <Select value={trigger.kind} onValueChange={(k) => setKind(k as TriggerKind)}>
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

      {trigger.kind === 'cron' && (
        <div className="grid gap-4 sm:grid-cols-2">
          <FormField label={t('editorV2.trigger.expr')}>
            <Input
              value={trigger.expr}
              onChange={(e) => onChange({ ...trigger, expr: e.target.value })}
              placeholder="0 9 * * *"
              className="font-mono text-sm"
            />
          </FormField>
          <FormField label={t('editorV2.trigger.timezone')}>
            <Input
              value={trigger.timezone}
              onChange={(e) => onChange({ ...trigger, timezone: e.target.value })}
              placeholder="Asia/Shanghai"
            />
          </FormField>
        </div>
      )}

      {trigger.kind === 'interval' && (
        <FormField label={t('editorV2.trigger.everySecs')}>
          <Input
            type="number"
            min={1}
            value={trigger.every_secs}
            onChange={(e) =>
              onChange({
                kind: 'interval',
                every_secs: Math.max(1, Number(e.target.value) || 1),
              })
            }
          />
        </FormField>
      )}
    </EditorSection>
  )
}
