import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import {
  ArrowLeft,
  Server,
  CheckCircle2,
  Loader2,
  TestTube,
  Edit,
  Trash2,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState, LoadingState } from '@/components/shared'
import { cn } from '@/lib/utils'
import { fetchAPI } from '@/lib/api'
import { UniversalPluginConfigDialog, type PluginInstance, type UnifiedPluginType } from '@/components/plugins/UniversalPluginConfigDialog'
import type {
  LlmBackendInstance,
  BackendTypeDefinition,
  BackendTestResult,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
  PluginConfigSchema,
  BackendCapabilities,
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
    iconBg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
  },
  openai: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/20 dark:text-emerald-400',
  },
  anthropic: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
  },
  google: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
  },
  xai: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400',
  },
  qwen: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-violet-100 text-violet-700 dark:bg-violet-900/20 dark:text-violet-400',
  },
  deepseek: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-cyan-100 text-cyan-700 dark:bg-cyan-900/20 dark:text-cyan-400',
  },
  glm: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-pink-100 text-pink-700 dark:bg-pink-900/20 dark:text-pink-400',
  },
  minimax: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/20 dark:text-indigo-400',
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
          Object.entries(configSchema.properties || {}).filter(
            ([key]) => !EXCLUDED_LLM_CONFIG_FIELDS.includes(key as string)
          )
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
    },
    status: {
      active: instance.id === activeId,
    },
    // Store capabilities at top level for easier access
    capabilities: instance.capabilities,
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

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    setLoading(true)
    try {
      const typesResponse = await fetchAPI<{ types: BackendTypeDefinition[] }>('/llm-backends/types', { skipAuth: true })
      setBackendTypes(typesResponse.types || [])

      const instancesResponse = await fetchAPI<{
        backends: LlmBackendInstance[]
        count: number
        active_id: string | null
      }>('/llm-backends', { skipAuth: true })
      setInstances(instancesResponse.backends || [])
      setActiveBackendId(instancesResponse.active_id || null)
    } catch (error) {
      handleError(error, { operation: 'Load LLM data', showToast: false })
      setBackendTypes([])
      setInstances([])
      setActiveBackendId(null)
    } finally {
      setLoading(false)
    }
  }

  const getInstancesForType = (typeId: string) => {
    return instances.filter(i => i.backend_type === typeId)
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
    const data: UpdateLlmBackendRequest = {
      name: config.name as string,
      endpoint: config.endpoint as string,
      model: config.model as string,
      api_key: config.api_key as string,
      temperature: config.temperature as number,
      top_p: config.top_p as number,
      top_k: config.top_k as number,
      capabilities: config.capabilities as BackendCapabilities | undefined,
    }
    await onUpdateBackend(id, data)
  }

  // Handle delete instance
  const handleDelete = async (id: string) => {
    await onDeleteBackend(id)
  }

  // Handle test connection
  const handleTest = async (id: string) => {
    const result = await onTestBackend(id)
    const message = result.success
      ? `${t('plugins:llm.latency')}: ${result.latency_ms?.toFixed(0) || '0'}ms`
      : (result.error || 'Failed')

    setTestResults(prev => ({
      ...prev,
      [id]: { success: result.success, message },
    }))
  }

  if (loading) {
    return <LoadingState variant="page" text={t('common:loading')} />
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    // Empty state when no backend types are available
    if (backendTypes.length === 0) {
      return (
        <EmptyState
          icon="plugin"
          title={t('plugins:llm.noBackends')}
          description={t('plugins:llm.noBackendsDesc')}
          action={{ label: t('common:retry'), onClick: loadData, icon: <Loader2 className="h-4 w-4" /> }}
        />
      )
    }

    return (
      <>
        {/* Provider Cards Grid */}
        <div className="grid gap-4 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {backendTypes.map((type) => {
            const typeInstances = getInstancesForType(type.id)
            const info = getLlmProviderInfo(type.id, t)
            const activeInstance = typeInstances.find(i => i.id === activeBackendId)
            const hasActive = !!activeInstance

            return (
              <Card
                key={type.id}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  hasActive && "border-green-500 border-2"
                )}
                onClick={() => {
                  setSelectedType(toUnifiedPluginType(type, t))
                  setView('detail')
                }}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", info.iconBg)}>
                    {info.icon}
                  </div>
                  <CardTitle className="text-base mt-3">{info.name}</CardTitle>
                  <CardDescription className="mt-1 text-xs line-clamp-2 min-h-[2.5em]">
                    {type.description}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('plugins:llm.status')}:</span>
                    <span className={hasActive ? "text-green-600 dark:text-green-400 font-medium" : "text-muted-foreground font-medium"}>
                      {hasActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">{t('plugins:llm.instances')}:</span>
                    <span className="font-medium">{t('plugins:llm.instancesCount', { count: typeInstances.length })}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedType) {
    const typeId = selectedType.id || 'ollama'
    const typeInstances = getInstancesForType(typeId)
    const info = getLlmProviderInfo(typeId, t)
    const pluginInstances = typeInstances.map(i => toPluginInstance(i, activeBackendId))

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            {t('plugins:llm.back')}
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", info.iconBg)}>
              {info.icon}
            </div>
            <div>
              <h2 className="text-2xl font-bold">{info.name}</h2>
              <p className="text-sm text-muted-foreground">{selectedType.description}</p>
            </div>
          </div>
        </div>

        {/* Type Info Badges */}
        <div className="flex flex-wrap gap-2 text-sm mb-4">
          {selectedType.requires_api_key && (
            <Badge variant="outline" className="text-warning border-warning">
              {t('plugins:llm.requiresApiKey')}
            </Badge>
          )}
          {selectedType.supports_streaming && (
            <Badge variant="outline">{t('plugins:llm.streamingOutput')}</Badge>
          )}
          {selectedType.default_model && (
            <Badge variant="outline" className="text-muted-foreground">
              {t('plugins:llm.defaultModel')}: {selectedType.default_model}
            </Badge>
          )}
        </div>

        {/* Instances */}
        {pluginInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", info.iconBg)}>
                {info.icon}
              </div>
              <h3 className="text-lg font-semibold mb-1">{t('plugins:llm.noInstanceYet', { name: info.name })}</h3>
              <p className="text-sm text-muted-foreground mb-4">
                {t('plugins:llm.configureToStart', { name: info.name })}
              </p>
              <Button onClick={() => {
                setEditingInstance(null)
                setConfigDialogOpen(true)
              }}>
                <Server className="mr-2 h-4 w-4" />
                {t('plugins:llm.addInstance2', { name: info.name })}
              </Button>
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const isActive = instance.id === activeBackendId
              const testResult = testResults[instance.id]

              return (
                <Card
                  key={instance.id}
                  className={cn(
                    "transition-all duration-200",
                    isActive && "border-green-500"
                  )}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <CardTitle className="text-base">{instance.name}</CardTitle>
                          {isActive && <Badge variant="default" className="text-xs">{t('plugins:llm.active')}</Badge>}
                        </div>
                        <CardDescription className="font-mono text-xs">
                          {instance.config?.model as string || '-'}
                        </CardDescription>
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
                            setEditingInstance(instance)
                            setConfigDialogOpen(true)
                          }}
                        >
                          <Edit className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0 text-destructive hover:text-destructive"
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
                        <div className="flex items-center justify-between">
                          <span className="text-muted-foreground">{t('plugins:llm.endpoint')}:</span>
                          <span className="font-mono text-xs truncate max-w-[200px]">{String(instance.config.endpoint)}</span>
                        </div>
                      )}
                      {testResult && (
                        <div className={cn(
                          "text-xs p-2 rounded",
                          testResult.success
                            ? "bg-green-50 text-green-700 dark:bg-green-900 dark:text-green-300"
                            : "bg-red-50 text-red-700 dark:bg-red-900 dark:text-red-300"
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
              setConfigDialogOpen(true)
            }}>
              <Server className="mr-2 h-4 w-4" />
              {t('plugins:llm.addInstance')}
            </Button>
          </div>
        )}

        {/* Unified Config Dialog */}
        <UniversalPluginConfigDialog
          open={configDialogOpen}
          onOpenChange={(open) => {
            setConfigDialogOpen(open)
            if (!open) {
              setEditingInstance(null)
              setTestResults({})
            }
          }}
          pluginType={selectedType}
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
