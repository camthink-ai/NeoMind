/**
 * CloudAiAddDialog — the add dialog for the Cloud AI card.
 *
 * One card, two protocol paths: the user picks the protocol (OpenAI-
 * compatible / Anthropic) inside this dialog, then fills the standard
 * fields. `backend_type` is mapped from the selection; capabilities come
 * from the concrete type (registry + runtime probe) as usual.
 */

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Cloud } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { CreateLlmBackendRequest } from '@/types'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSubmit: (data: CreateLlmBackendRequest) => Promise<string>
}

const DEFAULTS: Record<'openai' | 'anthropic', { endpoint: string; model: string }> = {
  // Anthropic MUST include /v1 — the runtime joins base_url + "/messages"
  // verbatim (matches the backend schema default).
  openai: { endpoint: 'https://api.openai.com/v1', model: 'gpt-4.1-mini' },
  anthropic: { endpoint: 'https://api.anthropic.com/v1', model: 'claude-sonnet-4-5' },
}

export function CloudAiAddDialog({ open, onOpenChange, onSubmit }: Props) {
  const { t } = useTranslation(['plugins'])
  const [protocol, setProtocol] = useState<'openai' | 'anthropic'>('openai')
  const [name, setName] = useState('')
  const [endpoint, setEndpoint] = useState(DEFAULTS.openai.endpoint)
  const [model, setModel] = useState(DEFAULTS.openai.model)
  const [apiKey, setApiKey] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Reset to the newly-selected protocol's defaults on dialog open.
  useEffect(() => {
    if (open) {
      setName('')
      setApiKey('')
      setError(null)
      setProtocol('openai')
      setEndpoint(DEFAULTS.openai.endpoint)
      setModel(DEFAULTS.openai.model)
    }
  }, [open])

  const switchProtocol = (p: 'openai' | 'anthropic') => {
    setProtocol(p)
    setEndpoint(DEFAULTS[p].endpoint)
    setModel(DEFAULTS[p].model)
    setError(null)
  }

  const handleSubmit = async () => {
    // Cloud protocol paths require an API key (backend validate() enforces
    // it for openai/anthropic) — catch it client-side for a cleaner error.
    if (!name.trim() || !endpoint.trim() || !model.trim() || !apiKey.trim()) {
      setError(t('plugins:llm.cloudFillRequired'))
      return
    }
    setSaving(true)
    try {
      await onSubmit({
        name: name.trim(),
        backend_type: protocol,
        endpoint: endpoint.trim(),
        model: model.trim(),
        ...(apiKey.trim() ? { api_key: apiKey.trim() } : {}),
        temperature: 0.7,
        ...(protocol === 'openai' ? { top_p: 0.9 } : {}),
      })
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
      title={t('plugins:llm.cloudAddTitle')}
      description={t('plugins:llm.cloudAddDesc')}
      icon={<Cloud className="h-5 w-5 text-muted-foreground" />}
      loading={false}
      isSubmitting={saving}
      onSubmit={handleSubmit}
      submitLabel={t('common:add')}
      cancelLabel={t('common:cancel')}
      submitDisabled={!name.trim() || !endpoint.trim() || !model.trim() || !apiKey.trim() || saving}
      submitError={error ?? undefined}
      className="z-[110]"
    >
      <div className="space-y-4">
        {/* Protocol path — OpenAI-compatible / Anthropic */}
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

        <FormField label={t('plugins:llm.endpoint')}>
          <Input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://…" />
        </FormField>

        <FormField label={t('plugins:llm.model')}>
          <Input value={model} onChange={(e) => setModel(e.target.value)} />
        </FormField>

        <FormField label={t('plugins:llm.apiKey')} helpText={t('plugins:llm.apiKeyHelp')}>
          <PasswordInput value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
        </FormField>
      </div>
    </UnifiedFormDialog>
  )
}
