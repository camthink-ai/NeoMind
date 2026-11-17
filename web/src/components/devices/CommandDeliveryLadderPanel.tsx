/**
 * One-line command delivery status.
 * Green only when a physical effect is evidenced. Dispatch is not a device ACK.
 */
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import {
  deriveCommandDeliveryLadder,
  type CommandDeliveryJson,
  type SendCommandDeliveryResult,
} from '@/lib/commandDeliveryLadder'

export interface CommandDeliveryLadderPanelProps {
  /** Full send response or bare delivery payload. */
  result: SendCommandDeliveryResult | CommandDeliveryJson | null
  className?: string
}

export function CommandDeliveryLadderPanel({
  result,
  className,
}: CommandDeliveryLadderPanelProps) {
  const { t } = useTranslation('devices')
  const ladder = deriveCommandDeliveryLadder(result)

  const summaryKey = ladder.allowVerifiedSuccess
    ? 'commandDelivery.summaryVerified'
    : ladder.reachedKind === 'adapter_accepted'
      ? 'commandDelivery.summaryAdapter'
      : ladder.reachedKind === 'extension_accepted'
        ? 'commandDelivery.summaryExtension'
        : ladder.reachedKind === 'device_acknowledged'
          ? 'commandDelivery.summaryDeviceAck'
          : ladder.dispatched
            ? 'commandDelivery.summaryUnknown'
            : 'commandDelivery.summaryNone'

  return (
    <p
      className={cn(
        'text-sm',
        ladder.allowVerifiedSuccess ? 'text-success' : 'text-muted-foreground',
        className,
      )}
      data-delivery-kind={ladder.reachedKind ?? 'none'}
      data-verified={ladder.allowVerifiedSuccess ? 'true' : 'false'}
    >
      {t(`devices:${summaryKey}`)}
    </p>
  )
}
