import { MapPin, Server, Zap, Activity, Puzzle, Workflow, Database } from 'lucide-react'

export type CategoryType = 'device-metric' | 'device-command' | 'device' | 'system' | 'extension' | 'extension-command' | 'transform' | 'ai' | 'ai'

// Device info property definitions factory (uses translations)
export function getDeviceInfoProperties(t: (key: string) => string) {
  return [
    { id: 'name', name: t('dataSource.deviceName') },
    { id: 'status', name: t('dataSource.status') },
    { id: 'online', name: t('dataSource.onlineStatus') },
    { id: 'last_seen', name: t('dataSource.lastSeen') },
    { id: 'device_type', name: t('dataSource.deviceType') },
    { id: 'plugin_name', name: t('dataSource.adapter') },
    { id: 'adapter_id', name: t('dataSource.adapterId') },
  ]
}

// System metrics definitions factory (uses translations)
export function getSystemMetrics(t: (key: string) => string) {
  return [
    { id: 'uptime', name: t('systemDataSource.uptime'), description: t('systemDataSource.uptimeDesc'), unit: '', dataType: 'number' as const },
    { id: 'cpu_count', name: t('systemDataSource.cpuCount'), description: t('systemDataSource.cpuCountDesc'), unit: ' cores', dataType: 'number' as const },
    { id: 'total_memory', name: t('systemDataSource.totalMemory'), description: t('systemDataSource.totalMemoryDesc'), unit: ' GB', dataType: 'bytes' as const },
    { id: 'used_memory', name: t('systemDataSource.usedMemory'), description: t('systemDataSource.usedMemoryDesc'), unit: ' GB', dataType: 'bytes' as const },
    { id: 'free_memory', name: t('systemDataSource.freeMemory'), description: t('systemDataSource.freeMemoryDesc'), unit: ' GB', dataType: 'bytes' as const },
    { id: 'available_memory', name: t('systemDataSource.availableMemory'), description: t('systemDataSource.availableMemoryDesc'), unit: ' GB', dataType: 'bytes' as const },
    { id: 'memory_percent', name: t('systemDataSource.memoryPercent'), description: t('systemDataSource.memoryPercentDesc'), unit: '%', dataType: 'number' as const },
    { id: 'platform', name: t('systemDataSource.platform'), description: t('systemDataSource.platformDesc'), unit: '', dataType: 'string' as const },
    { id: 'arch', name: t('systemDataSource.arch'), description: t('systemDataSource.archDesc'), unit: '', dataType: 'string' as const },
    { id: 'version', name: t('systemDataSource.version'), description: t('systemDataSource.versionDesc'), unit: '', dataType: 'string' as const },
  ]
}

// Category configuration factory (uses translations)
export function getCategories(t: (key: string) => string) {
  return [
    { id: 'device' as const, name: t('dataSource.device'), icon: MapPin, description: t('dataSource.deviceDesc') },
    { id: 'device-metric' as const, name: t('dataSource.metrics'), icon: Server, description: t('dataSource.metricsDesc') },
    { id: 'device-command' as const, name: t('dataSource.commands'), icon: Zap, description: t('dataSource.commandsDesc') },
    { id: 'system' as const, name: t('systemDataSource.title'), icon: Activity, description: t('systemDataSource.description') },
    { id: 'extension' as const, name: t('extensions:dataSource.extensionSource') || 'Extension Metrics', icon: Puzzle, description: t('extensions:dataSource.selectExtension') || 'Select extension metrics' },
    { id: 'extension-command' as const, name: t('extensions:dataSource.extensionCommand') || 'Extension Commands', icon: Zap, description: t('extensions:dataSource.selectExtensionCommand') || 'Select extension commands' },
    { id: 'transform' as const, name: t('dataSource.transform'), icon: Workflow, description: t('dataSource.transformDesc') },
    { id: 'ai' as const, name: t('dataSource.aiAgent'), icon: Database, description: t('dataSource.aiAgentDesc') },
  ]
}

// Convert old allowedTypes format to new format
export function normalizeAllowedTypes(
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'device' | 'metric' | 'command' | 'system' | 'extension' | 'extension-command' | 'transform' | 'ai'>
): CategoryType[] {
  if (!allowedTypes) return ['device', 'device-metric', 'device-command', 'system', 'extension', 'extension-command', 'transform', 'ai']

  const result: CategoryType[] = []

  // Device category (for map markers, etc.)
  if (allowedTypes.includes('device')) result.push('device')

  // New format types
  if (allowedTypes.includes('device-metric')) result.push('device-metric')
  if (allowedTypes.includes('device-command')) result.push('device-command')
  if (allowedTypes.includes('system')) result.push('system')
  if (allowedTypes.includes('extension')) result.push('extension')
  if (allowedTypes.includes('extension-command')) result.push('extension-command')
  if (allowedTypes.includes('transform')) result.push('transform')

  // Old format types - map to new format (but not 'device' since it's distinct now)
  if (allowedTypes.includes('metric')) {
    if (!result.includes('device-metric')) result.push('device-metric')
  }
  if (allowedTypes.includes('command')) {
    // When 'command' is specified, include both device-command and extension-command
    if (!result.includes('device-command')) result.push('device-command')
    if (!result.includes('extension-command')) result.push('extension-command')
  }

  return result.length > 0 ? result : ['device', 'device-metric', 'device-command', 'system', 'extension', 'extension-command', 'transform']
}
