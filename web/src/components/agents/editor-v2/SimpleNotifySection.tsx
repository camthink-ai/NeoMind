/**
 * Optional notify — plain complete/failure toggles + channel chips.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import { channelsApi } from '@/lib/api/channels'
import type { AlertChannel, NotifyPolicy } from '@/types'
import { normalizeNotify } from './NotifyPolicySection'

export interface SimpleNotifySectionProps {
  value: NotifyPolicy
  onChange: (next: NotifyPolicy) => void
  className?: string
}

export function SimpleNotifySection({ value, onChange, className }: SimpleNotifySectionProps) {
  const { t } = useTranslation('agents')
  const policy = normalizeNotify(value)
  const [channels, setChannels] = useState<AlertChannel[]>([])

  useEffect(() => {
    let cancelled = false
    void channelsApi
      .listMessageChannels()
      .then((resp) => {
        if (cancelled) return
        setChannels(resp.channels ?? [])
      })
      .catch(() => {
        if (!cancelled) setChannels([])
      })
    return () => {
      cancelled = true
    }
  }, [])

  const patch = (partial: Partial<NotifyPolicy>) => {
    onChange({ ...policy, ...partial })
  }

  const toggleChannel = (name: string) => {
    const on = policy.channels.includes(name)
    const next = on
      ? policy.channels.filter((c) => c !== name)
      : [...policy.channels, name]
    patch({ channels: next })
  }

  return (
    <div className={cn('space-y-4', className)} data-section="notify-simple">
      <div className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
        <div className="min-w-0 space-y-0.5">
          <Label htmlFor="simple-notify-complete" className="text-sm font-medium">
            {t('editorV2.notifySimple.onComplete')}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t('editorV2.notifySimple.onCompleteDesc')}
          </p>
        </div>
        <Switch
          id="simple-notify-complete"
          checked={!!policy.on_complete}
          onCheckedChange={(on_complete) => patch({ on_complete })}
        />
      </div>

      <div className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
        <div className="min-w-0 space-y-0.5">
          <Label htmlFor="simple-notify-failure" className="text-sm font-medium">
            {t('editorV2.notifySimple.onFailure')}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t('editorV2.notifySimple.onFailureDesc')}
          </p>
        </div>
        <Switch
          id="simple-notify-failure"
          checked={!!policy.on_failure}
          onCheckedChange={(on_failure) => patch({ on_failure })}
        />
      </div>

      {(policy.on_complete || policy.on_failure) && (
        <div className="space-y-2">
          <p className="text-sm font-medium text-foreground">
            {t('editorV2.notifySimple.channels')}
          </p>
          {channels.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t('editorV2.notifySimple.noChannels')}</p>
          ) : (
            <div className="flex flex-wrap gap-2">
              {channels.map((ch) => {
                const on = policy.channels.includes(ch.name)
                return (
                  <Button
                    key={ch.name}
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => toggleChannel(ch.name)}
                    className={cn(
                      'gap-1.5',
                      on ? 'border-primary bg-muted text-foreground' : 'text-muted-foreground',
                      !ch.enabled && 'opacity-50',
                    )}
                  >
                    {on && <Check className="h-3.5 w-3.5" />}
                    {ch.name}
                  </Button>
                )
              })}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
