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
import { EmptyStateInline } from "@/components/shared"
import { Bot, Edit, Play, Trash2, MoreVertical, Clock, Brain } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { AiAgent } from "@/types"

interface AgentsListProps {
  agents: AiAgent[]
  loading: boolean
  onEdit: (agent: AiAgent) => void
  onDelete: (agent: AiAgent) => void
  onToggleStatus: (agent: AiAgent) => void
  onExecute: (agent: AiAgent) => void
  onViewMemory?: (agentId: string, agentName: string) => void
}

export function AgentsList({
  agents,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
  onViewMemory,
}: AgentsListProps) {
  const { t } = useTranslation(['common', 'agents'])

  const getStatusLabel = (status: string) => {
    switch (status) {
      case 'Active': return t('agents:status.active')
      case 'Paused': return t('agents:status.paused')
      case 'Error': return t('agents:status.error')
      case 'Executing': return t('agents:status.executing')
      default: return status
    }
  }

  const formatDateTime = (dateStr: string | null) => {
    if (!dateStr) return '-'
    try {
      return new Date(dateStr).toLocaleString()
    } catch {
      return '-'
    }
  }

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-12">#</TableHead>
            <TableHead>{t('agents:agentName')}</TableHead>
            <TableHead>{t('agents:status')}</TableHead>
            <TableHead>{t('agents:executions')}</TableHead>
            <TableHead>{t('agents:lastExecution')}</TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline
              title={t('common:loading')}
              colSpan={6}
            />
          ) : agents.length === 0 ? (
            <EmptyStateInline
              title={t('agents:noAgents')}
              colSpan={6}
            />
          ) : (
            agents.map((agent, index) => (
              <TableRow key={agent.id} className={agent.status === 'Paused' ? "opacity-60" : ""}>
                <TableCell className="text-muted-foreground">{index + 1}</TableCell>
                <TableCell className="font-medium flex items-center gap-2">
                  <Bot className="h-4 w-4 text-primary" />
                  {agent.name}
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <Switch
                      checked={agent.status === 'Active'}
                      onCheckedChange={() => onToggleStatus(agent)}
                      disabled={agent.status === 'Executing'}
                    />
                    <span className="text-xs font-medium">{getStatusLabel(agent.status)}</span>
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-3 text-sm">
                    <div className="flex items-center gap-1 text-muted-foreground">
                      <Play className="h-3 w-3" />
                      <span>{agent.execution_count}</span>
                    </div>
                    <div className="flex items-center gap-1 text-green-600">
                      <span className="font-medium">{agent.success_count}</span>
                    </div>
                    <div className="flex items-center gap-1 text-red-600">
                      <span className="font-medium">{agent.error_count}</span>
                    </div>
                    {agent.avg_duration_ms > 0 && (
                      <div className="flex items-center gap-1 text-muted-foreground">
                        <Clock className="h-3 w-3" />
                        <span>{agent.avg_duration_ms}ms</span>
                      </div>
                    )}
                  </div>
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {formatDateTime(agent.last_execution_at)}
                </TableCell>
                <TableCell className="text-right">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon">
                        <MoreVertical className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={() => onEdit(agent)}>
                        <Edit className="mr-2 h-4 w-4" />
                        {t('common:edit')}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onClick={() => onExecute(agent)}
                        disabled={agent.status === 'Executing'}
                      >
                        <Play className="mr-2 h-4 w-4" />
                        {t('agents:execute')}
                      </DropdownMenuItem>
                      {onViewMemory && (
                        <DropdownMenuItem onClick={() => onViewMemory(agent.id, agent.name)}>
                          <Brain className="mr-2 h-4 w-4" />
                          {t('agents:viewMemory')}
                        </DropdownMenuItem>
                      )}
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onClick={() => onDelete(agent)}
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
