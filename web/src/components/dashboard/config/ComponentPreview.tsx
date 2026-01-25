/**
 * Component Preview
 *
 * Shows a live preview of a dashboard component with real-time data.
 * Used in the configuration dialog to visualize changes as they are made.
 * Uses the component's actual default size from the registry.
 */

import { memo } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Eye, EyeOff, AlertCircle } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import ComponentRenderer from '@/components/dashboard/registry/ComponentRenderer'
import { getComponentMeta } from '@/components/dashboard/registry/registry'
import type { DashboardComponent, DataSource, ImplementedComponentType } from '@/types/dashboard'

export interface ComponentPreviewProps {
  componentType: string
  config: Record<string, unknown>
  dataSource?: DataSource
  title?: string
  showHeader?: boolean
  className?: string
}

// Grid cell height in pixels (typical dashboard grid)
const GRID_CELL_HEIGHT = 80

// Minimum preview height in pixels
const MIN_PREVIEW_HEIGHT = 140

/**
 * Format data source label for display
 */
function formatDataSourceLabel(ds: DataSource | undefined): string {
  if (!ds) return '无数据源'

  switch (ds.type) {
    case 'device':
      return `设备: ${ds.deviceId}${ds.property ? ` (${ds.property})` : ''}`
    case 'device-info':
      return `设备信息: ${ds.deviceId}${ds.property ? ` (${ds.property})` : ''}`
    case 'metric':
      return `指标: ${ds.metricId || '未指定'}`
    case 'command':
      return `指令: ${ds.deviceId} → ${ds.command || 'toggle'}`
    case 'telemetry':
      return `遥测: ${ds.deviceId} / ${ds.metricId || 'raw'}`
    case 'api':
      return `API: ${ds.endpoint || '自定义'}`
    case 'websocket':
      return `WebSocket: ${ds.endpoint || '实时'}`
    case 'static':
      return `静态: ${JSON.stringify(ds.staticValue)?.slice(0, 20) || '值'}`
    default:
      return '未知类型'
  }
}

export const ComponentPreview = memo(function ComponentPreview({
  componentType,
  config,
  dataSource,
  title,
  showHeader = true,
  className,
}: ComponentPreviewProps) {
  const meta = getComponentMeta(componentType as ImplementedComponentType)

  // Try to fetch real data for preview
  const { data, loading, error } = useDataSource(dataSource, {
    // Only fetch if we have a valid data source
    enabled: !!dataSource && meta?.hasDataSource,
  })

  // Use component's default size from registry
  const defaultW = meta?.sizeConstraints.defaultW ?? 4
  const defaultH = meta?.sizeConstraints.defaultH ?? 3

  // Build a mock component for rendering with actual default size
  const mockComponent: DashboardComponent = {
    id: 'preview',
    type: componentType as ImplementedComponentType,
    position: { x: 0, y: 0, w: defaultW, h: defaultH },
    title: title || config.title as string || '预览',
    config,
    dataSource,
  }

  // Calculate preview height based on component's default grid height
  // Each grid cell is approximately GRID_CELL_HEIGHT pixels
  const calculatedHeight = defaultH * GRID_CELL_HEIGHT + 32 // +32 for padding
  const previewHeight = Math.max(MIN_PREVIEW_HEIGHT, calculatedHeight)

  const hasData = !loading && !error && !!dataSource
  const hasError = !!error

  return (
    <div className={cn('flex flex-col', className)}>
      {/* Header */}
      {showHeader && (
        <div className="flex items-center justify-between px-3 py-2 border-b bg-muted/30 shrink-0">
          <div className="flex items-center gap-2">
            <Eye className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">预览</span>
          </div>
          <div className="flex items-center gap-1.5">
            {/* Data source indicator */}
            {meta?.hasDataSource && (
              <div className={cn(
                'flex items-center gap-1 text-xs',
                hasError ? 'text-destructive' : hasData ? 'text-green-600' : 'text-muted-foreground'
              )}>
                {hasError ? (
                  <AlertCircle className="h-3 w-3" />
                ) : hasData ? (
                  <Eye className="h-3 w-3" />
                ) : (
                  <EyeOff className="h-3 w-3" />
                )}
                <span className="max-w-[120px] truncate">
                  {formatDataSourceLabel(dataSource)}
                </span>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Preview area - dynamic height based on component size */}
      <div
        className="flex-1 p-3 bg-muted/10 overflow-hidden"
        style={{ height: `${previewHeight}px` }}
      >
        {loading ? (
          <div className="w-full h-full flex items-center justify-center">
            <Skeleton className="w-full h-full" />
          </div>
        ) : hasError ? (
          <div className="w-full h-full flex flex-col items-center justify-center text-muted-foreground p-4 text-center">
            <AlertCircle className="h-8 w-8 text-destructive/60 mb-2" />
            <p className="text-sm">预览数据加载失败</p>
            <p className="text-xs text-muted-foreground/60 mt-1">使用静态数据预览</p>
          </div>
        ) : (
          <div className="w-full h-full p-2">
            <ComponentRenderer component={mockComponent} />
          </div>
        )}
      </div>

      {/* Footer with component info */}
      <div className="px-3 py-2 border-t bg-muted/20 shrink-0">
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{meta?.name || componentType}</span>
          <span className="text-muted-foreground/60">
            {defaultW}×{defaultH}
          </span>
          {dataSource && (
            <span className={cn(
              'flex items-center gap-1',
              hasError ? 'text-destructive' : 'text-green-600'
            )}>
              {hasError ? '数据错误' : hasData ? '实时数据' : '无数据'}
            </span>
          )}
        </div>
      </div>
    </div>
  )
})
