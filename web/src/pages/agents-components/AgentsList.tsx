/**
 * Agents List - Unified card-based table design
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
import { Bot, Edit, Play, Trash2, MoreVertical, Clock, Brain, Activity, Zap, CheckCircle2, XCircle, Loader2, History, Bell } from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import type { AiAgent, AgentRole } from "@/types"

interface AgentsListProps {
  agents: AiAgent[]
  loading: boolean
  onEdit: (agent: AiAgent) => void
  onDelete: (agent: AiAgent) => void
  onToggleStatus: (agent: AiAgent) => void
  onExecute: (agent: AiAgent) => void
  onViewMemory?: (agentId: string, agentName: string) => void
  onViewExecutions?: (agent: AiAgent) => void
}

// Role configuration
const ROLE_CONFIG: Record<AgentRole, { label: string; icon: typeof Brain; color: string }> = {
  Monitor: { label: 'agents:roles.monitor', icon: Activity, color: 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800' },
  Executor: { label: 'agents:roles.executor', icon: Zap, color: 'text-orange-700 bg-orange-50 border-orange-200 dark:text-orange-400 dark:bg-orange-950/30 dark:border-orange-800' },
  Analyst: { label: 'agents:roles.analyst', icon: Brain, color: 'text-purple-700 bg-purple-50 border-purple-200 dark:text-purple-400 dark:bg-purple-950/30 dark:border-purple-800' },
}

// Status configuration
const STATUS_CONFIG: Record<string, { label: string; icon: typeof CheckCircle2; color: string }> = {
  Active: { label: 'agents:status.active', icon: CheckCircle2, color: 'text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800' },
  Paused: { label: 'agents:status.paused', icon: XCircle, color: 'text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700' },
  Error: { label: 'agents:status.error', icon: XCircle, color: 'text-red-700 bg-red-50 border-red-200 dark:text-red-400 dark:bg-red-950/30 dark:border-red-800' },
  Executing: { label: 'agents:status.executing', icon: Loader2, color: 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800' },
}

const ITEMS_PER_PAGE = 10

export function AgentsList({
  agents,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
  onViewMemory,
  onViewExecutions,
}: AgentsListProps) {
  const { t } = useTranslation(['common', 'agents'])
  const [page, setPage] = useState(1)

  // Reset pagination when data changes
  const totalPages = Math.ceil(agents.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  const paginatedAgents = agents.slice(startIndex, endIndex)

  return (
    <>
      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent border-b bg-muted/30">
              <TableHead className="w-10 text-center">#</TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Bot className="h-4 w-4" />
                  {t('agents:agentName')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Brain className="h-4 w-4" />
                  {t('agents:role')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Activity className="h-4 w-4" />
                  {t('agents:stats')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Clock className="h-4 w-4" />
                  {t('agents:lastExecution')}
                </div>
              </TableHead>
              <TableHead className="text-center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('agents:statusColumn')}
                </div>
              </TableHead>
              <TableHead className="w-12"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={7} />
            ) : agents.length === 0 ? (
              <EmptyStateInline title={t('agents:noAgents')} colSpan={7} />
            ) : (
              paginatedAgents.map((agent, index) => {
                const roleConfig = ROLE_CONFIG[agent.role] || ROLE_CONFIG.Analyst
                const RoleIcon = roleConfig.icon
                const statusConfig = STATUS_CONFIG[agent.status] || STATUS_CONFIG.Paused
                const StatusIcon = statusConfig.icon

                return (
                  <TableRow
                    key={agent.id}
                    className={cn(
                      "group transition-colors hover:bg-muted/50",
                      agent.status === 'Paused' && "opacity-50"
                    )}
                  >
                    <TableCell className="text-center">
                      <span className="text-xs text-muted-foreground font-medium">{startIndex + index + 1}</span>
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-3">
                        <div className={cn(
                          "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                          agent.status === 'Active' || agent.status === 'Executing'
                            ? "bg-purple-500/10 text-purple-600"
                            : "bg-muted text-muted-foreground"
                        )}>
                          <Bot className="h-4 w-4" />
                        </div>
                        <div>
                          <div className="font-medium text-sm">{agent.name}</div>
                        </div>
                      </div>
                    </TableCell>

                    <TableCell>
                      <Badge variant="outline" className={cn("text-xs gap-1.5", roleConfig.color)}>
                        <RoleIcon className="h-3 w-3" />
                        {t(roleConfig.label)}
                      </Badge>
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-4 text-xs">
                        <div className="flex items-center gap-1.5">
                          <Activity className="h-3.5 w-3.5 text-muted-foreground" />
                          <span className="font-medium">{agent.execution_count}</span>
                        </div>
                        <div className="flex items-center gap-1.5">
                          <CheckCircle2 className="h-3.5 w-3.5 text-green-600" />
                          <span className="font-medium text-green-600">{agent.success_count}</span>
                        </div>
                        {agent.error_count > 0 && (
                          <div className="flex items-center gap-1.5">
                            <XCircle className="h-3.5 w-3.5 text-red-500" />
                            <span className="font-medium text-red-500">{agent.error_count}</span>
                          </div>
                        )}
                      </div>
                    </TableCell>

                    <TableCell>
                      <span className="text-xs text-muted-foreground">{formatTimestamp(agent.last_execution_at || undefined)}</span>
                    </TableCell>

                    <TableCell className="text-center">
                      <div className="flex items-center justify-center gap-2">
                        <Switch
                          checked={agent.status === 'Active' || agent.status === 'Executing'}
                          onCheckedChange={() => onToggleStatus(agent)}
                          disabled={agent.status === 'Executing'}
                          className="scale-90"
                        />
                        <Badge variant="outline" className={cn("text-xs gap-1 hidden sm:flex", statusConfig.color)}>
                          <StatusIcon className={cn("h-3 w-3", agent.status === 'Executing' && "animate-spin")} />
                          {t(statusConfig.label)}
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
                          {onViewExecutions && (
                            <DropdownMenuItem onClick={() => onViewExecutions(agent)}>
                              <History className="mr-2 h-4 w-4" />
                              {t('agents:viewExecutions')}
                            </DropdownMenuItem>
                          )}
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
                )
              })
            )}
          </TableBody>
        </Table>
      </Card>

      {agents.length > ITEMS_PER_PAGE && (
        <div className="sticky bottom-0 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 pt-4 pb-2 border-t mt-4">
          <Pagination
            total={agents.length}
            pageSize={ITEMS_PER_PAGE}
            currentPage={page}
            onPageChange={setPage}
          />
        </div>
      )}
    </>
  )
}
