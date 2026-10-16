import { Circle } from 'lucide-react'
import { cn } from '@/lib/utils'
import { textNano } from '@/design-system/tokens/typography'

export function ItemBadge({ itemType, t }: { itemType: 'template' | 'virtual' | 'runtime' | 'info'; t: (key: string) => string }) {
  const config = {
    template: { label: t('dataSource.badgeTemplate'), className: 'bg-info-light text-info border-info' },
    virtual: { label: t('dataSource.badgeVirtual'), className: 'bg-accent-purple-light text-accent-purple border-accent-purple-light' },
    runtime: { label: t('dataSource.badgeRuntime'), className: 'bg-muted-30 text-muted-foreground border-muted' },
    info: { label: t('dataSource.badgeInfo'), className: 'bg-warning-light text-warning border-warning' },
  }[itemType]
  return (
    <span className={cn('px-1.5 py-0.5', textNano, 'font-medium rounded-[3px] border shrink-0', config.className)}>
      {config.label}
    </span>
  )
}

export function DataIndicator({ hasData, count, t }: { hasData: boolean | null; count?: number; t: (key: string) => string }) {
  if (hasData === true) {
    return (
      <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-success-light border border-success-light" title={`${t('dataSource.hasHistoricalData')} (${count ?? 0} ${t('dataSource.dataPoints')})`}>
        <Circle className="h-1.5 w-1.5 fill-success text-success" />
        <span className={cn(textNano, "text-success font-medium")}>{count ?? 0}</span>
      </div>
    )
  }
  if (hasData === false) {
    return (
      <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-muted-30 border border-muted" title={t('dataSource.noHistoricalData')}>
        <Circle className="h-1.5 w-1.5 fill-muted-foreground text-muted-foreground" />
        <span className={cn(textNano, "text-muted-foreground")}>{t('dataSource.noData')}</span>
      </div>
    )
  }
  return null
}
