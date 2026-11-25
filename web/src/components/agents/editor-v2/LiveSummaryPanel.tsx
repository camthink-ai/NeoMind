/**
 * Right-rail live summary — plain-language mirror of the left form.
 * Sticky on desktop; does not compete with engineer jargon.
 */
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import type { AgentSpecDraft, InputRequirement, ToolPolicy, TriggerSpec } from '@/types'
import { normalizeNotify } from './NotifyPolicySection'
import { PlanConfirmLine } from './PlanConfirmLine'
import { templateById, type TaskTemplateId } from './templates'

export interface LiveSummaryPanelProps {
  draft: AgentSpecDraft
  templateId: TaskTemplateId | null
  className?: string
}

function whenLabel(
  trigger: TriggerSpec,
  t: (k: string, opts?: Record<string, unknown>) => string,
): string {
  if (trigger.kind === 'cron') {
    const hour = trigger.expr.trim().split(/\s+/)[1] ?? '9'
    return t('editorV2.planLine.dailyAt', { time: `${String(hour).padStart(2, '0')}:00` })
  }
  if (trigger.kind === 'interval') {
    return t('editorV2.planLine.everyMinutes', {
      count: Math.max(1, Math.round(trigger.every_secs / 60)),
    })
  }
  if (trigger.kind === 'event') return t('editorV2.planLine.onEvent')
  return t('editorV2.planLine.manual')
}

function toolsLabel(policy: ToolPolicy, t: (k: string) => string): string {
  if (policy === 'disabled') return t('editorV2.planLine.toolsOff')
  if (policy === 'unrestricted') return t('editorV2.planLine.toolsOn')
  return t('editorV2.planLine.toolsScoped')
}

function sourcePlain(req: InputRequirement): string {
  const src = req.source
  switch (src.kind) {
    case 'extension_instance':
      return src.id + (req.operation ? ` / ${req.operation}` : '')
    case 'device_metric':
      return `${src.device_id}:${src.field}`
    case 'whole_device':
      return src.device_id
    case 'upstream_ai':
      return `ai:${src.agent_id}:${src.field}`
    case 'caller_payload':
      return 'caller_payload'
  }
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-1">
      <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </dt>
      <dd className="text-sm text-foreground break-words">{children}</dd>
    </div>
  )
}

export function LiveSummaryPanel({ draft, templateId, className }: LiveSummaryPanelProps) {
  const { t } = useTranslation('agents')
  const notify = normalizeNotify(draft.notify)
  const tmpl = templateById(templateId)
  const reads = draft.input?.requirements ?? []

  const notifyText = notify.on_complete
    ? t('editorV2.planLine.notifyComplete')
    : notify.on_failure
      ? t('editorV2.planLine.notifyFailure')
      : t('editorV2.planLine.notifySilent')

  return (
    <aside
      className={cn(
        'flex h-full min-h-0 flex-col gap-4 overflow-y-auto border-border bg-muted-20 p-5',
        className,
      )}
      data-section="live-summary"
    >
      <header className="space-y-1">
        <h2 className="text-sm font-semibold tracking-tight text-foreground">
          {t('editorV2.summary.title')}
        </h2>
        <p className="text-xs text-muted-foreground">{t('editorV2.summary.hint')}</p>
      </header>

      <PlanConfirmLine draft={draft} className="bg-card" />

      <dl className="space-y-4">
        <Row label={t('editorV2.summary.template')}>
          {tmpl ? t(tmpl.nameKey) : t('editorV2.summary.noTemplate')}
        </Row>
        <Row label={t('editorV2.summary.name')}>
          {draft.name.trim() || (
            <span className="text-muted-foreground">{t('editorV2.summary.empty')}</span>
          )}
        </Row>
        <Row label={t('editorV2.summary.mission')}>
          {draft.user_prompt.trim() ? (
            <span className="line-clamp-4 whitespace-pre-wrap">{draft.user_prompt}</span>
          ) : (
            <span className="text-muted-foreground">{t('editorV2.summary.empty')}</span>
          )}
        </Row>
        <Row label={t('editorV2.summary.when')}>{whenLabel(draft.trigger, t)}</Row>
        <Row label={t('editorV2.summary.sources')}>
          {reads.length === 0 ? (
            <span className="text-muted-foreground">{t('editorV2.summary.noSources')}</span>
          ) : (
            <ul className="space-y-1">
              {reads.map((r, i) => (
                <li key={`${r.key}-${i}`} className="text-sm">
                  {sourcePlain(r)}
                </li>
              ))}
            </ul>
          )}
        </Row>
        <Row label={t('editorV2.summary.tools')}>{toolsLabel(draft.tool_policy, t)}</Row>
        <Row label={t('editorV2.summary.notify')}>
          <div className="space-y-1">
            <div>{notifyText}</div>
            {notify.channels.length > 0 && (
              <div className="text-xs text-muted-foreground">
                {notify.channels.join(', ')}
              </div>
            )}
          </div>
        </Row>
      </dl>
    </aside>
  )
}
