/**
 * AgentSpec-v2 editor — task-template create path + simple primary form.
 * Advanced engineer knobs stay collapsed behind “高级”.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Loader2 } from 'lucide-react'
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
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible'
import { AgentExecutionEvidencePanel } from '@/components/agents/AgentExecutionEvidencePanel'
import { cn } from '@/lib/utils'
import type {
  AgentSpecDraft,
  AiAgentDetail,
  AgentExecutionDetail,
  NotifyPolicy,
} from '@/types'
import { emptyDraft, detailToV2Draft } from './defaults'
import { EditorSection, FormSurface } from './EditorSection'
import { TriggerSection } from './TriggerSection'
import { InputContractSection } from './InputContractSection'
import { ToolPolicySection } from './ToolPolicySection'
import { NotifyPolicySection, normalizeNotify } from './NotifyPolicySection'
import { SimpleWhenSection } from './SimpleWhenSection'
import { SimpleSourcesSection } from './SimpleSourcesSection'
import { SimpleNotifySection } from './SimpleNotifySection'
import { PlanConfirmLine } from './PlanConfirmLine'
import {
  TASK_TEMPLATES,
  inferTemplateId,
  type TaskTemplateId,
} from './templates'

export interface AgentEditorV2Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Legacy detail when editing; create waits for a template pick. */
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
  const [draft, setDraft] = useState<AgentSpecDraft>(() => emptyDraft())
  const [templateId, setTemplateId] = useState<TaskTemplateId | null>(null)
  const [saving, setSaving] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)

  useEffect(() => {
    if (!open) return
    if (agent) {
      const next = detailToV2Draft(agent)
      setDraft(next)
      setTemplateId(inferTemplateId(next.trigger, next.preset))
      setAdvancedOpen(false)
    } else {
      setDraft(emptyDraft())
      setTemplateId(null)
      setAdvancedOpen(false)
    }
  }, [open, agent])

  const patch = (partial: Partial<AgentSpecDraft>) => {
    setDraft((prev) => ({ ...prev, ...partial }))
  }

  const applyTemplate = (id: TaskTemplateId) => {
    const tmpl = TASK_TEMPLATES.find((x) => x.id === id)
    if (!tmpl) return
    const seed = tmpl.seed()
    setDraft((prev) => ({
      ...seed,
      name: prev.name.trim() ? prev.name : seed.name,
      user_prompt: prev.user_prompt.trim() ? prev.user_prompt : seed.user_prompt,
      description: prev.description?.trim() ? prev.description : seed.description,
      input: prev.input.requirements.length ? prev.input : seed.input,
    }))
    setTemplateId(id)
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
  const isCreate = !agent
  const needsTemplate = isCreate && !templateId
  const canSave =
    !needsTemplate &&
    draft.name.trim().length > 0 &&
    draft.user_prompt.trim().length > 0

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        title={agent ? t('agents:editorV2.titleEdit') : t('agents:editorV2.titleCreate')}
        subtitle={t('agents:editorV2.subtitle')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-auto bg-background">
          <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 p-4 md:p-6">
            <section data-section="templates" className="space-y-3">
              <div className="space-y-1">
                <h2 className="text-base font-semibold tracking-tight text-foreground">
                  {t('agents:editorV2.templates.title')}
                </h2>
                <p className="text-sm text-muted-foreground">
                  {t('agents:editorV2.templates.hint')}
                </p>
              </div>
              <div className="grid gap-2 sm:grid-cols-3">
                {TASK_TEMPLATES.map((tmpl) => {
                  const Icon = tmpl.icon
                  const selected = templateId === tmpl.id
                  return (
                    <button
                      key={tmpl.id}
                      type="button"
                      onClick={() => applyTemplate(tmpl.id)}
                      className={cn(
                        'flex flex-col items-start gap-1.5 rounded-lg border px-3 py-3 text-left transition-colors',
                        selected
                          ? 'border-primary bg-muted'
                          : 'border-border hover:border-muted-foreground hover:bg-muted-20',
                      )}
                    >
                      <Icon className="h-4 w-4 text-muted-foreground" />
                      <span className="text-sm font-medium text-foreground">
                        {t(`agents:${tmpl.nameKey}`)}
                      </span>
                      <span className="line-clamp-2 text-xs text-muted-foreground">
                        {t(`agents:${tmpl.descKey}`)}
                      </span>
                    </button>
                  )
                })}
              </div>
            </section>

            {!needsTemplate && (
              <>
                <FormSurface>
                  <EditorSection
                    first
                    title={t('agents:editorV2.basicsTitle')}
                    description={t('agents:editorV2.basicsHint')}
                    data-section="basics"
                  >
                    <FormField label={t('agents:editorV2.name')} required>
                      <Input
                        value={draft.name}
                        onChange={(e) => patch({ name: e.target.value })}
                        placeholder={t('agents:editorV2.namePlaceholder')}
                        autoFocus={isCreate}
                      />
                    </FormField>
                    <FormField label={t('agents:editorV2.prompt')} required>
                      <Textarea
                        value={draft.user_prompt}
                        onChange={(e) => patch({ user_prompt: e.target.value })}
                        placeholder={t('agents:editorV2.promptPlaceholder')}
                        rows={4}
                        className="min-h-[6.5rem] resize-y"
                      />
                    </FormField>
                  </EditorSection>

                  <EditorSection
                    title={t('agents:editorV2.when.title')}
                    description={t('agents:editorV2.when.hint')}
                    data-section="when"
                  >
                    <SimpleWhenSection
                      value={draft.trigger}
                      onChange={(trigger) => patch({ trigger })}
                    />
                  </EditorSection>

                  <EditorSection
                    title={t('agents:editorV2.sources.title')}
                    description={t('agents:editorV2.sources.hint')}
                    data-section="sources"
                  >
                    <SimpleSourcesSection
                      value={draft.input}
                      onChange={(input) => patch({ input })}
                    />
                  </EditorSection>

                  <EditorSection
                    title={t('agents:editorV2.notifySimple.title')}
                    description={t('agents:editorV2.notifySimple.hint')}
                    data-section="notify"
                  >
                    <SimpleNotifySection
                      value={notify}
                      onChange={(n) => patch({ notify: n })}
                    />
                  </EditorSection>
                </FormSurface>

                <PlanConfirmLine draft={draft} />

                <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
                  <CollapsibleTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      className="w-full justify-between px-3 text-muted-foreground"
                    >
                      <span className="text-sm font-medium">
                        {t('agents:editorV2.advanced.title')}
                      </span>
                      <ChevronDown
                        className={cn(
                          'h-4 w-4 transition-transform',
                          advancedOpen && 'rotate-180',
                        )}
                      />
                    </Button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="pt-2">
                    <FormSurface>
                      <TriggerSection
                        first
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
                  </CollapsibleContent>
                </Collapsible>

                {evidenceDetail !== undefined && (
                  <AgentExecutionEvidencePanel detail={evidenceDetail} />
                )}
              </>
            )}

            {needsTemplate && (
              <p className="text-sm text-muted-foreground" data-testid="pick-template-hint">
                {t('agents:editorV2.templates.pickFirst')}
              </p>
            )}
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
