/**
 * Read-only Agent execution evidence panel (1.3 iter D).
 * Inputs (with times), output, actions, failure node — missing → unknown.
 */
import { useTranslation } from 'react-i18next'
import {
  AlertTriangle,
  CheckCircle2,
  HelpCircle,
  XCircle,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { textMini, textNano } from '@/design-system/tokens/typography'
import { formatTimestamp } from '@/lib/utils/format'
import {
  deriveAgentExecutionEvidence,
  type EvidencePresence,
  type FailureKind,
} from '@/lib/agentExecutionEvidence'
import type { AgentExecutionDetail } from '@/types/agent'

export interface AgentExecutionEvidencePanelProps {
  detail: AgentExecutionDetail | null
  className?: string
  compact?: boolean
}

function presenceLabel(
  presence: EvidencePresence,
  t: (k: string) => string,
): string {
  switch (presence) {
    case 'known':
      return t('agents:evidence.presence.known')
    case 'empty':
      return t('agents:evidence.presence.empty')
    case 'unknown':
      return t('agents:evidence.presence.unknown')
  }
}

function failureIcon(kind: FailureKind) {
  switch (kind) {
    case 'none':
      return CheckCircle2
    case 'unknown':
      return HelpCircle
    case 'error':
    case 'failed_action':
    case 'stop_reason':
      return XCircle
  }
}

function failureTone(kind: FailureKind): string {
  switch (kind) {
    case 'none':
      return 'text-success'
    case 'unknown':
      return 'text-muted-foreground'
    default:
      return 'text-error'
  }
}

export function AgentExecutionEvidencePanel({
  detail,
  className,
  compact,
}: AgentExecutionEvidencePanelProps) {
  const { t } = useTranslation(['agents', 'common'])
  const ev = deriveAgentExecutionEvidence(detail)
  const FailIcon = failureIcon(ev.failure.kind)

  return (
    <div
      className={cn('space-y-3', className)}
      data-evidence="agent-execution"
      data-inputs={ev.inputsPresence}
      data-output={ev.outputPresence}
      data-actions={ev.actionsPresence}
      data-failure={ev.failure.kind}
    >
      <div className="flex items-center justify-between gap-2">
        <h4 className={cn(textMini, 'font-medium uppercase tracking-wide text-muted-foreground')}>
          {t('agents:evidence.title')}
        </h4>
        {ev.tokensUsed != null && (
          <span className={cn(textNano, 'text-muted-foreground font-mono')}>
            {t('agents:evidence.tokens', { count: ev.tokensUsed })}
          </span>
        )}
      </div>

      {/* Failure node — first so operators see the blocker */}
      <div
        className={cn(
          'rounded-lg border bg-card p-3',
          ev.failure.kind === 'none'
            ? 'border-border'
            : ev.failure.kind === 'unknown'
              ? 'border-warning'
              : 'border-error',
        )}
        data-section="failure"
      >
        <div className="flex items-start gap-2">
          <FailIcon className={cn('h-4 w-4 mt-0.5 shrink-0', failureTone(ev.failure.kind))} aria-hidden="true" />
          <div className="min-w-0 space-y-0.5">
            <p className={cn(textMini, 'font-medium', failureTone(ev.failure.kind))}>
              {t(`agents:evidence.failure.${ev.failure.kind}`)}
            </p>
            {ev.failure.detail ? (
              <p className={cn(textMini, 'text-muted-foreground break-words font-mono')}>
                {ev.failure.detail}
              </p>
            ) : ev.failure.state === 'unknown' ? (
              <p className={cn(textNano, 'text-muted-foreground flex items-center gap-1')}>
                <HelpCircle className="h-3 w-3" aria-hidden="true" />
                {t('agents:evidence.unknownDetail')}
              </p>
            ) : null}
          </div>
        </div>
      </div>

      {/* Inputs */}
      <section data-section="inputs" className="rounded-lg border border-border bg-card p-3">
        <div className="flex items-center justify-between mb-1.5">
          <p className={cn(textMini, 'font-medium text-muted-foreground')}>
            {t('agents:evidence.inputs')}
          </p>
          <Badge variant="outline" className={cn(textNano)}>
            {presenceLabel(ev.inputsPresence, t)}
          </Badge>
        </div>
        {ev.inputsPresence === 'unknown' && (
          <p className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
            <HelpCircle className="h-3.5 w-3.5" />
            {t('agents:evidence.inputsUnknown')}
          </p>
        )}
        {ev.inputsPresence === 'empty' && (
          <p className={cn(textMini, 'text-muted-foreground')}>
            {t('agents:evidence.inputsEmpty')}
          </p>
        )}
        {ev.inputs.length > 0 && (
          <ul className={cn('space-y-1.5', compact && 'max-h-40 overflow-y-auto')}>
            {ev.inputs.map((row, i) => (
              <li key={`${row.source}-${i}`} className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <p className={cn(textMini, 'font-mono truncate')}>{row.source}</p>
                  <p className={cn(textNano, 'text-muted-foreground')}>{row.dataType}</p>
                </div>
                <span className={cn(textNano, 'shrink-0 font-mono text-muted-foreground')}>
                  {row.timeState === 'known' && row.timestampMs != null
                    ? formatTimestamp(row.timestampMs)
                    : t('agents:evidence.timeUnknown')}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Output */}
      <section data-section="output" className="rounded-lg border border-border bg-card p-3">
        <div className="flex items-center justify-between mb-1.5">
          <p className={cn(textMini, 'font-medium text-muted-foreground')}>
            {t('agents:evidence.output')}
          </p>
          <Badge variant="outline" className={cn(textNano)}>
            {presenceLabel(ev.outputPresence, t)}
          </Badge>
        </div>
        {ev.outputText ? (
          <p className={cn(textMini, 'whitespace-pre-wrap break-words', compact && 'line-clamp-4')}>
            {ev.outputText}
          </p>
        ) : (
          <p className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
            {ev.outputPresence === 'unknown' ? (
              <>
                <HelpCircle className="h-3.5 w-3.5" />
                {t('agents:evidence.outputUnknown')}
              </>
            ) : (
              t('agents:evidence.outputEmpty')
            )}
          </p>
        )}
      </section>

      {/* Actions */}
      <section data-section="actions" className="rounded-lg border border-border bg-card p-3">
        <div className="flex items-center justify-between mb-1.5">
          <p className={cn(textMini, 'font-medium text-muted-foreground')}>
            {t('agents:evidence.actions')}
          </p>
          <Badge variant="outline" className={cn(textNano)}>
            {presenceLabel(ev.actionsPresence, t)}
          </Badge>
        </div>
        {ev.actionsPresence === 'unknown' && (
          <p className={cn(textMini, 'text-muted-foreground flex items-center gap-1')}>
            <HelpCircle className="h-3.5 w-3.5" />
            {t('agents:evidence.actionsUnknown')}
          </p>
        )}
        {ev.actionsPresence === 'empty' && (
          <p className={cn(textMini, 'text-muted-foreground')}>
            {t('agents:evidence.actionsEmpty')}
          </p>
        )}
        {ev.actions.length > 0 && (
          <ul className={cn('space-y-2', compact && 'max-h-40 overflow-y-auto')}>
            {ev.actions.map((a, i) => (
              <li key={i} className="flex items-start gap-2">
                {a.success ? (
                  <CheckCircle2 className="h-3.5 w-3.5 text-success mt-0.5 shrink-0" />
                ) : (
                  <AlertTriangle className="h-3.5 w-3.5 text-error mt-0.5 shrink-0" />
                )}
                <div className="min-w-0 flex-1">
                  <p className={cn(textMini, 'truncate')}>{a.description}</p>
                  <p className={cn(textNano, 'text-muted-foreground truncate')}>{a.target}</p>
                  <p className={cn(textNano, 'font-mono text-muted-foreground break-words')}>
                    {a.resultState === 'known'
                      ? a.result
                      : t('agents:evidence.resultUnknown')}
                  </p>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Stop reason (explicit when known) */}
      {ev.stopReasonPresence === 'known' && ev.stopReason && (
        <p className={cn(textNano, 'text-muted-foreground')}>
          {t('agents:evidence.stopReason')}:{' '}
          <span className="font-mono">{ev.stopReason}</span>
        </p>
      )}
    </div>
  )
}
