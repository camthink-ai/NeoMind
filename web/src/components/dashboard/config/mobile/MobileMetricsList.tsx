import { Check } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { MetricDefinition } from '@/types'
import { ItemBadge } from '../shared'

interface MobileMetricsListProps {
  device: any
  deviceMetricsMap: Map<string, MetricDefinition[]>
  summaries: Map<string, any>
  availability: Map<string, { hasData: boolean; dataPointCount?: number }>
  checkingData: boolean
  getDeviceInfoProperties: (t: (key: string) => string) => Array<{ id: string; name: string }>
  selectedItems: Set<string>
  onSelectItem: (item: string) => void
  t: (key: string) => string
}

export function MobileMetricsList({
  device,
  deviceMetricsMap,
  summaries,
  availability,
  checkingData,
  getDeviceInfoProperties,
  selectedItems,
  onSelectItem,
  t,
}: MobileMetricsListProps) {
  const metrics = deviceMetricsMap.get(device.id) || []
  const deviceSummary = summaries.get(device.id) || {}
  const templateMetricNames = new Set(metrics.map((m: MetricDefinition) => m.name))

  type Item = {
    key: string
    propertyName: string
    propertyDisplayName: string
    currentValue?: unknown
    isSelected: boolean
    hasData: boolean | null
    dataPointCount?: number
    itemType: 'template' | 'virtual' | 'info'
    unit?: string
  }

  const items: Item[] = []

  // Template metrics
  for (const metric of metrics) {
    const itemKey = `device:${device.id}:${metric.name}`
    const availabilityKey = `${device.id}:${metric.name}`
    const metricAvailability = availability.get(availabilityKey)
    items.push({
      key: itemKey,
      propertyName: metric.name,
      propertyDisplayName: metric.display_name || metric.name,
      currentValue: device.current_values?.[metric.name],
      isSelected: selectedItems.has(itemKey),
      hasData: metricAvailability?.hasData ?? null,
      dataPointCount: metricAvailability?.dataPointCount,
      itemType: 'template',
      unit: metric.unit,
    })
  }

  // Virtual metrics
  for (const [metricId, metricSummary] of Object.entries(deviceSummary)) {
    const summary = metricSummary as { is_virtual?: boolean; display_name?: string; current?: unknown; unit?: string }
    if (!templateMetricNames.has(metricId) && summary.is_virtual) {
      const itemKey = `device:${device.id}:${metricId}`
      const availabilityKey = `${device.id}:${metricId}`
      const metricAvailability = availability.get(availabilityKey)
      items.push({
        key: itemKey,
        propertyName: metricId,
        propertyDisplayName: summary.display_name || metricId,
        currentValue: summary.current,
        isSelected: selectedItems.has(itemKey),
        hasData: metricAvailability?.hasData ?? null,
        dataPointCount: metricAvailability?.dataPointCount,
        itemType: 'virtual',
        unit: summary.unit,
      })
    }
  }

  // Device info properties
  for (const infoProp of getDeviceInfoProperties(t)) {
    const itemKey = `device:${device.id}:${infoProp.id}`
    let currentValue: unknown = undefined

    switch (infoProp.id) {
      case 'name': currentValue = device.name; break
      case 'status': currentValue = device.status; break
      case 'online': currentValue = device.online; break
      case 'last_seen': currentValue = device.last_seen; break
      case 'device_type': currentValue = device.device_type; break
      case 'plugin_name': currentValue = device.plugin_name; break
      case 'adapter_id': currentValue = device.adapter_id; break
    }

    items.push({
      key: itemKey,
      propertyName: infoProp.id,
      propertyDisplayName: infoProp.name,
      currentValue,
      isSelected: selectedItems.has(itemKey),
      hasData: null,
      itemType: 'info',
    })
  }

  // Sort: template -> info -> virtual
  items.sort((a, b) => {
    const order = { template: 0, info: 1, virtual: 2 }
    return order[a.itemType] - order[b.itemType]
  })

  const formatValue = (val: unknown): string => {
    if (val === null || val === undefined) return '-'
    if (typeof val === 'number') return val.toLocaleString('en-US', { maximumFractionDigits: 2 })
    if (typeof val === 'boolean') return val ? t('dataSource.yes') : t('dataSource.no')
    return String(val)
  }

  return (
    <div className="p-4 space-y-3">
      {items.map(item => (
        <button
          key={item.key}
          type="button"
          onClick={() => onSelectItem(item.key)}
          className={cn(
            'w-full text-left transition-colors duration-fast',
            'group relative rounded-lg border p-4',
            item.isSelected
              ? 'bg-muted border-border'
              : 'bg-card border-border active:bg-accent'
          )}
        >
          <div className="flex items-start gap-3">
            {/* Check icon */}
            <div className={cn(
              'shrink-0 w-6 h-6 rounded-full flex items-center justify-center transition-colors mt-0.5',
              item.isSelected
                ? 'bg-primary text-primary-foreground'
                : 'bg-muted text-muted-foreground'
            )}>
              <Check className={cn(
                'h-4 w-4',
                item.isSelected ? 'opacity-100' : 'opacity-0'
              )} />
            </div>

            {/* Content */}
            <div className="flex-1 min-w-0 space-y-2">
              {/* Header */}
              <div className="flex items-center gap-2 flex-wrap">
                <ItemBadge itemType={item.itemType} t={t} />
                <span className={cn(
                  'text-base font-medium',
                  item.isSelected ? 'text-foreground' : 'text-foreground'
                )}>
                  {item.propertyDisplayName}
                </span>
              </div>

              {/* Subtitle */}
              <div className="space-y-1">
                <code className="text-xs text-muted-foreground px-2 py-1 bg-muted rounded-md block">
                  {item.propertyName}
                </code>
                {item.currentValue !== undefined && item.currentValue !== null && (
                  <div className="text-sm text-muted-foreground break-all">
                    {t('dataSource.current')}: <span className="text-foreground font-medium" title={formatValue(item.currentValue)}>{formatValue(item.currentValue)}</span>
                    {item.unit && item.unit !== '-' && <span className="ml-1 text-muted-foreground">{item.unit}</span>}
                  </div>
                )}
              </div>
            </div>

            {/* Data indicator */}
            {item.hasData !== null && (
              <div className="shrink-0">
                {item.hasData ? (
                  <div className="px-2 py-1 rounded-lg bg-success-light border border-success-light text-xs text-success font-medium" title={`${t('dataSource.hasHistoricalData')} (${item.dataPointCount ?? 0} ${t('dataSource.dataPoints')})`}>
                    {item.dataPointCount ?? 0}
                  </div>
                ) : (
                  <div className="px-2 py-1 rounded-lg bg-muted-30 border border-muted text-xs text-muted-foreground">
                    {t('dataSource.noData')}
                  </div>
                )}
              </div>
            )}
          </div>
        </button>
      ))}
    </div>
  )
}
