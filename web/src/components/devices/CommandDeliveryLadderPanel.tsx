/**
 * Read-only command delivery ladder (1.3 iter D).
 * Green "verified" only when physical effect is evidenced.
 */
import { useTranslation } from 'react-i18next'
import {
  CheckCircle2,
  Circle,
  HelpCircle,
  MinusCircle,
  AlertTriangle,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { textMini, textNano } from '@/design-system/tokens/typography'
import {
  TIER_ORDER,
  deriveCommandDeliveryLadder,
  type CommandDeliveryJson,
  type CommandDeliveryTier,
  type SendCommandDeliveryResult,
  type TierState,
} from '@/lib/commandDeliveryLadder'

export interface CommandDeliveryLadderPanelProps {
  /** Full send response or bare delivery payload. */
  result: SendCommandDeliveryResult | CommandDeliveryJson | null
  className?: string
  compact?: boolean
}

function tierIcon(state: TierState) {
  switch (state) {
    case 'reached':
      return CheckCircle2
    case 'not_reached':
      return Circle
    case 'not_applicable':
      return MinusCircle
    case 'unknown':
      return HelpCircle
  }
}

function tierTone(state: TierState, verifiedGreen: boolean, isPhysical: boolean): string {
  if (state === 'reached') {
    if (isPhysical && verifiedGreen) return 'text-success'
    return 'text-warning'
  }
  if (state === 'unknown') return 'text-muted-foreground'
  if (state === 'not_applicable') return 'text-muted-foreground'
  return 'text-muted-foreground'
}

export function CommandDeliveryLadderPanel({
  result,
  className,
  compact,
}: CommandDeliveryLadderPanelProps) {
  const { t } = useTranslation('devices')
  const ladder = deriveCommandDeliveryLadder(result)
  const bannerTone = ladder.allowVerifiedSuccess
    ? 'border-success text-success'
    : ladder.dispatched
      ? 'border-warning text-warning'
      : 'border-border text-muted-foreground'
  const BannerIcon = ladder.allowVerifiedSuccess
    ? CheckCircle2
    : ladder.dispatched
      ? AlertTriangle
      : HelpCircle

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
    <div
      className={cn('space-y-2', className)}
      data-delivery-kind={ladder.reachedKind ?? 'none'}
      data-verified={ladder.allowVerifiedSuccess ? 'true' : 'false'}
    >
      <div className={cn('rounded-lg border bg-card p-3', bannerTone)}>
        <div className="flex items-start gap-2">
          <BannerIcon className="h-4 w-4 mt-0.5 shrink-0" aria-hidden="true" />
          <div className="min-w-0 space-y-1">
            <p className="text-sm font-medium">
              {t(`devices:${summaryKey}`)}
              {ladder.adapters != null && ladder.reachedKind === 'adapter_accepted'
                ? ` (${t('devices:commandDelivery.adapters', { count: ladder.adapters })})`
                : null}
            </p>
            {ladder.deliveryNote && (
              <p className={cn(textMini, 'text-muted-foreground font-mono break-words')}>
                {ladder.deliveryNote}
              </p>
            )}
            {!ladder.allowVerifiedSuccess && ladder.dispatched && (
              <p className={cn(textNano, 'text-muted-foreground')}>
                {t('devices:commandDelivery.noElevateHint')}
              </p>
            )}
          </div>
        </div>
      </div>

      <ol className={cn('rounded-lg border border-border bg-card', compact ? 'p-2' : 'p-3')}>
        {TIER_ORDER.map((tier: CommandDeliveryTier) => {
          const state = ladder.tiers[tier]
          const Icon = tierIcon(state)
          const isPhysical = tier === 'physical_effect'
          return (
            <li
              key={tier}
              className="flex items-center gap-2 py-1.5 first:pt-0 last:pb-0"
              data-tier={tier}
              data-tier-state={state}
            >
              <Icon
                className={cn(
                  'h-3.5 w-3.5 shrink-0',
                  tierTone(state, ladder.allowVerifiedSuccess, isPhysical),
                )}
                aria-hidden="true"
              />
              <span className={cn(textMini, 'font-medium')}>
                {t(`devices:commandDelivery.tier.${tier}`)}
              </span>
              <span className={cn(textNano, 'ml-auto text-muted-foreground')}>
                {t(`devices:commandDelivery.state.${state}`)}
              </span>
            </li>
          )
        })}
      </ol>
    </div>
  )
}
