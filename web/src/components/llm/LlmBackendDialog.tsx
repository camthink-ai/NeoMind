import { useState, useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Loader2 } from 'lucide-react'
import type { LlmBackendInstance, LlmBackendType, CreateLlmBackendRequest, UpdateLlmBackendRequest, BackendTypeDefinition } from '@/types'

// Helper function to get backend type definitions with translations
function getBackendTypes(t: (key: string) => string): Record<LlmBackendType, BackendTypeDefinition> {
  return {
    ollama: {
      id: 'ollama',
      name: 'Ollama',
      description: t('plugins:llm.localOpenSource'),
      default_model: 'qwen3-vl:2b',
      default_endpoint: 'http://localhost:11434',
      requires_api_key: false,
      supports_streaming: true,
      supports_thinking: true,
      supports_multimodal: true,
    },
    openai: {
      id: 'openai',
      name: 'OpenAI',
      description: t('plugins:llm.openaiGpt'),
      default_model: 'gpt-4',
      default_endpoint: 'https://api.openai.com/v1',
      requires_api_key: true,
      supports_streaming: true,
      supports_thinking: false,
      supports_multimodal: true,
    },
    anthropic: {
      id: 'anthropic',
      name: 'Anthropic',
      description: t('plugins:llm.anthropicClaude'),
      default_model: 'claude-3-opus-20240229',
      default_endpoint: 'https://api.anthropic.com/v1',
      requires_api_key: true,
      supports_streaming: true,
      supports_thinking: false,
      supports_multimodal: true,
    },
    google: {
      id: 'google',
      name: 'Google',
      description: t('plugins:llm.googleGemini'),
      default_model: 'gemini-pro',
      default_endpoint: 'https://generativelanguage.googleapis.com/v1',
      requires_api_key: true,
      supports_streaming: true,
      supports_thinking: false,
      supports_multimodal: true,
    },
    xai: {
      id: 'xai',
      name: 'xAI',
      description: t('plugins:llm.xaiGrok'),
      default_model: 'grok-beta',
      default_endpoint: 'https://api.x.ai/v1',
      requires_api_key: true,
      supports_streaming: true,
      supports_thinking: false,
      supports_multimodal: false,
    },
  }
}

const getSchema = (t: (key: string) => string) => z.object({
  name: z.string().min(2, t('plugins:llm.nameMinChars')),
  backend_type: z.enum(['ollama', 'openai', 'anthropic', 'google', 'xai']),
  endpoint: z.string().optional(),
  model: z.string().min(1, t('plugins:llm.modelRequired')),
  api_key: z.string().optional(),
  temperature: z.number().min(0).max(2).optional(),
  top_p: z.number().min(0).max(1).optional(),
  max_tokens: z.number().min(1).optional(),
})

type FormValues = z.infer<ReturnType<typeof getSchema>>

interface LlmBackendDialogProps {
  open: boolean
  onClose: () => void
  onSubmit: (data: CreateLlmBackendRequest | UpdateLlmBackendRequest) => Promise<boolean>
  editing?: LlmBackendInstance | null
}

export function LlmBackendDialog({ open, onClose, onSubmit, editing }: LlmBackendDialogProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [submitting, setSubmitting] = useState(false)
  const [selectedType, setSelectedType] = useState<LlmBackendType>(editing?.backend_type || 'ollama')

  const schema = getSchema(t)
  const BACKEND_TYPES = getBackendTypes(t)

  const {
    register,
    handleSubmit,
    formState: { errors },
    setValue,
    reset,
  } = useForm<FormValues>({
    resolver: zodResolver(schema as any),
    defaultValues: editing ? {
      name: editing.name,
      backend_type: editing.backend_type,
      endpoint: editing.endpoint,
      model: editing.model,
      temperature: editing.temperature,
      top_p: editing.top_p,
      max_tokens: editing.max_tokens,
    } : {
      name: '',
      backend_type: 'ollama',
      endpoint: BACKEND_TYPES.ollama.default_endpoint,
      model: BACKEND_TYPES.ollama.default_model,
      temperature: 0.7,
      top_p: 0.9,
      max_tokens: undefined,
    },
  })

  // Update form when editing changes
  useEffect(() => {
    if (editing) {
      reset({
        name: editing.name,
        backend_type: editing.backend_type,
        endpoint: editing.endpoint,
        model: editing.model,
        temperature: editing.temperature,
        top_p: editing.top_p,
        max_tokens: editing.max_tokens,
      })
      setSelectedType(editing.backend_type)
    } else {
      reset({
        name: '',
        backend_type: 'ollama',
        endpoint: BACKEND_TYPES.ollama.default_endpoint,
        model: BACKEND_TYPES.ollama.default_model,
        temperature: 0.7,
        top_p: 0.9,
        max_tokens: undefined,
      })
      setSelectedType('ollama')
    }
  }, [editing, reset, BACKEND_TYPES])

  const handleTypeChange = (type: LlmBackendType) => {
    setSelectedType(type)
    setValue('backend_type', type)

    // Set default values for the selected type
    const config = BACKEND_TYPES[type]
    if (!editing) {
      setValue('endpoint', config.default_endpoint)
      setValue('model', config.default_model)
    }
  }

  const handleFormSubmit = async (data: FormValues) => {
    setSubmitting(true)
    try {
      const success = await onSubmit(editing ? { ...data } : data as CreateLlmBackendRequest)
      if (success) {
        reset()
        onClose()
      }
    } finally {
      setSubmitting(false)
    }
  }

  const backendConfig = BACKEND_TYPES[selectedType]

  return (
    <Dialog open={open} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>{editing ? t('plugins:llm.editBackend') : t('plugins:llm.addBackend')}</DialogTitle>
          <DialogDescription>
            {editing ? t('plugins:llm.editDesc') : t('plugins:llm.addDesc')}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit(handleFormSubmit)} className="space-y-4">
          {/* Name */}
          <div className="space-y-2">
            <Label htmlFor="name">{t('plugins:llm.displayName')} *</Label>
            <Input
              id="name"
              placeholder={t('plugins:llm.displayNamePlaceholder')}
              {...register('name')}
            />
            {errors.name && (
              <p className="text-sm text-destructive">{errors.name.message}</p>
            )}
          </div>

          {/* Backend Type */}
          <div className="space-y-2">
            <Label htmlFor="backend_type">{t('plugins:llm.backendType')} *</Label>
            <Select
              value={selectedType}
              onValueChange={(v) => handleTypeChange(v as LlmBackendType)}
              disabled={!!editing}
            >
              <SelectTrigger id="backend_type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(BACKEND_TYPES).map(([key, config]) => (
                  <SelectItem key={key} value={key}>
                    <div className="flex items-center gap-2">
                      <span>{config.name}</span>
                      <span className="text-muted-foreground text-xs">- {config.description}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Endpoint */}
          <div className="space-y-2">
            <Label htmlFor="endpoint">{t('plugins:llm.apiEndpoint')}</Label>
            <Input
              id="endpoint"
              placeholder={backendConfig.default_endpoint}
              {...register('endpoint')}
            />
            <p className="text-xs text-muted-foreground">
              {t('plugins:llm.default')}: {backendConfig.default_endpoint}
            </p>
          </div>

          {/* Model */}
          <div className="space-y-2">
            <Label htmlFor="model">{t('plugins:llm.modelName')} *</Label>
            <Input
              id="model"
              placeholder={backendConfig.default_model}
              {...register('model')}
            />
            {errors.model && (
              <p className="text-sm text-destructive">{errors.model.message}</p>
            )}
          </div>

          {/* API Key (for cloud providers) */}
          {backendConfig.requires_api_key && (
            <div className="space-y-2">
              <Label htmlFor="api_key">{t('plugins:llm.apiKey')}</Label>
              <Input
                id="api_key"
                type="password"
                placeholder={editing ? t('plugins:llm.apiKeyKeep') : t('plugins:llm.apiKeyPlaceholder')}
                {...register('api_key')}
              />
              {!editing && (
                <p className="text-xs text-muted-foreground">
                  {t('plugins:llm.apiKeyHint')}
                </p>
              )}
            </div>
          )}

          {/* Advanced Settings */}
          <div className="space-y-3 pt-2 border-t">
            <p className="text-sm font-medium">{t('plugins:llm.advancedSettings')}</p>

            <div className="grid grid-cols-3 gap-4">
              <div className="space-y-1">
                <Label htmlFor="temperature" className="text-xs">{t('plugins:llm.temperature')}</Label>
                <Input
                  id="temperature"
                  type="number"
                  step="0.1"
                  min="0"
                  max="2"
                  className="h-8"
                  {...register('temperature', { valueAsNumber: true })}
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="top_p" className="text-xs">{t('plugins:llm.topP')}</Label>
                <Input
                  id="top_p"
                  type="number"
                  step="0.1"
                  min="0"
                  max="1"
                  className="h-8"
                  {...register('top_p', { valueAsNumber: true })}
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="max_tokens" className="text-xs">{t('plugins:llm.maxTokens')}</Label>
                <Input
                  id="max_tokens"
                  type="number"
                  min="1"
                  className="h-8"
                  {...register('max_tokens', { valueAsNumber: true })}
                />
              </div>
            </div>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={submitting}
            >
              {t('plugins:llm.cancel')}
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {editing ? t('plugins:llm.save') : t('plugins:llm.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
