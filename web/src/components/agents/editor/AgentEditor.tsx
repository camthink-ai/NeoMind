/**
 * AgentEditor — unified BuilderShell editor for AgentSpec.
 *
 * Left rail (BuilderShell.config):
 *   - Scenario template quick-picks (with real localized prompts + defaults)
 *   - Agent name (live validation)
 *   - LLM backend picker (with Vision / Tools / Thinking capability pills)
 *   - Live EffectivePlanPreview (POST /api/agents/plan-preview)
 *
 * Right workspace (BuilderShell.workspace) — single 3-step logical flow
 * with zero Simple-vs-Advanced duplication:
 *   1. Mission & Data Sources (Prompt + grouped SourcePickerDialog + freshness/on_missing)
 *   2. Actions & Output (ToolPolicy + ControlledActions + OutputContract with publish)
 *   3. Schedule, Guardrails & Notifications (Trigger + Pacing + three-way Notify & channels)
 *
 * Footer: Cancel · Dry Run (pre-save preview) · Save / Create (with Toast feedback).
 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertCircle, CheckCircle2, Loader2, Play, Sparkles, X } from 'lucide-react'
import { BuilderShell } from '@/components/automation/dialog/BuilderShell'
import { Button, IconButton } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Field, FieldLabel, FieldMessage } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { AgentExecutionEvidencePanel } from '@/components/agents/AgentExecutionEvidencePanel'
import { useToast } from '@/hooks/use-toast'
import { showErrorToast } from '@/lib/error-messages'
import { validateLength, validateRequired } from '@/lib/form-validation'
import { api } from '@/lib/api'
import { channelsApi } from '@/lib/api/channels'
import { cn } from '@/lib/utils'
import type {
  AgentExecutionDetail,
  AgentSpec,
  AgentSpecDraft,
  AgentToolCatalogItem,
  AiAgent,
  AlertChannel,
  Device,
  DeviceType,
  Extension,
  LlmBackendInstance,
  NotifyPolicy,
} from '@/types'
import { emptyDraft, specToDraft } from './defaults'
import { normalizeNotify } from './NotifyPolicySection'
import { EffectivePlanPreview } from './EffectivePlanPreview'
import { ModelPicker } from './ModelPicker'
import { SourcePickerDialog, UnifiedInputsSection } from './SourcePickerDialog'
import { ActionsOutputSection } from './ActionsOutputSection'
import { ScheduleNotifySection } from './ScheduleNotifySection'
import {
  TASK_TEMPLATES,
  inferTemplateId,
  type TaskTemplateId,
} from './templates'

export interface AgentEditorProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** AgentSpec from GET `/agents/:id` when editing; omitted when creating. */
  agent?: AgentSpec
  /** True while GET /agents/:id is in flight — blocks save and avoids create UX. */
  loading?: boolean
  /** Optional evidence for the preserved evidence panel. */
  evidenceDetail?: AgentExecutionDetail | null
  /** Save the draft directly (POST/PUT `/agents`). */
  onSave: (draft: AgentSpecDraft) => Promise<void>
}

interface DryRunPreviewState {
  status: string
  conclusion: string
  structuredResult?: Record<string, unknown> | null
  evidenceCount?: number
  error?: string | null
}

function FlowCard({
  title,
  hint,
  children,
  'data-section': dataSection,
}: {
  title: string
  hint?: string
  children: ReactNode
  'data-section'?: string
}) {
  return (
    <section
      className="overflow-hidden rounded-xl border border-border bg-card"
      data-section={dataSection}
    >
      <header className="border-b border-border bg-muted-20 px-5 py-3.5">
        <h3 className="text-sm font-semibold tracking-tight text-foreground">{title}</h3>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </header>
      <div className="space-y-5 p-5">{children}</div>
    </section>
  )
}

function parseDryRunResponse(resp: unknown): DryRunPreviewState {
  if (!resp || typeof resp !== 'object') {
    return { status: 'Completed', conclusion: '' }
  }
  const obj = resp as Record<string, unknown>
  // RunResponse shape: { execution: { status, conclusion, brief_text, structured_result, error, evidence } }
  const exec =
    obj.execution && typeof obj.execution === 'object'
      ? (obj.execution as Record<string, unknown>)
      : obj
  const status = typeof exec.status === 'string' ? exec.status : 'Completed'
  const conclusion =
    (typeof exec.conclusion === 'string' && exec.conclusion) ||
    (typeof exec.brief_text === 'string' && exec.brief_text) ||
    (typeof obj.raw_text === 'string' && obj.raw_text) ||
    ''
  const structuredResult =
    exec.structured_result && typeof exec.structured_result === 'object'
      ? (exec.structured_result as Record<string, unknown>)
      : obj.fields && typeof obj.fields === 'object'
        ? (obj.fields as Record<string, unknown>)
        : null
  const evidenceItems =
    exec.evidence &&
    typeof exec.evidence === 'object' &&
    Array.isArray((exec.evidence as { items?: unknown[] }).items)
      ? (exec.evidence as { items: unknown[] }).items.length
      : typeof obj.data_sources === 'number'
        ? obj.data_sources
        : undefined
  const error = typeof exec.error === 'string' ? exec.error : null
  return { status, conclusion, structuredResult, evidenceCount: evidenceItems, error }
}

export function AgentEditor({
  open,
  onOpenChange,
  agent,
  loading = false,
  evidenceDetail,
  onSave,
}: AgentEditorProps) {
  const { t } = useTranslation(['agents', 'common'])
  const { toast } = useToast()

  const [draft, setDraft] = useState<AgentSpecDraft>(() => emptyDraft())
  const [templateId, setTemplateId] = useState<TaskTemplateId | null>(null)
  const [saving, setSaving] = useState(false)
  const [fieldErrors, setFieldErrors] = useState<{ name?: string; prompt?: string }>({})
  const [sourcePickerOpen, setSourcePickerOpen] = useState(false)

  // Track whether name/prompt came from a template so switching templates replaces cleanly
  const templateSeededNameRef = useRef<string | null>(null)
  const templateSeededPromptRef = useRef<string | null>(null)

  // Catalog data loaded once per dialog open
  const [backends, setBackends] = useState<LlmBackendInstance[]>([])
  const [devices, setDevices] = useState<Device[]>([])
  const [deviceTypes, setDeviceTypes] = useState<DeviceType[]>([])
  const [extensions, setExtensions] = useState<Extension[]>([])
  const [agentsList, setAgentsList] = useState<AiAgent[]>([])
  const [toolsCatalog, setToolsCatalog] = useState<AgentToolCatalogItem[]>([])
  const [channels, setChannels] = useState<AlertChannel[]>([])

  // Dry-run state
  const [dryRunning, setDryRunning] = useState(false)
  const [dryRunResult, setDryRunResult] = useState<DryRunPreviewState | null>(null)
  const [dryRunError, setDryRunError] = useState<string | null>(null)

  // Sync draft when dialog opens or agent finishes loading
  useEffect(() => {
    if (!open) {
      setSourcePickerOpen(false)
      return
    }
    if (loading) return
    setFieldErrors({})
    setDryRunResult(null)
    setDryRunError(null)
    setSourcePickerOpen(false)
    templateSeededNameRef.current = null
    templateSeededPromptRef.current = null
    if (agent) {
      const next = specToDraft(agent)
      setDraft(next)
      setTemplateId(inferTemplateId(next.trigger, next.preset))
    } else {
      setDraft(emptyDraft())
      setTemplateId(null)
    }
  }, [open, agent, loading])

  // Load reference catalogs when editor opens
  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      const [
        backendsResp,
        devicesResp,
        deviceTypesResp,
        extensionsResp,
        agentsResp,
        toolsResp,
        channelsResp,
      ] = await Promise.all([
        typeof api.listLlmBackends === 'function'
          ? api.listLlmBackends({ include_vlm: true }).catch(() => ({ backends: [] as LlmBackendInstance[] }))
          : Promise.resolve({ backends: [] as LlmBackendInstance[] }),
        typeof api.getDevices === 'function'
          ? api.getDevices().catch(() => ({ devices: [] as Device[], count: 0 }))
          : Promise.resolve({ devices: [] as Device[], count: 0 }),
        typeof api.getDeviceTypes === 'function'
          ? api.getDeviceTypes().catch(() => ({ device_types: [] as DeviceType[], count: 0 }))
          : Promise.resolve({ device_types: [] as DeviceType[], count: 0 }),
        typeof api.listExtensions === 'function'
          ? api.listExtensions().catch(() => [] as Extension[])
          : Promise.resolve([] as Extension[]),
        typeof api.listAgents === 'function'
          ? api.listAgents().catch(() => ({ agents: [] as AiAgent[], count: 0 }))
          : Promise.resolve({ agents: [] as AiAgent[], count: 0 }),
        typeof api.getAgentTools === 'function'
          ? api.getAgentTools().catch(() => ({ tools: [] as AgentToolCatalogItem[], count: 0 }))
          : Promise.resolve({ tools: [] as AgentToolCatalogItem[], count: 0 }),
        typeof channelsApi.listMessageChannels === 'function'
          ? channelsApi.listMessageChannels().catch(() => ({ channels: [] as AlertChannel[] }))
          : Promise.resolve({ channels: [] as AlertChannel[] }),
      ])
      if (cancelled) return
      setBackends(backendsResp.backends ?? [])
      setDevices(devicesResp.devices ?? [])
      setDeviceTypes(deviceTypesResp.device_types ?? [])
      setExtensions(Array.isArray(extensionsResp) ? extensionsResp : [])
      setAgentsList(agentsResp.agents ?? [])
      setToolsCatalog(toolsResp.tools ?? [])
      setChannels(channelsResp.channels ?? [])
    })()
    return () => {
      cancelled = true
    }
  }, [open])

  const patch = (partial: Partial<AgentSpecDraft>) => {
    setDraft((prev) => ({ ...prev, ...partial }))
  }

  const applyTemplate = (id: TaskTemplateId) => {
    const tmpl = TASK_TEMPLATES.find((x) => x.id === id)
    if (!tmpl) return
    const seed = tmpl.seed()
    const localizedName = t(`agents:${tmpl.nameKey}`)
    const localizedPrompt = t(`agents:${tmpl.promptKey}`)

    setDraft((prev) => {
      const prevName = (prev.name ?? '').trim()
      const prevPrompt = (prev.user_prompt ?? '').trim()
      const canReplaceName = !prevName || prevName === templateSeededNameRef.current
      const canReplacePrompt = !prevPrompt || prevPrompt === templateSeededPromptRef.current
      const nextName = canReplaceName ? localizedName : prev.name
      const nextPrompt = canReplacePrompt ? localizedPrompt : prev.user_prompt
      if (canReplaceName) templateSeededNameRef.current = localizedName
      if (canReplacePrompt) templateSeededPromptRef.current = localizedPrompt
      return {
        ...seed,
        name: nextName,
        user_prompt: nextPrompt,
        description: prev.description?.trim() ? prev.description : seed.description,
        input: (prev.input?.requirements?.length ?? 0) > 0 ? prev.input : seed.input,
        llm_backend_id: prev.llm_backend_id ?? seed.llm_backend_id,
      }
    })
    setFieldErrors({})
    setTemplateId(id)
  }

  const validateName = (value: string): string | undefined =>
    validateRequired(value, t('agents:editorV2.name')) ||
    validateLength(value, t('agents:editorV2.name'), 1, 100) ||
    undefined

  const validatePrompt = (value: string): string | undefined =>
    validateRequired(value, t('agents:editorV2.prompt')) ||
    validateLength(value, t('agents:editorV2.prompt'), 1, 5000) ||
    undefined

  const handleDryRun = async () => {
    const promptVal = (draft.user_prompt ?? '').trim()
    if (!promptVal || dryRunning || loading) return
    setDryRunning(true)
    setDryRunError(null)
    setDryRunResult(null)
    try {
      const resp = await api.testAgentPreview({
        user_prompt: promptVal,
        llm_backend_id: draft.llm_backend_id,
        draft,
      })
      setDryRunResult(parseDryRunResponse(resp))
    } catch (error) {
      setDryRunError(error instanceof Error ? error.message : String(error))
    } finally {
      setDryRunning(false)
    }
  }

  const handleSave = async () => {
    if (loading || saving) return
    const nameErr = validateName(draft.name ?? '')
    const promptErr = validatePrompt(draft.user_prompt ?? '')
    if (nameErr || promptErr) {
      setFieldErrors({ name: nameErr, prompt: promptErr })
      return
    }
    setSaving(true)
    try {
      await onSave(draft)
      toast({
        title: t('common:success'),
        description: agent ? t('agents:agentUpdated') : t('agents:agentCreated'),
      })
      onOpenChange(false)
    } catch (error) {
      showErrorToast(toast, error, t('common:failed'))
    } finally {
      setSaving(false)
    }
  }

  const notify: NotifyPolicy = normalizeNotify(draft.notify)
  const isCreate = !loading && !agent
  const canSave =
    !loading &&
    (draft.name ?? '').trim().length > 0 &&
    (draft.user_prompt ?? '').trim().length > 0

  const inputKeys = useMemo(
    () =>
      (draft.input?.requirements ?? [])
        .map((r) => r.key?.trim())
        .filter((k): k is string => Boolean(k)),
    [draft.input],
  )

  // Left Config Rail (identity + model)
  const configRail = (
    <div className="space-y-4">
      <Field>
        <FieldLabel htmlFor="agent-editor-name">
          {t('agents:editorV2.name')} <span className="text-error">*</span>
        </FieldLabel>
        <Input
          id="agent-editor-name"
          value={draft.name}
          onChange={(e) => {
            patch({ name: e.target.value })
            if (fieldErrors.name) setFieldErrors((prev) => ({ ...prev, name: undefined }))
          }}
          onBlur={() => setFieldErrors((prev) => ({ ...prev, name: validateName(draft.name ?? '') }))}
          placeholder={t('agents:editorV2.namePlaceholder')}
          autoFocus={isCreate}
          aria-invalid={!!fieldErrors.name}
        />
        {fieldErrors.name && <FieldMessage>{fieldErrors.name}</FieldMessage>}
      </Field>

      {/* Model picker with capability badges */}
      <ModelPicker
        value={draft.llm_backend_id}
        onChange={(llm_backend_id) => patch({ llm_backend_id })}
        backends={backends}
      />
    </div>
  )

  // Rightmost column: live static EffectivePlan preview
  const planAside = !loading ? (
    <EffectivePlanPreview draft={draft} debounceMs={500} />
  ) : undefined

  // Center Main Workspace — Templates + 3-step logical flow
  const workspace = loading ? (
    <div
      className="flex min-h-[16rem] items-center justify-center"
      data-testid="agent-editor-loading"
    >
      <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
    </div>
  ) : (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-5">
      {/* Scenario Templates inside main workspace */}
      <section className="space-y-2.5" data-section="templates">
        <div>
          <h3 className="text-sm font-semibold tracking-tight text-foreground">
            {t('agents:editorV2.templates.title')}
          </h3>
          {isCreate && (
            <p className="mt-0.5 text-xs text-muted-foreground">
              {t('agents:editorV2.dailyBriefHint')}
            </p>
          )}
        </div>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {TASK_TEMPLATES.map((tmpl) => {
            const Icon = tmpl.icon
            const selected = templateId === tmpl.id
            return (
              <button
                key={tmpl.id}
                type="button"
                onClick={() => applyTemplate(tmpl.id)}
                title={t(`agents:${tmpl.descKey}`)}
                className={cn(
                  'flex items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors',
                  selected
                    ? 'border-primary bg-primary-light text-primary font-medium'
                    : 'border-border bg-card text-foreground hover:border-muted-foreground',
                )}
              >
                <Icon className="h-4 w-4 shrink-0 opacity-75" />
                <span className="truncate text-xs">{t(`agents:${tmpl.nameKey}`)}</span>
              </button>
            )
          })}
        </div>
      </section>
      {/* Inline Dry-Run Preview Banner */}
      {(dryRunResult || dryRunError) && (
        <div
          className={cn(
            'rounded-xl border p-4 space-y-2',
            dryRunError || dryRunResult?.error
              ? 'border-error-light bg-error-light'
              : 'border-primary bg-primary-light',
          )}
          data-testid="agent-dry-run-result"
        >
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              {dryRunError || dryRunResult?.error ? (
                <AlertCircle className="h-4 w-4 text-error" />
              ) : (
                <CheckCircle2 className="h-4 w-4 text-success" />
              )}
              <span className="text-sm font-semibold text-foreground">
                {t('agents:editorV2.dryRun.title')}
              </span>
              {dryRunResult?.status && (
                <Badge variant="outline" className="h-5 px-1.5 text-xs">
                  {dryRunResult.status}
                </Badge>
              )}
              {dryRunResult?.evidenceCount != null && (
                <span className="text-xs text-muted-foreground">
                  {t('agents:editorV2.dryRun.evidenceCount', { count: dryRunResult.evidenceCount })}
                </span>
              )}
            </div>
            <IconButton
              type="button"
              size="sm"
              onClick={() => {
                setDryRunResult(null)
                setDryRunError(null)
              }}
              aria-label={t('agents:editorV2.dryRun.dismiss')}
            >
              <X className="h-4 w-4" />
            </IconButton>
          </div>
          {dryRunError && <p className="text-xs text-error break-words">{dryRunError}</p>}
          {dryRunResult?.error && (
            <p className="text-xs text-error break-words">{dryRunResult.error}</p>
          )}
          {dryRunResult && !dryRunError && (
            <>
              <p className="whitespace-pre-wrap text-xs leading-relaxed text-foreground">
                {dryRunResult.conclusion || t('agents:editorV2.dryRun.noConclusion')}
              </p>
              {dryRunResult.structuredResult &&
                Object.keys(dryRunResult.structuredResult).length > 0 && (
                  <pre className="rounded-md border border-border bg-card p-2.5 font-mono text-xs overflow-auto max-h-36">
                    {JSON.stringify(dryRunResult.structuredResult, null, 2)}
                  </pre>
                )}
            </>
          )}
        </div>
      )}

      {/* Step 1: Mission & Data Sources */}
      <FlowCard
        title={t('agents:editorV2.flow.stepWhat')}
        hint={t('agents:editorV2.flow.stepWhatHint')}
        data-section="basics"
      >
        <Field>
          <FieldLabel htmlFor="agent-editor-prompt">
            {t('agents:editorV2.prompt')} <span className="text-error">*</span>
          </FieldLabel>
          <Textarea
            id="agent-editor-prompt"
            value={draft.user_prompt}
            onChange={(e) => {
              patch({ user_prompt: e.target.value })
              if (fieldErrors.prompt) setFieldErrors((prev) => ({ ...prev, prompt: undefined }))
            }}
            onBlur={() =>
              setFieldErrors((prev) => ({ ...prev, prompt: validatePrompt(draft.user_prompt ?? '') }))
            }
            placeholder={t('agents:editorV2.promptPlaceholder')}
            rows={5}
            aria-invalid={!!fieldErrors.prompt}
            className="min-h-[7.5rem] resize-y text-sm leading-relaxed"
          />
          {fieldErrors.prompt && <FieldMessage>{fieldErrors.prompt}</FieldMessage>}
        </Field>

        <UnifiedInputsSection
          value={draft.input}
          onChange={(input) => patch({ input })}
          devices={devices}
          extensions={extensions}
          agents={agentsList}
          onOpenPicker={() => setSourcePickerOpen(true)}
        />
      </FlowCard>

      {/* Step 2: Actions & Output */}
      <FlowCard
        title={t('agents:editorV2.flow.stepActions')}
        hint={t('agents:editorV2.flow.stepActionsHint')}
        data-section="actions-output"
      >
        <ActionsOutputSection
          toolPolicy={draft.tool_policy}
          onToolPolicyChange={(tool_policy) => patch({ tool_policy })}
          controlledActions={draft.controlled_actions ?? []}
          onControlledActionsChange={(controlled_actions) => patch({ controlled_actions })}
          output={draft.output}
          onOutputChange={(output) => patch({ output })}
          inputKeys={inputKeys}
          availableTools={toolsCatalog}
          devices={devices}
          deviceTypes={deviceTypes}
        />
      </FlowCard>

      {/* Step 3: Schedule, Guardrails & Notifications */}
      <FlowCard
        title={t('agents:editorV2.flow.stepWhen')}
        hint={t('agents:editorV2.flow.stepWhenHint')}
        data-section="schedule-notify"
      >
        <ScheduleNotifySection
          trigger={draft.trigger}
          onTriggerChange={(trigger) => patch({ trigger })}
          pacing={draft.pacing}
          onPacingChange={(pacing) => patch({ pacing })}
          notify={notify}
          onNotifyChange={(n) => patch({ notify: n })}
          devices={devices}
          channels={channels}
          inputKeys={inputKeys}
        />
      </FlowCard>

      {evidenceDetail !== undefined && (
        <AgentExecutionEvidencePanel detail={evidenceDetail} />
      )}
    </div>
  )

  const footer = (
    <div className="flex w-full items-center justify-end gap-2">
      <Button
        type="button"
        variant="outline"
        onClick={() => onOpenChange(false)}
        disabled={saving}
      >
        {t('common:cancel')}
      </Button>
      <Button
        type="button"
        variant="outline"
        data-testid="agent-editor-dry-run"
        onClick={() => void handleDryRun()}
        disabled={loading || saving || dryRunning || !(draft.user_prompt ?? '').trim()}
      >
        {dryRunning ? (
          <>
            <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            {t('agents:creator.structured.dryRunRunning')}
          </>
        ) : (
          <>
            <Play className="mr-1.5 h-3.5 w-3.5" />
            {t('agents:creator.structured.dryRun')}
          </>
        )}
      </Button>
      <Button
        type="button"
        className="min-w-[7rem] font-semibold"
        data-testid="agent-editor-save"
        onClick={() => void handleSave()}
        disabled={saving || !canSave}
      >
        {saving ? (
          <>
            <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            {t('agents:common.saving')}
          </>
        ) : (
          t('agents:editorV2.save')
        )}
      </Button>
    </div>
  )

  return (
    <>
      <BuilderShell
        open={open}
        onOpenChange={(next) => {
          if (!next && sourcePickerOpen) {
            setSourcePickerOpen(false)
            return
          }
          onOpenChange(next)
        }}
        accent="indigo"
        title={
          loading || agent
            ? t('agents:editorV2.titleEdit')
            : t('agents:editorV2.titleCreate')
        }
        icon={<Sparkles className="h-5 w-5" />}
        config={configRail}
        workspace={workspace}
        aside={planAside}
        footer={footer}
        mobileConfigLabel={t('agents:creator.steps.basic')}
      />

      {/* Sibling dialog prevents Radix focus-trap collision with FullScreenDialog */}
      <SourcePickerDialog
        open={sourcePickerOpen}
        onOpenChange={setSourcePickerOpen}
        value={draft.input}
        onChange={(input) => patch({ input })}
        devices={devices}
        deviceTypes={deviceTypes}
        extensions={extensions}
        agents={agentsList}
        excludeAgentId={agent?.id}
      />
    </>
  )
}
