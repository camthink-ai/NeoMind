/**
 * ResponsiveTable Component
 *
 * Desktop: Shows a standard table layout
 * Mobile: Shows cards with better readability
 */

import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { MoreVertical, Loader2, Inbox } from 'lucide-react'
import { Skeleton } from '@/components/ui/skeleton'
import { EmptyStateCompact } from '@/components/shared/EmptyState'
import { cn } from '@/lib/utils'
import { interactiveCardHover } from '@/design-system/tokens/size'
import { textMini } from "@/design-system/tokens/typography"

export interface TableColumn {
  key: string
  label: string | ReactNode
  width?: string
  align?: 'left' | 'center' | 'right'
  className?: string
}

export interface TableRowAction<T = Record<string, unknown>> {
  label: string
  icon?: ReactNode
  onClick: (rowData: T) => void
  variant?: 'default' | 'destructive'
  disabled?: boolean
  show?: (rowData: T) => boolean
}

export interface ResponsiveTableProps<T = Record<string, unknown>> {
  columns: TableColumn[]
  data: T[]
  renderCell: (columnKey: string, rowData: T) => ReactNode
  rowKey: (rowData: T) => string
  actions?: TableRowAction<T>[]
  onRowClick?: (rowData: T) => void
  className?: string
  loading?: boolean
  emptyState?: ReactNode
  getRowClassName?: (rowData: T) => string
  /** Enable sticky header for table */
  stickyHeader?: boolean
  /** Max height for table body scrolling (e.g., '400px', 'calc(100vh-200px)') */
  maxHeight?: string
  /** Use flex mode to fill parent container height (disables maxHeight) */
  flexHeight?: boolean
  /** Custom mobile card body renderer. When provided, replaces the default
   *  key-value list rendering for mobile cards. The card header (first
   *  column + actions menu) and outer Card chrome remain unchanged.
   *  Use this when the default key-value layout produces asymmetric or
   *  truncated content (e.g., multi-line cells, centered badges, mixed
   *  cell shapes in one row). */
  renderMobileBody?: (rowData: T) => ReactNode
  /** Extra content rendered in the top-right of the mobile card header.
   *  Occupies the same slot as the actions menu (and is hidden if actions
   *  are present). Useful for surfacing a status badge or chevron when
   *  the table has no row actions but the right side of the header would
   *  otherwise be empty. */
  renderMobileHeaderExtra?: (rowData: T) => ReactNode
  /** Flatten the mobile card header — drop the `bg-muted` band and the
   *  border under it so the header and body read as one continuous
   *  surface. Use when the body already provides enough visual structure
   *  (e.g., via renderMobileBody) and the gray header band feels heavy. */
  mobileFlatHeader?: boolean
}

/**
 * Render column label as ReactNode
 */
function renderColumnLabel(label: string | ReactNode): ReactNode {
  return label
}

/**
 * Responsive table component
 * Desktop: Standard table
 * Mobile: Card layout
 */
export function ResponsiveTable<T extends object>({
  columns,
  data,
  renderCell,
  rowKey,
  actions,
  onRowClick,
  className,
  loading,
  emptyState,
  getRowClassName,
  stickyHeader = true,
  maxHeight = 'calc(100vh - 280px)',
  flexHeight = false,
  renderMobileBody,
  renderMobileHeaderExtra,
  mobileFlatHeader = false,
}: ResponsiveTableProps<T>) {
  const { t } = useTranslation('common')
  // Show empty state only on mobile when no data
  const showEmptyState = data.length === 0 && !loading

  // Default empty state — uses shared EmptyStateCompact for consistency
  const defaultEmpty = (
    <EmptyStateCompact
      icon={<Inbox className="h-5 w-5" />}
      title={t('noData', { defaultValue: 'No data available' })}
    />
  )

  if (loading) {
    const skeletonRows = 8
    return (
      <>
        {/* Desktop skeleton — wrapper scrolls internally when the table
            outgrows it (min-w-full keeps the table filling the container) */}
        <div className="hidden md:block w-full overflow-x-auto rounded-lg border bg-card">
          <table className={cn("min-w-full caption-bottom text-sm", className)}>
            <thead className="[&_tr]:border-b">
              <tr className="rounded-t-lg">
                {columns.map((column) => (
                  <th
                    key={column.key}
                    className={cn(
                      "h-12 px-4 align-middle text-nano font-semibold uppercase tracking-wider text-foreground",
                      column.align === 'center' && 'text-center',
                      column.align === 'right' && 'text-right',
                      !column.align && 'text-left',
                      column.width
                    )}
                  >
                    {renderColumnLabel(column.label)}
                  </th>
                ))}
                {actions && actions.length > 0 && (
                  <th className="w-[60px]"></th>
                )}
              </tr>
            </thead>
            <tbody className="[&_tr:last-child]:border-0">
              {Array.from({ length: skeletonRows }).map((_, i) => (
                <tr key={i} className="border-b">
                  {columns.map((column) => (
                    <td key={column.key} className="px-4 py-3">
                      <Skeleton className="h-4 w-full" />
                    </td>
                  ))}
                  {actions && actions.length > 0 && (
                    <td className="px-4 py-3">
                      <Skeleton className="h-4 w-6" />
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {/* Mobile skeleton */}
        <div className="md:hidden space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="rounded-lg border bg-card overflow-hidden">
              <div className="bg-muted px-4 py-3 border-b">
                <Skeleton className="h-4 w-1/3" />
              </div>
              <div className="p-4 space-y-3">
                {columns.slice(1).map((column) => (
                  <div key={column.key} className="flex items-center justify-between gap-3">
                    <Skeleton className="h-3 w-24 shrink-0" />
                    <Skeleton className="h-4 flex-1 max-w-[60%]" />
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </>
    )
  }

  return (
    <>
      {/* Desktop Table - w-full wrapper scrolls internally (overflow-x-auto)
          when the table outgrows it; min-w-full keeps the table filling the
          container width so it adapts to window resizes */}
      <div className="hidden md:block w-full overflow-x-auto rounded-lg border bg-card">
        <table className={cn("min-w-full caption-bottom text-sm", className)}>
          <thead className={cn(
            "[&_tr]:border-b",
            stickyHeader && "sticky top-0 z-10 bg-card"
          )}>
            <tr className="rounded-t-xl">
              {columns.map((column) => (
                <th
                  key={column.key}
                  className={cn(
                    "h-12 px-4 align-middle text-nano font-semibold uppercase tracking-wider text-foreground",
                    column.align === 'center' && 'text-center',
                    column.align === 'right' && 'text-right',
                    !column.align && 'text-left',
                    column.width
                  )}
                >
                  {renderColumnLabel(column.label)}
                </th>
              ))}
              {actions && actions.length > 0 && (
                <th className="w-[60px]"></th>
              )}
            </tr>
          </thead>
          <tbody className="[&_tr:last-child]:border-0">
            {data.length === 0 ? (
              <tr>
                <td colSpan={columns.length + (actions && actions.length > 0 ? 1 : 0)} className="px-4 py-12">
                  <div className="flex flex-col items-center justify-center text-center">
                    {emptyState || defaultEmpty}
                  </div>
                </td>
              </tr>
            ) : (
              data.map((rowData, index) => {
                const rowClass = getRowClassName?.(rowData)
                const visibleActions = actions?.filter(a => a.show?.(rowData) !== false)

                return (
                  <tr
                    key={rowKey(rowData)}
                    className={cn(
                      "border-b transition-colors duration-150 hover:bg-muted-30 animate-fade-in-up",
                      onRowClick && 'cursor-pointer',
                      rowClass
                    )}
                    style={{ animationDelay: `calc(${index} * var(--stagger-step))`, animationFillMode: 'both' }}
                    onClick={() => onRowClick?.(rowData)}
                  >
                    {columns.map((column) => (
                      <td
                        key={column.key}
                        className={cn(
                          "px-4 py-3 align-middle",
                          column.align === 'center' && 'text-center',
                          column.align === 'right' && 'text-right',
                          !column.align && 'text-left',
                          column.className
                        )}
                      >
                        <div className={cn(
                          "flex items-center min-h-[36px]",
                          column.align === 'center' && 'justify-center',
                          column.align === 'right' && 'justify-end',
                          (!column.align || column.align === 'left') && 'justify-start',
                        )}>
                          {renderCell(column.key, rowData)}
                        </div>
                      </td>
                    ))}
                    {visibleActions && visibleActions.length > 0 && (
                      <td className="px-4 py-3 align-middle">
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 p-0"
                              aria-label="Actions"
                              onClick={(e) => e.stopPropagation()}
                            >
                              <MoreVertical className="h-4 w-4" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end" className="w-[160px]">
                            {visibleActions.map((action, idx) => (
                              <DropdownMenuItem
                                key={idx}
                                onClick={(e) => {
                                  e.stopPropagation()
                                  action.onClick(rowData)
                                }}
                                className={cn(
                                  "gap-2",
                                  action.variant === 'destructive' && 'text-error focus:text-error'
                                )}
                                disabled={action.disabled}
                              >
                                {action.icon && <span className="h-4 w-4 shrink-0">{action.icon}</span>}
                                <span>{action.label}</span>
                              </DropdownMenuItem>
                            ))}
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </td>
                    )}
                  </tr>
                )
              })
            )}
          </tbody>
        </table>
      </div>

      {/* Mobile Card List */}
      <div className="md:hidden space-y-2">
        {data.length === 0 ? (
          <Card className="overflow-hidden border-border">
            <div className="p-8">
              {emptyState || defaultEmpty}
            </div>
          </Card>
        ) : (
          data.map((rowData, index) => {
            const rowClass = getRowClassName?.(rowData)
            const visibleActions = actions?.filter(a => a.show?.(rowData) !== false)

            return (
              <Card
                key={rowKey(rowData)}
                className={cn(
                  'overflow-hidden border-border shadow-sm animate-fade-in-up',
                  interactiveCardHover,
                  onRowClick && 'cursor-pointer active:scale-[0.99]',
                  rowClass
                )}
                style={{ animationDelay: `calc(${index} * var(--stagger-step))`, animationFillMode: 'both' }}
                onClick={() => onRowClick?.(rowData)}
              >
                {/* Card Header - First column as title */}
                <div className={cn(
                  "px-3 py-2.5",
                  mobileFlatHeader
                    ? "pb-1"
                    : "bg-muted border-b border-border rounded-t-lg",
                )}>
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
                      <div className="min-w-0 flex-1 truncate">
                        {renderCell(columns[0].key, rowData)}
                      </div>
                    </div>
                    {visibleActions && visibleActions.length > 0 ? (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 shrink-0 hover:bg-muted"
                            aria-label="Actions"
                          >
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-[160px]">
                          {visibleActions.map((action, idx) => (
                            <DropdownMenuItem
                              key={idx}
                              onClick={(e) => {
                                e.stopPropagation()
                                action.onClick(rowData)
                              }}
                              className={cn(
                                "gap-2",
                                action.variant === 'destructive' && 'text-error focus:text-error'
                              )}
                              disabled={action.disabled}
                            >
                              {action.icon && <span className="h-4 w-4 shrink-0">{action.icon}</span>}
                              <span>{action.label}</span>
                            </DropdownMenuItem>
                          ))}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    ) : renderMobileHeaderExtra ? (
                      <div className="shrink-0">
                        {renderMobileHeaderExtra(rowData)}
                      </div>
                    ) : null}
                  </div>
                </div>

                {/* Card Body - Other columns as key-value pairs, OR a custom
                    layout when renderMobileBody is provided. The default
                    key-value list breaks for tables that mix multi-line
                    cells, centered badges, or asymmetric value shapes —
                    callers can pass renderMobileBody to swap in a tailored
                    layout without losing the shared Card chrome, header,
                    and actions menu. */}
                <div className="p-3">
                  {renderMobileBody ? (
                    renderMobileBody(rowData)
                  ) : (
                    <div className="space-y-1.5">
                      {columns.slice(1).map((column) => {
                        const cellContent = renderCell(column.key, rowData)
                        // Skip if content is empty
                        if (!cellContent || (typeof cellContent === 'object' && 'props' in cellContent && (cellContent as any).props.children === '')) {
                          return null
                        }

                        return (
                          <div key={column.key} className="flex items-start gap-2 py-0.5 min-w-0">
                            <span className={cn(textMini, "text-muted-foreground shrink-0 w-20 pt-0.5 font-medium truncate")}>
                              {typeof column.label === 'string' ? column.label : column.key}
                            </span>
                            <div className="text-sm flex-1 text-left min-w-0 overflow-hidden">
                              <div className="truncate">
                                {cellContent}
                              </div>
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  )}
                </div>
              </Card>
            )
          })
        )}
      </div>
    </>
  )
}

/**
 * Convenience component for simple card layout on mobile
 */
export interface MobileCardProps {
  title: ReactNode
  subtitle?: ReactNode
  icon?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
  onClick?: () => void
}

export function MobileCard({ title, subtitle, icon, actions, children, className, onClick }: MobileCardProps) {
  return (
    <Card className={cn('overflow-hidden border-border rounded-lg shadow-sm', onClick && 'cursor-pointer active:scale-[0.99] transition-all', className)}>
      {/* Card Header */}
      <div className="bg-muted px-3 py-2.5 border-b border-border rounded-t-lg">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
            {icon && <div className="shrink-0">{icon}</div>}
            <div className="min-w-0 flex-1">
              <div className="font-medium text-sm truncate">{title}</div>
              {subtitle && <div className="text-xs text-muted-foreground truncate">{subtitle}</div>}
            </div>
          </div>
          {actions && <div className="shrink-0">{actions}</div>}
        </div>
      </div>

      {/* Card Body */}
      <div className="p-3">
        {children}
      </div>
    </Card>
  )
}
