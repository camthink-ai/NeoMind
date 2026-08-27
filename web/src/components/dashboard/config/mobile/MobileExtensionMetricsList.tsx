import { Check } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Extension } from '@/types'

interface MobileExtensionMetricsListProps {
  extension: Extension
  extensionMetricsMap: Map<string, Array<{ name: string; display_name: string; data_type: string; unit?: string }>>
  selectedItems: Set<string>
  onSelectItem: (item: string) => void
  t: (key: string) => string
}

export function MobileExtensionMetricsList({
  extension,
  extensionMetricsMap,
  selectedItems,
  onSelectItem,
  t,
}: MobileExtensionMetricsListProps) {
  const metrics = extensionMetricsMap.get(extension.id) || []

  if (metrics.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4">
        {t('extensions:noMetrics') || 'No metrics available'}
      </div>
    )
  }

  return (
    <div className="p-4 space-y-3">
      {metrics.map(metric => {
        const itemKey = `extension:${extension.id}:produce:${metric.name}`
        const isSelected = selectedItems.has(itemKey)

        return (
          <button
            key={metric.name}
            type="button"
            onClick={() => onSelectItem(itemKey)}
            className={cn(
              'w-full text-left transition-colors duration-fast',
              'group relative rounded-lg border p-4',
              isSelected
                ? 'bg-muted border-border'
                : 'bg-card border-border active:bg-accent'
            )}
          >
            <div className="flex items-center gap-3">
              <div className={cn(
                'shrink-0 w-6 h-6 rounded-full flex items-center justify-center transition-colors',
                isSelected
                  ? 'bg-primary text-primary-foreground'
                  : 'bg-muted text-muted-foreground'
              )}>
                <Check className={cn(
                  'h-4 w-4',
                  isSelected ? 'opacity-100' : 'opacity-0'
                )} />
              </div>
              <div className="flex-1 min-w-0">
                <div className={cn(
                  'text-base font-medium truncate',
                  isSelected ? 'text-foreground' : 'text-foreground'
                )}>
                  {metric.display_name || metric.name}
                </div>
                <div className="text-sm text-muted-foreground truncate">
                  {metric.name}
                  {metric.unit && ` (${metric.unit})`}
                </div>
              </div>
            </div>
          </button>
        )
      })}
    </div>
  )
}
