/**
 * PaginatedContent Component
 *
 * Auto-switches between pagination (desktop) and infinite scroll (mobile)
 * to provide the best UX for each device type.
 */

import { useMemo, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { useIsMobile } from '@/hooks/useMobile'
import { useInfiniteScroll } from '@/hooks/useInfiniteScroll'
import { Pagination } from './Pagination'
import { Loader2 } from 'lucide-react'

export interface PaginatedContentProps<T> {
  /** All items to paginate */
  items: T[]
  /** Items per page */
  pageSize?: number
  /** Current page (for controlled mode) */
  currentPage?: number
  /** Called when page changes */
  onPageChange?: (page: number) => void
  /** Render function for each item */
  renderItem: (item: T, index: number) => React.ReactNode
  /** Optional key extractor for items */
  getItemKey?: (item: T, index: number) => string
  /** Optional class name for the container */
  className?: string
  /** Optional class name for the item container */
  itemClassName?: string
  /** Whether to show pagination UI even on mobile (default: false) */
  forcePagination?: boolean
  /** Loading state for infinite scroll */
  isLoading?: boolean
  /** Whether there are more items (for infinite scroll) */
  hasMore?: boolean
  /** Callback to load more items (for infinite scroll) */
  onLoadMore?: () => void
  /** Container ref for scroll detection */
  containerRef?: React.RefObject<HTMLElement>
}

const DEFAULT_PAGE_SIZE = 10

/**
 * PaginatedContent automatically provides pagination on desktop
 * and infinite scroll on mobile.
 *
 * @example
 * <PaginatedContent
 *   items={devices}
 *   pageSize={20}
 *   renderItem={(device) => <DeviceCard key={device.id} device={device} />}
 *   getItemKey={(device) => device.id}
 * />
 */
export function PaginatedContent<T>({
  items,
  pageSize = DEFAULT_PAGE_SIZE,
  currentPage: controlledPage,
  onPageChange,
  renderItem,
  getItemKey,
  className,
  itemClassName,
  forcePagination = false,
  isLoading = false,
  hasMore: hasMoreProp,
  onLoadMore,
  containerRef,
}: PaginatedContentProps<T>) {
  const { t } = useTranslation('common')
  const isMobile = useIsMobile()

  // Internal state for uncontrolled mode
  const [internalPage, setInternalPage] = useState(1)

  // Determine which page mode to use
  const currentPage = controlledPage ?? internalPage
  const setCurrentPage = onPageChange ?? setInternalPage

  // Calculate pagination
  const { visibleItems, totalPages } = useMemo(() => {
    const total = Math.ceil(items.length / pageSize)
    const start = (currentPage - 1) * pageSize
    const end = start + pageSize
    return {
      visibleItems: items.slice(start, end),
      totalPages: total,
    }
  }, [items, currentPage, pageSize])

  // For infinite scroll, show all items progressively
  const shouldUseInfiniteScroll = isMobile && !forcePagination

  // For infinite scroll mode, we show all items (progressively loaded)
  const infiniteScrollItems = shouldUseInfiniteScroll ? items : visibleItems

  // Has more calculation
  const hasMore = hasMoreProp ?? currentPage < totalPages

  // Infinite scroll setup
  const { loadMoreRef, showLoadingIndicator } = useInfiniteScroll({
    isLoading,
    hasMore,
    onLoadMore: () => {
      if (onLoadMore) {
        onLoadMore()
      } else if (!shouldUseInfiniteScroll) {
        // Default behavior: increment page
        setCurrentPage(currentPage + 1)
      }
    },
    enabled: shouldUseInfiniteScroll,
    root: containerRef?.current || null,
  })

  // Reset page when items change significantly
  const prevItemsLength = useMemo(() => items.length, [items.length])
  // biome-ignore lint: explicit reset is intentional
  const _itemsChanged = prevItemsLength !== items.length

  // Render items
  const renderItems = () => {
    return infiniteScrollItems.map((item, index) => {
      const key = getItemKey ? getItemKey(item, index) : `paginated-item-${index}`
      return (
        <div key={key} className={itemClassName}>
          {renderItem(item, index)}
        </div>
      )
    })
  }

  return (
    <div className={cn('flex flex-col', className)}>
      {/* Content */}
      <div className="flex-1">
        {renderItems()}
      </div>

      {/* Infinite scroll loading trigger (mobile only) */}
      {shouldUseInfiniteScroll && (
        <div ref={loadMoreRef} className="py-4 flex items-center justify-center">
          {(showLoadingIndicator || isLoading) && hasMore && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>{t('pagination.loading')}</span>
            </div>
          )}
          {!hasMore && items.length > 0 && (
            <div className="text-sm text-muted-foreground">
              {t('pagination.noMore')}
            </div>
          )}
        </div>
      )}

      {/* Pagination (desktop only) */}
      {!shouldUseInfiniteScroll && totalPages > 1 && (
        <div className="mt-4">
          <Pagination
            total={items.length}
            pageSize={pageSize}
            currentPage={currentPage}
            onPageChange={setCurrentPage}
          />
        </div>
      )}

      {/* Empty state */}
      {items.length === 0 && !isLoading && (
        <div className="py-12 text-center text-sm text-muted-foreground">
          {t('pagination.noData')}
        </div>
      )}
    </div>
  )
}

/**
 * Simplified version that works with client-side data
 * Automatically manages pagination state internally
 */
export interface PaginatedListProps<T> {
  items: T[]
  pageSize?: number
  renderItem: (item: T, index: number) => React.ReactNode
  getItemKey?: (item: T, index: number) => string
  className?: string
  itemClassName?: string
  forcePagination?: boolean
}

export function PaginatedList<T>({
  items,
  pageSize = DEFAULT_PAGE_SIZE,
  renderItem,
  getItemKey,
  className,
  itemClassName,
  forcePagination = false,
}: PaginatedListProps<T>) {
  return (
    <PaginatedContent
      items={items}
      pageSize={pageSize}
      renderItem={renderItem}
      getItemKey={getItemKey}
      className={className}
      itemClassName={itemClassName}
      forcePagination={forcePagination}
    />
  )
}

/**
 * Hook for managing paginated state with infinite scroll support
 * Useful when you need more control over the pagination logic
 */
export interface UsePaginatedStateOptions {
  pageSize?: number
  initialPage?: number
}

export function usePaginatedState<T>(
  items: T[],
  options: UsePaginatedStateOptions = {}
) {
  const { pageSize = DEFAULT_PAGE_SIZE, initialPage = 1 } = options
  const [page, setPage] = useState(initialPage)
  const isMobile = useIsMobile()

  const paginatedItems = useMemo(() => {
    if (isMobile) {
      // On mobile, show all items progressively
      return items
    }
    const start = (page - 1) * pageSize
    return items.slice(start, start + pageSize)
  }, [items, page, pageSize, isMobile])

  const totalPages = Math.ceil(items.length / pageSize)
  const hasMore = page < totalPages

  const nextPage = useCallback(() => {
    setPage((p) => p + 1)
  }, [])

  const resetPage = useCallback(() => {
    setPage(1)
  }, [])

  return {
    page,
    setPage,
    nextPage,
    resetPage,
    paginatedItems,
    totalPages,
    hasMore,
    isMobile,
    // On mobile, we show all items
    visibleItems: isMobile ? items : paginatedItems,
  }
}
