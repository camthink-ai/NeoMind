/**
 * Read-only "why not triggered" panel (1.3 iter D).
 * Consumes GET /rules/:id/explain; blockers derived client-side.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  HelpCircle,
  Loader2,
  RefreshCw,
  XCircle,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { textMini, textNano } from '@/design-system/tokens/typography'
import {
  deriveRuleBlockingReasons,
  flattenConditionLeaves,
  type RuleBlockCode,
} from '@/lib/ruleExplain'
import type {
  ConditionExplanation,
  EvalOutcome,
  RuleExplanation,
} from '@/types/rule'

export type ExplainLoadStatus = 'idle' | 'loading' | 'error' | 'ready'

export interface RuleWhyNotTriggeredPanelProps {
  explanation: RuleExplanation | null
  status: ExplainLoadStatus
  onRetry?: () => void
  className?: string
}

function outcomeTone(result: EvalOutcome): string {
  switch (result) {
    case 'true':
      return 'text-success'
    case 'false':
      return 'text-error'
    case 'unknown':
      return 'text-warning'
  }
}

function primaryTone(code: RuleBlockCode): {
  icon: typeof CheckCircle2
  className: string
  border: string
} {
  switch (code) {
    case 'ready':
      return {
        icon: CheckCircle2,
        className: 'text-success',
        border: 'border-success',
      }
    case 'condition_unknown':
    case 'for_duration':
    case 'cooldown':
      return {
        icon: AlertTriangle,
        className: 'text-warning',
        border: 'border-warning',
      }
    case 'disabled':
    case 'condition_false':
      return {
        icon: XCircle,
        className: 'text-error',
        border: 'border-error',
      }
  }
}

function formatValue(v: number | string | undefined): string {
  if (v === undefined) return '—'
  return String(v)
}

function formatAge(secs: number | undefined, t: (k: string, o?: Record<string, unknown>) => string): string {
  if (secs == null) return '—'
  if (secs < 60) return t('automation:ruleExplain.ageSecs', { secs })
  if (secs < 3600) return t('automation:ruleExplain.ageMins', { mins: Math.floor(secs / 60) })
  return t('automation:ruleExplain.ageHours', { hours: Math.floor(secs / 3600) })
}

function primaryLabel(
  code: RuleBlockCode,
  detail: string | undefined,
  t: (k: string, o?: Record<string, unknown>) => string,
): string {
  switch (code) {
    case 'disabled':
      return t('automation:ruleExplain.block.disabled')
    case 'cooldown':
      return detail
        ? t('automation:ruleExplain.block.cooldown', { secs: detail })
        : t('automation:ruleExplain.block.cooldownUnknown')
    case 'condition_false':
      return t('automation:ruleExplain.block.condition_false')
    case 'condition_unknown': {
      if (!detail) return t('automation:ruleExplain.block.condition_unknownGeneric')
      const reasonKey = `automation:ruleExplain.unknownReason.${detail}`
      const reason = t(reasonKey)
      const reasonText = reason === reasonKey ? detail : reason
      return t('automation:ruleExplain.block.condition_unknown', { reason: reasonText })
    }
    case 'for_duration':
      return detail
        ? t('automation:ruleExplain.block.for_duration', { progress: detail })
        : t('automation:ruleExplain.block.for_durationGeneric')
    case 'ready':
      return t('automation:ruleExplain.block.ready')
  }
}

function LeafRow({ leaf, t }: { leaf: ConditionExplanation; t: (k: string, o?: Record<string, unknown>) => string }) {
  return (
    <tr className="border-t border-border">
      <td className={cn(textMini, 'py-1.5 pr-2 font-mono break-all align-top')}>
        {leaf.source ?? '—'}
      </td>
      <td className={cn(textMini, 'py-1.5 pr-2 font-mono align-top whitespace-nowrap')}>
        {formatValue(leaf.value)}
      </td>
      <td className={cn(textMini, 'py-1.5 pr-2 text-muted-foreground align-top whitespace-nowrap')}>
        {formatAge(leaf.value_age_secs, t)}
      </td>
      <td className={cn(textMini, 'py-1.5 align-top whitespace-nowrap')}>
        <span className={outcomeTone(leaf.result)}>
          {t(`automation:ruleExplain.outcome.${leaf.result}`)}
        </span>
        {leaf.unknown_reason && (
          <span className={cn(textNano, 'block text-muted-foreground')}>
            {(() => {
              const k = `automation:ruleExplain.unknownReason.${leaf.unknown_reason}`
              const v = t(k)
              return v === k ? leaf.unknown_reason : v
            })()}
          </span>
        )}
      </td>
    </tr>
  )
}

export function RuleWhyNotTriggeredPanel({
  explanation,
  status,
  onRetry,
  className,
}: RuleWhyNotTriggeredPanelProps) {
  const { t } = useTranslation('automation')
  const [notesOpen, setNotesOpen] = useState(false)

  if (status === 'loading' || status === 'idle') {
    return (
      <div className={cn('rounded-lg border border-border bg-card p-3', className)}>
        <div className="flex items-center gap-2 text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          <span className="text-sm">{t('automation:ruleExplain.loading')}</span>
        </div>
      </div>
    )
  }

  if (status === 'error' || !explanation) {
    return (
      <div className={cn('rounded-lg border border-error bg-card p-3', className)}>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 text-error">
            <XCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span className="text-sm">{t('automation:ruleExplain.loadFailed')}</span>
          </div>
          {onRetry && (
            <Button type="button" variant="outline" size="sm" onClick={onRetry} className="gap-1.5">
              <RefreshCw className="h-3.5 w-3.5" />
              {t('automation:ruleExplain.retry')}
            </Button>
          )}
        </div>
      </div>
    )
  }

  const summary = deriveRuleBlockingReasons(explanation)
  const tone = primaryTone(summary.primary.code)
  const PrimaryIcon = tone.icon
  const leaves = flattenConditionLeaves(explanation.condition)

  return (
    <div className={cn('space-y-3', className)}>
      <div className="flex items-center justify-between gap-2">
        <h4 className={cn(textMini, 'font-medium uppercase tracking-wide text-muted-foreground')}>
          {t('automation:ruleExplain.title')}
        </h4>
        {onRetry && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onRetry}
            className="h-7 gap-1 px-2 text-muted-foreground"
            aria-label={t('automation:ruleExplain.retry')}
          >
            <RefreshCw className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>

      {/* Primary summary */}
      <div
        className={cn('rounded-lg border bg-card p-3', tone.border)}
        data-block={summary.primary.code}
        data-would-evaluate={summary.wouldEvaluateTrue ? 'true' : 'false'}
      >
        <div className="flex items-start gap-2">
          <PrimaryIcon className={cn('h-4 w-4 mt-0.5 shrink-0', tone.className)} aria-hidden="true" />
          <div className="min-w-0 space-y-1">
            <p className={cn('text-sm font-medium', tone.className)}>
              {primaryLabel(summary.primary.code, summary.primary.detail, t)}
            </p>
            {summary.wouldEvaluateTrue && (
              <p className={cn(textMini, 'text-muted-foreground')}>
                {t('automation:ruleExplain.wouldFireHint')}
              </p>
            )}
            {summary.reasons.length > 1 && (
              <ul className={cn(textMini, 'text-muted-foreground list-disc pl-4 space-y-0.5')}>
                {summary.reasons.slice(1).map((r) => (
                  <li key={`${r.code}-${r.detail ?? ''}`}>
                    {primaryLabel(r.code, r.detail, t)}
                  </li>
                ))}
              </ul>
            )}
            <p className={cn(textNano, 'text-muted-foreground')}>
              {t('automation:ruleExplain.snapshotAt', {
                time: new Date(explanation.snapshot_at).toLocaleString(),
              })}
            </p>
          </div>
        </div>
      </div>

      {/* Live condition leaves */}
      {leaves.length > 0 && (
        <div>
          <h5 className={cn(textMini, 'font-medium text-muted-foreground mb-1.5')}>
            {t('automation:ruleExplain.conditionTree')}
          </h5>
          <div className="rounded-lg border border-border bg-card overflow-x-auto px-3 pb-2">
            <table className="w-full text-left">
              <thead>
                <tr className={cn(textNano, 'text-muted-foreground uppercase tracking-wide')}>
                  <th className="pt-2 pb-1 pr-2 font-medium">{t('automation:ruleExplain.source')}</th>
                  <th className="pt-2 pb-1 pr-2 font-medium">{t('automation:ruleExplain.value')}</th>
                  <th className="pt-2 pb-1 pr-2 font-medium">{t('automation:ruleExplain.age')}</th>
                  <th className="pt-2 pb-1 font-medium">{t('automation:ruleExplain.result')}</th>
                </tr>
              </thead>
              <tbody>
                {leaves.map((leaf, i) => (
                  <LeafRow key={`${leaf.source ?? 'leaf'}-${i}`} leaf={leaf} t={t} />
                ))}
              </tbody>
            </table>
          </div>
          {explanation.condition_met && (
            <p className={cn(textMini, 'mt-1.5 text-muted-foreground')}>
              {t('automation:ruleExplain.conditionMet')}:{' '}
              <span className={outcomeTone(explanation.condition_met)}>
                {t(`automation:ruleExplain.outcome.${explanation.condition_met}`)}
              </span>
            </p>
          )}
        </div>
      )}

      {/* Gates */}
      <div className="grid gap-2 sm:grid-cols-2">
        {explanation.for_duration && (
          <div className="rounded-lg border border-border bg-card p-3">
            <p className={cn(textMini, 'font-medium text-muted-foreground mb-1')}>
              {t('automation:ruleExplain.forDuration')}
            </p>
            <Badge
              variant="outline"
              className={explanation.for_duration.satisfied ? 'text-success' : 'text-warning'}
            >
              {explanation.for_duration.satisfied
                ? t('automation:ruleExplain.satisfied')
                : t('automation:ruleExplain.notSatisfied')}
            </Badge>
            <p className={cn(textMini, 'mt-1.5 text-muted-foreground font-mono')}>
              {t('automation:ruleExplain.elapsed', {
                elapsed: explanation.for_duration.elapsed_secs ?? 0,
                configured: explanation.for_duration.configured_secs,
              })}
            </p>
          </div>
        )}
        <div className="rounded-lg border border-border bg-card p-3">
          <p className={cn(textMini, 'font-medium text-muted-foreground mb-1')}>
            {t('automation:ruleExplain.cooldown')}
          </p>
          <Badge
            variant="outline"
            className={explanation.cooldown.in_cooldown ? 'text-warning' : 'text-muted-foreground'}
          >
            {explanation.cooldown.in_cooldown
              ? t('automation:ruleExplain.inCooldown')
              : t('automation:ruleExplain.notInCooldown')}
          </Badge>
          {explanation.cooldown.in_cooldown && explanation.cooldown.remaining_secs != null && (
            <p className={cn(textMini, 'mt-1.5 text-muted-foreground font-mono')}>
              {t('automation:ruleExplain.remaining', { secs: explanation.cooldown.remaining_secs })}
            </p>
          )}
          {!explanation.cooldown.in_cooldown && explanation.cooldown.configured_secs > 0 && (
            <p className={cn(textMini, 'mt-1.5 text-muted-foreground font-mono')}>
              {explanation.cooldown.configured_secs}s
            </p>
          )}
        </div>
      </div>

      {/* Last execution */}
      <div className="rounded-lg border border-border bg-card p-3">
        <p className={cn(textMini, 'font-medium text-muted-foreground mb-1')}>
          {t('automation:ruleExplain.lastExecution')}
        </p>
        {explanation.last_execution ? (
          <div className="space-y-1">
            <div className="flex items-center gap-2 flex-wrap">
              {explanation.last_execution.success ? (
                <Badge variant="outline" className="text-success gap-1">
                  <CheckCircle2 className="h-3 w-3" />
                  {t('automation:ruleExplain.execSuccess')}
                </Badge>
              ) : (
                <Badge variant="outline" className="text-error gap-1">
                  <XCircle className="h-3 w-3" />
                  {t('automation:ruleExplain.execFailed')}
                </Badge>
              )}
              <span className={cn(textMini, 'text-muted-foreground')}>
                {new Date(explanation.last_execution.triggered_at).toLocaleString()}
              </span>
              <span className={cn(textMini, 'text-muted-foreground font-mono')}>
                {explanation.last_execution.duration_ms}ms
              </span>
            </div>
            {explanation.last_execution.error && (
              <p className={cn(textMini, 'text-error break-all')}>
                {explanation.last_execution.error}
              </p>
            )}
          </div>
        ) : (
          <p className={cn(textMini, 'text-muted-foreground flex items-center gap-1.5')}>
            <HelpCircle className="h-3.5 w-3.5" aria-hidden="true" />
            {t('automation:ruleExplain.noLastExecution')}
          </p>
        )}
      </div>

      {/* Notes */}
      {explanation.notes.length > 0 && (
        <div>
          <button
            type="button"
            className={cn(textMini, 'flex items-center gap-1 font-medium text-muted-foreground')}
            aria-expanded={notesOpen}
            onClick={() => setNotesOpen((v) => !v)}
          >
            {notesOpen ? (
              <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
            )}
            {t('automation:ruleExplain.notes')} ({explanation.notes.length})
          </button>
          {notesOpen && (
            <ul className={cn(textMini, 'mt-1.5 space-y-1 text-muted-foreground list-disc pl-5')}>
              {explanation.notes.map((n, i) => (
                <li key={i}>{n}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
