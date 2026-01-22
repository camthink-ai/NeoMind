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
import { EmptyStateInline, StatusBadge } from "@/components/shared"
import { Edit, Trash2, MoreVertical, Code, Database, Globe, Cpu, HardDrive } from "lucide-react"
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

// Get code summary for display
function getCodeSummary(jsCode: string): string {
  if (!jsCode) return '-'

  // Try to extract return statement
  const returnMatch = jsCode.match(/return\s+({[^}]*}|\[[^\]]*\]|[^;{};]+)/s)
  if (returnMatch) {
    const ret = returnMatch[1].trim()
    if (ret.length > 50) {
      return ret.substring(0, 47) + '...'
    }
    return ret
  }

  // Show first non-comment line
  const lines = jsCode.split('\n').filter(l => l.trim() && !l.trim().startsWith('//'))
  if (lines.length > 0) {
    const firstLine = lines[0].trim()
    if (firstLine.length > 50) {
      return firstLine.substring(0, 47) + '...'
    }
    return firstLine
  }

  return jsCode.substring(0, 50) + (jsCode.length > 50 ? '...' : '')
}

export function TransformsList({
  transforms,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
}: TransformsListProps) {
  const { t } = useTranslation(['common', 'automation'])

  const getScopeLabel = (scope: any): string => {
    if (!scope) return t('automation:scopes.global', { defaultValue: 'global' })
    if (typeof scope === 'string') {
      return scope === 'global'
        ? t('automation:scopes.global', { defaultValue: 'global' })
        : scope
    }
    // Handle backend format: { device_type: "xxx" } or { device: "xxx" }
    if (scope.device_type) {
      return `${t('automation:scopes.deviceType', { defaultValue: 'Type' })}: ${scope.device_type}`
    }
    if (scope.device) {
      return `${t('automation:scopes.device', { defaultValue: 'Device' })}: ${scope.device}`
    }
    // Fallback for type field
    if (scope.type) return scope.type
    return t('automation:scopes.global', { defaultValue: 'global' })
  }

  const getScopeColor = (scope: any): string => {
    if (!scope || scope === 'global' || (typeof scope === 'string' && scope === 'global')) {
      return 'text-purple-700 bg-purple-50 border-purple-200 dark:text-purple-400 dark:bg-purple-950/30 dark:border-purple-800'
    }
    if (scope.device_type || (typeof scope === 'object' && scope.device_type)) {
      return 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800'
    }
    if (scope.device || (typeof scope === 'object' && scope.device)) {
      return 'text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800'
    }
    return 'text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700'
  }

  const getScopeIcon = (scope: any): React.ReactNode => {
    if (!scope || scope === 'global' || (typeof scope === 'string' && scope === 'global')) {
      return <Globe className="h-3 w-3" />
    }
    if (scope.device_type || (typeof scope === 'object' && scope.device_type)) {
      return <Cpu className="h-3 w-3" />
    }
    if (scope.device || (typeof scope === 'object' && scope.device)) {
      return <HardDrive className="h-3 w-3" />
    }
    return <Globe className="h-3 w-3" />
  }

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent border-b">
            <TableHead className="w-12">#</TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Code className="h-4 w-4 text-muted-foreground" />
                {t('automation:name')}
              </div>
            </TableHead>
            <TableHead>{t('automation:scope')}</TableHead>
            <TableHead>{t('automation:transformBuilder.transformCode')}</TableHead>
            <TableHead>{t('automation:outputPrefix')}</TableHead>
            <TableHead>{t('automation:status')}</TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline title={t('common:loading')} colSpan={7} />
          ) : transforms.length === 0 ? (
            <EmptyStateInline title={t('automation:noTransforms')} colSpan={7} />
          ) : (
            transforms.map((transform, index) => (
              <TableRow
                key={transform.id}
                className={cn(
                  "group transition-colors hover:bg-muted/50",
                  !transform.enabled && "opacity-60"
                )}
              >
                <TableCell className="text-muted-foreground text-sm font-medium">
                  {index + 1}
                </TableCell>
                <TableCell className="font-medium">{transform.name}</TableCell>
                <TableCell>
                  <Badge variant="outline" className={cn("text-xs gap-1.5", getScopeColor(transform.scope))}>
                    {getScopeIcon(transform.scope)}
                    {getScopeLabel(transform.scope)}
                  </Badge>
                </TableCell>
                <TableCell>
                  <div className="flex items-start gap-2 max-w-sm">
                    <Code className="h-3 w-3 mt-0.5 text-muted-foreground flex-shrink-0" />
                    <code className="text-xs bg-muted px-2 py-1 rounded font-mono truncate block">
                      {getCodeSummary(transform.js_code || '')}
                    </code>
                  </div>
                </TableCell>
                <TableCell>
                  {transform.output_prefix && transform.output_prefix !== 'transform' ? (
                    <div className="flex items-center gap-1">
                      <Database className="h-3 w-3 text-muted-foreground" />
                      <code className="text-xs bg-muted px-2 py-0.5 rounded">
                        {transform.output_prefix}.
                      </code>
                    </div>
                  ) : (
                    <span className="text-muted-foreground text-xs">transform.</span>
                  )}
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <Switch
                      checked={transform.enabled}
                      onCheckedChange={() => onToggleStatus(transform)}
                      className="scale-90"
                    />
                    <StatusBadge status={transform.enabled ? 'enabled' : 'disabled'} />
                  </div>
                </TableCell>
                <TableCell className="text-right">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                        <MoreVertical className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
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
            ))
          )}
        </TableBody>
      </Table>
    </Card>
  )
}
