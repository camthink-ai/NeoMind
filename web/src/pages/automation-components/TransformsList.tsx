/**
 * Transforms List - Using ResponsiveTable for consistent styling
 */

import { useState } from "react"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import { ResponsiveTable, EmptyState } from "@/components/shared"
import { Edit, Trash2, Code, Globe, Cpu, HardDrive, Download, MoreVertical } from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { textMini } from "@/design-system/tokens/typography"
import type { TransformAutomation } from "@/types"
import { useIsMobile } from "@/hooks/useMobile"

interface TransformsListProps {
  transforms: TransformAutomation[]
  loading: boolean
  paginatedTransforms?: TransformAutomation[]
  page?: number
  onPageChange?: (page: number) => void
  onEdit: (transform: TransformAutomation) => void
  onDelete: (transform: TransformAutomation) => void
  onToggleStatus: (transform: TransformAutomation) => void
  onExport?: (transform: TransformAutomation) => void
}

// Scope configuration
const SCOPE_CONFIG: Record<string, { label: string; icon: typeof Globe; color: string }> = {
  global: { label: 'automation:transformBuilder.scopes.global', icon: Globe, color: 'text-accent-purple bg-accent-purple-light border-accent-purple-light' },
  device_type: { label: 'automation:transformBuilder.scopes.deviceType', icon: Cpu, color: 'text-info bg-info-light border-info' },
  device: { label: 'automation:transformBuilder.scopes.device', icon: HardDrive, color: 'text-success bg-success-light border-success-light dark:text-success dark:bg-success-light dark:border-success-light' },
}

export const ITEMS_PER_PAGE = 10

// Get code summary for display
function getCodeSummary(jsCode: string): string {
  if (!jsCode) return '-'

  const TRUNCATE_LEN = 45

  // Try to extract return statement with balanced braces/brackets
  const returnIdx = jsCode.indexOf('return ')
  if (returnIdx !== -1) {
    const afterReturn = jsCode.slice(returnIdx + 7).trimStart()
    const firstChar = afterReturn[0]

    if (firstChar === '{' || firstChar === '[') {
      // Balanced bracket matching
      const open = firstChar
      const close = firstChar === '{' ? '}' : ']'
      let depth = 0
      let inString: string | null = null
      let escaped = false
      let endIdx = -1

      for (let i = 0; i < afterReturn.length; i++) {
        const ch = afterReturn[i]

        if (escaped) { escaped = false; continue }
        if (ch === '\\') { escaped = true; continue }

        if (inString) {
          if (ch === inString) inString = null
          continue
        }
        if (ch === '"' || ch === "'" || ch === '`') { inString = ch; continue }

        if (ch === open) depth++
        else if (ch === close) {
          depth--
          if (depth === 0) { endIdx = i; break }
        }
      }

      if (endIdx !== -1) {
        const expr = afterReturn.slice(0, endIdx + 1).replace(/\s+/g, ' ').trim()
        return expr.length > TRUNCATE_LEN ? expr.substring(0, TRUNCATE_LEN - 3) + '...' : expr
      }
    } else {
      // Simple return (variable, literal, etc.) — take until ; or newline
      const match = afterReturn.match(/^[^;{}\n]+/)
      if (match) {
        const expr = match[0].trim()
        return expr.length > TRUNCATE_LEN ? expr.substring(0, TRUNCATE_LEN - 3) + '...' : expr
      }
    }
  }

  // Show first non-comment line
  const lines = jsCode.split('\n').filter(l => l.trim() && !l.trim().startsWith('//'))
  if (lines.length > 0) {
    const firstLine = lines[0].trim()
    return firstLine.length > TRUNCATE_LEN ? firstLine.substring(0, TRUNCATE_LEN - 3) + '...' : firstLine
  }

  return jsCode.substring(0, TRUNCATE_LEN) + (jsCode.length > TRUNCATE_LEN ? '...' : '')
}

export function TransformsList({
  transforms,
  loading,
  paginatedTransforms: propsPaginatedTransforms,
  page: propsPage,
  onPageChange,
  onEdit,
  onDelete,
  onToggleStatus,
  onExport,
}: TransformsListProps) {
  const { t } = useTranslation(['common', 'automation'])
  const isMobile = useIsMobile()
  const [internalPage, setInternalPage] = useState(1)

  // Use props if provided, otherwise use internal state (backward compatibility)
  const page = propsPage ?? internalPage
  const setPage = onPageChange ?? setInternalPage

  const totalPages = Math.ceil(transforms.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  const paginatedTransforms = propsPaginatedTransforms ?? transforms.slice(startIndex, endIndex)

  function getScopeInfo(scope: any): { label: string; icon: typeof Globe; color: string } {
    if (!scope || scope === 'global' || (typeof scope === 'string' && scope === 'global')) {
      return SCOPE_CONFIG.global
    }
    if (scope.device_type || (typeof scope === 'object' && scope.device_type)) {
      return SCOPE_CONFIG.device_type
    }
    if (scope.device || (typeof scope === 'object' && scope.device)) {
      return SCOPE_CONFIG.device
    }
    return SCOPE_CONFIG.global
  }

  function getScopeLabel(scope: any): string {
    if (!scope || scope === 'global' || (typeof scope === 'string' && scope === 'global')) {
      return t('automation:transformBuilder.scopes.global')
    }
    if (scope.device_type || (typeof scope === 'object' && scope.device_type)) {
      return `${t('automation:transformBuilder.scopes.deviceType')}: ${scope.device_type || scope.type}`
    }
    if (scope.device || (typeof scope === 'object' && scope.device)) {
      return `${t('automation:transformBuilder.scopes.device')}: ${scope.device}`
    }
    return t('automation:transformBuilder.scopes.global')
  }

  return (
    isMobile ? (
      <div className="space-y-2">
        {paginatedTransforms.map((transform) => {
          const scopeInfo = getScopeInfo(transform.scope)
          const ScopeIcon = scopeInfo.icon

          return (
            <Card
              key={transform.id}
              className={cn(
                "overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all",
                !transform.enabled && "opacity-50"
              )}
              onClick={() => onEdit(transform)}
            >
              <div className="px-3 py-2.5">
                {/* Row 1: icon + name + switch + actions */}
                <div className="flex items-center gap-2.5">
                  <div className={cn(
                    "w-8 h-8 rounded-lg flex items-center justify-center shrink-0",
                    transform.enabled ? "bg-accent-cyan-light text-accent-cyan" : "bg-muted text-muted-foreground"
                  )}>
                    <Code className="h-4 w-4" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-sm truncate">{transform.name}</div>
                  </div>
                  <Switch
                    checked={transform.enabled}
                    onCheckedChange={() => onToggleStatus(transform)}
                    className="scale-75"
                    onClick={(e) => e.stopPropagation()}
                  />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                      <IconButton>
                        <MoreVertical className="h-4 w-4" />
                      </IconButton>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onEdit(transform) }}>
                        <Edit className="h-4 w-4 mr-2" />
                        {t('common:edit')}
                      </DropdownMenuItem>
                      {onExport && (
                        <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onExport(transform) }}>
                          <Download className="h-4 w-4 mr-2" />
                          {t('common:export')}
                        </DropdownMenuItem>
                      )}
                      <DropdownMenuItem
                        className="text-error"
                        onClick={(e) => { e.stopPropagation(); onDelete(transform) }}
                      >
                        <Trash2 className="h-4 w-4 mr-2" />
                        {t('common:delete')}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {/* Row 2: scope badge + last executed */}
                <div className="flex items-center gap-1.5 mt-1.5 ml-[42px]">
                  <Badge variant="outline" className={cn(textMini, "h-5 px-1.5 gap-0.5", scopeInfo.color)}>
                    <ScopeIcon className="h-3 w-3" />
                    {getScopeLabel(transform.scope)}
                  </Badge>
                  {(() => {
                    const hasExecuted = transform.last_executed && transform.last_executed !== 0
                    const execCount = transform.execution_count || 0
                    return (
                      <span className={cn(textMini, "text-muted-foreground ml-auto flex items-center gap-1")}>
                        {hasExecuted ? (
                          <>
                            {formatTimestamp(transform.last_executed ?? undefined)}
                            {execCount > 1 && <span>{execCount}x</span>}
                          </>
                        ) : (
                          t('automation:never', 'Never')
                        )}
                      </span>
                    )
                  })()}
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    ) : (
    <ResponsiveTable
      columns={[
        {
          key: 'name',
          label: t('automation:name'),
          width: '28%',
        },
        {
          key: 'scope',
          label: t('automation:scope'),
          width: '18%',
        },
        {
          key: 'createdAt',
          label: t('common:createdAt', 'Created'),
          width: '18%',
        },
        {
          key: 'lastExecuted',
          label: t('automation:lastExecuted', 'Last Executed'),
          width: '18%',
        },
        {
          key: 'status',
          label: t('automation:status'),
          width: '10%',
        },
      ]}
      data={paginatedTransforms as unknown as Record<string, unknown>[]}
      rowKey={(transform) => (transform as unknown as TransformAutomation).id}
      onRowClick={(row) => onEdit(row as unknown as TransformAutomation)}
      loading={loading}
      getRowClassName={(rowData) => {
        const transform = rowData as unknown as TransformAutomation
        return cn(!transform.enabled && "opacity-50")
      }}
      renderCell={(columnKey, rowData) => {
        const transform = rowData as unknown as TransformAutomation
        const scopeInfo = getScopeInfo(transform.scope)
        const ScopeIcon = scopeInfo.icon

        switch (columnKey) {
          case 'name':
            return (
              <div className="flex items-center gap-3">
                <div className={cn(
                  "w-9 h-9 rounded-lg flex items-center justify-center transition-colors shrink-0",
                  transform.enabled ? "bg-accent-cyan-light text-accent-cyan" : "bg-muted text-muted-foreground"
                )}>
                  <Code className="h-4 w-4" />
                </div>
                <div className="min-w-0">
                  <div className="font-medium text-sm truncate">{transform.name}</div>
                  <div className="text-xs text-muted-foreground line-clamp-1">
                    {transform.description || <code>{(transform.output_prefix || 'transform')}.</code>}
                  </div>
                </div>
              </div>
            )

          case 'scope':
            return (
              <Badge variant="outline" className={cn("text-xs gap-1", scopeInfo.color)}>
                <ScopeIcon className="h-3 w-3" />
                {getScopeLabel(transform.scope)}
              </Badge>
            )

          case 'createdAt':
            return (
              <span className="text-xs text-muted-foreground">
                {formatTimestamp(transform.created_at)}
              </span>
            )

          case 'lastExecuted': {
            const hasExecuted = transform.last_executed && transform.last_executed !== 0
            const execCount = transform.execution_count || 0
            return !hasExecuted ? (
              <span className="text-xs text-muted-foreground">-</span>
            ) : (
              <div className="flex flex-col gap-0.5">
                <span className="text-xs">{formatTimestamp(transform.last_executed ?? undefined)}</span>
                {execCount > 1 && (
                  <span className="text-xs text-muted-foreground">{execCount}x</span>
                )}
              </div>
            )
          }

          case 'status':
            return (
              <Switch
                checked={transform.enabled}
                onCheckedChange={() => onToggleStatus(transform)}
                className="scale-90"
              />
            )

          default:
            return null
        }
      }}
      actions={[
        {
          label: t('common:edit'),
          icon: <Edit className="h-4 w-4" />,
          onClick: (rowData) => {
            const transform = rowData as unknown as TransformAutomation
            onEdit(transform)
          },
        },
        ...(onExport ? [{
          label: t('common:export'),
          icon: <Download className="h-4 w-4" />,
          onClick: (rowData: unknown) => {
            const transform = rowData as unknown as TransformAutomation
            onExport(transform)
          },
        }] : []),
        {
          label: t('common:delete'),
          icon: <Trash2 className="h-4 w-4" />,
          variant: 'destructive',
          onClick: (rowData) => {
            const transform = rowData as unknown as TransformAutomation
            onDelete(transform)
          },
        },
      ]}
      emptyState={
        <EmptyState
          icon={<Code className="h-12 w-12" />}
          title={t('automation:emptyTransforms.title', 'No transforms')}
          description={t('automation:emptyTransforms.description', 'Create your first transform to process device data')}
        />
      }
    />
    )
  )
}
