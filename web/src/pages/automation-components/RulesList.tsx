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
import { EmptyStateInline, StatusBadge } from "@/components/shared"
import { Zap, Edit, Play, Trash2, MoreVertical } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { Rule } from "@/types"

interface RulesListProps {
  rules: Rule[]
  loading: boolean
  onEdit: (rule: Rule) => void
  onDelete: (rule: Rule) => void
  onToggleStatus: (rule: Rule) => void
  onExecute: (rule: Rule) => void
}

export function RulesList({
  rules,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
}: RulesListProps) {
  const { t } = useTranslation(['common', 'automation'])

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-12">#</TableHead>
            <TableHead>{t('automation:ruleName')}</TableHead>
            <TableHead>{t('common:description')}</TableHead>
            <TableHead>{t('automation:trigger')}</TableHead>
            <TableHead>{t('automation:status')}</TableHead>
            <TableHead>{t('automation:triggerCount')}</TableHead>
            <TableHead>{t('automation:lastTriggered')}</TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline
              title={t('common:loading')}
              colSpan={8}
            />
          ) : rules.length === 0 ? (
            <EmptyStateInline
              title={t('automation:noRules')}
              colSpan={8}
            />
          ) : (
            rules.map((rule, index) => (
                  <TableRow key={rule.id} className={!rule.enabled ? "opacity-60" : ""}>
                    <TableCell className="text-muted-foreground">{index + 1}</TableCell>
                    <TableCell className="font-medium">{rule.name}</TableCell>
                    <TableCell className="text-muted-foreground max-w-md truncate">
                      {rule.description || '-'}
                    </TableCell>
                    <TableCell>
                      <code className="text-xs bg-muted px-2 py-1 rounded">
                        {rule.dsl?.split('\n')[0]?.replace('WHEN ', '') || '-'}
                      </code>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Switch
                          checked={rule.enabled}
                          onCheckedChange={() => onToggleStatus(rule)}
                        />
                        <StatusBadge status={rule.enabled ? 'enabled' : 'disabled'} />
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1 text-muted-foreground">
                        <Zap className="h-3.5 w-3.5" />
                        {rule.trigger_count || 0}
                      </div>
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {rule.last_triggered ? new Date(rule.last_triggered).toLocaleString() : '-'}
                    </TableCell>
                    <TableCell className="text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onClick={() => onEdit(rule)}>
                            <Edit className="mr-2 h-4 w-4" />
                            {t('common:edit')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => onExecute(rule)}>
                            <Play className="mr-2 h-4 w-4" />
                            {t('automation:execute')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => onDelete(rule)}
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
