/**
 * Notify policy section — on_complete / on_alert / on_failure + channels.
 */
import { useTranslation } from 'react-i18next'
import { Bell } from 'lucide-react'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { cardBase } from '@/design-system/tokens/size'
import { cn } from '@/lib/utils'
import type { NotifyPolicy } from '@/types'

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

export function normalizeNotify(value?: NotifyPolicy | null): NotifyPolicy {
  return {
    ...DEFAULT_NOTIFY,
    ...value,
    channels: [...(value?.channels ?? [])],
    alert_depends_on: [...(value?.alert_depends_on ?? [])],
  }
}

function FlagRow({
  id,
  label,
  description,
  checked,
  onCheckedChange,
}: {
  id: string
  label: string
  description: string
  checked: boolean
  onCheckedChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-lg border border-border px-3 py-2.5">
      <div className="min-w-0 space-y-0.5">
        <Label htmlFor={id} className="text-sm font-medium text-foreground">
          {label}
        </Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
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
    <section className={cn(cardBase, 'p-4 space-y-4', className)} data-section="notify">
      <header className="flex items-center gap-2">
        <Bell className="h-4 w-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">
          {t('editorV2.notify.title')}
        </h3>
      </header>

      <FormField
        label={t('editorV2.notify.channels')}
        helpText={t('editorV2.notify.channelsHint')}
      >
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
          description={t('editorV2.notify.onCompleteDesc')}
          checked={!!policy.on_complete}
          onCheckedChange={(on_complete) => patch({ on_complete })}
        />
        <FlagRow
          id="notify-on-alert"
          label={t('editorV2.notify.onAlert')}
          description={t('editorV2.notify.onAlertDesc')}
          checked={!!policy.on_alert}
          onCheckedChange={(on_alert) => patch({ on_alert })}
        />
        <FlagRow
          id="notify-on-failure"
          label={t('editorV2.notify.onFailure')}
          description={t('editorV2.notify.onFailureDesc')}
          checked={!!policy.on_failure}
          onCheckedChange={(on_failure) => patch({ on_failure })}
        />
        <FlagRow
          id="notify-pending"
          label={t('editorV2.notify.pendingConfirmation')}
          description={t('editorV2.notify.pendingConfirmationDesc')}
          checked={!!policy.pending_confirmation}
          onCheckedChange={(pending_confirmation) => patch({ pending_confirmation })}
        />
      </div>
    </section>
  )
}
