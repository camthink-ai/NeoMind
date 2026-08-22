import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { confirm } from '@/hooks/use-confirm'
import { toast } from '@/hooks/use-toast'
import {
  Server,
  CheckCircle2,
  Loader2,
  TestTube,
  Edit,
  Trash2,
  Eye,
  Wrench,
  Brain,
  Download,
  Power,
  RotateCcw,
  Zap,
  Cpu,
  BrainCircuit,
  Cloud,
  ChevronRight,
  Settings2,
  AlertTriangle,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Progress } from '@/components/ui/progress'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { FormField } from '@/components/ui/field'
import { EmptyState, LoadingState, ListToolbar } from '@/components/shared'
import { cn } from '@/lib/utils'
import { api, fetchAPI } from '@/lib/api'
import { UniversalPluginConfigDialog, API_KEY_MASK, type PluginInstance, type UnifiedPluginType } from '@/components/plugins/UniversalPluginConfigDialog'
import { BuiltinModelWizard } from '@/components/llm/BuiltinModelWizard'
import { CloudAiAddDialog, type CloudAiEditTarget } from '@/components/llm/CloudAiAddDialog'
import type {
  LlmBackendInstance,
  BackendTypeDefinition,
  BackendTestResult,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
  PluginConfigSchema,
  BackendCapabilities,
  BuiltinLlmStatus,
} from '@/types'

type View = 'list' | 'detail'

interface UnifiedLLMBackendsTabProps {
  onCreateBackend: (data: CreateLlmBackendRequest) => Promise<string>
  onUpdateBackend: (id: string, data: UpdateLlmBackendRequest) => Promise<boolean>
  onDeleteBackend: (id: string) => Promise<boolean>
  onTestBackend: (id: string) => Promise<BackendTestResult>
}

// LLM Provider icon and color config (names are internationalized via getLlmProviderInfo)
const LLM_PROVIDER_CONFIG: Record<string, {
  icon: React.ReactNode
  iconBg: string
}> = {
  ollama: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-success-light text-success dark:bg-success-light dark:text-success',
  },
  openai: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-emerald-light text-accent-emerald',
  },
  anthropic: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-muted text-foreground',
  },
  google: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-info-light text-info',
  },
  xai: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-muted text-foreground',
  },
  qwen: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-indigo-light text-accent-indigo',
  },
  deepseek: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-cyan-light text-accent-cyan',
  },
  glm: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-purple-light text-accent-purple',
  },
  minimax: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-indigo-light text-accent-indigo',
  },
  llamacpp: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-warning-light text-warning',
  },
  cloudai: {
    icon: <Cloud className="h-6 w-6" />,
    iconBg: 'bg-accent-indigo-light text-accent-indigo',
  },
}

/**
 * Get LLM provider info with internationalized name
 */
function getLlmProviderInfo(providerType: string, t: (key: string) => string) {
  const config = LLM_PROVIDER_CONFIG[providerType] || LLM_PROVIDER_CONFIG.ollama
  const i18nKey = `common:llm.providers.${providerType}`

  return {
    name: t(i18nKey),
    icon: config.icon,
    iconBg: config.iconBg,
  }
}

// Protocol-first backend types. The old UI surfaced every vendor as a card
// (openai/qwen/deepseek/glm/xai/minimax/...) — they are all just endpoint +
// key + model. Collapse to four protocol entries; other vendors are reached
// through Cloud AI → OpenAI-compatible (any /v1 endpoint: OpenRouter, Qwen
// dashscope compat, etc.).
const PROTOCOL_TYPE_IDS = ['ollama', 'llamacpp'] as const
// Cloud AI is one card whose protocol paths (OpenAI-compatible / Anthropic)
// are chosen inside it — other vendors ride the OpenAI path via their
// compatible endpoint.
const CLOUD_AI_PROTOCOLS = ['openai', 'anthropic'] as const

// Built-in bundled LLM (LFM2.5-2.6B) card actions.
type BuiltinAction = 'download' | 'restart' | 'activate' | 'delete'

interface BuiltinStatusInfo {
  text: string
  pillClass: string
  percent: number | null
}

/**
 * Map the backend `GET /api/builtin-llm/status` response to display text +
 * design-token pill color. `downloading` renders a percentage when the server
 * reported a total, otherwise an indeterminate "下载中" label.
 */
function getBuiltinStatusInfo(
  status: BuiltinLlmStatus,
  t: (key: string, options?: Record<string, unknown>) => string
): BuiltinStatusInfo {
  const { server_state, downloaded_bytes, total_bytes } = status

  switch (server_state) {
    case 'not_configured':
      return {
        text: t('plugins:llm.builtinNotDownloaded'),
        pillClass: 'bg-muted text-muted-foreground',
        percent: null,
      }
    case 'downloading': {
      const percent =
        total_bytes && total_bytes > 0 && downloaded_bytes != null
          ? Math.min(100, Math.round((downloaded_bytes / total_bytes) * 100))
          : null
      return {
        text:
          percent != null
            ? t('plugins:llm.builtinDownloading', { percent })
            : t('plugins:llm.builtinDownloadingNoProgress'),
        pillClass: 'bg-info-light text-info',
        percent,
      }
    }
    case 'running':
      return {
        text: t('plugins:llm.builtinRunning'),
        pillClass: 'bg-success-light text-success',
        percent: null,
      }
    case 'stopped':
      return {
        text: t('plugins:llm.builtinStopped'),
        pillClass: 'bg-warning-light text-warning',
        percent: null,
      }
    case 'error':
      return {
        text: t('plugins:llm.builtinError'),
        pillClass: 'bg-error-light text-error',
        percent: null,
      }
    default:
      return {
        text: server_state,
        pillClass: 'bg-muted text-muted-foreground',
        percent: null,
      }
  }
}

// Fields to exclude from config schema (managed by the system)
const EXCLUDED_LLM_CONFIG_FIELDS = ['id', 'name', 'backend_type']

/**
 * Convert BackendTypeDefinition to UnifiedPluginType
 */
function toUnifiedPluginType(type: BackendTypeDefinition, t: (key: string) => string): UnifiedPluginType {
  const info = getLlmProviderInfo(type.id, t)

  // Filter out system-managed fields from config schema
  const configSchema = type.config_schema as any
  const filteredSchema: PluginConfigSchema = configSchema
    ? {
        type: 'object' as const,
        properties: Object.fromEntries(
          Object.entries(configSchema.properties || {})
            .filter(([key]) => !EXCLUDED_LLM_CONFIG_FIELDS.includes(key as string))
            .map(([key, prop]) => {
              const typedProp = prop as any
              // Convert x_secret to secret for the form builder
              return [key, {
                ...typedProp,
                secret: typedProp.x_secret || typedProp.secret || false,
              }]
            })
        ) as any,
        required: (configSchema.required || []).filter(
          (field: string) => !EXCLUDED_LLM_CONFIG_FIELDS.includes(field)
        ),
        ui_hints: configSchema.ui_hints || undefined,
      }
    : {
        type: 'object',
        properties: {},
        required: [],
        ui_hints: undefined,
      }

  return {
    id: type.id,
    type: 'llm_backend',
    name: type.name,
    description: type.description,
    icon: info.icon,
    color: info.iconBg,
    config_schema: filteredSchema,
    can_add_multiple: true,
    builtin: false,
    requires_api_key: type.requires_api_key,
    supports_streaming: type.supports_streaming,
    default_model: type.default_model,
    default_endpoint: type.default_endpoint,
  }
}

/**
 * Convert LlmBackendInstance to PluginInstance
 */
function toPluginInstance(instance: LlmBackendInstance, activeId: string | null): PluginInstance {
  return {
    id: instance.id,
    name: instance.name,
    plugin_type: instance.backend_type,
    enabled: true,
    running: instance.id === activeId,
    config: {
      endpoint: instance.endpoint,
      model: instance.model,
      // Note: api_key is not returned by the API for security
      temperature: instance.temperature,
      top_p: instance.top_p,
      top_k: instance.top_k,
      max_tokens: instance.max_tokens,
      // Include capabilities so they can be accessed in the dialog
      capabilities: instance.capabilities,
      // Pass thinking_enabled + thinking_effort so the dialog can read them
      // back in edit mode. Mutated via direct PATCH (not the form Save),
      // mirroring the multimodal override pattern.
      thinking_enabled: instance.thinking_enabled,
      thinking_effort: instance.thinking_effort,
    },
    status: {
      active: instance.id === activeId,
    },
    // Store capabilities at top level for easier access
    capabilities: instance.capabilities,
    // Whether a key is stored on the server (the key itself is never
    // returned) — drives the edit form's "leave blank to keep" hint.
    api_key_configured: instance.api_key_configured,
  }
}

export function UnifiedLLMBackendsTab({
  onCreateBackend,
  onUpdateBackend,
  onDeleteBackend,
  onTestBackend,
}: UnifiedLLMBackendsTabProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const { handleError } = useErrorHandler()
  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)
  const [backendTypes, setBackendTypes] = useState<BackendTypeDefinition[]>([])
  const [instances, setInstances] = useState<LlmBackendInstance[]>([])
  const [activeBackendId, setActiveBackendId] = useState<string | null>(null)
  const [selectedType, setSelectedType] = useState<UnifiedPluginType | null>(null)

  // Config dialog state
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [editingInstance, setEditingInstance] = useState<PluginInstance | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Built-in LLM card state (polled from /api/builtin-llm/status)
  const [builtinStatus, setBuiltinStatus] = useState<BuiltinLlmStatus | null>(null)
  const [builtinBusyAction, setBuiltinBusyAction] = useState<BuiltinAction | null>(null)
  const builtinBusyActionRef = useRef<BuiltinAction | null>(null)

  // First-run wizard (empty-state strong guidance)
  const [wizardOpen, setWizardOpen] = useState(false)
  // The card's 切换模型 button opens the wizard straight at the model
  // picker; first-run CTAs (banner / empty state) use the default entry.
  const [wizardStartInPicker, setWizardStartInPicker] = useState(false)
  // Builtin engine-settings dialog (ctx today; port and friends later).
  const [ctxDialogOpen, setCtxDialogOpen] = useState(false)
  const [ctxDialogInput, setCtxDialogInput] = useState<number | null>(null)
  const [ctxDialogSaving, setCtxDialogSaving] = useState(false)
  const [ctxDialogError, setCtxDialogError] = useState<string | null>(null)
  const [cloudAddOpen, setCloudAddOpen] = useState(false)
  // Non-null → the add dialog is in edit mode for that instance.
  const [cloudEditTarget, setCloudEditTarget] = useState<CloudAiEditTarget | null>(null)

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async (quiet = false) => {
    // quiet = background refresh (e.g. after wizard activation) — skip the
    // page-level skeleton so the wizard stays mounted and can show 已就绪.
    if (!quiet) setLoading(true)
    try {
      const typesResponse = await fetchAPI<{ types: BackendTypeDefinition[] }>('/llm-backends/types')
      setBackendTypes(typesResponse.types || [])

      const instancesResponse = await fetchAPI<{
        backends: LlmBackendInstance[]
        count: number
        active_id: string | null
      }>('/llm-backends')
      setInstances(instancesResponse.backends || [])
      setActiveBackendId(instancesResponse.active_id || null)
    } catch (error) {
      handleError(error, { operation: 'Load LLM data', showToast: false })
      setBackendTypes([])
      setInstances([])
      setActiveBackendId(null)
    } finally {
      if (!quiet) setLoading(false)
    }
  }

  const getInstancesForType = (typeId: string) => {
    // The built-in instance is managed by its own card (top of the list view);
    // exclude it from the provider card's count/detail so it isn't editable
    // through the normal llamacpp instance flow.
    return instances.filter(i => i.backend_type === typeId && !i.is_builtin)
  }

  // Poll the built-in LLM status while the list view is visible (the builtin
  // card lives there). Stops on unmount / detail view. A failed fetch means
  // the server has no /api/builtin-llm/* endpoints → hide the card.
  useEffect(() => {
    if (view !== 'list') return
    let cancelled = false
    const poll = async () => {
      try {
        const data = await api.getBuiltinLlmStatus()
        if (!cancelled) setBuiltinStatus(data)
      } catch {
        if (!cancelled) setBuiltinStatus(null)
      }
    }
    poll()
    const timer = window.setInterval(poll, 3000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [view])

  const refreshBuiltinStatus = async () => {
    try {
      setBuiltinStatus(await api.getBuiltinLlmStatus())
    } catch {
      setBuiltinStatus(null)
    }
  }

  const openTypeDetail = (typeId: string) => {
    const type = backendTypes.find((b) => b.id === typeId)
    if (type) {
      setSelectedType(toUnifiedPluginType(type, t))
      setView('detail')
    }
  }

  const handleBuiltinAction = async (action: BuiltinAction) => {
    if (builtinBusyActionRef.current) return
    if (action === 'delete') {
      const confirmed = await confirm({
        title: t('plugins:llm.builtinDelete', { defaultValue: 'Delete Model' }),
        description: t('plugins:llm.builtinConfirmDelete'),
        confirmText: t('common:delete', { defaultValue: 'Delete' }),
        cancelText: t('common:cancel', { defaultValue: 'Cancel' }),
        variant: 'destructive',
      })
      if (!confirmed) return
    }

    builtinBusyActionRef.current = action
    setBuiltinBusyAction(action)
    try {
      let successMsg: string | null = null
      switch (action) {
        case 'download': {
          const res = await api.downloadBuiltinLlm()
          successMsg = res.started
            ? t('plugins:llm.builtinDownloadStarted')
            : t('plugins:llm.builtinDownloadInProgress')
          break
        }
        case 'restart':
          await api.restartBuiltinLlm()
          successMsg = t('plugins:llm.builtinRestarted')
          break
        case 'activate':
          await api.activateBuiltinLlm()
          successMsg = t('plugins:llm.builtinActivated')
          break
        case 'delete':
          await api.deleteBuiltinLlmModel()
          successMsg = t('plugins:llm.builtinDeleted')
          break
      }
      if (successMsg) toast({ title: successMsg })
      // Refresh the backend list (instance appears/disappears, active flips)
      // and the builtin status after each action.
      await loadData()
      await refreshBuiltinStatus()
    } catch (error) {
      handleError(error, { operation: 'Builtin LLM action' })
    } finally {
      builtinBusyActionRef.current = null
      setBuiltinBusyAction(null)
    }
  }

  // "添加自己的 API 后端" — the empty-state secondary CTA. `backendTypes` is
  // empty (a rare fallback), so there is no server-provided type to pick from.
  // Construct a generic OpenAI-compatible type inline and open the unified
  // config dialog directly (the backend accepts openai + a custom endpoint).
  const handleAddOwnBackend = () => {
    const manualType: BackendTypeDefinition = {
      id: 'openai',
      name: 'OpenAI',
      description: t('plugins:llm.emptyStateAddOwnBackend'),
      default_model: 'gpt-4.1-mini',
      default_endpoint: 'https://api.openai.com/v1',
      requires_api_key: true,
      supports_streaming: true,
      supports_thinking: false,
      supports_multimodal: true,
      config_schema: {
        type: 'object',
        properties: {
          endpoint: {
            type: 'string',
            title: 'API Endpoint',
            format: 'uri',
            default: 'https://api.openai.com/v1',
          },
          model: { type: 'string', title: 'Model Name', default: 'gpt-4.1-mini' },
          api_key: { type: 'string', title: 'API Key', x_secret: true },
        },
        required: ['name', 'api_key'],
        ui_hints: undefined,
      },
    }
    setSelectedType(toUnifiedPluginType(manualType, t))
    setView('detail')
    setEditingInstance(null)
    setConfigDialogOpen(true)
  }

  // Cloud AI create/edit — backend_type derived from the protocol pick
  // (edit mode: also how the type is switched). The tab keeps its own
  // `instances` state (separate from the store slice), so refresh it after
  // the mutation or the card won't update until the settings dialog is
  // reopened.
  const handleCloudAiSubmit = async (data: CreateLlmBackendRequest, editingId?: string) => {
    if (editingId) {
      await onUpdateBackend(editingId, data)
    } else {
      await onCreateBackend(data)
    }
    await loadData(true)
  }

  // Handle create instance
  const handleCreate = async (name: string, config: Record<string, unknown>) => {
    const type = selectedType!
    const data: CreateLlmBackendRequest = {
      name,
      backend_type: type.id as any,
      endpoint: config.endpoint as string || type.default_endpoint,
      model: config.model as string,
      api_key: config.api_key as string,
      temperature: config.temperature as number,
      top_p: config.top_p as number,
      top_k: config.top_k as number || 20,  // Default to 20 for faster responses
      capabilities: config.capabilities as BackendCapabilities | undefined,
    }
    return await onCreateBackend(data)
  }

  // Handle update instance
  const handleUpdate = async (id: string, config: Record<string, unknown>) => {
    // api_key: the API never returns it, so the edit form prefills with the
    // API_KEY_MASK sentinel when a key is stored. Both the untouched mask
    // and an empty value mean "keep the existing key" — omit the field so
    // the backend's `if let Some` skips it.
    const apiKeyProvided =
      typeof config.api_key === 'string' &&
      config.api_key.trim() &&
      config.api_key !== API_KEY_MASK
        ? config.api_key
        : undefined
    const data: UpdateLlmBackendRequest = {
      name: config.name as string,
      endpoint: config.endpoint as string,
      model: config.model as string,
      ...(apiKeyProvided ? { api_key: apiKeyProvided } : {}),
      temperature: config.temperature as number,
      top_p: config.top_p as number,
      top_k: config.top_k as number,
      capabilities: config.capabilities as BackendCapabilities | undefined,
    }
    await onUpdateBackend(id, data)
  }

  // Handle delete instance
  const handleDelete = async (id: string) => {
    // Find instance name for confirmation
    const instance = instances.find(i => i.id === id)
    const instanceName = instance?.name || instance?.model || id

    // Confirm deletion using project's confirm dialog
    const confirmed = await confirm({
      title: t('plugins:llm.deleteBackend', { defaultValue: 'Delete Backend' }),
      description: t('plugins:llm.confirmDelete', { name: instanceName, defaultValue: `Are you sure you want to delete "${instanceName}"? This action cannot be undone.` }),
      confirmText: t('common:delete', { defaultValue: 'Delete' }),
      cancelText: t('common:cancel', { defaultValue: 'Cancel' }),
      variant: 'destructive',
    })
    if (!confirmed) return

    const success = await onDeleteBackend(id)
    if (success) {
      // Update local state immediately
      setInstances(prev => prev.filter(i => i.id !== id))
      // Clear test result
      setTestResults(prev => {
        const next = { ...prev }
        delete next[id]
        return next
      })
    }
  }

  // Handle test connection
  const handleTest = async (id: string): Promise<{ success: boolean; message?: string; error?: string; latency_ms?: number }> => {
    const result = await onTestBackend(id)
    const message = result.success
      ? `${t('plugins:llm.latency')}: ${result.latency_ms?.toFixed(0) || '0'}ms`
      : (result.error || 'Failed')

    setTestResults(prev => ({
      ...prev,
      [id]: { success: result.success, message },
    }))
    return result
  }

  if (loading) {
    return <LoadingState variant="page" text={t('common:loading')} />
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    // Built-in bundled LLM card (top of the list, above the provider grid).
    const builtinInstance = instances.find(i => i.is_builtin)
    const builtinIsActive = !!builtinInstance && builtinInstance.id === activeBackendId
    const BUILTIN_MODEL_NAMES: Record<string, string> = {
      'lfm25-2.6b': 'LFM2.5-2.6B',
      'qwen3.5-4b': 'Qwen3.5-4B',
      'gemma4-e2b': 'Gemma4-E2B',
    }
    const installedModelName = builtinStatus?.model_id
      ? (BUILTIN_MODEL_NAMES[builtinStatus.model_id] ?? builtinStatus.model_id)
      : null
    const builtinInfo = builtinStatus ? getBuiltinStatusInfo(builtinStatus, t) : null
    const builtinCard = builtinStatus && builtinInfo ? (
      <Card
        className={cn(
          'mb-4 transition-all duration-200',
          builtinIsActive && 'border-success'
        )}
      >
        <CardHeader className="pb-3">
          <div className="flex items-start justify-between gap-2">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1 min-w-0">
                <div className="flex items-center justify-center h-8 w-8 rounded-lg shrink-0 bg-warning-light text-warning">
                  <BrainCircuit className="h-4 w-4" />
                </div>
                <CardTitle className="text-base truncate min-w-0">
                  {t('plugins:llm.builtinTitle')}
                  {installedModelName && <span className="text-muted-foreground"> · {installedModelName}</span>}
                </CardTitle>
                <Badge variant="outline" className="text-xs shrink-0">{t('plugins:llm.builtinBadge')}</Badge>
                {builtinIsActive && <Badge variant="default" className="text-xs shrink-0">{t('plugins:llm.active')}</Badge>}
              </div>
              <CardDescription className="text-xs line-clamp-1">
                {t('plugins:llm.builtinDesc')}
              </CardDescription>
            </div>
            <span className={cn('inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium shrink-0', builtinInfo.pillClass)}>
              {builtinInfo.text}
            </span>
          </div>
        </CardHeader>
        <CardContent className="pb-3 space-y-3">
          {builtinStatus.server_state === 'downloading' && (
            <div className="space-y-1">
              <Progress value={builtinInfo.percent ?? 0} />
              <p className="text-xs text-muted-foreground">{builtinInfo.text}</p>
            </div>
          )}
          {builtinStatus.installed && builtinStatus.memory_ok === false && (
            <div className="flex items-start gap-2 rounded-md bg-warning-light px-3 py-2 text-warning">
              <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
              <span className="text-xs leading-snug">
                {t('plugins:llm.builtinMemLowInstalled', {
                  min: ((builtinStatus.min_ram_mb ?? 0) / 1024).toFixed(0),
                  avail: ((builtinStatus.available_ram_mb ?? 0) / 1024).toFixed(1),
                })}
              </span>
            </div>
          )}
          {builtinStatus.installed && builtinStatus.server_state !== 'downloading' && (
            <div className="flex items-center gap-2 text-xs">
              <span className="text-muted-foreground">
                {t('plugins:llm.builtinCtx')}:
                <span className="font-mono text-foreground ml-1">{(builtinStatus.ctx ?? builtinStatus.default_ctx ?? 0).toLocaleString()}</span>
              </span>
              {builtinStatus.ctx_override != null && (
                <Badge variant="outline" className="text-[10px] shrink-0">
                  {t('plugins:llm.builtinCtxOverride', { def: (builtinStatus.default_ctx ?? 0).toLocaleString() })}
                </Badge>
              )}
              <Button
                size="sm"
                variant="outline"
                className="h-7 gap-1.5 px-2"
                disabled={!!builtinBusyAction}
                onClick={() => {
                  // Snap to the nearest preset (the Select only offers presets).
                  const cur = builtinStatus.ctx_override ?? builtinStatus.ctx ?? builtinStatus.default_ctx ?? 32768
                  const presets = [32768, 65536, 131072]
                  setCtxDialogInput(presets.reduce((a, b) => Math.abs(b - cur) < Math.abs(a - cur) ? b : a, presets[0]))
                  setCtxDialogOpen(true)
                }}
              >
                <Settings2 className="h-3.5 w-3.5" />
                {t('plugins:llm.builtinEngineSettings')}
              </Button>
            </div>
          )}
          <div className="flex flex-wrap items-center gap-2">
            {builtinStatus.server_state === 'downloading' ? (
              // Progress shown above; no actions while a download is in flight.
              null
            ) : !builtinStatus.installed ? (
              <Button onClick={() => handleBuiltinAction('download')} disabled={!!builtinBusyAction}>
                {builtinBusyAction === 'download'
                  ? <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  : <Download className="mr-2 h-4 w-4" />}
                {t('plugins:llm.builtinDownload')}
              </Button>
            ) : (
              <>
                <Button
                  variant="outline"
                  onClick={() => {
                    setWizardStartInPicker(true)
                    setWizardOpen(true)
                  }}
                  disabled={!!builtinBusyAction}
                >
                  <Download className="mr-2 h-4 w-4" />
                  {t('plugins:llm.switchModel')}
                </Button>
                <Button
                  onClick={() => handleBuiltinAction('activate')}
                  // Activate only makes sense while the bundled server is
                  // healthy — activating a stopped server would point chat at a
                  // dead endpoint. Use 重启引擎 to bring it up first.
                  disabled={!!builtinBusyAction || builtinIsActive || builtinStatus.server_state !== 'running'}
                  variant={builtinIsActive ? 'secondary' : 'default'}
                >
                  {builtinBusyAction === 'activate'
                    ? <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    : builtinIsActive
                      ? <CheckCircle2 className="mr-2 h-4 w-4" />
                      : <Zap className="mr-2 h-4 w-4" />}
                  {builtinIsActive ? t('plugins:llm.builtinActive') : t('plugins:llm.builtinActivate')}
                </Button>
                {builtinStatus.server_state === 'stopped' && (
                  <Button
                    onClick={() => handleBuiltinAction('restart')}
                    disabled={!!builtinBusyAction}
                    variant="secondary"
                  >
                    {builtinBusyAction === 'restart'
                      ? <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      : <RotateCcw className="mr-2 h-4 w-4" />}
                    {t('plugins:llm.builtinRestart')}
                  </Button>
                )}
                <Button
                  onClick={() => handleBuiltinAction('delete')}
                  disabled={!!builtinBusyAction}
                  variant="destructive"
                >
                  {builtinBusyAction === 'delete'
                    ? <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    : <Trash2 className="mr-2 h-4 w-4" />}
                  {t('plugins:llm.builtinDelete')}
                </Button>
              </>
            )}
          </div>
        </CardContent>
      </Card>
    ) : null

    // First-run wizard. Mounted in the list view so it survives the
    // empty-state ↔ provider-grid branch switch (a quiet reload after
    // activation may finally populate backendTypes). open={false} hides it.
    const wizardElement = (
      <BuiltinModelWizard
        open={wizardOpen}
        onOpenChange={setWizardOpen}
        startInPicker={wizardStartInPicker}
        hasActiveBackend={!!activeBackendId && !builtinIsActive}
        isBuiltinActive={builtinIsActive}
        onActivated={() => {
          // Activation may have created/updated the builtin instance and
          // flipped the active id — refresh the slice-backed lists quietly
          // (no loading skeleton) so the wizard stays mounted for 已就绪.
          loadData(true)
          refreshBuiltinStatus()
        }}
      />
    )

    // Empty state when no backend types are available — strong guidance:
    // primary CTA opens the first-run wizard for the built-in model, secondary
    // CTA lets the user wire up their own OpenAI-compatible API backend.
    if (backendTypes.length === 0) {
      return (
        <>
          {/* No builtinCard here — the CTA buttons below already pitch the
              built-in model; stacking both was the duplicate CTA users saw. */}
          <EmptyState
            icon="plugin"
            title={t('plugins:llm.noBackends')}
            description={t('plugins:llm.noBackendsDesc')}
          />
          <div className="mt-2 flex flex-col items-center gap-3">
            <Button size="lg" onClick={() => {
              setWizardStartInPicker(false)
              setWizardOpen(true)
            }}>
              <Download className="mr-2 h-4 w-4" />
              {t('plugins:llm.emptyStateDownloadBuiltin')}
            </Button>
            <Button variant="secondary" onClick={handleAddOwnBackend}>
              <Server className="mr-2 h-4 w-4" />
              {t('plugins:llm.emptyStateAddOwnBackend')}
            </Button>
          </div>
        {wizardElement}
        </>
      )
    }

    return (
      <>
        {/* While the first-run banner below is pitching the built-in model,
            don't also render the builtin card — that duplication (two
            download CTAs on one screen) is what fresh users saw. Once
            installed, the banner disappears and the card takes over as the
            persistent status/management surface. */}
        {!(builtinStatus && !builtinStatus.installed && instances.length === 0) && builtinCard}

        {/* First-run guidance (non-empty type grid): no backend configured AND
            the built-in model not yet installed → prominent CTA into the
            first-run wizard (spec §7). Deliberately a banner, not an auto-open,
            to avoid surprising the user. Gated on builtinStatus being present
            so servers without the /api/builtin-llm endpoints never show it. */}
        {instances.length === 0 && builtinStatus && !builtinStatus.installed && (
          <Card className="mb-4 border-primary">
            <CardContent className="p-5">
              <div className="flex flex-col items-start gap-4 sm:flex-row sm:items-center">
                <div className="flex items-center justify-center h-12 w-12 rounded-xl shrink-0 bg-primary-light text-primary">
                  <Cpu className="h-6 w-6" />
                </div>
                <div className="min-w-0 flex-1">
                  <h3 className="text-base font-semibold text-foreground">
                    {t('plugins:llm.firstRunBannerTitle')}
                  </h3>
                  <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                    {t('plugins:llm.firstRunBannerDesc')}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2 shrink-0">
                  <Button onClick={() => {
                    setWizardStartInPicker(false)
                    setWizardOpen(true)
                  }}>
                    <Download className="mr-2 h-4 w-4" />
                    {t('plugins:llm.emptyStateDownloadBuiltin')}
                  </Button>
                  <Button variant="secondary" onClick={handleAddOwnBackend}>
                    <Server className="mr-2 h-4 w-4" />
                    {t('plugins:llm.emptyStateAddOwnBackend')}
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        )}

        {/* Provider Cards Grid — protocol-first: Ollama / llama.cpp / Cloud AI
            (OpenAI-compatible, Anthropic). All other vendors ride the
            OpenAI-compatible path via their endpoint. */}
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,260px),1fr))]">
          {PROTOCOL_TYPE_IDS.map((pid) => {
            const type = backendTypes.find((b) => b.id === pid)
            if (!type) return null
            const typeInstances = getInstancesForType(type.id)
            const info = getLlmProviderInfo(type.id, t)
            // Protocol label overrides the raw vendor name (Cloud AI buckets).
            const protocolName = t(`plugins:llm.protocol.${type.id}.name`, { defaultValue: info.name })
            const protocolDesc = t(`plugins:llm.protocol.${type.id}.desc`, { defaultValue: type.description })
            const activeInstance = typeInstances.find(i => i.id === activeBackendId)
            const hasActive = !!activeInstance

            return (
              <Card
                key={pid}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  hasActive && "border-success border-2"
                )}
                onClick={() => {
                  setSelectedType(toUnifiedPluginType(type, t))
                  setView('detail')
                }}
              >
                <CardContent className="p-4">
                  <div className="flex items-start gap-3">
                    <div className={cn("flex items-center justify-center h-10 w-10 rounded-lg shrink-0", info.iconBg)}>
                      {info.icon}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <CardTitle className="text-base truncate min-w-0">{protocolName}</CardTitle>
                        <span className={cn("text-xs font-medium shrink-0", hasActive ? "text-success" : "text-muted-foreground")}>
                          {hasActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                        </span>
                      </div>
                      <CardDescription className="mt-1 text-xs line-clamp-1">
                        {protocolDesc}
                      </CardDescription>
                    </div>
                  </div>
                  <div className="mt-3 flex items-center justify-between text-xs">
                    <span className="text-muted-foreground">{t('plugins:llm.instances')}</span>
                    <span className="font-medium text-foreground">{t('plugins:llm.instancesCount', { count: typeInstances.length })}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })}

          {/* Cloud AI — one card; the add dialog picks the protocol path. */}
          {(() => {
            // Same grouping as the detail view: every non-local, non-builtin
            // instance (incl. legacy vendor types from before the
            // protocol-first refactor) lives under Cloud AI.
            const cloudInstances = instances.filter(
              i => !i.is_builtin && i.backend_type !== 'ollama' && i.backend_type !== 'llamacpp'
            )
            const hasActive = cloudInstances.some(i => i.id === activeBackendId)
            return (
              <Card
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  hasActive && "border-success border-2"
                )}
                onClick={() => {
                  setSelectedType({
                    id: 'cloudai',
                    type: 'llm_backend',
                    name: t('plugins:llm.protocol.cloudai.name'),
                    description: t('plugins:llm.protocol.cloudai.desc'),
                    icon: <Cloud className="h-6 w-6" />,
                    color: 'bg-accent-indigo-light text-accent-indigo',
                    // Never rendered as a form (add/edit dialogs use the concrete
                    // protocol type) — an empty schema satisfies the type.
                    config_schema: { type: 'object', properties: {} },
                    can_add_multiple: true,
                    builtin: false,
                    requires_api_key: true,
                    supports_streaming: true,
                    default_model: '',
                    default_endpoint: undefined,
                  })
                  setView('detail')
                }}
              >
                <CardContent className="p-4">
                  <div className="flex items-start gap-3">
                    <div className="flex items-center justify-center h-10 w-10 rounded-lg shrink-0 bg-accent-indigo-light text-accent-indigo">
                      <Cloud className="h-5 w-5" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <CardTitle className="text-base truncate min-w-0">{t('plugins:llm.protocol.cloudai.name')}</CardTitle>
                        <span className={cn("text-xs font-medium shrink-0", hasActive ? "text-success" : "text-muted-foreground")}>
                          {hasActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                        </span>
                      </div>
                      <CardDescription className="mt-1 text-xs line-clamp-1">
                        {t('plugins:llm.protocol.cloudai.desc')}
                      </CardDescription>
                    </div>
                  </div>
                  <div className="mt-3 flex items-center justify-between text-xs">
                    <span className="text-muted-foreground">{t('plugins:llm.instances')}</span>
                    <span className="font-medium text-foreground">{t('plugins:llm.instancesCount', { count: cloudInstances.length })}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })()}
        </div>
          {/* Builtin engine settings (ctx) */}
        <UnifiedFormDialog
          open={ctxDialogOpen}
          onOpenChange={(o) => { setCtxDialogOpen(o); if (!o) setCtxDialogError(null) }}
          title={t('plugins:llm.builtinEngineSettings')}
          description={t('plugins:llm.builtinEngineSettingsDesc', {
            def: (builtinStatus?.default_ctx ?? 0).toLocaleString(),
            model: builtinStatus?.model_id ?? '',
          })}
          icon={<BrainCircuit className="h-5 w-5 text-muted-foreground" />}
          className="z-[110]"
          isSubmitting={ctxDialogSaving}
          onSubmit={async () => {
            const v = ctxDialogInput
            if (v == null || !Number.isFinite(v) || v < 1024 || v > 131_072) {
              setCtxDialogError(t('plugins:llm.builtinCtxInvalid'))
              return
            }
            setCtxDialogSaving(true)
            setCtxDialogError(null)
            try {
              await api.restartBuiltinLlm(v)
              toast({ title: t('plugins:llm.builtinCtxApplied', { ctx: v.toLocaleString() }) })
              setCtxDialogOpen(false)
              await refreshBuiltinStatus()
              await loadData(true)
            } catch (e) {
              setCtxDialogError(e instanceof Error ? e.message : String(e))
            } finally {
              setCtxDialogSaving(false)
            }
          }}
          submitLabel={t('plugins:llm.builtinCtxApply')}
          submitDisabled={ctxDialogSaving}
          submitError={ctxDialogError ?? undefined}
        >
          <div className="space-y-4">
            <FormField label={t('plugins:llm.builtinCtx')} helpText={t('plugins:llm.builtinCtxHelp')}>
              <Select
                value={String(ctxDialogInput ?? '')}
                onValueChange={(v) => setCtxDialogInput(Number(v))}
              >
                <SelectTrigger className="w-full font-mono">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {[32768, 65536, 131072].map((n) => {
                    const def = builtinStatus?.default_ctx ?? 32768
                    return (
                      <SelectItem key={n} value={String(n)}>
                        {(n / 1024).toFixed(0)}K{n === def ? ` · ${t('plugins:llm.builtinCtxDefault')}` : ''}
                      </SelectItem>
                    )
                  })}
                </SelectContent>
              </Select>
            </FormField>
            {builtinStatus?.ctx_override != null && (
              <Button
                size="sm"
                variant="ghost"
                type="button"
                disabled={ctxDialogSaving}
                onClick={async () => {
                  setCtxDialogSaving(true)
                  try {
                    await api.restartBuiltinLlm(builtinStatus!.default_ctx!)
                    toast({ title: t('plugins:llm.builtinCtxReset') })
                    setCtxDialogOpen(false)
                    setCtxDialogInput(null)
                    await refreshBuiltinStatus()
                    await loadData(true)
                  } catch (e) {
                    setCtxDialogError(e instanceof Error ? e.message : String(e))
                  } finally {
                    setCtxDialogSaving(false)
                  }
                }}
              >
                {t('plugins:llm.builtinCtxResetBtn')}
              </Button>
            )}
          </div>
        </UnifiedFormDialog>

        {wizardElement}
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedType) {
    const typeId = selectedType.id || 'ollama'
    // Cloud AI groups both protocol paths (OpenAI-compatible + Anthropic).
    const isCloudAi = typeId === 'cloudai'
    // Include legacy vendor-typed instances too (qwen/deepseek/glm/xai/…
    // created before the protocol-first refactor) — they are all cloud
    // endpoints and their cards no longer exist.
    const typeInstances = isCloudAi
      ? instances.filter(i => !i.is_builtin && i.backend_type !== 'ollama' && i.backend_type !== 'llamacpp')
      : getInstancesForType(typeId)
    const info = getLlmProviderInfo(typeId, t)
    // 'cloudai' is a synthetic type — its display name/icon come from the
    // card the user clicked, not from the per-vendor i18n key.
    const displayName = isCloudAi ? selectedType.name : info.name
    const displayIcon = isCloudAi ? selectedType.icon : info.icon
    const displayIconBg = isCloudAi ? 'bg-accent-indigo-light text-accent-indigo' : info.iconBg
    // Editing inside Cloud AI needs the concrete protocol type (its config
    // schema + per-type dialog behavior) — the synthetic 'cloudai' type has
    // no schema. Resolve from the instance's backend_type; also covers
    // legacy vendor instances.
    const dialogPluginType = (() => {
      if (!isCloudAi || !editingInstance) return selectedType
      const concrete = backendTypes.find(b => b.id === editingInstance.plugin_type)
      return concrete ? toUnifiedPluginType(concrete, t) : selectedType
    })()
    const pluginInstances = typeInstances.map(i => toPluginInstance(i, activeBackendId))

    return (
      <>
        {/* Header with back button — sticky (see shared ListToolbar). */}
        <ListToolbar
          onBack={() => setView('list')}
          backLabel={t('plugins:llm.back')}
          icon={displayIcon}
          iconBg={displayIconBg}
          title={displayName}
          description={selectedType.description}
          badges={
            <>
              {selectedType.supports_streaming && (
                <Badge variant="outline" className="text-xs shrink-0">{t('plugins:llm.streamingOutput')}</Badge>
              )}
              {selectedType.default_model && (
                <Badge variant="outline" className="text-xs text-muted-foreground shrink-0">
                  {t('plugins:llm.defaultModel')}: {selectedType.default_model}
                </Badge>
              )}
              {selectedType.requires_api_key && (
                <Badge variant="outline" className="text-xs text-warning border-warning shrink-0">
                  {t('plugins:llm.requiresApiKey')}
                </Badge>
              )}
            </>
          }
        />

        {/* Instances */}
        {pluginInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", displayIconBg)}>
                {displayIcon}
              </div>
              <h3 className="text-lg font-semibold mb-1">{t('plugins:llm.noInstanceYet', { name: displayName })}</h3>
              <p className="text-sm text-muted-foreground mb-4">
                {t('plugins:llm.configureToStart', { name: displayName })}
              </p>
              <Button onClick={() => {
                setEditingInstance(null)
                if (isCloudAi) { setCloudAddOpen(true) } else { setConfigDialogOpen(true) }
              }}>
                <Server className="mr-2 h-4 w-4" />
                {t('plugins:llm.addInstance2', { name: displayName })}
              </Button>
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 grid-cols-[minmax(0,1fr)] md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const isActive = instance.id === activeBackendId
              const testResult = testResults[instance.id]
              // Cloud AI groups multiple protocols — label each card with
              // its concrete one (protocol key first, legacy vendor name as
              // fallback). The type is immutable after creation (backend
              // contract has no backend_type on update).
              const protocolLabel = t(`plugins:llm.protocol.${instance.plugin_type}.name`, {
                defaultValue: getLlmProviderInfo(instance.plugin_type, t).name,
              })

              return (
                <Card
                  key={instance.id}
                  className={cn(
                    "transition-all duration-200",
                    isActive && "border-success"
                  )}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1 min-w-0">
                          <CardTitle className="text-base truncate min-w-0">{instance.name}</CardTitle>
                          {isCloudAi && (
                            <Badge variant="outline" className="text-xs shrink-0">{protocolLabel}</Badge>
                          )}
                          {isActive && <Badge variant="default" className="text-xs">{t('plugins:llm.active')}</Badge>}
                        </div>
                        <CardDescription className="font-mono text-xs">
                          {instance.config?.model as string || '-'}
                        </CardDescription>
                        {/* Capability icons — same monochrome style as chat model dropdown */}
                        {(() => {
                          const caps = instance.capabilities as BackendCapabilities | undefined
                          if (!caps) return null
                          return (
                            <div className="flex items-center gap-1 mt-2 text-muted-foreground">
                              {caps.supports_multimodal && (
                                <span
                                  title={
                                    caps.multimodal_user_override != null
                                      ? t('plugins:llm.capabilityVisionOverride', {
                                          source: caps.multimodal_source ?? 'user_override',
                                        })
                                      : t('plugins:llm.capabilityVision')
                                  }
                                >
                                  <Eye className="h-3.5 w-3.5" />
                                </span>
                              )}
                              {caps.supports_tools && (
                                <span title={t('plugins:llm.capabilityTools')}>
                                  <Wrench className="h-3.5 w-3.5" />
                                </span>
                              )}
                              {caps.supports_thinking && (
                                <span title={t('plugins:llm.capabilityThinking')}>
                                  <Brain className="h-3.5 w-3.5" />
                                </span>
                              )}
                            </div>
                          )
                        })()}
                      </div>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0"
                          onClick={() => handleTest(instance.id)}
                        >
                          <TestTube className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0"
                          onClick={() => {
                            if (isCloudAi) {
                              // Cloud AI edits go through the add dialog in
                              // edit mode — its protocol select doubles as
                              // the backend_type switcher.
                              setCloudEditTarget({
                                id: instance.id,
                                name: instance.name,
                                backend_type: instance.plugin_type,
                                endpoint: String(instance.config?.endpoint ?? ''),
                                model: String(instance.config?.model ?? ''),
                                api_key_configured: !!(instance as { api_key_configured?: boolean }).api_key_configured,
                              })
                              setCloudAddOpen(true)
                            } else {
                              setEditingInstance(instance)
                              setConfigDialogOpen(true)
                            }
                          }}
                        >
                          <Edit className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0 text-error hover:text-error"
                          onClick={() => handleDelete(instance.id)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  </CardHeader>

                  <CardContent className="pb-3">
                    <div className="space-y-2 text-sm">
                      {instance.config?.endpoint != null && (
                        <div className="flex items-center justify-between gap-2">
                          <span className="text-muted-foreground shrink-0">{t('plugins:llm.endpoint')}:</span>
                          <span className="font-mono text-xs truncate min-w-0">{String(instance.config.endpoint)}</span>
                        </div>
                      )}
                      {testResult && (
                        <div className={cn(
                          "text-xs p-2 rounded",
                          testResult.success
                            ? "bg-success-light text-success dark:bg-success-light dark:text-success"
                            : "bg-error-light text-error"
                        )}>
                          {testResult.message}
                        </div>
                      )}
                    </div>
                  </CardContent>
                </Card>
              )
            })}
          </div>
        )}

        {/* Add Instance Button */}
        {pluginInstances.length > 0 && (
          <div className="mt-4">
            <Button onClick={() => {
              setEditingInstance(null)
              if (isCloudAi) { setCloudAddOpen(true) } else { setConfigDialogOpen(true) }
            }}>
              <Server className="mr-2 h-4 w-4" />
              {t('plugins:llm.addInstance')}
            </Button>
          </div>
        )}

        {/* Unified Config Dialog */}

        <CloudAiAddDialog
          open={cloudAddOpen}
          onOpenChange={(open) => {
            setCloudAddOpen(open)
            if (!open) setCloudEditTarget(null)
          }}
          onSubmit={handleCloudAiSubmit}
          editing={cloudEditTarget}
        />
        <UniversalPluginConfigDialog
          open={configDialogOpen}
          onOpenChange={(open) => {
            setConfigDialogOpen(open)
            if (!open) {
              setEditingInstance(null)
              setTestResults({})
            }
          }}
          pluginType={dialogPluginType}
          instances={pluginInstances}
          editingInstance={editingInstance}
          onCreate={handleCreate}
          onUpdate={handleUpdate}
          onDelete={handleDelete}
          onTest={handleTest}
          onRefresh={loadData}
          testResults={testResults}
          setTestResults={setTestResults}
        />
      </>
    )
  }

  return null
}
