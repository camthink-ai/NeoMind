/**
 * Binding-state overlays for dashboard widgets.
 *
 * DanglingBindingState replaces the whole card when every bound device has
 * been removed from the registry — a plain "No Data Available" would wrongly
 * suggest the device might come back.
 *
 * StaleDataBadge is a corner dot telling the viewer the displayed value is
 * the last one reported by a device that is currently offline / idle. It is
 * a dot, not a text pill: compact cards have no room for a label that would
 * cover their content. Color follows the 4-state connection model — warning
 * orange for a truly offline device, muted for connectedIdle (transport
 * alive, awaiting data — a calm state the device list already renders blue,
 * not orange). The full explanation, including the age of the oldest stale
 * report, lives in the hover title.
 */

import { useTranslation } from 'react-i18next'
import { Unplug } from 'lucide-react'
import { cn } from '@/lib/utils'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { EmptyState } from './DefaultStates'
import type { StaleDeviceRef } from './useDeviceBindingStatus'

export interface DanglingBindingStateProps {
  deviceIds: string[]
  className?: string
}

export function DanglingBindingState({ deviceIds: _deviceIds, className }: DanglingBindingStateProps) {
  const { t } = useTranslation('dashboardComponents')
  return (
    <EmptyState
      className={cn('border-warning-light', className)}
      icon={<Unplug className="h-8 w-8 text-warning" />}
      message={t('bindingDeviceRemoved', 'Bound device was removed')}
      subMessage={t('bindingDeviceRemovedHint', 'Rebind this component to an existing device')}
    />
  )
}

/** Locale-aware relative age ("5 minutes ago" / "5分钟前") for tooltip lines. */
function formatAge(epochMs: number, language: string): string {
  const rtf = new Intl.RelativeTimeFormat(language || undefined, { numeric: 'auto' })
  const diffSec = Math.round((epochMs - Date.now()) / 1000)
  const abs = Math.abs(diffSec)
  if (abs < 60) return rtf.format(diffSec, 'second')
  if (abs < 3600) return rtf.format(Math.round(diffSec / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(diffSec / 3600), 'hour')
  if (abs < 7 * 86400) return rtf.format(Math.round(diffSec / 86400), 'day')
  return rtf.format(Math.round(diffSec / (7 * 86400)), 'week')
}

export interface StaleDataBadgeProps {
  /** Stale device bindings (offline + connectedIdle), from useDeviceBindingStatus. */
  devices: StaleDeviceRef[]
  className?: string
}

export function StaleDataBadge({ devices, className }: StaleDataBadgeProps) {
  const { t, i18n } = useTranslation('dashboardComponents')
  // Worst state wins — one truly-offline device must not be diluted to a
  // calm dot by idle siblings in a multi-source binding.
  const hasOffline = devices.some((d) => d.state === 'offline')
  const label = hasOffline
    ? t('staleData', 'Last value — device offline')
    : t('staleDataIdle', 'Last value — awaiting data')

  // The viewer cares about the OLDEST stale report — that bounds how old the
  // number on the card can be.
  const seenEpochs = devices
    .map((d) => d.lastSeen)
    .filter((ts): ts is number => ts != null)
  const oldest = seenEpochs.length > 0 ? Math.min(...seenEpochs) : null

  const titleLines = [label]
  if (oldest != null) {
    titleLines.push(t('staleDataLastSeen', { defaultValue: 'Last reported {{age}}', age: formatAge(oldest, i18n.resolvedLanguage ?? i18n.language) }))
  }
  titleLines.push(t('staleDataDevices', { defaultValue: 'Devices: {{names}}', names: devices.map((d) => d.name).join(', ') }))

  return (
    <TooltipProvider delayDuration={150}>
      <Tooltip>
        <TooltipTrigger asChild>
          {/* The 8px dot is impossible to hover reliably — pad the hit area
              out to ~20px and pull it back with negative margin so the visual
              anchor (dot center) stays exactly where the caller positioned
              it. */}
          <span
            role="status"
            aria-label={label}
            className={cn('-m-1.5 inline-flex p-1.5', className)}
          >
            <span
              className={cn(
                // Hollow status ring at minimum weight — 8px, 1px outline.
                // No slash-opacity tokens: nested DEFAULT var tokens don't
                // compile with /opacity (silently invisible).
                'block h-2 w-2 rounded-full border-2 bg-transparent',
                hasOffline ? 'border-accent-orange' : 'border-muted-foreground',
              )}
            />
          </span>
        </TooltipTrigger>
        <TooltipContent side="bottom" align="end" className="max-w-64">
          <p className="text-xs font-medium">{label}</p>
          {titleLines.slice(1).map((line) => (
            <p key={line} className="mt-1 text-xs text-muted-foreground break-words">{line}</p>
          ))}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
