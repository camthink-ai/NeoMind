/**
 * Rule Detail Dialog — reuses RuleInspectorView so the Automation page and
 * Chat side-inspector share one high-craft detail presentation (Hero + Copy ID
 * + When→If→Then logic tree + Trigger History tab).
 */

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Edit3, Zap } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Button } from '@/components/ui/button'
import { type ExplainLoadStatus } from '@/components/automation/RuleWhyNotTriggeredPanel'
import { RuleInspectorView } from '@/components/chat/attachments/inspector/RuleInspectorView'
import { api } from '@/lib/api'
import type { Rule, RuleExecutionResult, RuleExplanation } from '@/types'

interface RuleDetailDialogProps {
  rule: Rule | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onEdit: (rule: Rule) => void
}

export function RuleDetailDialog({
  rule,
  open,
  onOpenChange,
  onEdit,
}: RuleDetailDialogProps) {
  const { t, i18n } = useTranslation(['automation', 'common'])
  const isZh = i18n.language?.toLowerCase().startsWith('zh')

  const [explain, setExplain] = useState<RuleExplanation | null>(null)
  const [explainStatus, setExplainStatus] = useState<ExplainLoadStatus>('idle')
  const [history, setHistory] = useState<RuleExecutionResult[]>([])
  const [fetchNonce, setFetchNonce] = useState(0)

  useEffect(() => {
    if (!open || !rule) {
      setExplain(null)
      setExplainStatus('idle')
      setHistory([])
      return
    }
    let cancelled = false
    setExplainStatus('loading')
    api
      .getRuleExplain(rule.id, { skipErrorToast: true })
      .then((res) => {
        if (cancelled) return
        setExplain(res)
        setExplainStatus('ready')
      })
      .catch(() => {
        if (cancelled) return
        setExplain(null)
        setExplainStatus('error')
      })
    api
      .getRuleHistory(rule.id, { skipErrorToast: true })
      .then((res) => {
        if (cancelled) return
        setHistory(res?.executions ?? [])
      })
      .catch(() => {
        if (cancelled) return
        setHistory([])
      })
    return () => {
      cancelled = true
    }
  }, [open, rule?.id, fetchNonce])

  if (!rule) return null

  const handleEdit = () => {
    onOpenChange(false)
    onEdit(rule)
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={rule.name}
      icon={<Zap className="h-4 w-4 text-foreground shrink-0" strokeWidth={1.75} />}
      width="lg"
      footer={
        <Button onClick={handleEdit} className="gap-1.5">
          <Edit3 className="h-4 w-4" />
          {t('common:edit', { defaultValue: 'Edit' })}
        </Button>
      }
    >
      <RuleInspectorView
        key={rule.id}
        rule={rule}
        ruleExplain={explain}
        explainStatus={explainStatus}
        history={history}
        entityId={rule.id}
        isZh={isZh}
        onRefresh={() => setFetchNonce((n) => n + 1)}
      />
    </UnifiedFormDialog>
  )
}
