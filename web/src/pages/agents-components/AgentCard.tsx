/**
 * Agent Card - Grid item for displaying an AI Agent
 */

import { useTranslation } from "react-i18next"
import { Button, IconButton } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { useConfirm } from "@/components/ui/use-confirm"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Bot,
  Edit,
  Play,
  Trash2,
  MoreVertical,
  Activity,
  Pause,
  AlertTriangle,
  Loader2,
  Clock,
  CheckCircle2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { interactiveCard } from "@/design-system/tokens/size"
import type { AiAgent } from "@/types"

interface AgentCardProps {
  agent: AiAgent & { currentThinking?: string | null }
  onToggleStatus: (agent: AiAgent) => void
  onExecute: (agent: AiAgent) => void
  onEdit: (agent: AiAgent) => void
  onDelete: (agent: AiAgent) => void
  onClick: () => void
}

// Status icons configuration (labels use i18n)
const STATUS_CONFIG: Record<string, { icon: typeof Activity; color: string; bg: string }> = {
  Active: { icon: Activity, color: 'text-success', bg: 'bg-success-light' },
  Paused: { icon: Pause, color: 'text-muted-foreground', bg: 'bg-muted-50' },
  Error: { icon: AlertTriangle, color: 'text-error', bg: 'bg-error-light' },
  Executing: { icon: Loader2, color: 'text-info', bg: 'bg-info-light' },
  // One-shot task finished — check icon, quiet green; falls back to Paused
  // styling via the `|| STATUS_CONFIG.Paused` default in the component.
  Completed: { icon: CheckCircle2, color: 'text-success', bg: 'bg-muted-50' },
}

export function AgentCard({
  agent,
  onToggleStatus,
  onExecute,
  onEdit,
  onDelete,
  onClick,
}: AgentCardProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { confirm } = useConfirm()

  const handleToggleStatus = async () => {
    const isCurrentlyActive = agent.status === 'Active' || agent.status === 'Executing'

    const confirmed = await confirm({
      title: isCurrentlyActive ? t('agents:confirm.pauseTitle') : t('agents:confirm.resumeTitle'),
      description: isCurrentlyActive
        ? t('agents:confirm.pauseDesc', { name: agent.name })
        : t('agents:confirm.resumeDesc', { name: agent.name }),
      confirmText: isCurrentlyActive ? t('agents:confirm.confirmPause') : t('agents:confirm.confirmResume'),
      cancelText: t('common:cancel'),
      variant: 'default',
    })

    if (confirmed) {
      onToggleStatus(agent)
    }
  }

  const statusConfig = STATUS_CONFIG[agent.status] || STATUS_CONFIG.Paused
  const StatusIcon = statusConfig.icon

  const successRate = agent.execution_count > 0
    ? Math.round((agent.success_count / agent.execution_count) * 100)
    : 0

  // Get status label from i18n
  const getStatusLabel = (status: string) => {
    const key = status.toLowerCase() as 'active' | 'paused' | 'error' | 'executing'
    return t(`agents:status.${key}`)
  }

  return (
    <div
      className={cn(
        "group relative p-4 h-full cursor-pointer",
        interactiveCard,
      )}
      onClick={onClick}
    >
      {/* Header: Icon + Name + Status */}
      <div className="flex items-start gap-3 mb-4">
        {/* Icon with status glow */}
        <div className={cn(
          "w-12 h-12 rounded-xl flex items-center justify-center shrink-0 transition-colors",
          agent.status === 'Active' && "bg-success-light",
          agent.status === 'Executing' && "bg-info-light",
          agent.status === 'Error' && "bg-error-light",
          agent.status === 'Paused' && "bg-muted-50"
        )}>
          <Bot className={cn(
            "h-6 w-6",
            agent.status === 'Active' && "text-success",
            agent.status === 'Executing' && "text-info",
            agent.status === 'Error' && "text-error",
            agent.status === 'Paused' && "text-muted-foreground"
          )} />
        </div>

        {/* Name and Status */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-base font-semibold truncate" title={agent.name}>{agent.name}</h3>
            <StatusIcon className={cn(
              "h-4 w-4 shrink-0",
              statusConfig.color,
              agent.status === 'Executing' && "animate-spin"
            )} />
          </div>
          <p className="text-sm text-muted-foreground mt-0.5 line-clamp-1">
            {agent.user_prompt || agent.description || t('agents:card.noDescription')}
          </p>
        </div>

        {/* More menu */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <IconButton
              onClick={(e) => e.stopPropagation()}
              aria-label={t('agents:card.moreOptions')}
            >
              <MoreVertical className="h-4 w-4" />
            </IconButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-40">
            <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onEdit(agent); }}>
              <Edit className="h-4 w-4 mr-2" />
              {t('common:edit')}
            </DropdownMenuItem>
            <DropdownMenuItem
              onClick={(e) => { e.stopPropagation(); onExecute(agent); }}
              disabled={agent.status === 'Executing'}
            >
              <Play className="h-4 w-4 mr-2" />
              {t('agents:execute')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={(e) => { e.stopPropagation(); onDelete(agent); }}
              className="text-error"
            >
              <Trash2 className="h-4 w-4 mr-2" />
              {t('common:delete')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Stats Grid - simplified without borders */}
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)] gap-2 mb-3">
        {/* Execution Count */}
        <div className="text-center p-2 rounded-lg bg-muted-30">
          <div className="text-lg font-semibold tabular-nums">{agent.execution_count}</div>
          <div className="text-xs text-muted-foreground">{t('agents:card.executions')}</div>
        </div>

        {/* Success Rate */}
        <div className="text-center p-2 rounded-lg bg-muted-30">
          <div className={cn(
            "text-lg font-semibold tabular-nums",
            successRate >= 80 ? "text-success" : successRate >= 50 ? "text-warning" : "text-error"
          )}>
            {successRate}%
          </div>
          <div className="text-xs text-muted-foreground">{t('agents:card.successRate')}</div>
        </div>

        {/* Avg Duration */}
        <div className="text-center p-2 rounded-lg bg-muted-30">
          <div className="text-lg font-semibold tabular-nums">
            {agent.avg_duration_ms > 0 ? `${(agent.avg_duration_ms / 1000).toFixed(1)}s` : '-'}
          </div>
          <div className="text-xs text-muted-foreground">{t('agents:card.avgDuration')}</div>
        </div>
      </div>

      {/* Footer: Executing status or Last Execution + Toggle */}
      {agent.status === 'Executing' ? (
        <div className="flex items-center gap-2 pt-1.5 border-t border-info bg-info-light -mx-1 px-1">
          <Loader2 className="h-4 w-4 text-info animate-spin shrink-0" />
          <span className="text-xs text-info truncate flex-1">
            {agent.currentThinking || t('agents:thinking.executing')}
          </span>
        </div>
      ) : (
        <div className="flex items-center justify-between pt-1.5 border-t border-border">
          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Clock className="h-4 w-4" />
            <span className="truncate max-w-[120px]">
              {agent.last_execution_at ? formatTimestamp(agent.last_execution_at, false) : t('agents:card.neverExecuted')}
            </span>
          </div>

          <Switch
            checked={agent.status === 'Active'}
            onCheckedChange={handleToggleStatus}
            onClick={(e) => e.stopPropagation()}
            aria-label={t('agents:card.toggleStatus', { name: agent.name })}
          />
        </div>
      )}
    </div>
  )
}
