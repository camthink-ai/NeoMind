/**
 * Binding-state overlays for dashboard widgets.
 *
 * DanglingBindingState replaces the whole card when every bound device has
 * been removed from the registry — a plain "No Data Available" would wrongly
 * suggest the device might come back.
 *
 * StaleDataBadge is a corner pill telling the viewer the displayed value is
 * the last one reported by a device that is currently offline / idle.
 */

import { useTranslation } from 'react-i18next'
import { History, Unplug } from 'lucide-react'
import { cn } from '@/lib/utils'
import { EmptyState } from './DefaultStates'

export interface DanglingBindingStateProps {
  deviceIds: string[]
  className?: string
}

export function DanglingBindingState({ deviceIds, className }: DanglingBindingStateProps) {
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

export interface StaleDataBadgeProps {
  /** Device IDs the stale value comes from — surfaced in the hover title. */
  deviceIds: string[]
  className?: string
}

export function StaleDataBadge({ deviceIds, className }: StaleDataBadgeProps) {
  const { t } = useTranslation('dashboardComponents')
  const label = t('staleData', 'Last value — device offline')
  return (
    <span
      title={`${label} (${deviceIds.join(', ')})`}
      className={cn(
        'inline-flex items-center gap-1 rounded-full bg-warning-light text-warning',
        'px-1.5 py-0.5 text-nano font-medium pointer-events-none',
        className,
      )}
    >
      <History className="h-3 w-3 shrink-0" />
      <span className="truncate max-w-28">{label}</span>
    </span>
  )
}
