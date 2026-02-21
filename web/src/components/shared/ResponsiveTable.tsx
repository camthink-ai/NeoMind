/**
 * ResponsiveTable Component
 *
 * Desktop: Shows a standard table layout
 * Mobile: Shows cards with better readability
 */

import { ReactNode } from 'react'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { MoreVertical, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface TableColumn {
  key: string
  label: string | ReactNode
  width?: string
  align?: 'left' | 'center' | 'right'
  className?: string
}

export interface TableRowAction {
  label: string
  icon?: ReactNode
  onClick: (rowData?: Record<string, unknown>) => void
  variant?: 'default' | 'destructive'
  disabled?: boolean
  show?: (rowData: Record<string, unknown>) => boolean
}

export interface ResponsiveTableProps {
  columns: TableColumn[]
  data: Record<string, unknown>[]
  renderCell: (columnKey: string, rowData: Record<string, unknown>) => ReactNode
  rowKey: (rowData: Record<string, unknown>) => string
  actions?: TableRowAction[]
  onRowClick?: (rowData: Record<string, unknown>) => void
  className?: string
  loading?: boolean
  emptyState?: ReactNode
  getRowClassName?: (rowData: Record<string, unknown>) => string
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
export function ResponsiveTable({
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
}: ResponsiveTableProps) {
  // Show empty state only on mobile when no data
  const showEmptyState = data.length === 0 && !loading

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-4">
        <Loader2 className="h-8 w-8 animate-spin text-primary mb-3" />
        <p className="text-sm text-muted-foreground font-medium">Loading data...</p>
      </div>
    )
  }

  return (
    <>
      {/* Desktop Table */}
      <div className="hidden md:block rounded-xl border bg-card overflow-hidden">
        <table className={cn("w-full caption-bottom text-sm", className)}>
          <thead>
            <tr className="border-b bg-muted/50/50">
              {columns.map((column) => (
                <th
                  key={column.key}
                  className={cn(
                    "h-12 px-4 align-middle text-xs font-semibold text-muted-foreground",
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
          <tbody className="[&_tr:last-child]:border-0 divide-y divide-border/50">
            {data.length === 0 ? (
              <tr>
                <td colSpan={columns.length + (actions && actions.length > 0 ? 1 : 0)} className="px-4 py-12">
                  <div className="flex flex-col items-center justify-center text-center">
                    {emptyState || (
                      <>
                        <div className="w-12 h-12 rounded-full bg-muted/50 flex items-center justify-center mb-3">
                          <div className="w-5 h-5 rounded-full border-2 border-muted-foreground/30" />
                        </div>
                        <p className="text-sm text-muted-foreground">No data available</p>
                      </>
                    )}
                  </div>
                </td>
              </tr>
            ) : (
              data.map((rowData) => {
                const rowClass = getRowClassName?.(rowData)
                const visibleActions = actions?.filter(a => a.show?.(rowData) !== false)

                return (
                  <tr
                    key={rowKey(rowData)}
                    className={cn(
                      "transition-all duration-150 hover:bg-muted/40 group",
                      onRowClick && 'cursor-pointer',
                      rowClass
                    )}
                    onClick={() => onRowClick?.(rowData)}
                  >
                    {columns.map((column) => (
                      <td
                        key={column.key}
                        className={cn(
                          "px-4 py-3.5 align-middle",
                          column.align === 'center' && 'text-center',
                          column.align === 'right' && 'text-right',
                          !column.align && 'text-left',
                          column.className
                        )}
                      >
                        {renderCell(column.key, rowData)}
                      </td>
                    ))}
                    {visibleActions && visibleActions.length > 0 && (
                      <td className="px-4 py-3.5 align-middle">
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-9 w-9 opacity-0 group-hover:opacity-100 transition-opacity data-[state=open]:opacity-100"
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
                                  action.variant === 'destructive' && 'text-destructive focus:text-destructive'
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
      <div className="md:hidden space-y-3">
        {data.length === 0 ? (
          <Card className="overflow-hidden border-border/60">
            <div className="p-8">
              {emptyState || (
                <div className="flex flex-col items-center justify-center text-center">
                  <div className="w-12 h-12 rounded-full bg-muted/50 flex items-center justify-center mb-3">
                    <div className="w-5 h-5 rounded-full border-2 border-muted-foreground/30" />
                  </div>
                  <p className="text-sm text-muted-foreground">No data available</p>
                </div>
              )}
            </div>
          </Card>
        ) : (
          data.map((rowData) => {
          const rowClass = getRowClassName?.(rowData)
          const visibleActions = actions?.filter(a => a.show?.(rowData) !== false)

          return (
            <Card
              key={rowKey(rowData)}
              className={cn(
                'overflow-hidden border-border/60 shadow-sm',
                onRowClick && 'cursor-pointer active:scale-[0.99] transition-all',
                rowClass
              )}
            >
              {/* Card Header - First column as title */}
              <div className="bg-muted/30 px-4 py-3 border-b border-border/60 rounded-t-xl">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2 min-w-0 flex-1">
                    {renderCell(columns[0].key, rowData)}
                  </div>
                  {visibleActions && visibleActions.length > 0 && (
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-9 w-9 shrink-0 hover:bg-muted-foreground/10"
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
                              action.variant === 'destructive' && 'text-destructive focus:text-destructive'
                            )}
                            disabled={action.disabled}
                          >
                            {action.icon && <span className="h-4 w-4 shrink-0">{action.icon}</span>}
                            <span>{action.label}</span>
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  )}
                </div>
              </div>

              {/* Card Body - Other columns as key-value pairs */}
              <div className="p-3 sm:p-4 space-y-2 sm:space-y-2.5">
                {columns.slice(1).map((column) => {
                  const cellContent = renderCell(column.key, rowData)
                  // Skip if content is empty
                  if (!cellContent || (typeof cellContent === 'object' && 'props' in cellContent && (cellContent as any).props.children === '')) {
                    return null
                  }

                  return (
                    <div key={column.key} className="flex items-start gap-2 sm:gap-3 py-0.5">
                      <span className="text-xs text-muted-foreground shrink-0 w-24 sm:w-28 pt-0.5 font-medium">
                        {renderColumnLabel(column.label)}
                      </span>
                      <div className="text-sm flex-1 text-left min-w-0 break-words">
                        {cellContent}
                      </div>
                    </div>
                  )
                })}
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
    <Card className={cn('overflow-hidden border-border/60 rounded-xl shadow-sm', onClick && 'cursor-pointer active:scale-[0.99] transition-all', className)}>
      {/* Card Header */}
      <div className="bg-muted/30 px-4 py-3 border-b border-border/60 rounded-t-xl">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0 flex-1">
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
      <div className="p-3 sm:p-4">
        {children}
      </div>
    </Card>
  )
}
