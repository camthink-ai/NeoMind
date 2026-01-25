/**
 * Data Table Component
 *
 * Unified with dashboard design system.
 * Sortable, filterable data table with data binding support.
 * Displays tabular data with pagination.
 */

import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  ChevronUp,
  ChevronDown,
  ChevronsUpDown,
  Search,
  ChevronLeft,
  ChevronRight,
} from 'lucide-react'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import type { DataSource } from '@/types/dashboard'

export interface Column {
  key: string
  label: string
  type?: 'text' | 'number' | 'date' | 'boolean' | 'status'
  width?: string
  align?: 'left' | 'center' | 'right'
  sortable?: boolean
  filterable?: boolean
  format?: (value: any) => string
}

export interface Row {
  [key: string]: any
}

export interface DataTableProps {
  dataSource?: DataSource
  data?: Row[]
  columns?: Column[]
  title?: string
  pageSize?: number
  sortable?: boolean
  filterable?: boolean
  showPagination?: boolean
  showBorder?: boolean
  striped?: boolean
  hover?: boolean
  compact?: boolean
  emptyMessage?: string
  showCard?: boolean
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

const defaultColumns: Column[] = [
  { key: 'name', label: 'Name', type: 'text', sortable: true },
  { key: 'value', label: 'Value', type: 'number', sortable: true },
  { key: 'status', label: 'Status', type: 'status' },
]

// Status color mapping
const statusColors: Record<string, string> = {
  online: 'bg-green-500 text-green-500',
  active: 'bg-green-500 text-green-500',
  enabled: 'bg-green-500 text-green-500',
  success: 'bg-green-500 text-green-500',
  offline: 'bg-gray-500 text-gray-500',
  inactive: 'bg-gray-500 text-gray-500',
  disabled: 'bg-gray-500 text-gray-500',
  error: 'bg-red-500 text-red-500',
  warning: 'bg-yellow-500 text-yellow-500',
  pending: 'bg-blue-500 text-blue-500',
  processing: 'bg-blue-500 text-blue-500',
}

export function DataTable({
  dataSource,
  data: propData = [],
  columns: propColumns = [],
  title,
  pageSize = 10,
  sortable = true,
  filterable = true,
  showPagination = true,
  showBorder = true,
  striped = true,
  hover = true,
  compact = false,
  emptyMessage = 'No data available',
  showCard = false,
  size = 'md',
  className,
}: DataTableProps) {
  const config = dashboardComponentSize[size]
  const { data, loading } = useDataSource<Row[]>(dataSource, { fallback: propData })
  const tableData = data ?? propData

  const columns = propColumns.length > 0 ? propColumns : defaultColumns

  // Sorting state
  const [sortConfig, setSortConfig] = useState<{ key: string; direction: 'asc' | 'desc' } | null>(null)

  // Pagination state
  const [currentPage, setCurrentPage] = useState(1)

  // Filter state
  const [filters, setFilters] = useState<Record<string, string>>({})

  // Handle sort
  const handleSort = (key: string) => {
    if (!sortable) return

    let direction: 'asc' | 'desc' = 'asc'
    if (sortConfig?.key === key) {
      if (sortConfig.direction === 'asc') {
        direction = 'desc'
      } else if (sortConfig.direction === 'desc') {
        setSortConfig(null)
        return
      }
    }
    setSortConfig({ key, direction })
  }

  // Handle filter
  const handleFilter = (key: string, value: string) => {
    setFilters((prev) => ({ ...prev, [key]: value }))
    setCurrentPage(1)
  }

  // Apply sorting and filtering
  const processedData = tableData.filter((row) => {
    return Object.entries(filters).every(([key, value]) => {
      if (!value) return true
      const cellValue = String(row[key] ?? '').toLowerCase()
      return cellValue.includes(value.toLowerCase())
    })
  }).sort((a, b) => {
    if (!sortConfig) return 0

    const aValue = a[sortConfig.key]
    const bValue = b[sortConfig.key]

    if (aValue === bValue) return 0

    const comparison = aValue > bValue ? 1 : -1
    return sortConfig.direction === 'asc' ? comparison : -comparison
  })

  // Pagination
  const totalPages = Math.ceil(processedData.length / pageSize)
  const paginatedData = processedData.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  // Render cell value
  const renderCellValue = (row: Row, column: Column) => {
    const value = row[column.key]

    if (column.format) {
      return column.format(value)
    }

    switch (column.type) {
      case 'boolean':
        return value ? (
          <span className="text-green-500">✓</span>
        ) : (
          <span className="text-muted-foreground">✗</span>
        )
      case 'status':
        const statusColor = statusColors[String(value).toLowerCase()] || 'bg-gray-500'
        return (
          <span className={cn('inline-flex items-center gap-1.5')}>
            <span className={cn('w-2 h-2 rounded-full', statusColor.split(' ')[0])} />
            <span className="capitalize">{String(value)}</span>
          </span>
        )
      case 'date':
        return value ? new Date(value).toLocaleString() : '-'
      default:
        return value ?? '-'
    }
  }

  const content = (
    <div className={cn('space-y-4', className)}>
      {/* Header */}
      {(title || filterable) && (
        <div className="flex items-center justify-between">
          {title && <h3 className={cn(indicatorFontWeight.title, config.titleText)}>{title}</h3>}
          {filterable && (
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search..."
                className="pl-8 h-8 w-64"
                onChange={(e) => handleFilter('search', e.target.value)}
              />
            </div>
          )}
        </div>
      )}

      {/* Table */}
      {loading ? (
        <div className="space-y-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-10 w-full" />
          ))}
        </div>
      ) : (
        <>
          <div className={cn('rounded-lg border', showBorder && 'border-border')}>
            <Table>
              <TableHeader>
                <TableRow className="bg-muted/50">
                  {columns.map((column) => (
                    <TableHead
                      key={column.key}
                      className={cn(
                        column.align === 'center' && 'text-center',
                        column.align === 'right' && 'text-right',
                        sortable && column.sortable && 'cursor-pointer hover:bg-muted transition-colors'
                      )}
                      onClick={() => column.sortable && handleSort(column.key)}
                      style={{ width: column.width }}
                    >
                      <div className="flex items-center gap-1">
                        {column.label}
                        {sortable && column.sortable && (
                          <span className="ml-1">
                            {sortConfig?.key === column.key ? (
                              sortConfig.direction === 'asc' ? (
                                <ChevronUp className="h-3 w-3" />
                              ) : (
                                <ChevronDown className="h-3 w-3" />
                              )
                            ) : (
                              <ChevronsUpDown className="h-3 w-3 text-muted-foreground" />
                            )}
                          </span>
                        )}
                      </div>
                    </TableHead>
                  ))}
                </TableRow>
              </TableHeader>
              <TableBody>
                {paginatedData.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={columns.length} className="text-center text-muted-foreground py-8">
                      {emptyMessage}
                    </TableCell>
                  </TableRow>
                ) : (
                  paginatedData.map((row, index) => (
                    <TableRow
                      key={index}
                      className={cn(
                        striped && index % 2 === 0 && 'bg-muted/25',
                        hover && 'hover:bg-muted/50 transition-colors',
                        compact && '[&_td]:py-1 [&_th]:py-1'
                      )}
                    >
                      {columns.map((column) => (
                        <TableCell
                          key={column.key}
                          className={cn(
                            column.align === 'center' && 'text-center',
                            column.align === 'right' && 'text-right'
                          )}
                        >
                          {renderCellValue(row, column)}
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          {/* Pagination */}
          {showPagination && totalPages > 1 && (
            <div className="flex items-center justify-between">
              <p className="text-sm text-muted-foreground">
                Showing {(currentPage - 1) * pageSize + 1} to{' '}
                {Math.min(currentPage * pageSize, processedData.length)} of {processedData.length} results
              </p>
              <div className="flex items-center gap-1">
                <Button
                  size="icon"
                  variant="outline"
                  className="h-7 w-7"
                  onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
                  disabled={currentPage === 1}
                >
                  <ChevronLeft className="h-3 w-3" />
                </Button>
                {Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
                  const page = i + 1
                  return (
                    <Button
                      key={page}
                      size="icon"
                      variant={currentPage === page ? 'default' : 'outline'}
                      className="h-7 w-7"
                      onClick={() => setCurrentPage(page)}
                    >
                      {page}
                    </Button>
                  )
                })}
                <Button
                  size="icon"
                  variant="outline"
                  className="h-7 w-7"
                  onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
                  disabled={currentPage === totalPages}
                >
                  <ChevronRight className="h-3 w-3" />
                </Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )

  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'overflow-hidden flex flex-col h-full', config.padding, className)}>
        <div className="flex-1 min-h-0 overflow-auto">
          {content}
        </div>
      </div>
    )
  }

  return content
}
