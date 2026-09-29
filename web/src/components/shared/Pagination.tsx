import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ChevronLeft, ChevronRight, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { useIsMobile } from '@/hooks/useMobile'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useInfiniteScroll } from '@/hooks/useInfiniteScroll'

export interface PaginationProps {
  total: number
  pageSize: number
  currentPage: number
  onPageChange: (page: number) => void
  showSizeChanger?: boolean
  pageSizeOptions?: number[]
  onPageSizeChange?: (size: number) => void
  className?: string
}

/**
 * Mobile infinite scroll sentinel rendered via React Portal into the page
 * scroll container's inner content wrapper.
 *
 * Uses IntersectionObserver with `root: null` (viewport) so it works regardless
 * of which element actually scrolls (window vs. internal overflow container).
 * The sentinel is portaled into the scroll container's first child so it
 * appears at the bottom of the list, not trapped inside a hidden footer.
 */
function MobileInfiniteSentinel({
  isLoading,
  hasMore,
  onLoadMore,
  containerSelector,
  total,
  t,
}: {
  isLoading: boolean
  hasMore: boolean
  onLoadMore: () => void
  containerSelector: string
  total: number
  t: (k: string, opts?: Record<string, unknown>) => string
}) {
  const [mountNode, setMountNode] = useState<HTMLElement | null>(null)

  // Locate the infinite-scroll mount node. PageLayout renders
  // [data-infinite-scroll-mount] inside the page inner wrapper, BEFORE the
  // bottom spacer — portaling into it keeps the hint clear of the fixed
  // bottom nav. Retry for ~30 frames (~500ms) to handle conditional
  // rendering timing.
  useEffect(() => {
    let raf = 0
    let n = 0
    const find = () => {
      const container = document.querySelector(containerSelector) as HTMLElement | null
      if (container) {
        setMountNode(container)
        return true
      }
      return false
    }
    if (find()) return
    const tick = () => {
      if (find() || ++n > 30) return
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [containerSelector])

  const { loadMoreRef, showLoadingIndicator } = useInfiniteScroll({
    isLoading,
    hasMore,
    onLoadMore,
    enabled: !!mountNode, // only observe once portal target is ready
    root: null, // viewport — independent of which element scrolls
    threshold: 250,
  })

  if (!mountNode) return null

  return createPortal(
    <div className="relative py-4 flex items-center justify-center gap-2 text-sm text-muted-foreground">
      {showLoadingIndicator && hasMore && (
        <>
          <Loader2 className="h-4 w-4 animate-spin" />
          <span>{t('pagination.loading')}</span>
        </>
      )}
      {!hasMore && total > 0 && <span>{t('pagination.noMore')}</span>}
      {/* 1px IntersectionObserver sentinel — absolutely positioned so its
          w-full never shoves the hint text off-center. */}
      <div ref={loadMoreRef} aria-hidden className="absolute bottom-0 left-0 h-px w-full" />
    </div>,
    mountNode,
  )
}

/**
 * Pagination component with page size changer
 *
 * @example
 * <Pagination
 *   total={100}
 *   pageSize={10}
 *   currentPage={1}
 *   onPageChange={(page) => setCurrentPage(page)}
 *   showSizeChanger
 *   onPageSizeChange={(size) => setPageSize(size)}
 * />
 */
export function Pagination({
  total,
  pageSize,
  currentPage,
  onPageChange,
  showSizeChanger = false,
  pageSizeOptions = [10, 20, 50, 100],
  onPageSizeChange,
  className,
  // Mobile infinite scroll props
  isLoading = false,
  onLoadMore,
  hideOnMobile = true, // Hide pagination UI on mobile when using infinite scroll
}: PaginationProps & { isLoading?: boolean; onLoadMore?: () => void; hideOnMobile?: boolean }) {
  const { t } = useTranslation('common')
  const isMobile = useIsMobile()
  const totalPages = Math.ceil(total / pageSize)

  const hasMore = currentPage < totalPages

  // Mobile: render sentinel via portal; standard Pagination UI is hidden.
  // The sentinel lives inside the scroll container so IntersectionObserver
  // fires regardless of which element actually scrolls.
  const useMobileInfiniteScroll = isMobile && hideOnMobile && totalPages > 1

  if (useMobileInfiniteScroll) {
    return (
      <MobileInfiniteSentinel
        isLoading={isLoading}
        hasMore={hasMore}
        onLoadMore={onLoadMore || (() => hasMore && onPageChange(currentPage + 1))}
        containerSelector="[data-infinite-scroll-mount]"
        total={total}
        t={t}
      />
    )
  }

  // Early return for no pagination needed (desktop single page)
  if (totalPages <= 1) return null

  // Desktop: Show standard pagination
  // Generate page numbers to show
  const getPageNumbers = () => {
    const pages: (number | string)[] = []
    const showEllipsis = totalPages > 7

    if (!showEllipsis) {
      for (let i = 1; i <= totalPages; i++) {
        pages.push(i)
      }
    } else {
      // Always show first page
      pages.push(1)

      if (currentPage > 3) {
        pages.push('...')
      }

      // Show pages around current
      const start = Math.max(2, currentPage - 1)
      const end = Math.min(totalPages - 1, currentPage + 1)

      for (let i = start; i <= end; i++) {
        pages.push(i)
      }

      if (currentPage < totalPages - 2) {
        pages.push('...')
      }

      // Always show last page
      pages.push(totalPages)
    }

    return pages
  }

  const pageNumbers = getPageNumbers()

  return (
    <div className={cn('flex items-center justify-between gap-4', className)}>
      {/* Count text hidden on mobile — manual pagination (e.g. inside dialogs)
          has limited horizontal space, keep only prev/next + page numbers. */}
      <div className="text-sm text-muted-foreground hidden md:block">
        {t('pagination.total', { total, currentPage, totalPages })}
      </div>

      <div className="flex items-center gap-2 ml-auto">
        <Button
          variant="outline"
          size="icon"
          onClick={() => onPageChange(currentPage - 1)}
          disabled={currentPage === 1}
          className="h-8 w-8"
        >
          <ChevronLeft className="h-4 w-4" />
        </Button>

        {pageNumbers.map((page, index) => (
          typeof page === 'number' ? (
            <Button
              key={index}
              variant={page === currentPage ? 'default' : 'outline'}
              size="icon"
              onClick={() => onPageChange(page)}
              className="h-8 w-8"
            >
              {page}
            </Button>
          ) : (
            <span key={index} className="px-1 text-muted-foreground">
              {page}
            </span>
          )
        ))}

        <Button
          variant="outline"
          size="icon"
          onClick={() => onPageChange(currentPage + 1)}
          disabled={currentPage === totalPages}
          className="h-8 w-8"
        >
          <ChevronRight className="h-4 w-4" />
        </Button>

        {showSizeChanger && onPageSizeChange && (
          <Select
            value={String(pageSize)}
            onValueChange={(value) => onPageSizeChange(Number(value))}
          >
            <SelectTrigger className="ml-2 h-8 w-auto">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {pageSizeOptions.map((size) => (
                <SelectItem key={size} value={String(size)}>
                  {t('pagination.perPage', { size })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      </div>
    </div>
  )
}

/**
 * Simple pagination without page size changer
 */
export interface PaginationSimpleProps {
  total: number
  pageSize: number
  currentPage: number
  onPageChange: (page: number) => void
  className?: string
}

export function PaginationSimple({
  total,
  pageSize,
  currentPage,
  onPageChange,
  className,
}: PaginationSimpleProps) {
  return (
    <Pagination
      total={total}
      pageSize={pageSize}
      currentPage={currentPage}
      onPageChange={onPageChange}
      className={className}
    />
  )
}
