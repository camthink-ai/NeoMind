import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { confirm } from '@/hooks/use-confirm'
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
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState, LoadingState, ListToolbar } from '@/components/shared'
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
    iconBg: 'bg-success-light text-success dark:bg-success-light dark:text-success',
  },
  openai: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-emerald-light text-accent-emerald',
  },
  anthropic: {
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-accent-orange-light text-accent-orange',
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
      // Pass thinking_enabled so the dialog Switch can read it back in edit mode.
      // Mutated via direct PATCH (not the form Save), mirroring the multimodal
      // override pattern.
      thinking_enabled: instance.thinking_enabled,
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
      // Only include api_key if it's provided (non-empty string)
      // This prevents overwriting the existing key with an empty value
      ...(config.api_key && typeof config.api_key === 'string' && config.api_key.trim()
        ? { api_key: config.api_key }
        : {}),
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
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(260px,1fr))]">
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
                  hasActive && "border-success border-2"
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
                    <span className={hasActive ? "text-success dark:text-success font-medium" : "text-muted-foreground font-medium"}>
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
        {/* Header with back button — sticky (see shared ListToolbar). */}
        <ListToolbar
          onBack={() => setView('list')}
          backLabel={t('plugins:llm.back')}
          icon={info.icon}
          iconBg={info.iconBg}
          title={info.name}
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
          <div className="grid gap-4 grid-cols-[minmax(0,1fr)] md:grid-cols-2">
            {pluginInstances.map((instance) => {
              const isActive = instance.id === activeBackendId
              const testResult = testResults[instance.id]

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
                            setEditingInstance(instance)
                            setConfigDialogOpen(true)
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
