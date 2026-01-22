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
import { EmptyStateInline } from "@/components/shared"
import { Bot, Edit, Play, Trash2, MoreVertical, Clock, Brain, Activity, Zap } from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
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

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'Active': return 'bg-green-500/10 text-green-700 dark:text-green-400 border-green-500/20'
      case 'Paused': return 'bg-gray-500/10 text-gray-700 dark:text-gray-400 border-gray-500/20'
      case 'Error': return 'bg-red-500/10 text-red-700 dark:text-red-400 border-red-500/20'
      case 'Executing': return 'bg-blue-500/10 text-blue-700 dark:text-blue-400 border-blue-500/20'
      default: return 'bg-gray-500/10 text-gray-700 dark:text-gray-400 border-gray-500/20'
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
          <TableRow className="hover:bg-transparent border-b">
            <TableHead className="w-12">#</TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Bot className="h-4 w-4 text-muted-foreground" />
                {t('agents:agentName')}
              </div>
            </TableHead>
            <TableHead>{t('agents:status')}</TableHead>
            <TableHead>统计</TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Clock className="h-4 w-4 text-muted-foreground" />
                {t('agents:lastExecution')}
              </div>
            </TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline title={t('common:loading')} colSpan={6} />
          ) : agents.length === 0 ? (
            <EmptyStateInline title={t('agents:noAgents')} colSpan={6} />
          ) : (
            agents.map((agent, index) => (
              <TableRow
                key={agent.id}
                className={cn(
                  "group transition-colors hover:bg-muted/50",
                  agent.status === 'Paused' && "opacity-60"
                )}
              >
                <TableCell className="text-muted-foreground text-sm font-medium">
                  {index + 1}
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <div className={cn(
                      "p-1.5 rounded-md transition-colors",
                      agent.status === 'Active' ? "bg-purple-500/10 text-purple-600" : "bg-muted text-muted-foreground"
                    )}>
                      <Bot className="h-3.5 w-3.5" />
                    </div>
                    <span className="font-medium">{agent.name}</span>
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <Switch
                      checked={agent.status === 'Active'}
                      onCheckedChange={() => onToggleStatus(agent)}
                      disabled={agent.status === 'Executing'}
                      className="scale-90"
                    />
                    <Badge variant="outline" className={cn("text-xs", getStatusColor(agent.status))}>
                      {getStatusLabel(agent.status)}
                    </Badge>
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-3 text-sm">
                    <div className="flex items-center gap-1 text-muted-foreground">
                      <Activity className="h-3 w-3" />
                      <span className="font-medium">{agent.execution_count}</span>
                    </div>
                    <div className="flex items-center gap-1 text-green-600">
                      <Zap className="h-3 w-3" />
                      <span className="font-medium">{agent.success_count}</span>
                    </div>
                    <div className="flex items-center gap-1 text-red-500">
                      <Activity className="h-3 w-3" />
                      <span className="font-medium">{agent.error_count}</span>
                    </div>
                    {agent.avg_duration_ms > 0 && (
                      <div className="text-xs text-muted-foreground">
                        {agent.avg_duration_ms}ms
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
                      <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
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
