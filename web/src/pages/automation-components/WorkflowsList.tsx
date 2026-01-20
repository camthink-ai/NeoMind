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
import { Edit, Play, Trash2, MoreVertical } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { Workflow as WorkflowType } from "@/types"

interface WorkflowsListProps {
  workflows: WorkflowType[]
  loading: boolean
  onEdit: (workflow: WorkflowType) => void
  onDelete: (workflow: WorkflowType) => void
  onToggleStatus: (workflow: WorkflowType) => void
  onExecute: (workflow: WorkflowType) => void
}

function getTriggerLabel(trigger: any): string {
  switch (trigger.type) {
    case 'manual': return '手动执行'
    case 'cron': return '定时执行'
    case 'event': return '事件触发'
    case 'device': return '设备状态变化'
    default: return trigger.type || '手动'
  }
}

export function WorkflowsList({
  workflows,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
}: WorkflowsListProps) {
  const { t } = useTranslation(['common', 'automation'])

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-12">#</TableHead>
            <TableHead>{t('automation:workflowName')}</TableHead>
            <TableHead>{t('common:description')}</TableHead>
            <TableHead>{t('automation:triggerType')}</TableHead>
            <TableHead>{t('automation:status')}</TableHead>
            <TableHead>{t('automation:executionCount')}</TableHead>
            <TableHead>{t('automation:updatedAt')}</TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline
              title={t('common:loading')}
              colSpan={8}
            />
          ) : workflows.length === 0 ? (
            <EmptyStateInline
              title={t('automation:noWorkflows')}
              colSpan={8}
            />
          ) : (
            workflows.map((workflow, index) => (
                  <TableRow key={workflow.id} className={!workflow.enabled ? "opacity-60" : ""}>
                    <TableCell className="text-muted-foreground">{index + 1}</TableCell>
                    <TableCell className="font-medium">{workflow.name}</TableCell>
                    <TableCell className="text-muted-foreground max-w-md truncate">
                      {workflow.description || '-'}
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {workflow.triggers && workflow.triggers.length > 0 ? (
                          workflow.triggers.map((trigger, i) => (
                            <Badge key={i} variant="outline" className="text-xs">
                              {getTriggerLabel(trigger)}
                            </Badge>
                          ))
                        ) : (
                          <Badge variant="secondary" className="text-xs">手动执行</Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Switch
                          checked={workflow.enabled}
                          onCheckedChange={() => onToggleStatus(workflow)}
                        />
                        <StatusBadge status={workflow.enabled ? 'enabled' : 'disabled'} />
                      </div>
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {workflow.execution_count || 0}
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {workflow.updated_at ? new Date(workflow.updated_at).toLocaleString() : '-'}
                    </TableCell>
                    <TableCell className="text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onClick={() => onEdit(workflow)}>
                            <Edit className="mr-2 h-4 w-4" />
                            {t('common:edit')}
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => onExecute(workflow)}
                            disabled={!workflow.enabled}
                          >
                            <Play className="mr-2 h-4 w-4" />
                            {t('automation:execute')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => onDelete(workflow)}
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
