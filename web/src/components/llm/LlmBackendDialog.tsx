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
import { Loader2, Brain, Wrench } from 'lucide-react'
import type { LlmBackendInstance, LlmBackendType, CreateLlmBackendRequest, UpdateLlmBackendRequest, BackendTypeDefinition } from '@/types'

// Ollama model with capabilities from API
interface OllamaModel {
  name: string
  size?: number
  family: string
  parameter_size: string
  capabilities: {
    supports_thinking: boolean
    supports_tools: boolean
    supports_multimodal: boolean
  }
}

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
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])
  const [loadingModels, setLoadingModels] = useState(false)
  const [selectedModel, setSelectedModel] = useState<string>(editing?.model || '')

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

  // Fetch Ollama models when dialog opens and type is Ollama
  useEffect(() => {
    if (open && selectedType === 'ollama' && ollamaModels.length === 0) {
      fetchOllamaModels()
    }
  }, [open, selectedType])

  // Fetch Ollama models from API
  const fetchOllamaModels = async () => {
    setLoadingModels(true)
    try {
      const endpoint = editing?.endpoint || BACKEND_TYPES.ollama.default_endpoint || 'http://localhost:11434'
      const response = await fetch(`/api/settings/llm/models?endpoint=${encodeURIComponent(endpoint)}`)
      if (response.ok) {
        const data = await response.json()
        if (data.models) {
          setOllamaModels(data.models)
        }
      }
    } catch {
      console.error('Failed to fetch Ollama models')
    } finally {
      setLoadingModels(false)
    }
  }

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
      setSelectedModel(editing.model)
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
      setSelectedModel(BACKEND_TYPES.ollama.default_model)
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
      setSelectedModel(config.default_model)
    }

    // Fetch Ollama models if switching to Ollama
    if (type === 'ollama') {
      fetchOllamaModels()
    } else {
      setOllamaModels([])
    }
  }

  const handleModelSelect = (modelName: string) => {
    setSelectedModel(modelName)
    setValue('model', modelName)
  }

  // Get capabilities for selected Ollama model
  const getSelectedModelCapabilities = () => {
    if (selectedType !== 'ollama') return null
    return ollamaModels.find(m => m.name === selectedModel)?.capabilities || null
  }

  const handleFormSubmit = async (data: FormValues) => {
    setSubmitting(true)
    try {
      // Add capabilities for Ollama backends
      const submitData: CreateLlmBackendRequest | UpdateLlmBackendRequest = { ...data }
      if (selectedType === 'ollama') {
        const capabilities = getSelectedModelCapabilities()
        if (capabilities) {
          ;(submitData as any).capabilities = capabilities
        }
      }
      const success = await onSubmit(editing ? { ...submitData } : submitData as CreateLlmBackendRequest)
      if (success) {
        reset()
        setOllamaModels([])
        onClose()
      }
    } finally {
      setSubmitting(false)
    }
  }

  const backendConfig = BACKEND_TYPES[selectedType]
  const modelCapabilities = getSelectedModelCapabilities()

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
              onBlur={() => {
                // Refresh models when endpoint changes for Ollama
                if (selectedType === 'ollama' && !editing) {
                  fetchOllamaModels()
                }
              }}
            />
            <p className="text-xs text-muted-foreground">
              {t('plugins:llm.default')}: {backendConfig.default_endpoint}
            </p>
          </div>

          {/* Model Selection */}
          <div className="space-y-2">
            <Label htmlFor="model">{t('plugins:llm.modelName')} *</Label>
            {selectedType === 'ollama' && ollamaModels.length > 0 ? (
              <>
                <Select
                  value={selectedModel}
                  onValueChange={handleModelSelect}
                >
                  <SelectTrigger id="model">
                    {loadingModels ? (
                      <div className="flex items-center gap-2">
                        <Loader2 className="h-4 w-4 animate-spin" />
                        <span className="text-muted-foreground">{t('common:loading')}</span>
                      </div>
                    ) : (
                      <SelectValue placeholder={t('plugins:llm.selectModel')} />
                    )}
                  </SelectTrigger>
                  <SelectContent>
                    {ollamaModels.map((model) => (
                      <SelectItem key={model.name} value={model.name}>
                        <div className="flex items-center gap-2 flex-1 min-w-0">
                          <span className="text-xs truncate">{model.name}</span>
                          <span className="text-muted-foreground text-xs whitespace-nowrap">{model.parameter_size}</span>
                          <div className="flex items-center gap-1 shrink-0 ml-auto">
                            {model.capabilities.supports_thinking && (
                              <Brain className="h-3 w-3 text-info" />
                            )}
                            {model.capabilities.supports_tools && (
                              <Wrench className="h-3 w-3 text-success" />
                            )}
                          </div>
                        </div>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {modelCapabilities && (
                  <div className="flex items-center gap-2 text-xs">
                    {modelCapabilities.supports_thinking && (
                      <span className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-info/20 text-info border border-info/30">
                        <Brain className="h-3 w-3" />
                        <span>Thinking</span>
                      </span>
                    )}
                    {modelCapabilities.supports_tools && (
                      <span className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-success/20 text-success border border-success/30">
                        <Wrench className="h-3 w-3" />
                        <span>Tools</span>
                      </span>
                    )}
                    {modelCapabilities.supports_multimodal && (
                      <span className="flex items-center gap-1 px-2 py-0.5 rounded-full bg-purple-500/10 text-purple-600 border border-purple-500/20">
                        <span>🖼️</span>
                        <span>Image</span>
                      </span>
                    )}
                  </div>
                )}
              </>
            ) : (
              <>
                <Input
                  id="model"
                  placeholder={backendConfig.default_model}
                  {...register('model')}
                  defaultValue={selectedModel}
                />
                {errors.model && (
                  <p className="text-sm text-destructive">{errors.model.message}</p>
                )}
              </>
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
