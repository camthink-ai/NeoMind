/**
 * M3 orthogonal AgentSpec-v2 editor shell.
 * Single readable form hierarchy + quiet plan summary (legacy FullScreen removed).
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Loader2 } from 'lucide-react'
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
import type {
  AgentSpecDraft,
  AiAgentDetail,
  AgentExecutionDetail,
  NotifyPolicy,
} from '@/types'
import { defaultDailyBriefDraft, detailToV2Draft } from './defaults'
import { EditorSection, FormSurface } from './EditorSection'
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
  /** Save the v2 draft directly (POST/PUT `/agents/v2`) — no legacy projection. */
  onSave: (draft: AgentSpecDraft) => Promise<void>
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
      await onSave(draft)
      onOpenChange(false)
    } finally {
      setSaving(false)
    }
  }

  const notify: NotifyPolicy = normalizeNotify(draft.notify)
  const canSave = draft.name.trim().length > 0 && draft.user_prompt.trim().length > 0

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        title={agent ? t('agents:editorV2.titleEdit') : t('agents:editorV2.titleCreate')}
        subtitle={t('agents:editorV2.subtitle')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-auto bg-background">
          <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 p-4 md:p-6 lg:flex-row lg:items-start lg:gap-8">
            {/* Primary form — one composition */}
            <div className="min-w-0 flex-1 space-y-5">
              <FormSurface>
                <EditorSection
                  first
                  title={t('agents:editorV2.basicsTitle')}
                  description={t('agents:editorV2.basicsHint')}
                  data-section="basics"
                >
                  <div className="grid gap-4 sm:grid-cols-2">
                    <FormField label={t('agents:editorV2.name')} required>
                      <Input
                        value={draft.name}
                        onChange={(e) => patch({ name: e.target.value })}
                        autoFocus={!agent}
                      />
                    </FormField>
                    <FormField label={t('agents:editorV2.description')}>
                      <Input
                        value={draft.description ?? ''}
                        onChange={(e) => patch({ description: e.target.value })}
                      />
                    </FormField>
                  </div>
                  <FormField label={t('agents:editorV2.prompt')} required>
                    <Textarea
                      value={draft.user_prompt}
                      onChange={(e) => patch({ user_prompt: e.target.value })}
                      rows={5}
                      className="min-h-[7.5rem] resize-y"
                    />
                  </FormField>
                </EditorSection>

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
              </FormSurface>

              {evidenceDetail !== undefined && (
                <AgentExecutionEvidencePanel detail={evidenceDetail} />
              )}

              <div className="lg:hidden">
                <EffectivePlanPreview draft={draft} />
              </div>
            </div>

            <div className="hidden w-72 shrink-0 lg:block xl:w-80">
              <EffectivePlanPreview
                draft={draft}
                className="lg:sticky lg:top-4"
              />
            </div>
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>
      <FullScreenDialogFooter>
        <div className="flex w-full items-center justify-between gap-3">
          <p className="hidden text-xs text-muted-foreground sm:block">
            {t('agents:editorV2.footerHint')}
          </p>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
              {t('common:cancel')}
            </Button>
            <Button
              size="lg"
              className="min-w-[7.5rem] font-semibold"
              onClick={() => void handleSave()}
              disabled={saving || !canSave}
            >
              {saving ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('agents:common.saving')}
                </>
              ) : (
                t('agents:editorV2.save')
              )}
            </Button>
          </div>
        </div>
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}
