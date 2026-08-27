import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import {
  getDeviceState,
  getDeviceStateColor,
  type DeviceStateInfo,
} from '@/lib/utils/deviceStatus'
import type { Device } from '@/types'

export interface DeviceStatusBadgeProps {
  device: Pick<Device, 'online' | 'transport_connected' | 'last_seen'>
  className?: string
  /** Hide the leading dot (useful in dense table cells). */
  hideDot?: boolean
}

/**
 * 4-state device status badge.
 *
 * Renders Online / Connected·Standby / Offline / Never Connected using the
 * backend-supplied `transport_connected` + `online` + `last_seen` signals.
 * Falls back to legacy 3-state rendering when `transport_connected` is
 * undefined (older backend or external broker without $SYS).
 */
export function DeviceStatusBadge({ device, className, hideDot }: DeviceStatusBadgeProps) {
  const { t } = useTranslation('common')
  const info: DeviceStateInfo = getDeviceState(device)
  const color = getDeviceStateColor(info.state)
  // i18n key — `connectedIdle` is a synthetic status key not present on the
  // backend `status` string, so we look it up under `statusLabels.*`.
  const label = t(`statusLabels.${info.state}`, {
    defaultValue: info.state,
  })

  const variantClass = {
    success: 'badge-success',
    info: 'badge-info',
    warning: 'badge-warning',
    muted: 'bg-muted text-muted-foreground',
  }[color]

  const dotClass = {
    success: 'bg-success animate-pulse',
    info: 'bg-info',
    warning: 'bg-warning',
    // Hollow dot = never reported; filled gray = was online, now offline.
    muted: info.state === 'disconnected'
      ? 'border-[1.5px] border-muted-foreground bg-transparent'
      : 'bg-muted-foreground',
  }[color]

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-xs font-medium transition-colors',
        variantClass,
        className,
      )}
    >
      {!hideDot && <span className={cn('w-1.5 h-1.5 rounded-full', dotClass)} />}
      {label}
    </span>
  )
}
