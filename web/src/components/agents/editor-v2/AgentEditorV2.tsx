/**
 * M3 orthogonal AgentSpec-v2 editor shell.
 * Composes Trigger / Input / Tool / Notify sections + EffectivePlanPreview.
 * Sole create/edit surface on AgentsPage (legacy FullScreen removed).
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Bot } from 'lucide-react'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
  FullScreenDialogFooter,
} from '@/components/automation/dialog/FullScreenDialog'
import { Button } from '@/components/ui/button'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { AgentExecutionEvidencePanel } from '@/components/agents/AgentExecutionEvidencePanel'
import { v2SpecToFormState } from '@/lib/api/agentV2Adapter'
import type {
  AgentSpecDraft,
  AiAgentDetail,
  AgentExecutionDetail,
  CreateAgentRequest,
  NotifyPolicy,
} from '@/types'
import { defaultDailyBriefDraft, detailToV2Draft } from './defaults'
import { TriggerSection } from './TriggerSection'
import { InputContractSection } from './InputContractSection'
import { ToolPolicySection } from './ToolPolicySection'
import { NotifyPolicySection, normalizeNotify } from './NotifyPolicySection'
import { EffectivePlanPreview } from './EffectivePlanPreview'

export interface AgentEditorV2Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Legacy detail when editing; create seeds daily-brief when omitted. */
  agent?: AiAgentDetail
  /** Optional evidence for the preserved evidence panel. */
  evidenceDetail?: AgentExecutionDetail | null
  /**
   * Called with a legacy CreateAgentRequest projected from the v2 draft
   * so the existing agents page save path keeps working.
   */
  onSave: (data: CreateAgentRequest) => Promise<void>
}

function draftToLegacyRequest(draft: AgentSpecDraft): CreateAgentRequest {
  const form = v2SpecToFormState(draft)
  return {
    name: form.name,
    description: form.description,
    user_prompt: form.user_prompt,
    llm_backend_id: form.llm_backend_id,
    schedule: form.schedule,
    resources: form.resources,
    notify: form.notify,
    tool_config: form.tool_config,
    output_schema: form.output_schema,
    execution_mode: 'structured',
  }
}

export function AgentEditorV2({
  open,
  onOpenChange,
  agent,
  evidenceDetail,
  onSave,
}: AgentEditorV2Props) {
  const { t } = useTranslation(['agents', 'common'])
  const [draft, setDraft] = useState<AgentSpecDraft>(() => defaultDailyBriefDraft())
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!open) return
    setDraft(agent ? detailToV2Draft(agent) : defaultDailyBriefDraft())
  }, [open, agent])

  const patch = (partial: Partial<AgentSpecDraft>) => {
    setDraft((prev) => ({ ...prev, ...partial }))
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      await onSave(draftToLegacyRequest(draft))
      onOpenChange(false)
    } finally {
      setSaving(false)
    }
  }

  const notify: NotifyPolicy = normalizeNotify(draft.notify)

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<Bot className="h-5 w-5" />}
        iconBg="bg-accent-purple-light"
        iconColor="text-accent-purple"
        title={agent ? t('agents:editorV2.titleEdit') : t('agents:editorV2.titleCreate')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-auto">
          <div className="mx-auto grid w-full max-w-6xl gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-4 min-w-0">
              <p className="text-sm text-muted-foreground">{t('agents:editorV2.dailyBriefHint')}</p>

              <section className="rounded-xl border bg-card p-4 space-y-4">
                <FormField label={t('agents:editorV2.name')} required>
                  <Input
                    value={draft.name}
                    onChange={(e) => patch({ name: e.target.value })}
                  />
                </FormField>
                <FormField label={t('agents:editorV2.description')}>
                  <Input
                    value={draft.description ?? ''}
                    onChange={(e) => patch({ description: e.target.value })}
                  />
                </FormField>
                <FormField label={t('agents:editorV2.prompt')} required>
                  <Textarea
                    value={draft.user_prompt}
                    onChange={(e) => patch({ user_prompt: e.target.value })}
                    rows={4}
                  />
                </FormField>
              </section>

              <TriggerSection
                value={draft.trigger}
                onChange={(trigger) => patch({ trigger })}
              />
              <InputContractSection
                value={draft.input}
                onChange={(input) => patch({ input })}
              />
              <ToolPolicySection
                value={draft.tool_policy}
                onChange={(tool_policy) => patch({ tool_policy })}
              />
              <NotifyPolicySection
                value={notify}
                onChange={(n) => patch({ notify: n })}
              />

              {evidenceDetail !== undefined && (
                <AgentExecutionEvidencePanel detail={evidenceDetail} />
              )}
            </div>

            <EffectivePlanPreview draft={draft} className="lg:sticky lg:top-4 h-fit" />
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>
      <FullScreenDialogFooter>
        <div className="flex w-full items-center justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            {t('common:cancel')}
          </Button>
          <Button onClick={() => void handleSave()} disabled={saving || !draft.name.trim()}>
            {saving ? t('agents:common.saving') : t('agents:common.save')}
          </Button>
        </div>
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}
