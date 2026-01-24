/**
 * Transforms List - Unified card-based table design
 */

import { useState } from "react"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { EmptyStateInline, Pagination } from "@/components/shared"
import { Edit, Trash2, MoreVertical, Code, Database, Globe, Cpu, HardDrive, CheckCircle2 } from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import type { TransformAutomation } from "@/types"

interface TransformsListProps {
  transforms: TransformAutomation[]
  loading: boolean
  onEdit: (transform: TransformAutomation) => void
  onDelete: (transform: TransformAutomation) => void
  onToggleStatus: (transform: TransformAutomation) => void
}

// Scope configuration
const SCOPE_CONFIG: Record<string, { label: string; icon: typeof Globe; color: string }> = {
  global: { label: 'automation:transformBuilder.scopes.global', icon: Globe, color: 'text-purple-700 bg-purple-50 border-purple-200 dark:text-purple-400 dark:bg-purple-950/30 dark:border-purple-800' },
  device_type: { label: 'automation:transformBuilder.scopes.deviceType', icon: Cpu, color: 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800' },
  device: { label: 'automation:transformBuilder.scopes.device', icon: HardDrive, color: 'text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800' },
}

const ITEMS_PER_PAGE = 10

// Get code summary for display
function getCodeSummary(jsCode: string): string {
  if (!jsCode) return '-'

  // Try to extract return statement
  const returnMatch = jsCode.match(/return\s+({[^}]*}|\[[^\]]*\]|[^;{};]+)/s)
  if (returnMatch) {
    const ret = returnMatch[1].trim()
    if (ret.length > 45) {
      return ret.substring(0, 42) + '...'
    }
    return ret
  }

  // Show first non-comment line
  const lines = jsCode.split('\n').filter(l => l.trim() && !l.trim().startsWith('//'))
  if (lines.length > 0) {
    const firstLine = lines[0].trim()
    if (firstLine.length > 45) {
      return firstLine.substring(0, 42) + '...'
    }
    return firstLine
  }

  return jsCode.substring(0, 45) + (jsCode.length > 45 ? '...' : '')
}

export function TransformsList({
  transforms,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
}: TransformsListProps) {
  const { t } = useTranslation(['common', 'automation'])
  const [page, setPage] = useState(1)

  const totalPages = Math.ceil(transforms.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  const paginatedTransforms = transforms.slice(startIndex, endIndex)

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
    <>
      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent border-b bg-muted/30">
              <TableHead className="w-10 text-center">#</TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Code className="h-4 w-4" />
                  {t('automation:name')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Globe className="h-4 w-4" />
                  {t('automation:scope')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Code className="h-4 w-4" />
                  {t('automation:transformBuilder.transformCode')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Database className="h-4 w-4" />
                  {t('automation:outputPrefix')}
                </div>
              </TableHead>
              <TableHead className="text-center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('automation:status')}
                </div>
              </TableHead>
              <TableHead className="w-12"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={7} />
            ) : transforms.length === 0 ? (
              <EmptyStateInline title={t('automation:noTransforms')} colSpan={7} />
            ) : (
              paginatedTransforms.map((transform, index) => {
                const scopeInfo = getScopeInfo(transform.scope)
                const ScopeIcon = scopeInfo.icon

                return (
                  <TableRow
                    key={transform.id}
                    className={cn(
                      "group transition-colors hover:bg-muted/50",
                      !transform.enabled && "opacity-50"
                    )}
                  >
                    <TableCell className="text-center">
                      <span className="text-xs text-muted-foreground font-medium">{startIndex + index + 1}</span>
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-3">
                        <div className={cn(
                          "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                          transform.enabled ? "bg-cyan-500/10 text-cyan-600" : "bg-muted text-muted-foreground"
                        )}>
                          <Code className="h-4 w-4" />
                        </div>
                        <div>
                          <div className="font-medium text-sm">{transform.name}</div>
                          <div className="text-xs text-muted-foreground line-clamp-1 max-w-[180px]">
                            {transform.description || '-'}
                          </div>
                        </div>
                      </div>
                    </TableCell>

                    <TableCell>
                      <Badge variant="outline" className={cn("text-xs gap-1.5", scopeInfo.color)}>
                        <ScopeIcon className="h-3 w-3" />
                        {getScopeLabel(transform.scope)}
                      </Badge>
                    </TableCell>

                    <TableCell>
                      <code className="text-xs bg-muted px-2 py-1 rounded-md font-mono truncate block max-w-[200px]">
                        {getCodeSummary(transform.js_code || '')}
                      </code>
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-1.5">
                        <Database className="h-3 w-3 text-muted-foreground" />
                        <code className="text-xs bg-muted px-2 py-0.5 rounded">
                          {(transform.output_prefix || 'transform') + '.'}
                        </code>
                      </div>
                    </TableCell>

                    <TableCell className="text-center">
                      <div className="flex items-center justify-center gap-2">
                        <Switch
                          checked={transform.enabled}
                          onCheckedChange={() => onToggleStatus(transform)}
                          className="scale-90"
                        />
                        <Badge variant="outline" className={cn(
                          "text-xs gap-1 hidden sm:flex",
                          transform.enabled
                            ? "text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800"
                            : "text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700"
                        )}>
                          <CheckCircle2 className="h-3 w-3" />
                          {transform.enabled ? t('automation:statusEnabled') : t('automation:statusDisabled')}
                        </Badge>
                      </div>
                    </TableCell>

                    <TableCell>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-40">
                          <DropdownMenuItem onClick={() => onEdit(transform)}>
                            <Edit className="mr-2 h-4 w-4" />
                            {t('common:edit')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => onDelete(transform)}
                            className="text-destructive"
                          >
                            <Trash2 className="mr-2 h-4 w-4" />
                            {t('common:delete')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                )
              })
            )}
          </TableBody>
        </Table>
      </Card>

      {transforms.length > ITEMS_PER_PAGE && (
        <div className="sticky bottom-0 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 pt-4 pb-2 border-t mt-4">
          <Pagination
            total={transforms.length}
            pageSize={ITEMS_PER_PAGE}
            currentPage={page}
            onPageChange={setPage}
          />
        </div>
      )}
    </>
  )
}
