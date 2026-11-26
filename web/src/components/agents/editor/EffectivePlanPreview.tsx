/**
 * EffectivePlan preview — quiet secondary summary (not a competing card).
 * POST /api/agents/plan-preview (static, zero reads).
 * Accepts optional `previewFn` so tests can mock without hitting the network.
 */
import { useCallback, useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Loader2, RefreshCw, AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { agentsApi } from '@/lib/api/agents'
import type { AgentSpecDraft, EffectivePlan, ToolPolicy } from '@/types'

export type PlanPreviewFn = (
  draft: AgentSpecDraft,
) => Promise<{ effective_plan: EffectivePlan; dry_run: boolean }>

export interface EffectivePlanPreviewProps {
  draft: AgentSpecDraft
  /** Override network call (tests / offline). Defaults to agentsApi.planPreview. */
  previewFn?: PlanPreviewFn
  /** Debounce ms before auto-refresh (0 = only manual). Default 400. */
  debounceMs?: number
  className?: string
}

function toolPolicyLabel(policy: ToolPolicy | null | undefined, t: (k: string) => string): string {
  if (!policy || policy === 'disabled') return t('editorV2.tools.disabled')
  if (policy === 'unrestricted') return t('editorV2.tools.unrestricted')
  if (typeof policy === 'object' && 'scoped' in policy && policy.scoped) {
    const tools = policy.scoped.allowed_tools?.join(', ') || '—'
    return `${t('editorV2.tools.scoped')}: ${tools}`
  }
  return t('editorV2.tools.disabled')
}

function SummaryRow({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <div className="space-y-0.5">
      <dt className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </dt>
      <dd className="text-sm text-foreground break-words">{children}</dd>
    </div>
  )
}

export function EffectivePlanPreview({
  draft,
  previewFn,
  debounceMs = 400,
  className,
}: EffectivePlanPreviewProps) {
  const { t } = useTranslation('agents')
  const [plan, setPlan] = useState<EffectivePlan | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)

  const runPreview = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const fn = previewFn ?? ((d: AgentSpecDraft) => agentsApi.planPreview(d))
      const resp = await fn(draft)
      setPlan(resp.effective_plan)
    } catch (e) {
      setPlan(null)
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [draft, previewFn])

  useEffect(() => {
    if (debounceMs <= 0) return
    const id = window.setTimeout(() => {
      void runPreview()
    }, debounceMs)
    return () => window.clearTimeout(id)
  }, [runPreview, debounceMs, tick])

  return (
    <aside
      className={cn(
        'rounded-lg border border-dashed border-border bg-muted-20 p-4 space-y-3',
        className,
      )}
      data-section="plan-preview"
      data-loading={loading ? 'true' : 'false'}
    >
      <header className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {t('editorV2.planPreview.title')}
          </h3>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="shrink-0 text-muted-foreground"
          onClick={() => {
            if (debounceMs <= 0) void runPreview()
            else setTick((n) => n + 1)
          }}
          disabled={loading}
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
          )}
          {t('editorV2.planPreview.refresh')}
        </Button>
      </header>

      {error && (
        <div
          className="flex items-start gap-2 rounded-md border border-error-light bg-error-light px-3 py-2 text-xs text-error"
          data-testid="plan-preview-error"
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
          <span>{error || t('editorV2.planPreview.error')}</span>
        </div>
      )}

      {plan && !error && (
        <dl className="space-y-3" data-testid="plan-preview-body">
          <SummaryRow label={t('editorV2.planPreview.triggerSummary')}>
            <span className="font-mono text-xs">{plan.trigger_summary}</span>
          </SummaryRow>
          {plan.timezone != null && plan.timezone !== '' && (
            <SummaryRow label={t('editorV2.planPreview.timezone')}>
              {plan.timezone}
            </SummaryRow>
          )}
          <SummaryRow label={t('editorV2.planPreview.toolPolicy')}>
            {toolPolicyLabel(plan.tool_policy, t)}
          </SummaryRow>
          <SummaryRow label={t('editorV2.planPreview.reads')}>
            {plan.authorized_reads.length === 0 ? (
              <span className="text-muted-foreground">{t('editorV2.planPreview.noReads')}</span>
            ) : (
              <ul className="mt-0.5 space-y-1">
                {plan.authorized_reads.map((r) => (
                  <li key={r.key} className="font-mono text-xs text-muted-foreground">
                    {r.key} → {r.source_id}
                    {r.operation ? ` / ${r.operation}` : ''}
                  </li>
                ))}
              </ul>
            )}
          </SummaryRow>
          {plan.warnings && plan.warnings.length > 0 && (
            <SummaryRow label={t('editorV2.planPreview.warnings')}>
              <ul className="mt-0.5 space-y-1 text-warning text-xs">
                {plan.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            </SummaryRow>
          )}
        </dl>
      )}

      {!plan && !error && !loading && (
        <p className="text-sm text-muted-foreground">{t('editorV2.planPreview.empty')}</p>
      )}
    </aside>
  )
}
