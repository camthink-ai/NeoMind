/**
 * Section 2 of the unified AgentEditor flow: Actions & Output.
 *
 * Covers:
 *   - ToolPolicy (disabled / scoped with tool chips + command allowlist / unrestricted)
 *   - ControlledActions (name, destination picker from device commands, idempotent, depends_on)
 *   - OutputContract (enabled toggle, publish switch, fields with name + description + depends_on)
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronDown, Link2, Plus, Trash2, Zap } from 'lucide-react'
import { Button, IconButton } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import type {
  AgentToolCatalogItem,
  ControlledAction,
  Device,
  DeviceType,
  OutputContract,
  OutputFieldV2,
  ToolPolicy,
} from '@/types'
import { normalizeToolPolicy } from './defaults'

function DependsOnToggleButton({
  open,
  count,
  onClick,
}: {
  open: boolean
  count: number
  onClick: () => void
}) {
  const { t } = useTranslation('agents')
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={open}
      aria-label={t('editorV2.notifyUnified.advancedDependsOn')}
      title={t('editorV2.notifyUnified.dependsOnHint')}
      className={cn(
        'inline-flex h-8 shrink-0 items-center gap-1 rounded-md border px-2 text-nano transition-colors',
        open || count > 0
          ? 'border-primary/30 bg-primary-light text-primary font-medium'
          : 'border-transparent text-muted-foreground hover:bg-muted-30 hover:text-foreground',
      )}
    >
      <Link2 className="h-3.5 w-3.5" />
      {count > 0 && <span className="font-mono">{count}</span>}
      <ChevronDown className={cn('h-3 w-3 transition-transform', open && 'rotate-180')} />
    </button>
  )
}

function DependsOnDrawer({
  selected,
  inputKeys,
  onToggle,
}: {
  selected: string[]
  inputKeys: string[]
  onToggle: (key: string) => void
}) {
  const { t } = useTranslation('agents')
  return (
    <div className="flex flex-wrap items-center gap-1.5 rounded-md bg-muted-20 px-2.5 py-2">
      <span className="text-nano text-muted-foreground mr-0.5">
        {t('editorV2.notifyUnified.advancedDependsOn')}:
      </span>
      {inputKeys.map((k) => {
        const on = selected.includes(k)
        return (
          <button
            key={k}
            type="button"
            onClick={() => onToggle(k)}
            className={cn(
              'inline-flex h-6 items-center gap-1 rounded border px-1.5 font-mono text-nano transition-colors',
              on
                ? 'border-primary bg-primary-light text-primary font-medium'
                : 'border-border bg-card text-muted-foreground hover:text-foreground',
            )}
          >
            {on && <Check className="h-2.5 w-2.5" />}
            {k}
          </button>
        )
      })}
    </div>
  )
}

export interface ActionsOutputSectionProps {
  toolPolicy: ToolPolicy
  onToolPolicyChange: (next: ToolPolicy) => void
  controlledActions: ControlledAction[]
  onControlledActionsChange: (next: ControlledAction[]) => void
  output: OutputContract | undefined
  onOutputChange: (next: OutputContract) => void
  inputKeys: string[]
  availableTools: AgentToolCatalogItem[]
  devices: Device[]
  deviceTypes: DeviceType[]
}

export function ActionsOutputSection({
  toolPolicy,
  onToolPolicyChange,
  controlledActions,
  onControlledActionsChange,
  output,
  onOutputChange,
  inputKeys,
  availableTools,
  devices,
  deviceTypes,
}: ActionsOutputSectionProps) {
  const { t } = useTranslation('agents')
  const safePolicy = normalizeToolPolicy(toolPolicy)
  const [openActionDeps, setOpenActionDeps] = useState<Record<number, boolean>>({})
  const [openFieldDeps, setOpenFieldDeps] = useState<Record<number, boolean>>({})
  const policyMode: 'disabled' | 'scoped' | 'unrestricted' =
    safePolicy === 'disabled'
      ? 'disabled'
      : safePolicy === 'unrestricted'
        ? 'unrestricted'
        : 'scoped'

  const scopedTools =
    typeof safePolicy === 'object' && 'scoped' in safePolicy
      ? safePolicy.scoped.allowed_tools ?? []
      : []
  const scopedCommands =
    typeof safePolicy === 'object' && 'scoped' in safePolicy
      ? safePolicy.scoped.allowed_commands ?? []
      : []

  // Flattened device commands for controlled actions + scoped command allowlist
  const deviceCommandOptions = useMemo(() => {
    const dtMap = new Map(deviceTypes.map((dt) => [dt.device_type, dt]))
    const out: Array<{ destination: string; label: string; commandName: string }> = []
    for (const d of devices) {
      const id = d.device_id || d.id
      const dt = dtMap.get(d.device_type)
      for (const cmd of dt?.commands ?? []) {
        out.push({
          destination: `device:${id}:${cmd.name}`,
          label: `${d.name || id} · ${cmd.display_name || cmd.name}`,
          commandName: cmd.display_name || cmd.name,
        })
      }
    }
    return out
  }, [devices, deviceTypes])

  const setPolicyMode = (mode: 'disabled' | 'scoped' | 'unrestricted') => {
    if (mode === 'disabled') onToolPolicyChange('disabled')
    else if (mode === 'unrestricted') onToolPolicyChange('unrestricted')
    else {
      onToolPolicyChange({
        scoped: {
          allowed_tools: scopedTools.length > 0 ? scopedTools : ['shell'],
          allowed_commands: scopedCommands,
        },
      })
    }
  }

  const toggleAllowedTool = (toolName: string) => {
    const exists = scopedTools.includes(toolName)
    const next = exists
      ? scopedTools.filter((x) => x !== toolName)
      : [...scopedTools, toolName]
    onToolPolicyChange({
      scoped: { allowed_tools: next, allowed_commands: scopedCommands },
    })
  }

  // Controlled actions helpers
  const addControlledAction = () => {
    onControlledActionsChange([
      ...controlledActions,
      { name: '', destination: '', idempotent: true, depends_on: [] },
    ])
  }

  const updateAction = (index: number, patch: Partial<ControlledAction>) => {
    onControlledActionsChange(
      controlledActions.map((a, i) => (i === index ? { ...a, ...patch } : a)),
    )
  }

  const removeAction = (index: number) => {
    onControlledActionsChange(controlledActions.filter((_, i) => i !== index))
  }

  const toggleActionDep = (index: number, key: string) => {
    const current = controlledActions[index]?.depends_on ?? []
    const next = current.includes(key)
      ? current.filter((k) => k !== key)
      : [...current, key]
    updateAction(index, { depends_on: next })
  }

  // Output contract helpers
  const outputContract: OutputContract = {
    fields: [...(output?.fields ?? [])],
    publish: Boolean(output?.publish),
  }
  const outputEnabled = outputContract.fields.length > 0

  const setOutputEnabled = (on: boolean) => {
    if (on) {
      onOutputChange({
        fields: outputContract.fields.length
          ? outputContract.fields
          : [{ name: 'result', description: '', depends_on: [] }],
        publish: outputContract.publish,
      })
    } else {
      onOutputChange({ fields: [], publish: false })
    }
  }

  const updateOutputField = (index: number, patch: Partial<OutputFieldV2>) => {
    const next = outputContract.fields.map((f, i) => (i === index ? { ...f, ...patch } : f))
    onOutputChange({ ...outputContract, fields: next })
  }

  const toggleFieldDep = (index: number, key: string) => {
    const current = outputContract.fields[index]?.depends_on ?? []
    const next = current.includes(key)
      ? current.filter((k) => k !== key)
      : [...current, key]
    updateOutputField(index, { depends_on: next })
  }

  return (
    <div className="space-y-6">
      {/* 1. Tool Policy */}
      <div className="space-y-2.5" data-section="tool-policy">
        <Label className="text-sm font-medium">{t('editorV2.toolPolicy.title')}</Label>
        <div className="grid grid-cols-3 gap-2">
          {([
            { id: 'disabled', label: t('editorV2.toolPolicy.disabled') },
            { id: 'scoped', label: t('editorV2.toolPolicy.scoped') },
            { id: 'unrestricted', label: t('editorV2.toolPolicy.unrestricted') },
          ] as const).map(({ id, label }) => (
            <button
              key={id}
              type="button"
              onClick={() => setPolicyMode(id)}
              className={cn(
                'rounded-lg border px-3 py-2 text-xs font-medium transition-colors text-center',
                policyMode === id
                  ? 'border-primary bg-primary-light text-primary'
                  : 'border-border bg-card text-muted-foreground hover:text-foreground',
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {policyMode === 'disabled' && (
          <p className="text-xs text-muted-foreground">{t('editorV2.toolPolicy.disabledHint')}</p>
        )}

        {policyMode === 'scoped' && (
          <div className="space-y-2 rounded-lg border border-border bg-muted-20 p-3">
            {availableTools.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {availableTools
                  .filter((tool) => !tool.disabled)
                  .map((tool) => {
                    const on = scopedTools.includes(tool.name)
                    return (
                      <button
                        key={tool.name}
                        type="button"
                        onClick={() => toggleAllowedTool(tool.name)}
                        className={cn(
                          'inline-flex items-center gap-1 rounded-md border px-2 py-1 font-mono text-xs transition-colors',
                          on
                            ? 'border-primary bg-primary-light text-primary font-medium'
                            : 'border-border bg-card text-muted-foreground hover:text-foreground',
                        )}
                      >
                        {on && <Check className="h-3 w-3" />}
                        {tool.name}
                      </button>
                    )
                  })}
              </div>
            )}
            <Input
              value={scopedTools.join(', ')}
              onChange={(e) => {
                const allowed_tools = e.target.value
                  .split(',')
                  .map((s) => s.trim())
                  .filter(Boolean)
                onToolPolicyChange({
                  scoped: { allowed_tools, allowed_commands: scopedCommands },
                })
              }}
              placeholder={t('editorV2.toolPolicy.allowedToolsHint')}
              className="h-8 font-mono text-xs"
            />
          </div>
        )}
      </div>

      {/* 2. Controlled Actions */}
      <div className="space-y-2.5" data-section="controlled-actions">
        <div className="flex items-center justify-between gap-2">
          <Label className="text-sm font-medium">{t('editorV2.actions.title')}</Label>
          <Button type="button" variant="outline" size="sm" onClick={addControlledAction}>
            <Plus className="mr-1.5 h-3.5 w-3.5" />
            {t('editorV2.actions.add')}
          </Button>
        </div>
        {controlledActions.length === 0 ? (
          <p className="text-xs text-muted-foreground">{t('editorV2.actions.empty')}</p>
        ) : (
          <div className="space-y-2.5">
            {controlledActions.map((action, idx) => (
              <div
                key={idx}
                className="space-y-3 rounded-lg border border-border bg-card p-3.5"
              >
                {/* Header row: icon + title + delete */}
                <div className="flex items-center gap-2.5">
                  <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-warning-light text-warning">
                    <Zap className="h-3.5 w-3.5" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-foreground">
                      {action.name.trim() || `${t('editorV2.actions.title')} #${idx + 1}`}
                    </div>
                    {action.destination.trim() && (
                      <div className="truncate font-mono text-nano text-muted-foreground">
                        {action.destination}
                      </div>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    {inputKeys.length > 0 && (
                      <DependsOnToggleButton
                        open={Boolean(openActionDeps[idx])}
                        count={(action.depends_on ?? []).length}
                        onClick={() =>
                          setOpenActionDeps((prev) => ({ ...prev, [idx]: !prev[idx] }))
                        }
                      />
                    )}
                    <IconButton
                      type="button"
                      size="sm"
                      className="shrink-0 text-muted-foreground hover:text-error"
                      onClick={() => removeAction(idx)}
                      aria-label={t('editorV2.actions.remove')}
                    >
                      <Trash2 className="h-4 w-4" />
                    </IconButton>
                  </div>
                </div>

                {inputKeys.length > 0 && openActionDeps[idx] && (
                  <DependsOnDrawer
                    selected={action.depends_on ?? []}
                    inputKeys={inputKeys}
                    onToggle={(k) => toggleActionDep(idx, k)}
                  />
                )}

                {/* Quick device command picker */}
                {deviceCommandOptions.length > 0 && (
                  <Select
                    value={
                      deviceCommandOptions.some((o) => o.destination === action.destination)
                        ? action.destination
                        : ''
                    }
                    onValueChange={(dest) => {
                      const picked = deviceCommandOptions.find((o) => o.destination === dest)
                      updateAction(idx, {
                        destination: dest,
                        name: action.name.trim() ? action.name : picked?.commandName || dest,
                      })
                    }}
                  >
                    <SelectTrigger className="h-9 text-xs">
                      <SelectValue placeholder={t('editorV2.actions.pickCommand')} />
                    </SelectTrigger>
                    <SelectContent>
                      {deviceCommandOptions.map((opt) => (
                        <SelectItem key={opt.destination} value={opt.destination}>
                          {opt.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}

                {/* Name + Destination fields */}
                <div className="grid gap-2.5 sm:grid-cols-2">
                  <div className="space-y-1">
                    <span className="text-xs text-muted-foreground">
                      {t('editorV2.actions.actionName')}
                    </span>
                    <Input
                      value={action.name}
                      onChange={(e) => updateAction(idx, { name: e.target.value })}
                      placeholder={t('editorV2.actions.namePlaceholder')}
                      className="h-9 text-xs"
                    />
                  </div>
                  <div className="space-y-1">
                    <span className="text-xs text-muted-foreground">
                      {t('editorV2.actions.actionDestination')}
                    </span>
                    <Input
                      value={action.destination}
                      onChange={(e) => updateAction(idx, { destination: e.target.value })}
                      placeholder={t('editorV2.actions.destinationPlaceholder')}
                      className="h-9 font-mono text-xs"
                    />
                  </div>
                </div>

                {/* Idempotent toggle */}
                <div className="flex items-center justify-between gap-3 rounded-md bg-muted-20 px-3 py-2">
                  <span className="text-xs text-muted-foreground">
                    {t('editorV2.actions.idempotent')}
                  </span>
                  <Switch
                    checked={Boolean(action.idempotent)}
                    onCheckedChange={(idempotent) => updateAction(idx, { idempotent })}
                  />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 3. Structured Output Contract */}
      <div className="space-y-3" data-section="output">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-muted-20 px-3 py-2.5">
          <Label htmlFor="unified-output-enabled" className="text-sm font-medium">
            {t('editorV2.output.enabled')}
          </Label>
          <Switch
            id="unified-output-enabled"
            checked={outputEnabled}
            onCheckedChange={setOutputEnabled}
          />
        </div>

        {outputEnabled && (
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
              <div className="min-w-0">
                <Label htmlFor="unified-output-publish" className="text-xs font-medium">
                  {t('editorV2.output.publish')}
                </Label>
                <p className="text-xs text-muted-foreground">{t('editorV2.output.publishHint')}</p>
              </div>
              <Switch
                id="unified-output-publish"
                checked={Boolean(outputContract.publish)}
                onCheckedChange={(publish) => onOutputChange({ ...outputContract, publish })}
              />
            </div>

            <div className="space-y-2.5">
              {outputContract.fields.map((field, index) => (
                <div
                  key={index}
                  className="space-y-2 rounded-lg border border-border bg-card p-2.5"
                >
                  <div className="flex items-center gap-1.5">
                    <div className="grid min-w-0 flex-1 gap-2 sm:grid-cols-[11rem_1fr]">
                      <Input
                        value={field.name}
                        onChange={(e) => updateOutputField(index, { name: e.target.value })}
                        placeholder={t('editorV2.output.namePlaceholder')}
                        className="h-9 font-mono text-xs"
                      />
                      <Input
                        value={field.description ?? ''}
                        onChange={(e) => updateOutputField(index, { description: e.target.value })}
                        placeholder={t('editorV2.output.descPlaceholder')}
                        className="h-9 text-xs"
                      />
                    </div>
                    {inputKeys.length > 0 && (
                      <DependsOnToggleButton
                        open={Boolean(openFieldDeps[index])}
                        count={(field.depends_on ?? []).length}
                        onClick={() =>
                          setOpenFieldDeps((prev) => ({ ...prev, [index]: !prev[index] }))
                        }
                      />
                    )}
                    <IconButton
                      type="button"
                      size="sm"
                      className="shrink-0 text-muted-foreground hover:text-error"
                      onClick={() =>
                        onOutputChange({
                          ...outputContract,
                          fields: outputContract.fields.filter((_, i) => i !== index),
                        })
                      }
                      aria-label={t('editorV2.output.remove')}
                    >
                      <Trash2 className="h-4 w-4" />
                    </IconButton>
                  </div>
                  {inputKeys.length > 0 && openFieldDeps[index] && (
                    <DependsOnDrawer
                      selected={field.depends_on ?? []}
                      inputKeys={inputKeys}
                      onToggle={(k) => toggleFieldDep(index, k)}
                    />
                  )}
                </div>
              ))}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  onOutputChange({
                    ...outputContract,
                    fields: [...outputContract.fields, { name: '', description: '', depends_on: [] }],
                  })
                }
              >
                <Plus className="mr-1.5 h-3.5 w-3.5" />
                {t('editorV2.output.add')}
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
