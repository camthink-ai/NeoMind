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
import { Edit, Trash2, MoreVertical } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { TransformAutomation } from "@/types"

interface TransformsListProps {
  transforms: TransformAutomation[]
  loading: boolean
  onEdit: (transform: TransformAutomation) => void
  onDelete: (transform: TransformAutomation) => void
  onToggleStatus: (transform: TransformAutomation) => void
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
    if (!scope) return 'global'
    if (typeof scope === 'string') return scope
    if (scope.type) return scope.type
    return 'global'
  }

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-12">#</TableHead>
            <TableHead>{t('automation:name')}</TableHead>
            <TableHead>{t('automation:scope')}</TableHead>
            <TableHead>{t('common:description')}</TableHead>
            <TableHead>{t('automation:status')}</TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline
              title={t('common:loading')}
              colSpan={6}
            />
          ) : transforms.length === 0 ? (
            <EmptyStateInline
              title={t('automation:noTransforms')}
              colSpan={6}
            />
          ) : (
            transforms.map((transform, index) => (
                  <TableRow key={transform.id} className={!transform.enabled ? "opacity-60" : ""}>
                    <TableCell className="text-muted-foreground">{index + 1}</TableCell>
                    <TableCell className="font-medium">{transform.name}</TableCell>
                    <TableCell>
                      <Badge variant="outline">{getScopeLabel(transform.scope)}</Badge>
                    </TableCell>
                    <TableCell className="text-muted-foreground max-w-md truncate">
                      {transform.description || '-'}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Switch
                          checked={transform.enabled}
                          onCheckedChange={() => onToggleStatus(transform)}
                        />
                        <StatusBadge status={transform.enabled ? 'enabled' : 'disabled'} />
                      </div>
                    </TableCell>
                    <TableCell className="text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon">
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
