/**
 * Agent Card - Grid item for displaying an AI Agent
 *
 * The card answers the operator's three questions in order: what role this
 * agent plays (badges), whether it's healthy right now (status + freshness),
 * and what it last produced (latest output row). Stats collapsed to a single
 * meta line — they're operator telemetry, not the card's job.
 */

import { useTranslation } from "react-i18next"
import { IconButton } from "@/components/ui/button"
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
  BarChart3,
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
  Wrench,
  BrainCircuit,
  MessageSquareText,
  Workflow,
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

/** Role badge derived from the compiled axes — customer language, never the
 *  execution-mode jargon. */
function roleOf(agent: AiAgent): { icon: typeof BarChart3; key: string } {
  const hasContract = (agent.output_fields?.length ?? 0) > 0
  if (agent.execution_mode === 'structured' && hasContract) return { icon: BarChart3, key: 'recordData' }
  if (agent.execution_mode === 'free') return { icon: Workflow, key: 'actOrInvestigate' }
  return { icon: MessageSquareText, key: 'answer' }
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
    : null

  const role = roleOf(agent)
  const RoleIcon = role.icon
  const isToolMemory = (agent.memory_mode ?? (agent.execution_mode === 'structured' ? 'tool' : 'assistant')) === 'tool'
  const MemoryIcon = isToolMemory ? Wrench : BrainCircuit
  const latestEntries = Object.entries(agent.latest_output ?? {})

  return (
    <div
      className={cn(
        "group relative p-4 h-full cursor-pointer",
        interactiveCard,
      )}
      onClick={onClick}
    >
      {/* Role badges + status + menu */}
      <div className="flex items-center gap-1.5 mb-2">
        <RoleIcon className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
        <span className="text-xs text-muted-foreground truncate">{t(`agents:card.role.${role.key}`)}</span>
        <span className={cn("h-1 w-1 rounded-full bg-border shrink-0")} />
        <MemoryIcon className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
        <span className="text-xs text-muted-foreground truncate">{t(`agents:card.memory.${isToolMemory ? 'tool' : 'assistant'}`)}</span>
        <StatusIcon className={cn(
          "h-4 w-4 shrink-0 ml-auto",
          statusConfig.color,
          agent.status === 'Executing' && "animate-spin"
        )} />
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

      {/* Name + prompt line */}
      <div className="flex items-start gap-2.5 mb-2">
        <div className={cn(
          "w-9 h-9 rounded-lg flex items-center justify-center shrink-0 transition-colors",
          agent.status === 'Active' && "bg-success-light",
          agent.status === 'Executing' && "bg-info-light",
          agent.status === 'Error' && "bg-error-light",
          agent.status === 'Paused' && "bg-muted-50"
        )}>
          <Bot className={cn(
            "h-5 w-5",
            agent.status === 'Active' && "text-success",
            agent.status === 'Executing' && "text-info",
            agent.status === 'Error' && "text-error",
            agent.status === 'Paused' && "text-muted-foreground"
          )} />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold truncate" title={agent.name}>{agent.name}</h3>
          <p className="text-xs text-muted-foreground mt-0.5 line-clamp-1">
            {agent.user_prompt || agent.description || t('agents:card.noDescription')}
          </p>
        </div>
      </div>

      {/* Latest output — the "it's alive and producing" row */}
      {latestEntries.length > 0 ? (
        <div className="flex flex-wrap gap-1 mb-2.5">
          {latestEntries.slice(0, 3).map(([field, value]) => (
            <span
              key={field}
              className="inline-flex max-w-full items-baseline gap-1 rounded-md bg-muted-30 px-1.5 py-0.5 text-xs"
              title={`${field} = ${String(value)}`}
            >
              <span className="text-muted-foreground shrink-0">{field}</span>
              <span className="font-medium truncate">{String(value)}</span>
            </span>
          ))}
          {latestEntries.length > 3 && (
            <span className="text-xs text-muted-foreground self-center">+{latestEntries.length - 3}</span>
          )}
        </div>
      ) : null}

      {/* Error line replaces stats when broken */}
      {agent.status === 'Error' && (
        <p className="mb-2 line-clamp-1 rounded-md bg-error-light px-2 py-1 text-xs text-error">
          {agent.error || t('agents:card.errorUnknown')}
        </p>
      )}

      {/* Executing banner */}
      {agent.status === 'Executing' && (
        <div className="flex items-center gap-2 mb-2 rounded-md bg-info-light px-2 py-1">
          <Loader2 className="h-3.5 w-3.5 text-info animate-spin shrink-0" />
          <span className="text-xs text-info truncate flex-1">
            {agent.currentThinking || t('agents:thinking.executing')}
          </span>
        </div>
      )}

      {/* Footer: freshness + collapsed stats + toggle */}
      <div className="flex items-center justify-between gap-2 pt-1.5 border-t border-border">
        <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
          <Clock className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">
            {agent.last_execution_at
              ? formatTimestamp(agent.last_execution_at, false)
              : t('agents:card.neverExecuted')}
          </span>
          {successRate !== null && agent.execution_count > 0 && (
            <span className="shrink-0 text-muted-foreground/70">
              · {agent.execution_count}次 {successRate}%
            </span>
          )}
        </div>

        <Switch
          checked={agent.status === 'Active'}
          onCheckedChange={handleToggleStatus}
          onClick={(e) => e.stopPropagation()}
          aria-label={t('agents:card.toggleStatus', { name: agent.name })}
        />
      </div>
    </div>
  )
}
