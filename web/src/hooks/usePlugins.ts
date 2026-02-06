/**
 * Unified Plugin Hook
 *
 * Provides a unified interface for managing extensions:
 * - Dynamic extensions (loaded from .so/.dylib/.dll files)
 * - LLM provider extensions
 * - Device protocol extensions
 * - Tool extensions
 *
 * Integrates with the extension store and provides helper functions
 * for working with the UnifiedPluginCard component.
 *
 * Note: Device adapters (MQTT, HTTP, Webhook) are now built-in and managed
 * via the devices API, not as dynamic plugins.
 */

import { useCallback, useMemo } from 'react'
import { useStore } from '@/store'
import type {
  Plugin,
  PluginStatsDto,
  ExtensionStatsDto,
} from '@/types'
import type {
  UnifiedPluginData,
  PluginStatus,
  DynamicPluginMetadata,
  PluginCategory,
  PluginType,
} from '@/types/plugin-schema'
import type {
  PluginUISchema,
} from '@/types/plugin-schema'
import { COMMON_FIELDS } from '@/types/plugin-schema'

// ============================================================================
// Hook Return Types
// ============================================================================

export interface UsePluginsReturn {
  // Data
  plugins: UnifiedPluginData[]
  loading: boolean
  discovering: boolean
  stats: Record<string, PluginStatsDto>

  // Filtering
  filterByCategory: (category: PluginCategory) => UnifiedPluginData[]
  filterByType: (type: PluginType) => UnifiedPluginData[]
  filterByState: (state: string) => UnifiedPluginData[]
  search: (query: string) => UnifiedPluginData[]

  // Actions
  refresh: () => Promise<void>
  toggle: (id: string, enabled: boolean) => Promise<boolean>
  start: (id: string) => Promise<boolean>
  stop: (id: string) => Promise<boolean>
  remove: (id: string) => Promise<boolean>
  configure: (id: string, config: Record<string, unknown>) => Promise<boolean>
  test: (id: string) => Promise<boolean>
  discover: () => Promise<{ discovered: number; message: string }>
}

// ============================================================================
// Plugin State Mapping
// ============================================================================

/**
 * Maps backend plugin state to unified PluginStatus
 */
function mapPluginState(backendState: string): PluginStatus['state'] {
  const state = backendState.toLowerCase()
  if (state.includes('running')) return 'running'
  if (state.includes('loaded') || state.includes('initialized')) return 'loaded'
  if (state.includes('error')) return 'error'
  if (state.includes('stop')) return 'stopped'
  return 'stopped'
}

/**
 * Maps backend plugin state to health status
 */
function mapPluginHealth(backendState: string, stats?: PluginStatsDto): PluginStatus['health'] {
  const state = backendState.toLowerCase()
  if (state.includes('error')) return 'unhealthy'
  if (state.includes('running')) {
    // Check error rate in stats
    if (stats && stats.error_count > 10 && stats.start_count > 0) {
      const errorRate = stats.error_count / stats.start_count
      if (errorRate > 0.5) return 'unhealthy'
      if (errorRate > 0.1) return 'degraded'
    }
    return 'healthy'
  }
  return 'healthy'
}

/**
 * Extract plugin type from backend plugin_type string
 */
function mapPluginType(backendType: string): PluginType {
  const type = backendType.toLowerCase().replace(/_/g, '_')
  switch (type) {
    case 'llm_backend':
    case 'llmbackend':
      return 'llm_backend'
    case 'device_adapter':
    case 'deviceadapter':
      return 'device_adapter'
    case 'storage_backend':
    case 'storagebackend':
      return 'storage_backend'
    case 'alert_channel':
    case 'alertchannel':
      return 'alert_channel'
    case 'tool':
      return 'tool'
    case 'integration':
      return 'integration'
    default:
      return 'dynamic'
  }
}

/**
 * Get UI schema for a plugin type
 */
function getPluginSchema(plugin: Plugin): PluginUISchema {
  const type = mapPluginType(plugin.plugin_type)
  const category: PluginCategory =
    plugin.category === 'ai' ? 'ai' :
    plugin.category === 'devices' ? 'devices' :
    plugin.category === 'notify' ? 'notify' :
    'integration'

  // Build schema based on plugin type
  const baseSchema: PluginUISchema = {
    id: plugin.id,
    type,
    category,
    name: plugin.name,
    description: plugin.description,
    version: plugin.version,
    icon: getPluginIcon(type),
    canAddMultiple: type === 'device_adapter' || type === 'tool',
    builtin: !plugin.path, // No path means it's a built-in plugin
    fields: {
      name: COMMON_FIELDS.name,
      enabled: COMMON_FIELDS.enabled,
      auto_start: COMMON_FIELDS.autoStart,
    },
    groups: {
      basic: {
        label: '基本设置',
        description: '插件基本配置',
      },
    },
    actions: [
      {
        id: 'configure',
        label: '配置',
        icon: 'Settings',
      },
      {
        id: 'test',
        label: '测试',
        icon: 'TestTube',
      },
    ],
    statusFields: {
      running: {
        icon: 'CheckCircle',
        label: '运行中',
      },
      stopped: {
        icon: 'Server',
        label: '已停止',
      },
      error: {
        icon: 'AlertTriangle',
        label: '错误',
      },
    },
  }

  // Add type-specific fields
  if (type === 'llm_backend') {
    baseSchema.fields = {
      ...baseSchema.fields,
      endpoint: COMMON_FIELDS.url,
      api_key: COMMON_FIELDS.apiKey,
      model: {
        name: 'model',
        type: 'string',
        label: '模型',
        description: 'LLM 模型名称',
        placeholder: 'llama3.2',
      },
    }
  } else if (type === 'device_adapter') {
    baseSchema.fields = {
      ...baseSchema.fields,
      host: COMMON_FIELDS.host,
      port: COMMON_FIELDS.port,
      username: COMMON_FIELDS.username,
      password: COMMON_FIELDS.password,
    }
  }

  return baseSchema
}

/**
 * Get icon name for plugin type
 */
function getPluginIcon(type: PluginType): string {
  switch (type) {
    case 'llm_backend':
      return 'BrainCircuit'
    case 'device_adapter':
      return 'Network'
    case 'storage_backend':
      return 'Server'
    case 'alert_channel':
      return 'Wifi'
    case 'tool':
      return 'Zap'
    default:
      return 'Server'
  }
}

/**
 * Convert ExtensionStatsDto to PluginStatsDto by adding default values for missing fields
 */
function extensionStatsToPluginStats(extensionStats?: ExtensionStatsDto): PluginStatsDto | undefined {
  if (!extensionStats) return undefined
  return {
    start_count: extensionStats.start_count,
    stop_count: extensionStats.stop_count,
    error_count: extensionStats.error_count,
    total_execution_ms: 0, // Extension stats don't track this
    avg_response_time_ms: 0, // Extension stats don't track this
    last_start_time: undefined,
    last_stop_time: undefined,
  }
}

/**
 * Convert backend Plugin to UnifiedPluginData
 */
function toUnifiedPluginData(plugin: Plugin): UnifiedPluginData {
  const schema = getPluginSchema(plugin)
  const state = mapPluginState(plugin.state)
  const health = mapPluginHealth(plugin.state, plugin.stats)

  const status: PluginStatus = {
    state,
    enabled: plugin.enabled,
    health,
    lastError: plugin.state.includes('Error') ? plugin.state : undefined,
    stats: plugin.stats ? {
      totalRequests: plugin.stats.start_count,
      successfulRequests: plugin.stats.start_count - plugin.stats.error_count,
      failedRequests: plugin.stats.error_count,
      averageLatency: plugin.stats.avg_response_time_ms,
    } : undefined,
  }

  // Build metadata for dynamic plugins
  let metadata: DynamicPluginMetadata | undefined
  if (plugin.path) {
    metadata = {
      id: plugin.id,
      pluginType: plugin.plugin_type,
      name: plugin.name,
      version: plugin.version,
      description: plugin.description,
      author: plugin.author,
      requiredNeoMind: plugin.required_version,
      filePath: plugin.path,
      capabilities: {
        async: true,
        threadSafe: false,
        streaming: false,
        stateless: false,
        hasConfig: true,
        hotReload: false,
        singleton: false,
      },
    }
  }

  return {
    id: plugin.id,
    schema,
    metadata,
    status,
    config: {
      name: plugin.name,
      enabled: plugin.enabled,
    },
    deviceCount: undefined,
    connected: plugin.enabled,
    version: plugin.version,
    author: plugin.author,
  }
}

// ============================================================================
// Main Hook
// ============================================================================

export function usePlugins(): UsePluginsReturn {
  const {
    extensions,
    extensionsLoading,
    discovering,
    extensionStats,
    fetchExtensions,
    startExtension,
    stopExtension,
    unregisterExtension,
    executeExtensionCommand,
    discoverExtensions,
  } = useStore()

  // Map extension type to plugin category
  const mapExtensionToCategory = (extensionType: string): 'ai' | 'devices' | 'notify' => {
    const type = extensionType.toLowerCase()
    if (type === 'llm_provider') return 'ai'
    if (type === 'device_protocol') return 'devices'
    return 'notify' // Default to notify for other types
  }

  // Refresh all extension data
  const refresh = useCallback(async () => {
    await fetchExtensions()
  }, [fetchExtensions])

  // Convert all extensions to unified format
  const unifiedPlugins = useMemo(() => {
    const result: UnifiedPluginData[] = []

    // Add extensions as plugins
    for (const extension of extensions) {
      // Convert Extension to Plugin format for compatibility
      const stats = extensionStatsToPluginStats(extensionStats[extension.id])
      const plugin: Plugin = {
        id: extension.id,
        name: extension.name,
        plugin_type: extension.extension_type,
        state: extension.state,
        enabled: extension.state === 'Running',
        version: extension.version,
        description: extension.description || '',
        author: extension.author,
        required_version: '1.0.0',
        stats: stats || {
          start_count: 0,
          stop_count: 0,
          error_count: 0,
          total_execution_ms: 0,
          avg_response_time_ms: 0,
        },
        loaded_at: extension.loaded_at ? new Date(extension.loaded_at * 1000).toISOString() : new Date().toISOString(),
        path: extension.file_path,
        category: mapExtensionToCategory(extension.extension_type),
      }
      result.push(toUnifiedPluginData(plugin))
    }

    return result
  }, [extensions, extensionStats])

  // Filter by category
  const filterByCategory = useCallback((category: PluginCategory) => {
    if (category === 'all') return unifiedPlugins
    return unifiedPlugins.filter(p => p.schema.category === category)
  }, [unifiedPlugins])

  // Filter by type
  const filterByType = useCallback((type: PluginType) => {
    return unifiedPlugins.filter(p => p.schema.type === type)
  }, [unifiedPlugins])

  // Filter by state
  const filterByState = useCallback((state: string) => {
    return unifiedPlugins.filter(p => p.status.state === state)
  }, [unifiedPlugins])

  // Search plugins
  const search = useCallback((query: string) => {
    const q = query.toLowerCase()
    return unifiedPlugins.filter(p =>
      p.schema.name.toLowerCase().includes(q) ||
      p.schema.description.toLowerCase().includes(q) ||
      p.id.toLowerCase().includes(q)
    )
  }, [unifiedPlugins])

  // Toggle enable/disable (start/stop for extensions)
  const toggle = useCallback(async (id: string, enabled: boolean) => {
    const result = enabled
      ? await startExtension(id)
      : await stopExtension(id)
    if (result) {
      await fetchExtensions()
    }
    return result
  }, [startExtension, stopExtension, fetchExtensions])

  // Start plugin
  const start = useCallback(async (id: string) => {
    const result = await startExtension(id)
    if (result) {
      await fetchExtensions()
    }
    return result
  }, [startExtension, fetchExtensions])

  // Stop plugin
  const stop = useCallback(async (id: string) => {
    const result = await stopExtension(id)
    if (result) {
      await fetchExtensions()
    }
    return result
  }, [stopExtension, fetchExtensions])

  // Remove plugin
  const remove = useCallback(async (id: string) => {
    return await unregisterExtension(id)
  }, [unregisterExtension])

  // Configure plugin (execute command with config)
  const configure = useCallback(async (id: string, config: Record<string, unknown>) => {
    const result = await executeExtensionCommand(id, 'update_config', config)
    return result.success
  }, [executeExtensionCommand])

  // Test plugin (execute a test command)
  const test = useCallback(async (id: string) => {
    const result = await executeExtensionCommand(id, 'test', {})
    return result.success
  }, [executeExtensionCommand])

  // Discover new plugins
  const discover = useCallback(async () => {
    const result = await discoverExtensions()
    return {
      discovered: result.discovered,
      message: `Discovered ${result.discovered} new extensions`,
    }
  }, [discoverExtensions])

  // Convert extension stats to plugin stats for return type
  const convertedStats: Record<string, PluginStatsDto> = useMemo(() => {
    const result: Record<string, PluginStatsDto> = {}
    for (const [id, stats] of Object.entries(extensionStats)) {
      result[id] = extensionStatsToPluginStats(stats) || {
        start_count: 0,
        stop_count: 0,
        error_count: 0,
        total_execution_ms: 0,
        avg_response_time_ms: 0,
      }
    }
    return result
  }, [extensionStats])

  return {
    plugins: unifiedPlugins,
    loading: extensionsLoading,
    discovering,
    stats: convertedStats,
    filterByCategory,
    filterByType,
    filterByState,
    search,
    refresh,
    toggle,
    start,
    stop,
    remove,
    configure,
    test,
    discover,
  }
}

// ============================================================================
// Plugin Category Utilities
// ============================================================================

export const PLUGIN_CATEGORIES: Array<{ value: PluginCategory; label: string; icon: string }> = [
  { value: 'all', label: '全部插件', icon: 'Package' },
  { value: 'ai', label: 'AI 后端', icon: 'BrainCircuit' },
  { value: 'devices', label: '设备适配器', icon: 'Network' },
  { value: 'storage', label: '存储', icon: 'Server' },
  { value: 'notify', label: '通知通道', icon: 'Wifi' },
  { value: 'tools', label: '工具', icon: 'Zap' },
]

export const PLUGIN_TYPE_LABELS: Record<PluginType, string> = {
  llm_backend: 'LLM 后端',
  device_adapter: '设备适配器',
  storage_backend: '存储后端',
  alert_channel: '告警通道',
  integration: '集成',
  tool: '工具',
  dynamic: '动态插件',
  native: '内置插件',
}

export function getPluginCategoryColor(category: PluginCategory): string {
  switch (category) {
    case 'ai':
      return 'bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400'
    case 'devices':
      return 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400'
    case 'storage':
      return 'bg-amber-100 text-amber-700 dark:bg-amber-900/20 dark:text-amber-400'
    case 'notify':
      return 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400'
    case 'tools':
      return 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400'
    default:
      return 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-400'
  }
}
