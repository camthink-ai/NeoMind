/**
 * CloudAiAddDialog — the add/edit dialog for the Cloud AI card.
 *
 * One card, two protocol paths: the user picks the protocol (OpenAI-
 * compatible / Anthropic) inside this dialog, then fills the standard
 * fields. `backend_type` is mapped from the selection; capabilities come
 * from the concrete type (registry + runtime probe) as usual.
 *
 * Edit mode: pass `editing`. The protocol select doubles as the type
 * switcher — changing it PUTs a different backend_type, and the server
 * re-bases capabilities (the endpoint/model fields are kept, so switching
 * protocols usually also means editing the endpoint). The API key field
 * prefills with the API_KEY_MASK sentinel when a key is stored; both the
 * untouched mask and an empty value mean "keep the existing key".
 */

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Cloud } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { API_KEY_MASK } from '@/components/plugins/UniversalPluginConfigDialog'
import type { CreateLlmBackendRequest } from '@/types'

/** An existing instance loaded into the dialog for editing. */
export interface CloudAiEditTarget {
  id: string
  name: string
  /** Concrete stored backend_type ('openai' | 'anthropic' | legacy vendor). */
  backend_type: string
  endpoint: string
  model: string
  api_key_configured: boolean
  /** Stored context window, if any (prefill the ctx field on edit). */
  max_context?: number
}

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Receives the payload plus the instance id when editing. */
  onSubmit: (data: CreateLlmBackendRequest, editingId?: string) => Promise<void>
  /** Present → edit mode. */
  editing?: CloudAiEditTarget | null
}

const DEFAULTS: Record<'openai' | 'anthropic', { endpoint: string; model: string }> = {
  // Anthropic may be pasted with or without /v1 — the runtime normalizes
  // (auto-appends /v1 before joining "/messages").
  openai: { endpoint: 'https://api.openai.com/v1', model: 'gpt-4.1-mini' },
  anthropic: { endpoint: 'https://api.anthropic.com/v1', model: 'claude-sonnet-4-5' },
}

/** Map a stored backend_type onto a protocol path. Legacy vendor types
 *  (qwen/deepseek/glm/…) are all OpenAI-compatible endpoints. */
function protocolOf(backendType: string): 'openai' | 'anthropic' {
  return backendType === 'anthropic' ? 'anthropic' : 'openai'
}

export function CloudAiAddDialog({ open, onOpenChange, onSubmit, editing }: Props) {
  const { t } = useTranslation(['plugins'])
  const [protocol, setProtocol] = useState<'openai' | 'anthropic'>('openai')
  const [name, setName] = useState('')
  const [endpoint, setEndpoint] = useState(DEFAULTS.openai.endpoint)
  const [model, setModel] = useState(DEFAULTS.openai.model)
  const [apiKey, setApiKey] = useState('')
  // Optional real context window for custom backends (e.g. RKLLM3 runs -c 16384
  // but openai-typed backends default to 128000 — over-sized prompts hang the
  // runtime). Sent as capabilities.max_context; the server respects it.
  const [ctx, setCtx] = useState<string>('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setError(null)
    if (editing) {
      const p = protocolOf(editing.backend_type)
      if (editing.max_context) setCtx(String(editing.max_context))
      setProtocol(p)
      setName(editing.name)
      setEndpoint(editing.endpoint)
      setModel(editing.model)
      // Never returned by the API — sentinel mask says "configured".
      setApiKey(editing.api_key_configured ? API_KEY_MASK : '')
    } else {
      setProtocol('openai')
      setName('')
      setApiKey('')
      setEndpoint(DEFAULTS.openai.endpoint)
      setModel(DEFAULTS.openai.model)
    }
  }, [open, editing])

  const switchProtocol = (p: 'openai' | 'anthropic') => {
    // Only the protocol changes — never clobber the endpoint/model the
    // user already has (custom gateways may serve both protocol shapes;
    // /v1 is auto-normalized per protocol on the backend).
    setProtocol(p)
    setError(null)
  }

  const keyProvided =
    apiKey.trim() && apiKey !== API_KEY_MASK ? apiKey.trim() : undefined

  const handleSubmit = async () => {
    // Create: cloud protocol paths require an API key (backend validate()
    // enforces it) — catch it client-side. Edit: blank/masked = keep stored.
    if (!name.trim() || !endpoint.trim() || !model.trim() || (!editing && !keyProvided)) {
      setError(t('plugins:llm.cloudFillRequired'))
      return
    }
    setSaving(true)
    try {
      await onSubmit(
        {
          name: name.trim(),
          backend_type: protocol,
          endpoint: endpoint.trim(),
          model: model.trim(),
          ...(keyProvided ? { api_key: keyProvided } : {}),
          temperature: 0.7,
          ...(protocol === 'openai' ? { top_p: 0.9 } : {}),
          ...(ctx.trim() && !Number.isNaN(Number(ctx))
            ? { max_context: Number(ctx) }
            : {}),
        },
        editing?.id
      )
      onOpenChange(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={editing ? t('plugins:llm.cloudEditTitle') : t('plugins:llm.cloudAddTitle')}
      description={t('plugins:llm.cloudAddDesc')}
      icon={<Cloud className="h-5 w-5 text-muted-foreground" />}
      className="z-[110]"
      loading={false}
      isSubmitting={saving}
      onSubmit={handleSubmit}
      submitLabel={editing ? t('common:save', { defaultValue: 'Save' }) : t('common:add', { defaultValue: 'Add' })}
      cancelLabel={t('common:cancel', { defaultValue: 'Cancel' })}
      submitDisabled={!name.trim() || !endpoint.trim() || !model.trim() || (!editing && !keyProvided) || saving}
      submitError={error ?? undefined}
    >
      <div className="space-y-4">
        {/* Protocol path — OpenAI-compatible / Anthropic. In edit mode this
            switches the instance's backend_type. */}
        <FormField label={t('plugins:llm.cloudProtocol')} helpText={t('plugins:llm.cloudProtocolHelp')}>
          <Select value={protocol} onValueChange={(v) => switchProtocol(v as 'openai' | 'anthropic')}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="openai">{t('plugins:llm.protocol.openai.name')}</SelectItem>
              <SelectItem value="anthropic">{t('plugins:llm.protocol.anthropic.name')}</SelectItem>
            </SelectContent>
          </Select>
        </FormField>

        <FormField label={t('plugins:llm.name')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="my-cloud-llm" />
        </FormField>

        <FormField label={t('plugins:llm.cloudCtx')} helpText={t('plugins:llm.cloudCtxHelp')}>
          <input
            value={ctx}
            onChange={(e) => setCtx(e.target.value)}
            placeholder="16384"
            inputMode="numeric"
            className="h-9 w-full rounded-md border border-border bg-background px-3 text-sm text-foreground outline-none focus:border-primary"
          />
        </FormField>
        <FormField label={t('plugins:llm.endpoint')}>
          <Input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://…" />
        </FormField>

        <FormField label={t('plugins:llm.model')}>
          <Input value={model} onChange={(e) => setModel(e.target.value)} />
        </FormField>

        <FormField label={t('plugins:llm.apiKey')} helpText={t('plugins:llm.apiKeyHelp')}>
          <PasswordInput
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={editing && editing.api_key_configured ? t('plugins:llm.apiKeyKeepHint') : undefined}
          />
        </FormField>
      </div>
    </UnifiedFormDialog>
  )
}
