/**
 * One-line confirmation of the effective plan — not a competing jargon column.
 */
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import type { AgentSpecDraft, ToolPolicy, TriggerSpec } from '@/types'
import { normalizeTrigger, normalizeToolPolicy } from './defaults'
import { normalizeNotify } from './NotifyPolicySection'

export interface PlanConfirmLineProps {
  draft: AgentSpecDraft
  className?: string
}

function triggerPhrase(
  trigger: TriggerSpec | null | undefined,
  t: (k: string, opts?: Record<string, unknown>) => string,
): string {
  const safe = normalizeTrigger(trigger)
  if (safe.kind === 'cron') {
    const parts = (safe.expr ?? '').trim().split(/\s+/)
    const hour = parts[1] ?? '9'
    return t('agents:editorV2.planLine.dailyAt', {
      time: `${String(hour).padStart(2, '0')}:00`,
    })
  }
  if (safe.kind === 'interval') {
    const mins = Math.max(1, Math.round(safe.every_secs / 60))
    return t('agents:editorV2.planLine.everyMinutes', { count: mins })
  }
  if (safe.kind === 'event') return t('editorV2.planLine.onEvent')
  return t('editorV2.planLine.manual')
}

function toolsPhrase(policy: ToolPolicy | null | undefined, t: (k: string) => string): string {
  const safe = normalizeToolPolicy(policy)
  if (safe === 'disabled') return t('editorV2.planLine.toolsOff')
  if (safe === 'unrestricted') return t('editorV2.planLine.toolsOn')
  return t('editorV2.planLine.toolsScoped')
}

export function PlanConfirmLine({ draft, className }: PlanConfirmLineProps) {
  const { t } = useTranslation('agents')
  const notify = normalizeNotify(draft.notify)
  const reads = draft.input?.requirements?.length ?? 0

  const line = useMemo(() => {
    const parts = [
      triggerPhrase(draft.trigger, t),
      t('agents:editorV2.planLine.reads', { count: reads }),
      toolsPhrase(draft.tool_policy, t),
    ]
    if (notify.on_complete) parts.push(t('editorV2.planLine.notifyComplete'))
    else if (notify.on_failure) parts.push(t('editorV2.planLine.notifyFailure'))
    else parts.push(t('editorV2.planLine.notifySilent'))
    return parts.join(t('editorV2.planLine.sep'))
  }, [draft, notify.on_complete, notify.on_failure, reads, t])

  return (
    <p
      className={cn(
        'rounded-lg border border-dashed border-border bg-muted-20 px-4 py-3 text-sm text-foreground',
        className,
      )}
      data-section="plan-confirm"
      data-testid="plan-confirm-line"
    >
      <span className="font-medium text-muted-foreground">{t('editorV2.planLine.label')} </span>
      {line}
    </p>
  )
}
