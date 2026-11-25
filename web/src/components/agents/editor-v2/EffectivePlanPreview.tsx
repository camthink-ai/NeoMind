/**
 * EffectivePlan preview panel — POST /api/agents/v2/plan-preview (static, zero reads).
 * Accepts an optional `previewFn` so tests can mock without hitting the network.
 */
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Eye, Loader2, RefreshCw, AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cardBase } from '@/design-system/tokens/size'
import { textNano } from '@/design-system/tokens/typography'
import { cn } from '@/lib/utils'
import { agentsApi } from '@/lib/api/agents'
import type { AgentSpecDraft, EffectivePlan, ToolPolicy } from '@/types'

export type PlanPreviewFn = (
  draft: AgentSpecDraft,
) => Promise<{ effective_plan: EffectivePlan; dry_run: boolean }>

export interface EffectivePlanPreviewProps {
  draft: AgentSpecDraft
  /** Override network call (tests / offline). Defaults to agentsApi.planPreviewV2. */
  previewFn?: PlanPreviewFn
  /** Debounce ms before auto-refresh (0 = only manual). Default 400. */
  debounceMs?: number
  className?: string
}

function toolPolicyLabel(policy: ToolPolicy, t: (k: string) => string): string {
  if (policy === 'disabled') return t('editorV2.tools.disabled')
  if (policy === 'unrestricted') return t('editorV2.tools.unrestricted')
  const tools = policy.scoped.allowed_tools?.join(', ') || '—'
  return `${t('editorV2.tools.scoped')}: ${tools}`
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
      const fn = previewFn ?? ((d: AgentSpecDraft) => agentsApi.planPreviewV2(d))
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
      className={cn(cardBase, 'p-4 space-y-3', className)}
      data-section="plan-preview"
      data-loading={loading ? 'true' : 'false'}
    >
      <header className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Eye className="h-4 w-4 text-muted-foreground" />
          <h3 className="text-sm font-medium text-foreground">
            {t('editorV2.planPreview.title')}
          </h3>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
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

      <p className={cn('text-muted-foreground', textNano)}>
        {t('editorV2.planPreview.hint')}
      </p>

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
        <dl className="space-y-2 text-sm" data-testid="plan-preview-body">
          <div>
            <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.triggerSummary')}</dt>
            <dd className="font-mono text-xs break-all">{plan.trigger_summary}</dd>
          </div>
          {plan.timezone != null && plan.timezone !== '' && (
            <div>
              <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.timezone')}</dt>
              <dd>{plan.timezone}</dd>
            </div>
          )}
          <div>
            <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.toolPolicy')}</dt>
            <dd>{toolPolicyLabel(plan.tool_policy, t)}</dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.reads')}</dt>
            <dd>
              {plan.authorized_reads.length === 0 ? (
                <span className="text-muted-foreground">{t('editorV2.planPreview.noReads')}</span>
              ) : (
                <ul className="mt-1 space-y-1">
                  {plan.authorized_reads.map((r) => (
                    <li key={r.key} className="font-mono text-xs">
                      {r.key} → {r.source_id}
                      {r.operation ? ` / ${r.operation}` : ''}
                    </li>
                  ))}
                </ul>
              )}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.externalReads')}</dt>
            <dd className="font-mono text-xs">
              weather={plan.external_read_counts.weather} tariff=
              {plan.external_read_counts.tariff} other=
              {plan.external_read_counts.other}
            </dd>
          </div>
          {plan.warnings && plan.warnings.length > 0 && (
            <div>
              <dt className="text-xs text-muted-foreground">{t('editorV2.planPreview.warnings')}</dt>
              <dd>
                <ul className="mt-1 space-y-1 text-warning text-xs">
                  {plan.warnings.map((w) => (
                    <li key={w}>{w}</li>
                  ))}
                </ul>
              </dd>
            </div>
          )}
        </dl>
      )}

      {!plan && !error && !loading && (
        <p className="text-sm text-muted-foreground">{t('editorV2.planPreview.empty')}</p>
      )}
    </aside>
  )
}
