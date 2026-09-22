/**
 * Agent Card — grid item for the agents page.
 *
 * Three zones with one hierarchy each:
 *   header  identity (status-tinted icon + name + axis meta) & health (dot)
 *   hero    what it last produced (latest output as mini metrics) — or, in
 *           order, what's happening now (executing / error) or what it's for
 *           (prompt excerpt when nothing has run yet)
 *   footer  freshness + collapsed stats + enable toggle, one quiet line
 *
 * Visual language follows the app's card pattern (status-tinted icon block,
 * see ExtensionCard); the old badge-soup first row and the three stat tiles
 * are gone — they competed with the name and the output for attention.
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
  Bot,
  Edit,
  Play,
  Trash2,
  MoreVertical,
  Pause,
  AlertTriangle,
  Loader2,
  Clock,
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

  const successRate = agent.execution_count > 0
    ? Math.round((agent.success_count / agent.execution_count) * 100)
    : null

  // Axis vocabulary in customer language — same words the editor and the
  // detail panel use, never the execution-mode jargon.
  const hasContract = (agent.output_fields?.length ?? 0) > 0
  const roleKey = agent.execution_mode === 'structured' && hasContract
    ? 'recordData'
    : agent.execution_mode === 'free'
      ? 'actOrInvestigate'
      : 'answer'
  const isToolMemory = (agent.memory_mode ?? (agent.execution_mode === 'structured' ? 'tool' : 'assistant')) === 'tool'

  const latestEntries = Object.entries(agent.latest_output ?? {})
  const isExecuting = agent.status === 'Executing'
  const isError = agent.status === 'Error'

  return (
    <div
      className={cn("group relative flex h-full flex-col p-4 cursor-pointer", interactiveCard)}
      onClick={onClick}
    >
      {/* ── Header: identity + health ─────────────────────────────── */}
      <div className="flex items-start gap-3">
        <div className={cn(
          "flex h-10 w-10 shrink-0 items-center justify-center rounded-lg transition-colors",
          agent.status === 'Active' && "bg-success-light",
          isExecuting && "bg-info-light",
          isError && "bg-error-light",
          (agent.status === 'Paused' || agent.status === 'Completed') && "bg-muted-50"
        )}>
          <Bot className={cn(
            "h-5 w-5",
            agent.status === 'Active' && "text-success",
            isExecuting && "text-info",
            isError && "text-error",
            (agent.status === 'Paused' || agent.status === 'Completed') && "text-muted-foreground"
          )} />
        </div>

        <div className="min-w-0 flex-1">
          <h3 className="truncate text-base font-semibold leading-tight" title={agent.name}>
            {agent.name}
          </h3>
          <p className="mt-0.5 truncate text-xs text-muted-foreground">
            {t(`agents:card.role.${roleKey}`)}
            <span className="mx-1.5 text-border">·</span>
            {t(`agents:card.memory.${isToolMemory ? 'tool' : 'assistant'}`)}
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-1.5 pt-1">
          {isError && (
            <AlertTriangle className="h-4 w-4 text-error" aria-label={t('agents:status.error')} />
          )}
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
                disabled={isExecuting}
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
      </div>

      {/* ── Hero: what it produced / what's happening ─────────────── */}
      <div className="mt-3 flex-1">
        {isExecuting ? (
          <div className="flex items-center gap-2 rounded-lg bg-info-light px-2.5 py-2">
            <Loader2 className="h-4 w-4 shrink-0 animate-spin text-info" />
            <span className="min-w-0 flex-1 truncate text-xs text-info">
              {agent.currentThinking || t('agents:thinking.executing')}
            </span>
          </div>
        ) : isError ? (
          <p className="line-clamp-2 rounded-lg bg-error-light px-2.5 py-2 text-xs text-error" title={agent.error}>
            {agent.error || t('agents:card.errorUnknown')}
          </p>
        ) : latestEntries.length > 0 ? (
          // Latest published fields, read like the metric cards they are:
          // muted label above a tabular value. Up to 4 shown, 2 per row.
          <div className="grid grid-cols-2 gap-x-3 gap-y-2">
            {latestEntries.slice(0, 4).map(([field, value]) => (
              <div key={field} className="min-w-0">
                <p className="truncate text-xs text-muted-foreground" title={field}>{field}</p>
                <p className="truncate text-lg font-semibold tabular-nums leading-tight" title={String(value)}>
                  {String(value)}
                </p>
              </div>
            ))}
          </div>
        ) : (
          <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
            {agent.user_prompt || agent.description || t('agents:card.noDescription')}
          </p>
        )}
      </div>

      {/* ── Footer: freshness + collapsed stats + toggle ──────────── */}
      <div className="mt-3 flex items-center justify-between gap-2 border-t border-border pt-2.5">
        <div className="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
          <Clock className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate">
            {agent.last_execution_at
              ? formatTimestamp(agent.last_execution_at, false)
              : t('agents:card.neverExecuted')}
          </span>
          {successRate !== null && (
            <span className="shrink-0">
              {" · "}
              {agent.execution_count}
              {t('agents:card.runsSuffix')}
              {" · "}
              {successRate}%
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
