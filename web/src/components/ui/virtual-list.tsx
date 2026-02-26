/**
 * Virtual Scroll Components for Large Lists
 * 
 * Usage:
 * ```tsx
 * import { VirtualDeviceList } from '@/components/ui/virtual-list'
 * 
 * function DevicesPage() {
 *   const { data: devices } = useDevices()
 *   return <VirtualDeviceList items={devices || []} itemHeight={50} />
 * }
 * ```
 */

import { useRef, useMemo } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'

export interface VirtualListProps<T> {
  items: T[]
  itemHeight: number
  overscan?: number
  renderItem: (item: T, index: number) => React.ReactNode
  className?: string
  containerHeight?: number | string
}

/**
 * Virtual list component - only renders visible items.
 * Ideal for lists with 100+ items.
 */
export function VirtualList<T>({
  items,
  itemHeight,
  overscan = 5,
  renderItem,
  className = '',
  containerHeight = 400,
}: VirtualListProps<T>) {
  const parentRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => itemHeight,
    overscan,
  })

  return (
    <div
      ref={parentRef}
      className={`overflow-auto ${className}`}
      style={{ 
        height: typeof containerHeight === 'number' ? `${containerHeight}px` : containerHeight,
      }}
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => (
          <div
            key={virtualRow.key}
            data-index={virtualRow.index}
            ref={virtualizer.measureElement}
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              width: '100%',
              transform: `translateY(${virtualRow.start}px)`,
            }}
          >
            {renderItem(items[virtualRow.index], virtualRow.index)}
          </div>
        ))}
      </div>
    </div>
  )
}

/**
 * Virtual grid component for card layouts.
 */
export interface VirtualGridProps<T> {
  items: T[]
  itemHeight: number
  columnCount?: number
  gap?: number
  overscan?: number
  renderItem: (item: T, index: number) => React.ReactNode
  className?: string
  containerHeight?: number | string
}

export function VirtualGrid<T>({
  items,
  itemHeight,
  columnCount = 3,
  gap = 16,
  overscan = 3,
  renderItem,
  className = '',
  containerHeight = 400,
}: VirtualGridProps<T>) {
  const parentRef = useRef<HTMLDivElement>(null)

  // Calculate row height including gap
  const rowHeight = useMemo(() => itemHeight + gap, [itemHeight, gap])
  
  // Calculate total rows
  const rowCount = useMemo(() => Math.ceil(items.length / columnCount), [items.length, columnCount])

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan,
  })

  return (
    <div
      ref={parentRef}
      className={`overflow-auto ${className}`}
      style={{ 
        height: typeof containerHeight === 'number' ? `${containerHeight}px` : containerHeight,
      }}
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const startIndex = virtualRow.index * columnCount
          const endIndex = Math.min(startIndex + columnCount, items.length)
          const rowItems = items.slice(startIndex, endIndex)

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${virtualRow.start}px)`,
                display: 'flex',
                gap: `${gap}px`,
              }}
            >
              {rowItems.map((item, idx) => (
                <div
                  key={startIndex + idx}
                  style={{
                    flex: `0 0 calc(${100 / columnCount}% - ${(columnCount - 1) * gap / columnCount}px)`,
                    height: `${itemHeight}px`,
                  }}
                >
                  {renderItem(item, startIndex + idx)}
                </div>
              ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

/**
 * Simple list component that automatically switches to virtual scrolling for large lists.
 */
export interface SmartListProps<T> {
  items: T[]
  itemHeight: number
  virtualThreshold?: number
  renderItem: (item: T, index: number) => React.ReactNode
  className?: string
  containerHeight?: number | string
}

export function SmartList<T>({
  items,
  itemHeight,
  virtualThreshold = 100,
  renderItem,
  className = '',
  containerHeight = 400,
}: SmartListProps<T>) {
  // Use virtual scrolling only for large lists
  if (items.length >= virtualThreshold) {
    return (
      <VirtualList
        items={items}
        itemHeight={itemHeight}
        overscan={5}
        renderItem={renderItem}
        className={className}
        containerHeight={containerHeight}
      />
    )
  }

  // Use native rendering for small lists
  return (
    <div
      className={`overflow-auto ${className}`}
      style={{ 
        height: typeof containerHeight === 'number' ? `${containerHeight}px` : containerHeight,
      }}
    >
      {items.map((item, index) => (
        <div key={index} style={{ height: `${itemHeight}px` }}>
          {renderItem(item, index)}
        </div>
      ))}
    </div>
  )
}
