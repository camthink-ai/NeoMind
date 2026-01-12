import { useState, useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
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

// Backend type configurations
const BACKEND_TYPES: Record<LlmBackendType, BackendTypeDefinition> = {
  ollama: {
    id: 'ollama',
    name: 'Ollama',
    description: '本地开源模型运行时',
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
    description: 'OpenAI GPT 模型',
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
    description: 'Anthropic Claude 模型',
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
    description: 'Google Gemini 模型',
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
    description: 'xAI Grok 模型',
    default_model: 'grok-beta',
    default_endpoint: 'https://api.x.ai/v1',
    requires_api_key: true,
    supports_streaming: true,
    supports_thinking: false,
    supports_multimodal: false,
  },
}

const schema = z.object({
  name: z.string().min(2, '名称至少2个字符'),
  backend_type: z.enum(['ollama', 'openai', 'anthropic', 'google', 'xai']),
  endpoint: z.string().optional(),
  model: z.string().min(1, '请输入模型名称'),
  api_key: z.string().optional(),
  temperature: z.number().min(0).max(2).optional(),
  top_p: z.number().min(0).max(1).optional(),
  max_tokens: z.number().min(1).optional(),
})

type FormValues = z.infer<typeof schema>

interface LlmBackendDialogProps {
  open: boolean
  onClose: () => void
  onSubmit: (data: CreateLlmBackendRequest | UpdateLlmBackendRequest) => Promise<boolean>
  editing?: LlmBackendInstance | null
}

export function LlmBackendDialog({ open, onClose, onSubmit, editing }: LlmBackendDialogProps) {
  const [submitting, setSubmitting] = useState(false)
  const [selectedType, setSelectedType] = useState<LlmBackendType>(editing?.backend_type || 'ollama')

  const {
    register,
    handleSubmit,
    formState: { errors },
    setValue,
    reset,
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
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
      max_tokens: 2048,
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
        max_tokens: 2048,
      })
      setSelectedType('ollama')
    }
  }, [editing, reset])

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
          <DialogTitle>{editing ? '编辑 LLM 后端' : '添加 LLM 后端'}</DialogTitle>
          <DialogDescription>
            {editing ? '修改现有 LLM 后端配置。' : '添加新的 LLM 后端实例。'}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit(handleFormSubmit)} className="space-y-4">
          {/* Name */}
          <div className="space-y-2">
            <Label htmlFor="name">显示名称 *</Label>
            <Input
              id="name"
              placeholder="例如: 本地 Ollama、OpenAI 主账号"
              {...register('name')}
            />
            {errors.name && (
              <p className="text-sm text-destructive">{errors.name.message}</p>
            )}
          </div>

          {/* Backend Type */}
          <div className="space-y-2">
            <Label htmlFor="backend_type">后端类型 *</Label>
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
            <Label htmlFor="endpoint">API 端点</Label>
            <Input
              id="endpoint"
              placeholder={backendConfig.default_endpoint}
              {...register('endpoint')}
            />
            <p className="text-xs text-muted-foreground">
              默认: {backendConfig.default_endpoint}
            </p>
          </div>

          {/* Model */}
          <div className="space-y-2">
            <Label htmlFor="model">模型名称 *</Label>
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
              <Label htmlFor="api_key">API 密钥</Label>
              <Input
                id="api_key"
                type="password"
                placeholder={editing ? '保持现有密钥不变' : '输入 API 密钥'}
                {...register('api_key')}
              />
              {!editing && (
                <p className="text-xs text-muted-foreground">
                  留空可在创建后配置，或使用环境变量
                </p>
              )}
            </div>
          )}

          {/* Advanced Settings */}
          <div className="space-y-3 pt-2 border-t">
            <p className="text-sm font-medium">高级设置</p>

            <div className="grid grid-cols-3 gap-4">
              <div className="space-y-1">
                <Label htmlFor="temperature" className="text-xs">Temperature</Label>
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
                <Label htmlFor="top_p" className="text-xs">Top-P</Label>
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
                <Label htmlFor="max_tokens" className="text-xs">Max Tokens</Label>
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
              取消
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {editing ? '保存' : '创建'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
