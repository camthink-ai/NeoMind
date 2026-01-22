import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Card } from "@/components/ui/card"
import { EmptyStateInline } from "@/components/shared"
import { Clock, AlertCircle, FileText } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { AgentExecution } from "@/types"

interface AgentExecutionsListProps {
  executions: AgentExecution[]
  loading: boolean
  agentId: string
  onViewDetail?: (agentId: string, executionId: string) => void
}

export function AgentExecutionsList({
  executions,
  loading,
  agentId,
  onViewDetail,
}: AgentExecutionsListProps) {
  const { t } = useTranslation(['common', 'agents'])

  const formatDateTime = (dateStr: string) => {
    try {
      return new Date(dateStr).toLocaleString()
    } catch {
      return '-'
    }
  }

  const getStatusLabel = (status: string) => {
    switch (status) {
      case 'Running': return t('agents:executionStatus.running')
      case 'Completed': return t('agents:executionStatus.completed')
      case 'Failed': return t('agents:executionStatus.failed')
      case 'Cancelled': return t('agents:executionStatus.cancelled')
      default: return status
    }
  }

  const getStatusClass = (status: string) => {
    switch (status) {
      case 'Running': return 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400'
      case 'Completed': return 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
      case 'Failed': return 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
      case 'Cancelled': return 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-400'
      default: return 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-400'
    }
  }

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-12">#</TableHead>
            <TableHead>{t('agents:timestamp')}</TableHead>
            <TableHead>{t('agents:triggerType')}</TableHead>
            <TableHead>{t('agents:status')}</TableHead>
            <TableHead>{t('agents:duration')}</TableHead>
            <TableHead>{t('agents:error')}</TableHead>
            {onViewDetail && <TableHead className="w-16"></TableHead>}
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline
              title={t('common:loading')}
              colSpan={onViewDetail ? 7 : 6}
            />
          ) : executions.length === 0 ? (
            <EmptyStateInline
              title={t('agents:noExecutions')}
              colSpan={onViewDetail ? 7 : 6}
            />
          ) : (
            executions.map((execution, index) => (
              <TableRow key={execution.id} className={onViewDetail ? "cursor-pointer hover:bg-muted/50" : ""}>
                <TableCell className="text-muted-foreground">{index + 1}</TableCell>
                <TableCell className="text-sm">
                  <div className="flex items-center gap-2">
                    <Clock className="h-3.5 w-3.5 text-muted-foreground" />
                    {formatDateTime(execution.timestamp)}
                  </div>
                </TableCell>
                <TableCell>
                  <code className="text-xs bg-muted px-2 py-1 rounded">
                    {execution.trigger_type}
                  </code>
                </TableCell>
                <TableCell>
                  <span className={`inline-flex items-center px-2 py-1 rounded text-xs font-medium ${getStatusClass(execution.status)}`}>
                    {getStatusLabel(execution.status)}
                  </span>
                </TableCell>
                <TableCell className="text-sm">
                  {execution.duration_ms > 0 ? `${execution.duration_ms}ms` : '-'}
                </TableCell>
                <TableCell className="text-sm">
                  {execution.error ? (
                    <div className="flex items-center gap-1 text-destructive">
                      <AlertCircle className="h-3.5 w-3.5" />
                      <span className="truncate max-w-xs">{execution.error}</span>
                    </div>
                  ) : '-'}
                </TableCell>
                {onViewDetail && (
                  <TableCell className="text-right">
                    <button
                      onClick={() => onViewDetail(agentId, execution.id)}
                      className="p-1 hover:bg-muted rounded transition-colors"
                      title={t('agents:viewDetails') || "View Details"}
                    >
                      <FileText className="h-4 w-4 text-muted-foreground" />
                    </button>
                  </TableCell>
                )}
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </Card>
  )
}
