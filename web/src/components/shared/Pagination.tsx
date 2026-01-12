import { Button } from '@/components/ui/button'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

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
}: PaginationProps) {
  const { t } = useTranslation('common')
  const totalPages = Math.ceil(total / pageSize)

  if (totalPages <= 1) return null

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
      <div className="text-sm text-muted-foreground">
        {t('pagination.total', { total, currentPage, totalPages })}
      </div>

      <div className="flex items-center gap-2">
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
          <select
            value={pageSize}
            onChange={(e) => onPageSizeChange(Number(e.target.value))}
            className="ml-2 h-8 rounded-md border border-input bg-background px-2 py-1 text-sm ring-offset-background focus:outline-none focus:ring-2 focus:ring-ring"
          >
            {pageSizeOptions.map((size) => (
              <option key={size} value={size}>
                {t('pagination.perPage', { size })}
              </option>
            ))}
          </select>
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
