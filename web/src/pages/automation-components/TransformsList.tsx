/**
 * Transforms List - Using ResponsiveTable for consistent styling
 */

import { useState } from "react"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { IconButton } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { EmptyState, EmptyStateCard } from "@/components/shared"
import { Edit, Trash2, Workflow, Globe, Cpu, HardDrive, Download, MoreVertical } from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { textMini } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
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
const SCOPE_CONFIG: Record<string, { label: string; icon: typeof Globe }> = {
  global: { label: 'automation:transformBuilder.scopes.global', icon: Globe },
  device_type: { label: 'automation:transformBuilder.scopes.deviceType', icon: Cpu },
  device: { label: 'automation:transformBuilder.scopes.device', icon: HardDrive },
}

function listSubtitle(value?: string | null): string | null {
  const text = value?.trim()
  if (!text || text === '-') return null
  return text
}

export const ITEMS_PER_PAGE = 10

// Get code summary for display
function _getCodeSummary(jsCode: string): string {
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
  const _setPage = onPageChange ?? setInternalPage

  const _totalPages = Math.ceil(transforms.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  // Mobile infinite-scroll parents pass a cumulative window; if props are
  // omitted, mirror that here so footer load-more cannot replace-slice.
  const paginatedTransforms = propsPaginatedTransforms ?? (
    isMobile
      ? transforms.slice(0, page * ITEMS_PER_PAGE)
      : transforms.slice(startIndex, endIndex)
  )

  function getScopeInfo(scope: any): { label: string; icon: typeof Globe } {
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
        {/* Mobile has no ResponsiveTable — carry its own loading skeleton. */}
        {loading && paginatedTransforms.length === 0 && (
          [0, 1, 2].map((i) => (
            <div key={i} className="rounded-2xl border border-border bg-card px-4 py-3.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <Skeleton className="h-4 w-36" />
                <Skeleton className="h-5 w-9 rounded-full" />
              </div>
              <Skeleton className="h-3 w-48" />
            </div>
          ))
        )}
        {paginatedTransforms.map((transform) => {
          return (
            <Card
              key={transform.id}
              className={cn(
                "cursor-pointer overflow-hidden rounded-2xl border border-border bg-card shadow-none transition-colors active:bg-muted",
                !transform.enabled && "opacity-50"
              )}
              onClick={() => onEdit(transform)}
            >
              <div className="px-4 py-3.5">
                <div className="flex items-center gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="truncate text-heading font-medium">{transform.name}</div>
                    <p className="mt-1 truncate text-xs text-muted-foreground">
                      {getScopeLabel(transform.scope)}
                    </p>
                  </div>
                  <Switch
                    checked={transform.enabled}
                    onCheckedChange={() => onToggleStatus(transform)}
                    onClick={(e) => e.stopPropagation()}
                  />
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    ) : loading && paginatedTransforms.length === 0 ? (
      <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="flex items-center gap-4 px-5 py-3.5">
            <Skeleton className="h-9 w-9 rounded-lg shrink-0" />
            <div className="flex-1 space-y-2">
              <Skeleton className="h-4 w-44" />
              <Skeleton className="h-5 w-36 rounded-md" />
            </div>
            <Skeleton className="h-4 w-24 hidden lg:block" />
            <Skeleton className="h-5 w-9 rounded-full" />
          </div>
        ))}
      </div>
    ) : paginatedTransforms.length === 0 ? (
      <EmptyStateCard>
        <EmptyState
          title={t('automation:emptyTransforms.title', 'No transforms')}
          description={t('automation:emptyTransforms.description', 'Create your first transform to process device data')}
        />
      </EmptyStateCard>
    ) : (
      <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden">
        {paginatedTransforms.map((transform) => {
          const subtitle = listSubtitle(transform.description)
          const scopeInfo = getScopeInfo(transform.scope)
          const ScopeIcon = scopeInfo.icon
          const hasExecuted = transform.last_executed && transform.last_executed !== 0
          const execCount = transform.execution_count || 0

          return (
            <div
              key={transform.id}
              role="button"
              tabIndex={0}
              onClick={() => onEdit(transform)}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onEdit(transform) } }}
              className={cn(
                "group/row flex items-center gap-4 px-5 py-3.5 transition-colors duration-fast hover:bg-muted-20 cursor-pointer",
                !transform.enabled && "opacity-55"
              )}
            >
              {/* Icon */}
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted-30 text-foreground">
                <Workflow className="h-4 w-4" strokeWidth={1.75} />
              </div>

              {/* Main 2-line content: Name + Scope/Meta */}
              <div className="min-w-0 flex-1 space-y-1.5">
                <div className="flex items-baseline gap-2 min-w-0">
                  <span className="truncate text-sm font-medium text-foreground">{transform.name}</span>
                  {subtitle && (
                    <span className="truncate text-xs text-muted-foreground">{subtitle}</span>
                  )}
                </div>
                <div className="flex flex-wrap items-center gap-2 min-w-0">
                  <Badge variant="outline" className="max-w-full gap-1 text-xs font-normal bg-muted-20 border-glass-border text-muted-foreground">
                    <ScopeIcon className="h-3 w-3 shrink-0" />
                    <span className="truncate">{getScopeLabel(transform.scope)}</span>
                  </Badge>
                  <span className="text-xs text-muted-foreground tabular-nums">
                    {formatTimestamp(transform.created_at)}
                  </span>
                </div>
              </div>

              {/* Quiet right meta: last executed + count */}
              <div className="hidden lg:flex flex-col items-end gap-0.5 shrink-0 text-xs tabular-nums text-muted-foreground">
                <span>
                  {hasExecuted ? formatTimestamp(transform.last_executed ?? undefined) : '-'}
                </span>
                {hasExecuted && execCount > 1 && (
                  <span className={cn(textMini, "text-muted-foreground")}>
                    {execCount}x
                  </span>
                )}
              </div>

              {/* Switch + hover-reveal actions */}
              <div className="flex items-center gap-2 shrink-0" onClick={(e) => e.stopPropagation()}>
                <Switch
                  checked={transform.enabled}
                  onCheckedChange={() => onToggleStatus(transform)}
                  className="scale-90"
                />
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <IconButton
                      size="sm"
                      aria-label={t('common:actions', { defaultValue: 'Actions' })}
                      className="opacity-50 transition-opacity group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                    >
                      <MoreVertical className="h-4 w-4" />
                    </IconButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-[160px]">
                    <DropdownMenuItem onClick={() => onEdit(transform)} className="gap-2">
                      <Edit className="h-4 w-4 shrink-0" />
                      <span>{t('common:edit')}</span>
                    </DropdownMenuItem>
                    {onExport && (
                      <DropdownMenuItem onClick={() => onExport(transform)} className="gap-2">
                        <Download className="h-4 w-4 shrink-0" />
                        <span>{t('common:export')}</span>
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem onClick={() => onDelete(transform)} className="gap-2 text-error focus:text-error">
                      <Trash2 className="h-4 w-4 shrink-0" />
                      <span>{t('common:delete')}</span>
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>
          )
        })}
      </div>
    )
  )
}
