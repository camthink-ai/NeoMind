/**
 * AgentSpec-v2 editor — templates-first form (left) + live summary (right).
 * Mirrors the old FullScreen left/right workspace; advanced stays collapsed.
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
import { useIsMobile } from '@/hooks/useMobile'
import { cn } from '@/lib/utils'
import type {
  AgentSpec,
  AgentSpecDraft,
  AgentExecutionDetail,
  NotifyPolicy,
} from '@/types'
import { emptyDraft, specToDraft } from './defaults'
import { EditorSection, FormSurface } from './EditorSection'
import { TriggerSection } from './TriggerSection'
import { InputContractSection } from './InputContractSection'
import { ToolPolicySection } from './ToolPolicySection'
import { NotifyPolicySection, normalizeNotify } from './NotifyPolicySection'
import { SimpleWhenSection } from './SimpleWhenSection'
import { SimpleSourcesSection } from './SimpleSourcesSection'
import { SimpleNotifySection } from './SimpleNotifySection'
import { SimpleModelSection } from './SimpleModelSection'
import { SimpleOutputSection } from './SimpleOutputSection'
import { LiveSummaryPanel } from './LiveSummaryPanel'
import {
  TASK_TEMPLATES,
  inferTemplateId,
  type TaskTemplateId,
} from './templates'

export interface AgentEditorV2Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** V2 AgentSpec from GET `/agents/v2/:id` when editing; create waits for a template pick. */
  agent?: AgentSpec
  /** True while GET /agents/v2/:id is in flight — blocks save and avoids create UX. */
  loading?: boolean
  /** Optional evidence for the preserved evidence panel. */
  evidenceDetail?: AgentExecutionDetail | null
  /** Save the v2 draft directly (POST/PUT `/agents/v2`) — no legacy projection. */
  onSave: (draft: AgentSpecDraft) => Promise<void>
}

export function AgentEditorV2({
  open,
  onOpenChange,
  agent,
  loading = false,
  evidenceDetail,
  onSave,
}: AgentEditorV2Props) {
  const { t } = useTranslation(['agents', 'common'])
  const isMobile = useIsMobile()
  const [draft, setDraft] = useState<AgentSpecDraft>(() => emptyDraft())
  const [templateId, setTemplateId] = useState<TaskTemplateId | null>(null)
  const [saving, setSaving] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)

  useEffect(() => {
    if (!open) return
    if (loading) return
    if (agent) {
      const next = specToDraft(agent)
      setDraft(next)
      setTemplateId(inferTemplateId(next.trigger, next.preset))
      setAdvancedOpen(false)
    } else {
      setDraft(emptyDraft())
      setTemplateId(null)
      setAdvancedOpen(false)
    }
  }, [open, agent, loading])

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
      llm_backend_id: prev.llm_backend_id ?? seed.llm_backend_id,
    }))
    setTemplateId(id)
  }

  const handleSave = async () => {
    if (loading) return
    setSaving(true)
    try {
      await onSave(draft)
      onOpenChange(false)
    } finally {
      setSaving(false)
    }
  }

  const notify: NotifyPolicy = normalizeNotify(draft.notify)
  // While GET is in flight, agent is cleared — do not flip into create UX.
  const isCreate = !loading && !agent
  const needsTemplate = isCreate && !templateId
  const canSave =
    !loading &&
    !needsTemplate &&
    draft.name.trim().length > 0 &&
    draft.user_prompt.trim().length > 0

  const formColumn = loading ? (
    <div
      className="flex min-h-[12rem] items-center justify-center"
      data-testid="agent-editor-v2-loading"
    >
      <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
    </div>
  ) : (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5 lg:mx-0 lg:max-w-none">
      <section data-section="templates" className="space-y-3">
        <h2 className="text-base font-semibold tracking-tight text-foreground">
          {t('agents:editorV2.templates.title')}
        </h2>
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
          {TASK_TEMPLATES.map((tmpl) => {
            const Icon = tmpl.icon
            const selected = templateId === tmpl.id
            return (
              <button
                key={tmpl.id}
                type="button"
                onClick={() => applyTemplate(tmpl.id)}
                className={cn(
                  'flex items-center gap-2 rounded-lg border px-3 py-2.5 text-left transition-colors',
                  selected
                    ? 'border-primary bg-muted'
                    : 'border-border hover:border-muted-foreground hover:bg-muted-20',
                )}
              >
                <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="text-sm font-medium text-foreground">
                  {t(`agents:${tmpl.nameKey}`)}
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
              <SimpleModelSection
                value={draft.llm_backend_id}
                onChange={(llm_backend_id) => patch({ llm_backend_id })}
              />
            </EditorSection>

            <EditorSection title={t('agents:editorV2.when.title')} data-section="when">
              <SimpleWhenSection
                value={draft.trigger}
                onChange={(trigger) => patch({ trigger })}
              />
            </EditorSection>

            <EditorSection title={t('agents:editorV2.sources.title')} data-section="sources">
              <SimpleSourcesSection
                value={draft.input}
                onChange={(input) => patch({ input })}
              />
            </EditorSection>

            <EditorSection title={t('agents:editorV2.output.title')} data-section="output">
              <SimpleOutputSection
                value={draft.output}
                onChange={(output) => patch({ output })}
              />
            </EditorSection>

            <EditorSection
              title={t('agents:editorV2.notifySimple.title')}
              data-section="notify"
            >
              <SimpleNotifySection
                value={notify}
                onChange={(n) => patch({ notify: n })}
              />
            </EditorSection>
          </FormSurface>

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

    </div>
  )

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        title={
          loading || agent
            ? t('agents:editorV2.titleEdit')
            : t('agents:editorV2.titleCreate')
        }
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        {isMobile ? (
          <FullScreenDialogMain className="overflow-auto bg-background p-4">
            {formColumn}
            {!loading && !needsTemplate && (
              <div className="mt-5">
                <LiveSummaryPanel
                  draft={draft}
                  templateId={templateId}
                  className="rounded-xl border border-border"
                />
              </div>
            )}
          </FullScreenDialogMain>
        ) : (
          <>
            {/* Left: edit form (primary) */}
            <FullScreenDialogMain className="overflow-auto bg-background p-5 md:p-6">
              {formColumn}
            </FullScreenDialogMain>
            {/* Right: live summary rail — border-l so it sits after the form */}
            {!loading && (
              <aside className="hidden w-[300px] shrink-0 border-l border-border lg:flex lg:w-[340px] xl:w-[360px]">
                <LiveSummaryPanel
                  draft={draft}
                  templateId={templateId}
                  className="w-full"
                />
              </aside>
            )}
          </>
        )}
      </FullScreenDialogContent>
      <FullScreenDialogFooter>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            {t('common:cancel')}
          </Button>
          <Button
            size="lg"
            className="min-w-[7.5rem] font-semibold"
            data-testid="agent-editor-v2-save"
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
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}
