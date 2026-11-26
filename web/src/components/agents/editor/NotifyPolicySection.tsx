/**
 * Notify policy section — on_complete / on_alert / on_failure + channels.
 */
import { useTranslation } from 'react-i18next'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import type { AgentNotify, NotifyPolicy } from '@/types'
import { EditorSection } from './EditorSection'

export interface NotifyPolicySectionProps {
  value: NotifyPolicy
  onChange: (next: NotifyPolicy) => void
  className?: string
}

const DEFAULT_NOTIFY: NotifyPolicy = {
  channels: [],
  on_complete: false,
  on_alert: false,
  alert_depends_on: [],
  on_failure: true,
  pending_confirmation: false,
}

export function normalizeNotify(value?: NotifyPolicy | AgentNotify | null): NotifyPolicy {
  if (!value || typeof value !== 'object') {
    return { ...DEFAULT_NOTIFY, channels: [], alert_depends_on: [] }
  }
  if ('on' in value) {
    const channels = Array.isArray(value.channels) ? [...value.channels] : []
    return {
      channels,
      on_complete: value.on === 'always',
      on_alert: false,
      alert_depends_on: [],
      on_failure: value.on === 'failure' || value.on === 'always',
      pending_confirmation: value.on === 'judgment',
    }
  }
  return {
    ...DEFAULT_NOTIFY,
    ...value,
    channels: Array.isArray(value.channels) ? [...value.channels] : [],
    alert_depends_on: Array.isArray(value.alert_depends_on) ? [...value.alert_depends_on] : [],
  }
}

function FlagRow({
  id,
  label,
  checked,
  onCheckedChange,
}: {
  id: string
  label: string
  checked: boolean
  onCheckedChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
      <Label htmlFor={id} className="text-sm font-medium text-foreground">
        {label}
      </Label>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  )
}

export function NotifyPolicySection({ value, onChange, className }: NotifyPolicySectionProps) {
  const { t } = useTranslation('agents')
  const policy = normalizeNotify(value)

  const patch = (partial: Partial<NotifyPolicy>) => {
    onChange({ ...policy, ...partial })
  }

  return (
    <EditorSection
      title={t('editorV2.notify.title')}
      className={cn(className)}
      data-section="notify"
    >
      <FormField label={t('editorV2.notify.channels')}>
        <Input
          value={policy.channels.join(', ')}
          onChange={(e) =>
            patch({
              channels: e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
          placeholder="test-channel"
          data-testid="notify-channels"
        />
      </FormField>

      <div className="space-y-2" data-testid="notify-flags">
        <FlagRow
          id="notify-on-complete"
          label={t('editorV2.notify.onComplete')}
          checked={!!policy.on_complete}
          onCheckedChange={(on_complete) => patch({ on_complete })}
        />
        <FlagRow
          id="notify-on-alert"
          label={t('editorV2.notify.onAlert')}
          checked={!!policy.on_alert}
          onCheckedChange={(on_alert) => patch({ on_alert })}
        />
        <FlagRow
          id="notify-on-failure"
          label={t('editorV2.notify.onFailure')}
          checked={!!policy.on_failure}
          onCheckedChange={(on_failure) => patch({ on_failure })}
        />
        <FlagRow
          id="notify-pending"
          label={t('editorV2.notify.pendingConfirmation')}
          checked={!!policy.pending_confirmation}
          onCheckedChange={(pending_confirmation) => patch({ pending_confirmation })}
        />
      </div>
    </EditorSection>
  )
}
