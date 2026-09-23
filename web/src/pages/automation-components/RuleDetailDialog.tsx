import { useTranslation } from 'react-i18next'
import {
  Zap, Clock, Hand, Activity, Bell, Code2, Bot,
  Edit3, Calendar, Timer, Gauge, Tag,
} from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { Rule } from '@/types/rule'
import { cn } from '@/lib/utils'
import { textMini, textNano } from '@/design-system/tokens/typography'

interface Props {
  rule: Rule | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onEdit: (rule: Rule) => void
}

/** Format a trigger for human-readable display */
function formatTrigger(trigger: Rule['trigger'], t: (k: string, o?: any) => string): { icon: any; label: string; detail: string } {
  switch (trigger.trigger_type) {
    case 'data_change':
      return {
        icon: Activity,
        label: t('triggers.dataChange', { defaultValue: 'Data Change' }),
        detail: trigger.sources?.length
          ? `${trigger.sources.length} ${t('sources', { defaultValue: 'sources' })}`
          : t('triggers.dataChangeDesc', { defaultValue: 'Triggers on data updates' }),
      }
    case 'schedule':
      return {
        icon: Clock,
        label: t('triggers.schedule', { defaultValue: 'Schedule' }),
        detail: trigger.cron || '—',
      }
    case 'manual':
      return {
        icon: Hand,
        label: t('triggers.manual', { defaultValue: 'Manual' }),
        detail: t('triggers.manualDesc', { defaultValue: 'Execute on demand' }),
      }
  }
}

/** Format a condition for human-readable display */
function formatCondition(rule: Rule): string {
  if (!rule.condition) return '—'
  const c = rule.condition
  switch (c.condition_type) {
    case 'comparison':
      return `${c.source || '?'} ${c.operator || '?'} ${c.threshold ?? c.threshold_value ?? '?'}`
    case 'range':
      return `${c.source || '?'} ∈ [${c.min ?? '?'}, ${c.max ?? '?'}]`
    case 'logical':
      return `${(c.operator || 'and').toUpperCase()} (${c.conditions?.length ?? 0} sub-conditions)`
    default:
      return JSON.stringify(c).slice(0, 80)
  }
}

/** Get icon + label for an action */
function getActionInfo(action: Rule['actions'][number], t: (k: string, o?: any) => string): { icon: any; label: string; detail: string } {
  switch (action.type) {
    case 'notify':
      return { icon: Bell, label: t('actions.notify', { defaultValue: 'Notify' }), detail: action.message.slice(0, 80) }
    case 'execute':
      return { icon: Code2, label: t('actions.execute', { defaultValue: 'Execute' }), detail: `${action.target_type}: ${action.target} → ${action.command}` }
    case 'trigger_agent':
      return { icon: Bot, label: t('actions.triggerAgent', { defaultValue: 'Trigger Agent' }), detail: action.agent_id }
    case 'run_operator':
      // Keyed off ruleBuilder.actionType — the `actions.*` keys the sibling
      // cases reference do not exist in the locale files, so those always
      // render their English defaultValue.
      return {
        icon: Bot,
        label: t('ruleBuilder.actionType.runOperator', { defaultValue: 'Run Operator' }),
        detail: action.agent_id,
      }
    default:
      return { icon: Zap, label: String((action as any).type), detail: '' }
  }
}

export function RuleDetailDialog({ rule, open, onOpenChange, onEdit }: Props) {
  const { t } = useTranslation(['automation', 'common'])

  if (!rule) return null

  const triggerInfo = formatTrigger(rule.trigger, t)
  const TriggerIcon = triggerInfo.icon

  const handleEdit = () => {
    onOpenChange(false)
    onEdit(rule)
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={rule.name}
      description={rule.description || undefined}
      icon={<Zap className="h-4 w-4 text-primary shrink-0" />}
      width="md"
      footer={
        <Button onClick={handleEdit} className="gap-1.5">
          <Edit3 className="h-4 w-4" />
          {t('common:edit', { defaultValue: 'Edit' })}
        </Button>
      }
    >
      <div className="space-y-4">
        {/* Rule ID (mono, for debugging/API reference) */}
        <div className={cn(textMini, 'text-muted-foreground flex items-center gap-1.5')}>
          <span className="uppercase tracking-wide">ID</span>
          <code className="font-mono bg-muted-30 px-1.5 py-0.5 rounded break-all">{rule.id}</code>
        </div>

        {/* Status row */}
        <div className="flex items-center gap-2 flex-wrap">
          <Badge variant={rule.enabled ? 'default' : 'secondary'} className="gap-1">
            <span className={cn('w-1.5 h-1.5 rounded-full', rule.enabled ? 'bg-success' : 'bg-muted-foreground')} />
            {rule.enabled ? t('enabled') : t('disabled', { defaultValue: 'Disabled' })}
          </Badge>
          <span className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
            <Gauge className="h-3 w-3" />
            {t('triggeredTimes', { count: rule.trigger_count })}
          </span>
          {rule.last_triggered && (
            <span className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
              <Calendar className="h-3 w-3" />
              {new Date(rule.last_triggered).toLocaleString()}
            </span>
          )}
          {rule.cooldown && rule.cooldown > 0 && (
            <span className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
              <Timer className="h-3 w-3" />
              {(rule.cooldown / 1000).toFixed(0)}s {t('cooldown', { defaultValue: 'cooldown' })}
            </span>
          )}
        </div>

        {/* Tags */}
        {rule.tags && rule.tags.length > 0 && (
          <div className="flex items-center gap-1.5 flex-wrap">
            <Tag className="h-3 w-3 text-muted-foreground shrink-0" />
            {rule.tags.map(tag => (
              <Badge key={tag} variant="outline" className={cn(textNano)}>{tag}</Badge>
            ))}
          </div>
        )}

        {/* Trigger */}
        <div>
          <h4 className={cn(textMini, 'font-medium uppercase tracking-wide text-muted-foreground mb-2')}>
            {t('trigger')}
          </h4>
          <div className="flex items-center gap-3 rounded-lg border border-border bg-card p-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary-light text-primary shrink-0">
              <TriggerIcon className="h-4 w-4" />
            </div>
            <div className="min-w-0">
              <p className="text-sm font-medium">{triggerInfo.label}</p>
              <p className={cn(textMini, 'text-muted-foreground truncate font-mono')} title={triggerInfo.detail}>
                {triggerInfo.detail}
              </p>
            </div>
          </div>
        </div>

        {/* Condition */}
        {rule.condition && (
          <div>
            <h4 className={cn(textMini, 'font-medium uppercase tracking-wide text-muted-foreground mb-2')}>
              {t('conditions')}
            </h4>
            <div className="rounded-lg border border-border bg-card p-3">
              <p className="text-sm font-mono break-all">{formatCondition(rule)}</p>
            </div>
          </div>
        )}

        {/* Actions */}
        <div>
          <h4 className={cn(textMini, 'font-medium uppercase tracking-wide text-muted-foreground mb-2')}>
            {t('actions')} ({rule.actions.length})
          </h4>
          <div className="space-y-2">
            {rule.actions.map((action, i) => {
              const info = getActionInfo(action, t)
              const ActionIcon = info.icon
              return (
                <div key={i} className="flex items-center gap-3 rounded-lg border border-border bg-card p-3">
                  <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-muted-30 text-muted-foreground shrink-0">
                    <ActionIcon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium">{info.label}</p>
                    <p className={cn(textMini, 'text-muted-foreground truncate')} title={info.detail}>
                      {info.detail}
                    </p>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </div>
    </UnifiedFormDialog>
  )
}
