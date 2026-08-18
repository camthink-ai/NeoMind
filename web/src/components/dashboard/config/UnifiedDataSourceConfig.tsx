/**
 * Unified DataSource Configuration
 *
 * Simplified data source selector with direct list-based selection.
 * For device-metric and device-command categories, shows device list on left.
 */

import { useState, useMemo, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Search, Check, Server, Zap, Info, X, ChevronRight, Loader2, Database, MapPin, Activity, Puzzle, Workflow } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { textMicro, textNano } from '@/design-system/tokens/typography'
import { useStore } from '@/store'
import type { DataSource, DataSourceOrList, DataSourceMode } from '@/types/dashboard'
import { normalizeDataSource, getUnifiedId, getUnifiedField } from '@/types/dashboard'
import type { MetricDefinition, CommandDefinition } from '@/types'
import { useDataAvailability } from '@/hooks/useDataAvailability'
import { useIsMobile, useSafeAreaInsets } from '@/hooks/useMobile'
import { api } from '@/lib/api'
import type { ExtensionCommandDescriptor, UnifiedDataSourceInfo } from '@/types'
import { findDevice } from '@/lib/deviceUtils'
import { type CategoryType, getDeviceInfoProperties, getSystemMetrics, getCategories, normalizeAllowedTypes } from './categories'
import { MobileItemSelector } from './mobile'
import { ItemBadge, DataIndicator } from './shared'

// ============================================================================
// Types
// ============================================================================

export interface UnifiedDataSourceConfigProps {
  value?: DataSourceOrList
  onChange: (dataSource: DataSourceOrList | undefined) => void
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'device' | 'metric' | 'command' | 'system' | 'extension' | 'extension-command' | 'transform'>
  multiple?: boolean
  maxSources?: number
  className?: string
  disabled?: boolean
  /** Suggested mode for the data source (e.g. 'latest' for LED, 'timeseries' for charts) */
  suggestedMode?: DataSourceMode
}

// ============================================================================
// Constants & Factory Functions
// ============================================================================

const DEFAULT_TIME_RANGE = 1
const DEFAULT_LIMIT = 50
const DEFAULT_AGGREGATE = 'raw' as const
const DEFAULT_REFRESH = 10

function createDeviceLocationDS(deviceId: string): DataSource {
  return { type: 'device', sourceId: deviceId, source: 'device', id: deviceId, field: 'location', mode: 'info' }
}

function createMetricDS(deviceId: string, field: string, suggestedMode?: DataSourceMode): DataSource {
  const mode = suggestedMode ?? 'timeseries'
  return {
    type: 'telemetry', sourceId: deviceId, metricId: field,
    timeRange: DEFAULT_TIME_RANGE, limit: DEFAULT_LIMIT, aggregate: DEFAULT_AGGREGATE,
    params: { includeRawPoints: true }, transform: 'raw',
    source: 'device', id: deviceId, field, mode,
  }
}

function createCommandDS(deviceId: string, command: string): DataSource {
  return { type: 'command', sourceId: deviceId, command, source: 'device', id: deviceId, field: command, mode: 'command' }
}

function createDeviceInfoDS(deviceId: string, property: string): DataSource {
  return { type: 'device-info', sourceId: deviceId, infoProperty: property as any, source: 'device', id: deviceId, field: property, mode: 'info' }
}

function createSystemDS(metric: string): DataSource {
  return { type: 'system', systemMetric: metric as any, refresh: DEFAULT_REFRESH, source: 'system', id: 'neomind', field: metric, mode: 'latest' }
}

function createExtensionMetricDS(extId: string, metricName: string, suggestedMode?: DataSourceMode): DataSource {
  const extMetric = `produce:${metricName}`
  return {
    type: 'extension', extensionId: extId, extensionMetric: extMetric,
    refresh: DEFAULT_REFRESH, timeRange: DEFAULT_TIME_RANGE, limit: DEFAULT_LIMIT, aggregate: DEFAULT_AGGREGATE,
    params: { includeRawPoints: true }, transform: 'raw',
    source: 'extension', id: extId, field: extMetric, mode: (suggestedMode ?? 'timeseries') as DataSourceMode,
  } as any
}

function createExtensionCommandDS(extId: string, command: string): DataSource {
  return {
    type: 'extension-command', extensionId: extId, command, extensionCommand: command,
    source: 'extension', id: extId, field: command, mode: 'command',
  } as any
}

function createTransformDS(transformId: string, field: string): DataSource {
  return {
    type: 'transform', sourceId: `transform:${transformId}`, metricId: field, transformId,
    timeRange: DEFAULT_TIME_RANGE, limit: DEFAULT_LIMIT, aggregate: DEFAULT_AGGREGATE,
    params: { includeRawPoints: true }, transform: 'raw',
    source: 'transform', id: transformId, field, mode: 'timeseries',
  }
}

/** Derive unified source from type for legacy DataSources that lack the `source` field */
function deriveSource(ds: DataSource): string {
  if (ds.source) return ds.source
  switch (ds.type) {
    case 'telemetry': case 'metric': case 'command': case 'device-info': case 'device':
      return 'device'
    case 'system':
      return 'system'
    case 'extension': case 'extension-command':
      return 'extension'
    case 'transform':
      return 'transform'
    default:
      return ''
  }
}

/** Internal identity key for checking if a DataSource is selected. NOT a state format. */
function dsIdentityKey(ds: DataSource): string {
  // Use helper functions with fallback chains for robustness with legacy DataSources
  // that may only have legacy fields (sourceId/metricId) without unified fields (id/field)
  return `${deriveSource(ds)}:${getUnifiedId(ds) ?? ''}:${getUnifiedField(ds) ?? ''}`
}

/** Make identity key from raw components */
function makeKey(source: string, id: string, field: string): string {
  return `${source}:${id}:${field}`
}

// ============================================================================
// Main Component
// ============================================================================

export function UnifiedDataSourceConfig({
  value,
  onChange,
  allowedTypes,
  multiple = false,
  maxSources = 10,
  className,
  suggestedMode,
}: UnifiedDataSourceConfigProps) {
  const { t } = useTranslation('dashboardComponents')
  const devices = useStore((s) => s.devices) ?? []
  const deviceTypes = useStore((s) => s.deviceTypes) ?? []
  const extensions = useStore((s) => s.extensions) ?? []
  const extensionDataSources = useStore((s) => s.extensionDataSources) ?? []
  const extensionsLoading = useStore((s) => s.extensionsLoading)
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<CategoryType>('device-metric')
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null)
  const [selectedExtensionId, setSelectedExtensionId] = useState<string | null>(null)

  // Mobile: full-screen selector state
  const [showMobileSelector, setShowMobileSelector] = useState(false)

  // Unified data sources state (for transform tab — not available in store)
  const [unifiedDataSources, setUnifiedDataSources] = useState<UnifiedDataSourceInfo[]>([])
  const [unifiedSourcesLoading, setUnifiedSourcesLoading] = useState(false)
  const hasFetchedUnifiedSources = useRef(false)

  // Stable ref to store actions to avoid useEffect dependency issues
  const storeActions = useStore((s) => ({
    fetchDevices: s.fetchDevices,
    fetchDeviceTypes: s.fetchDeviceTypes,
    fetchExtensions: s.fetchExtensions,
    extensions: s.extensions,
    extensionDataSources: s.extensionDataSources,
    setExtensionDataSources: s.setExtensionDataSources,
  }))
  const storeRef = useRef(storeActions)
  storeRef.current = storeActions

  // Ensure devices and deviceTypes are loaded when config opens
  // (dashboard page may delay loading, causing empty metrics on first render)
  useEffect(() => {
    if (devices.length === 0) storeRef.current.fetchDevices()
    if (deviceTypes.length === 0) storeRef.current.fetchDeviceTypes()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Data availability checking - now includes summaries with virtual metrics
  const { availability, summaries, loading: checkingData, checkDevice } = useDataAvailability()

  // Initialize selected items from current data source
  const [selectedDataSources, setSelectedDataSources] = useState<DataSource[]>(() =>
    value ? normalizeDataSource(value) : []
  )

  // Derive lookup Set from state for O(1) selection checks
  const selectedKeys = useMemo(
    () => new Set(selectedDataSources.map(dsIdentityKey)),
    [selectedDataSources]
  )

  // Track previous value to detect actual selection changes
  const prevCoreFieldsRef = useRef<string>()
  const prevValueRef = useRef<DataSourceOrList>()

  // Track if we've already restored extension selection from value
  const hasRestoredExtensionSelection = useRef(false)

  // Extract core identifying fields for comparison (ignores transform settings)
  const getCoreFields = (ds: DataSourceOrList | undefined): string => {
    if (!ds) return ''
    const sources = normalizeDataSource(ds)
    return sources.map(dsIdentityKey).sort().join('|')
  }

  // Calculate current core fields
  const currentCoreFields = getCoreFields(value)

  useEffect(() => {
    // Initialize on first render
    if (prevValueRef.current === undefined) {
      prevValueRef.current = value
      prevCoreFieldsRef.current = currentCoreFields
      return
    }

    // Only update if core fields actually changed (selection changed)
    // Transform settings like timeRange, aggregate, etc. should NOT trigger reset
    if (prevCoreFieldsRef.current !== currentCoreFields) {
      prevValueRef.current = value
      prevCoreFieldsRef.current = currentCoreFields
      setSelectedDataSources(value ? normalizeDataSource(value) : [])
    }
    // If only transform settings changed, update the value ref but don't reset selection
    else if (prevCoreFieldsRef.current === currentCoreFields && value !== prevValueRef.current) {
      prevValueRef.current = value
    }
  }, [value, currentCoreFields])

  // Helper for checking selection in render code
  const isSelected = (source: string, id: string, field: string) => selectedKeys.has(makeKey(source, id, field))

  // Available categories based on allowedTypes
  const availableCategories = useMemo(
    () => getCategories(t).filter(c => (normalizeAllowedTypes(allowedTypes) as CategoryType[]).includes(c.id)),
    [allowedTypes, t]
  )

  // Set initial category to first available (useEffect instead of useMemo to avoid setState during render)
  useEffect(() => {
    if (availableCategories.length > 0 && !availableCategories.find(c => c.id === selectedCategory)) {
      setSelectedCategory(availableCategories[0]!.id)
      setSelectedDeviceId(null)
    }
  }, [availableCategories, selectedCategory])

  // Build device metrics map
  const deviceMetricsMap = useMemo(() => {
    const map = new Map<string, MetricDefinition[]>()
    for (const device of devices) {
      const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)

      if (deviceType?.metrics && deviceType.metrics.length > 0) {
        map.set(device.id, deviceType.metrics)
      } else {
        // Fallback: use current_values
        if (device.current_values && typeof device.current_values === 'object') {
          const dynamicMetrics: MetricDefinition[] = Object.keys(device.current_values).map(key => ({
            name: key,
            display_name: key,
            data_type: 'float' as const,
            unit: '',
          }))
          map.set(device.id, dynamicMetrics)
        } else {
          map.set(device.id, [{ name: 'value', display_name: t('chart.value'), data_type: 'float' as const, unit: '' }])
        }
      }
    }
    return map
  }, [devices, deviceTypes, t])

  // Fetch extension + unified data sources when needed
  useEffect(() => {
    const needsExt = availableCategories.some(c => c.id === 'extension' || c.id === 'extension-command')
    const needsUnified = availableCategories.some(c => c.id === 'transform')
    if (!needsExt && !needsUnified) {
      hasFetchedUnifiedSources.current = false
      return
    }

    // Skip if all needed data already loaded
    const extAlreadyLoaded = !needsExt || (storeRef.current.extensions.length > 0 && storeRef.current.extensionDataSources.length > 0)
    const unifiedAlreadyLoaded = !needsUnified || hasFetchedUnifiedSources.current
    if (extAlreadyLoaded && unifiedAlreadyLoaded) return

    let cancelled = false
    const fetchData = async () => {
      setUnifiedSourcesLoading(true)

      try {
        const promises: Promise<unknown>[] = []

        // Fetch extensions list via store (separate endpoint)
        if (needsExt && !extAlreadyLoaded) {
          promises.push(storeRef.current.fetchExtensions())
        }

        // Single unified API call for all data source types (extension metrics, transforms, AI)
        if (!unifiedAlreadyLoaded) {
          promises.push(
            api.listUnifiedDataSources({ limit: 500, skip_telemetry: 'true' })
              .then((result) => {
                if (cancelled) return
                const sources = (result as { data?: UnifiedDataSourceInfo[] })?.data ?? []
                setUnifiedDataSources(sources)
                hasFetchedUnifiedSources.current = true

                // Feed extension data into store if needed (same data, no extra request)
                if (needsExt && !extAlreadyLoaded) {
                  const extSources = sources
                    .filter(s => s.source_type === 'extension')
                    .map(ds => ({
                      id: ds.id,
                      extension_id: ds.source_name,
                      command: '',
                      field: ds.field,
                      display_name: ds.source_display_name + ': ' + ds.field_display_name,
                      data_type: (ds.data_type as any) || 'float',
                      unit: ds.unit,
                      description: ds.description || ds.field_display_name,
                      aggregatable: true,
                      default_agg_func: 'last' as const,
                    }))
                  storeRef.current.setExtensionDataSources(extSources)
                }
              })
          )
        }

        await Promise.all(promises)
      } finally {
        if (!cancelled) setUnifiedSourcesLoading(false)
      }
    }
    fetchData()
    return () => { cancelled = true }
  }, [availableCategories])

  // Computed lists for transform sources
  const transformSources = useMemo(() =>
    unifiedDataSources.filter(s => s.source_type === 'transform'),
    [unifiedDataSources]
  )

  // Restore extension selection from saved value (only runs once after extensions are loaded)
  useEffect(() => {
    // Skip if already restored or no extensions loaded yet
    if (hasRestoredExtensionSelection.current || extensions.length === 0) return

    if (!value) {
      hasRestoredExtensionSelection.current = true
      return
    }

    const dataSources = normalizeDataSource(value)

    // Find first extension or extension-command data source
    const extensionSource = dataSources.find(ds =>
      ds.type === 'extension' || ds.type === 'extension-command'
    ) as any

    if (extensionSource?.extensionId) {
      // Check if this extension exists in the loaded extensions
      const extensionExists = extensions.some(e => e.id === extensionSource.extensionId)

      if (extensionExists) {
        // Restore the selected extension
        setSelectedExtensionId(extensionSource.extensionId)

        // Also set the correct category based on the data source type
        if (extensionSource.type === 'extension-command') {
          setSelectedCategory('extension-command')
        } else if (extensionSource.type === 'extension') {
          setSelectedCategory('extension')
        }
      }
    }

    // Mark as restored regardless of whether we found an extension
    hasRestoredExtensionSelection.current = true
  }, [value, extensions])

  // Build extension metrics map from store data
  const extensionMetricsMap = useMemo(() => {
    const map = new Map<string, Array<{ name: string; display_name: string; data_type: string; unit?: string }>>()
    extensionDataSources.forEach(ds => {
      if (!map.has(ds.extension_id)) {
        map.set(ds.extension_id, [])
      }
      map.get(ds.extension_id)!.push({
        name: ds.field,
        display_name: ds.display_name,
        data_type: ds.data_type,
        unit: ds.unit,
      })
    })
    return map
  }, [extensionDataSources])

  // Check data availability when device is selected (for metrics category)
  useEffect(() => {
    if (selectedCategory === 'device-metric' && selectedDeviceId) {
      const metrics = deviceMetricsMap.get(selectedDeviceId) || []
      const metricIds = metrics.map(m => m.name)
      if (metricIds.length > 0) {
        checkDevice(selectedDeviceId, metricIds)
      }
    }
  }, [selectedCategory, selectedDeviceId, deviceMetricsMap, checkDevice])

  // Build device commands map - only show commands that are actually defined in the device template
  const deviceCommandsMap = useMemo(() => {
    const map = new Map<string, CommandDefinition[]>()
    for (const device of devices) {
      const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)

      // Only include commands that are actually defined in the device type
      // No fallback commands - this prevents users from selecting commands that don't exist
      if (deviceType?.commands && deviceType.commands.length > 0) {
        map.set(device.id, deviceType.commands)
      } else {
        // No commands defined - return empty array
        map.set(device.id, [])
      }
    }
    return map
  }, [devices, deviceTypes])

  // Handle item selection - takes a DataSource object directly
  const handleSelectItem = (ds: DataSource) => {
    const key = dsIdentityKey(ds)
    setSelectedDataSources(prev => {
      if (multiple) {
        if (prev.some(d => dsIdentityKey(d) === key)) {
          return prev.filter(d => dsIdentityKey(d) !== key)
        } else if (prev.length < maxSources) {
          return [...prev, ds]
        }
        return prev
      } else {
        if (prev.some(d => dsIdentityKey(d) === key)) {
          return []
        }
        return [ds]
      }
    })
  }

  // Clear all selections
  const handleClearSelection = () => setSelectedDataSources([])

  // Notify parent of data source changes via effect (avoids setState-during-render warning)
  useEffect(() => {
    if (selectedDataSources.length === 0) {
      onChange(undefined)
    } else if (multiple) {
      onChange(selectedDataSources)
    } else {
      onChange(selectedDataSources[0])
    }
  }, [selectedDataSources, multiple])

  // Filter devices by search query
  const filteredDevices = useMemo(() => {
    if (!searchQuery) return devices

    const query = searchQuery.toLowerCase()
    return devices.filter(d =>
      d.id.toLowerCase().includes(query) ||
      (d.name && d.name.toLowerCase().includes(query))
    )
  }, [devices, searchQuery])

  // selectedDataSources is already an array for display

  // Check if category uses split layout
  const usesDeviceSplitLayout = selectedCategory === 'device-metric' || selectedCategory === 'device-command'
  const usesExtensionSplitLayout = selectedCategory === 'extension' || selectedCategory === 'extension-command'

  // Get selected device/extension
  const selectedDevice = findDevice(devices, selectedDeviceId ?? undefined)
  const selectedExtension = extensions.find(e => e.id === selectedExtensionId)

  // Filter extensions by search query
  const filteredExtensions = useMemo(() => {
    if (!searchQuery) return extensions
    return extensions.filter(e =>
      e.id.toLowerCase().includes(searchQuery) ||
      (e.name && e.name.toLowerCase().includes(searchQuery))
    )
  }, [extensions, searchQuery])

  // Render device list for split layout
  const renderDeviceList = () => {
    return (
      <div className="flex flex-col h-full">
        {/* Search input inside device list */}
        <div className="p-2 border-b">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder={t('dataSource.searchPlaceholder')}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 pl-8 text-xs"
            />
          </div>
        </div>

        {/* Device list header with count */}
        <div className="px-3 py-1.5 border-b text-xs font-medium text-muted-foreground bg-muted-30 flex items-center justify-between">
          <span>{t('dataSource.deviceList')}</span>
          <span className={cn(textNano, "text-muted-foreground bg-muted-50 px-1.5 py-0.5 rounded")}>
            {filteredDevices.length} {t('dataSource.count')}
          </span>
        </div>

        {/* Device list */}
        {filteredDevices.length === 0 ? (
          <div className="p-4 text-center text-muted-foreground text-xs">{t('dataSource.noDevices')}</div>
        ) : (
          <div className="flex-1 overflow-y-auto">
            {filteredDevices.map(device => {
              const isDeviceRowSelected = selectedDeviceId === device.id
              // Count selected items for this device
              const deviceSourcePrefix = selectedCategory === 'device-metric' || selectedCategory === 'device-command' ? 'device' : 'device'
              const selectedCount = selectedDataSources.filter(ds =>
                dsIdentityKey(ds).startsWith(`${deviceSourcePrefix}:${device.id}:`)
              ).length

              // Get available items count (metrics or commands)
              let availableCount = 0
              if (selectedCategory === 'device-metric') {
                const metrics = deviceMetricsMap.get(device.id) || []
                availableCount = metrics.length
              } else if (selectedCategory === 'device-command') {
                const commands = deviceCommandsMap.get(device.id) || []
                availableCount = commands.length
              }

              return (
                <button
                  key={device.id}
                  type="button"
                  onClick={() => {
                    if (isMobile) {
                      setShowMobileSelector(true)
                    }
                    setSelectedDeviceId(device.id)
                  }}
                  className={cn(
                    'w-full flex items-center gap-2 px-3 py-2 text-left border-b transition-all duration-150',
                    isDeviceRowSelected
                      ? 'bg-muted border-l-2 border-l-primary'
                      : 'bg-transparent border-l-2 border-l-transparent hover:bg-muted'
                  )}
                >
                  <div className="flex-1 min-w-0">
                    <div className={cn(
                      'text-sm truncate',
                      isDeviceRowSelected ? 'font-medium text-foreground' : 'font-normal text-foreground'
                    )}>{device.name || device.id}</div>
                    <div className={cn(textNano, "text-muted-foreground truncate flex items-center gap-1.5")}>
                      <span>{device.device_type}</span>
                      {availableCount > 0 && (
                        <>
                          <span className="text-muted-foreground">•</span>
                          <span className="text-muted-foreground">
                            {selectedCategory === 'device-metric' ? `${availableCount} ${t('dataSource.metricsCount')}` : `${availableCount} ${t('dataSource.commandsCount')}`}
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                  {isMobile && (
                    <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0" />
                  )}
                  {!isMobile && selectedCount > 0 && (
                    <span className={cn("shrink-0 min-w-[18px] h-[18px] flex items-center justify-center", textNano, "font-medium bg-primary text-primary-foreground rounded-sm")}>
                      {selectedCount}
                    </span>
                  )}
                </button>
              )
            })}
          </div>
        )}
      </div>
    )
  }

  // Render metrics/commands for selected device
  const renderDeviceContent = () => {
    if (!selectedDevice) {
      return (
        <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
          {t('dataSource.selectDevice')}
        </div>
      )
    }

    if (selectedCategory === 'device-metric') {
      const metrics = deviceMetricsMap.get(selectedDevice.id) || []
      const deviceSummary = summaries.get(selectedDevice.id) || {}

      // Track template metric names to identify virtual metrics
      const templateMetricNames = new Set(metrics.map(m => m.name))

      type Item = {
        key: string
        propertyName: string
        propertyDisplayName: string
        currentValue?: unknown
        isSelected: boolean
        hasData: boolean | null
        dataPointCount?: number
        itemType: 'template' | 'virtual' | 'info'
        unit?: string
        dataType?: string
      }

      const items: Item[] = []

      // Helper to resolve dot-notation path in nested objects (e.g., "values.devName" → current_values.values.devName)
      const resolvePath = (obj: unknown, path: string): unknown => {
        if (!obj || typeof obj !== 'object') return undefined
        let current: unknown = obj
        for (const part of path.split('.')) {
          if (current && typeof current === 'object' && part in (current as Record<string, unknown>)) {
            current = (current as Record<string, unknown>)[part]
          } else {
            return undefined
          }
        }
        return current
      }

      // Add template metrics
      for (const metric of metrics) {
        const availabilityKey = `${selectedDevice.id}:${metric.name}`
        const metricAvailability = availability.get(availabilityKey)
        items.push({
          key: `device:${selectedDevice.id}:${metric.name}`,
          propertyName: metric.name,
          propertyDisplayName: metric.display_name || metric.name,
          currentValue: resolvePath(selectedDevice.current_values, metric.name),
          isSelected: isSelected('device', selectedDevice.id, metric.name),
          hasData: metricAvailability?.hasData ?? null,
          dataPointCount: metricAvailability?.dataPointCount,
          itemType: 'template',
          unit: metric.unit,
          dataType: metric.data_type,
        })
      }

      // Add virtual metrics from summary (not in template)
      for (const [metricId, metricSummary] of Object.entries(deviceSummary)) {
        if (!templateMetricNames.has(metricId) && metricSummary.is_virtual) {
          const availabilityKey = `${selectedDevice.id}:${metricId}`
          const metricAvailability = availability.get(availabilityKey)
          items.push({
            key: `device:${selectedDevice.id}:${metricId}`,
            propertyName: metricId,
            propertyDisplayName: metricSummary.display_name || metricId,
            currentValue: metricSummary.current,
            isSelected: isSelected('device', selectedDevice.id, metricId),
            hasData: metricAvailability?.hasData ?? null,
            dataPointCount: metricAvailability?.dataPointCount,
            itemType: 'virtual',
            unit: metricSummary.unit,
            dataType: metricSummary.data_type,
          })
        }
      }

      // Add device info properties
      for (const infoProp of getDeviceInfoProperties(t)) {
        const itemKey = `device:${selectedDevice.id}:${infoProp.id}`
        let currentValue: unknown = undefined

        // Get current value from device
        switch (infoProp.id) {
          case 'name':
            currentValue = selectedDevice.name
            break
          case 'status':
            currentValue = selectedDevice.status
            break
          case 'online':
            currentValue = selectedDevice.online
            break
          case 'last_seen':
            currentValue = selectedDevice.last_seen
            break
          case 'device_type':
            currentValue = selectedDevice.device_type
            break
          case 'plugin_name':
            currentValue = selectedDevice.plugin_name
            break
          case 'adapter_id':
            currentValue = selectedDevice.adapter_id
            break
        }

        items.push({
          key: itemKey,
          propertyName: infoProp.id,
          propertyDisplayName: infoProp.name,
          currentValue,
          isSelected: isSelected('device', selectedDevice.id, infoProp.id),
          hasData: null, // Info props don't have historical data
          itemType: 'info',
        })
      }

      // Sort items: template metrics first, then device info, then virtual metrics
      items.sort((a, b) => {
        const order = { template: 0, info: 1, virtual: 2 }
        return order[a.itemType] - order[b.itemType]
      })

      if (items.length === 0) {
        return <div className="p-4 text-center text-muted-foreground text-sm">{t('dataSource.noAvailableMetrics')}</div>
      }

      // Helper to format current value
      const formatValue = (val: unknown): string => {
        if (val === null || val === undefined) return '-'
        if (typeof val === 'number') return val.toLocaleString('en-US', { maximumFractionDigits: 2 })
        if (typeof val === 'boolean') return val ? t('dataSource.yes') : t('dataSource.no')
        return String(val)
      }


      return (
        <div className="flex flex-col h-full">
          <div className="px-3 py-2.5 border-b text-xs font-medium text-muted-foreground bg-muted-30 flex items-center justify-between">
            <span className="flex items-center gap-1.5">
              <Database className="h-4 w-4" />
              {t('dataSource.metricsOf', { device: selectedDevice.name || selectedDevice.id })}
            </span>
            <span className={cn(textNano, "text-muted-foreground bg-muted-50 px-1.5 py-0.5 rounded")}>
              {items.length} {t('dataSource.count')}
            </span>
          </div>
          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {items.map(item => {
              const [_source, deviceId, ...fieldParts] = item.key.split(':')
              const field = fieldParts.join(':')
              return (
              <button
                key={item.key}
                type="button"
                onClick={() => {
                  const ds = item.itemType === 'info'
                    ? createDeviceInfoDS(selectedDevice.id, field)
                    : createMetricDS(selectedDevice.id, field, suggestedMode)
                  handleSelectItem(ds)
                }}
                className={cn(
                  'w-full text-left transition-colors duration-150',
                  'group relative rounded-md border',
                  item.isSelected
                    ? 'bg-muted border-border'
                    : 'bg-card border-border hover:bg-accent hover:border-border'
                )}
              >
                <div className="flex items-center gap-2 p-2">
                  {/* Check icon - positioned on left */}
                  <div className={cn(
                    'shrink-0 w-4 h-4 rounded-sm flex items-center justify-center transition-colors',
                    item.isSelected
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground group-hover:bg-muted'
                  )}>
                    <Check className={cn(
                      'h-4 w-4',
                      item.isSelected ? 'opacity-100' : 'opacity-0'
                    )} />
                  </div>

                  {/* Main content */}
                  <div className="flex-1 min-w-0 space-y-0.5">
                    {/* Header row */}
                    <div className="flex items-center gap-1.5">
                      <ItemBadge itemType={item.itemType} t={t} />
                      <span className={cn(
                        'text-sm truncate',
                        item.isSelected ? 'font-medium text-foreground' : 'font-normal text-foreground'
                      )}>{item.propertyDisplayName}</span>
                      <div className="flex-1" />
                      {item.hasData !== null && (
                        <DataIndicator hasData={item.hasData} count={item.dataPointCount} t={t} />
                      )}
                      {item.hasData === null && checkingData && (
                        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                      )}
                    </div>

                    {/* Subtitle row */}
                    <div className={cn("flex items-center gap-1.5", textNano, "text-muted-foreground")}>
                      <code className={cn("px-1 py-0.5 bg-muted rounded", textMicro, "font-mono")}>
                        {item.propertyName}
                      </code>
                      {item.currentValue !== undefined && item.currentValue !== null && (
                        <>
                          <span className="text-muted-foreground">·</span>
                          <span className="text-foreground truncate max-w-[120px] inline-block align-bottom overflow-hidden" title={formatValue(item.currentValue)}>{t('dataSource.current')}: {formatValue(item.currentValue)}</span>
                          {item.unit && item.unit !== '-' && (
                            <>
                              <span className="text-muted-foreground">·</span>
                              <span className="text-muted-foreground">{item.unit}</span>
                            </>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                </div>
              </button>
              )
            })}
          </div>
        </div>
      )
    }

    if (selectedCategory === 'device-command') {
      const commands = deviceCommandsMap.get(selectedDevice.id) || []
      const items: Array<{
        key: string
        commandName: string
        commandDisplayName: string
        isSelected: boolean
      }> = []

      for (const cmd of commands) {
        items.push({
          key: `device:${selectedDevice.id}:${cmd.name}`,
          commandName: cmd.name,
          commandDisplayName: cmd.display_name || cmd.name,
          isSelected: isSelected('device', selectedDevice.id, cmd.name),
        })
      }

      if (items.length === 0) {
        return <div className="p-4 text-center text-muted-foreground text-sm">{t('dataSource.noAvailableCommands')}</div>
      }

      return (
        <div className="flex flex-col h-full">
          <div className="px-3 py-2 border-b text-xs font-medium text-muted-foreground bg-muted-30">
            {t('dataSource.commandsOf', { device: selectedDevice.name || selectedDevice.id })}
          </div>
          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {items.map(item => (
              <button
                key={item.key}
                type="button"
                onClick={() => {
                  const [_src, devId, ...cmdParts] = item.key.split(':')
                  handleSelectItem(createCommandDS(selectedDevice.id, cmdParts.join(':')))
                }}
                className={cn(
                  'w-full flex items-center justify-between p-3 rounded-lg border text-left transition-colors',
                  'hover:bg-accent',
                  item.isSelected
                    ? 'border-primary bg-muted'
                    : 'border-muted'
                )}
              >
                <div className="flex-1 min-w-0">
                  <div className="font-medium text-sm truncate">{item.commandDisplayName}</div>
                </div>
                {item.isSelected && (
                  <Check className="h-4 w-4 text-primary shrink-0 ml-2" />
                )}
              </button>
            ))}
          </div>
        </div>
      )
    }

    return null
  }

  // Render extension list for split layout (left sidebar)
  const renderExtensionList = () => {
    return (
      <div className="flex flex-col h-full">
        {/* Search input inside extension list */}
        <div className="p-2 border-b">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder={t('dataSource.searchPlaceholder')}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 pl-8 text-xs"
            />
          </div>
        </div>

        {/* Extension list header with count */}
        <div className="px-3 py-1.5 border-b text-xs font-medium text-muted-foreground bg-muted-30 flex items-center justify-between">
          <span>{t('extensions:extensionList') || 'Extensions'}</span>
          <span className={cn(textNano, "text-muted-foreground bg-muted-50 px-1.5 py-0.5 rounded")}>
            {filteredExtensions.length} {t('dataSource.count')}
          </span>
        </div>

        {/* Extension list */}
        <div className="flex-1 overflow-y-auto">
          {extensionsLoading ? (
            <div className="p-4 text-center text-muted-foreground text-sm flex items-center justify-center gap-2">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t('extensions:loading') || 'Loading...'}
            </div>
          ) : filteredExtensions.length === 0 ? (
            <div className="p-4 text-center text-muted-foreground text-sm">
              {t('extensions:noExtensions') || 'No extensions'}
            </div>
          ) : (
            filteredExtensions.map(ext => {
              const isSelected = selectedExtensionId === ext.id
              return (
                <button
                  key={ext.id}
                  type="button"
                  onClick={() => setSelectedExtensionId(ext.id)}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 border-b text-left transition-colors',
                    isSelected
                      ? 'bg-muted border-border'
                      : 'hover:bg-muted-30 border-transparent'
                  )}
                >
                  <Puzzle className={cn(
                    'h-4 w-4 shrink-0',
                    isSelected ? 'text-primary' : 'text-muted-foreground'
                  )} />
                  <div className="flex-1 min-w-0">
                    <div className={cn(
                      'text-sm truncate',
                      isSelected ? 'font-medium text-foreground' : 'text-foreground'
                    )}>
                      {ext.name}
                    </div>
                    <div className={cn(textNano, "text-muted-foreground truncate")}>
                      {ext.version && `v${ext.version}`}
                      {ext.description && ` · ${ext.description}`}
                    </div>
                  </div>
                  {isSelected && (
                    <Check className="h-4 w-4 text-primary shrink-0" />
                  )}
                </button>
              )
            })
          )}
        </div>
      </div>
    )
  }

  // Render extension content for split layout (right side)
  const renderExtensionContent = () => {
    if (!selectedExtension) {
      return (
        <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
          {t('extensions:selectExtension') || 'Select an extension'}
        </div>
      )
    }

    if (selectedCategory === 'extension') {
      // Show extension metrics
      const metrics = extensionMetricsMap.get(selectedExtension.id) || []

      // Show loading spinner while data sources are being fetched
      if (metrics.length === 0 && extensionsLoading) {
        return (
          <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm gap-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t('extensions:loading') || 'Loading...'}
          </div>
        )
      }

      if (metrics.length === 0) {
        return (
          <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
            {t('extensions:noMetrics') || 'No metrics available'}
          </div>
        )
      }

      return (
        <div className="flex flex-col h-full">
          <div className="px-3 py-2 border-b text-xs font-medium text-muted-foreground bg-muted-30">
            {selectedExtension.name} · {t('dataSource.metrics') || 'Metrics'}
          </div>
          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {metrics.map(metric => {
              const metricIsSelected = isSelected('extension', selectedExtension.id, `produce:${metric.name}`)

              return (
                <button
                  key={metric.name}
                  type="button"
                  onClick={() => handleSelectItem(createExtensionMetricDS(selectedExtension.id, metric.name, suggestedMode))}
                  className={cn(
                    'w-full flex items-center justify-between p-3 rounded-lg border text-left transition-colors',
                    'hover:bg-accent',
                    metricIsSelected
                      ? 'border-primary bg-muted'
                      : 'border-muted'
                  )}
                >
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-sm truncate">{metric.display_name || metric.name}</div>
                    <div className="text-xs text-muted-foreground truncate">{metric.name}</div>
                  </div>
                  {metricIsSelected && (
                    <Check className="h-4 w-4 text-primary shrink-0 ml-2" />
                  )}
                </button>
              )
            })}
          </div>
        </div>
      )
    }

    if (selectedCategory === 'extension-command') {
      // Show extension commands
      const commands = selectedExtension.commands || []

      if (commands.length === 0) {
        return (
          <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
            {t('extensions:noCommands') || 'No commands available'}
          </div>
        )
      }

      return (
        <div className="flex flex-col h-full">
          <div className="px-3 py-2 border-b text-xs font-medium text-muted-foreground bg-muted-30">
            {selectedExtension.name} · {t('dataSource.commands') || 'Commands'}
          </div>
          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {commands.map((cmd: ExtensionCommandDescriptor) => {
              const cmdIsSelected = isSelected('extension', selectedExtension.id, cmd.id)

              return (
                <button
                  key={cmd.id}
                  type="button"
                  onClick={() => handleSelectItem(createExtensionCommandDS(selectedExtension.id, cmd.id))}
                  className={cn(
                    'w-full flex items-center justify-between p-3 rounded-lg border text-left transition-colors',
                    'hover:bg-accent',
                    cmdIsSelected
                      ? 'border-primary bg-muted'
                      : 'border-muted'
                  )}
                >
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-sm truncate">{cmd.display_name || cmd.id}</div>
                    <div className="text-xs text-muted-foreground truncate">{cmd.description || cmd.id}</div>
                  </div>
                  <Zap className={cn(
                    'h-4 w-4 shrink-0 ml-2',
                    cmdIsSelected ? 'text-warning' : 'text-muted-foreground'
                  )} />
                </button>
              )
            })}
          </div>
        </div>
      )
    }

    return null
  }

  // Render content based on category
  const renderCategoryContent = () => {
    switch (selectedCategory) {
      case 'device-metric':
      case 'device-command':
      case 'extension':
      case 'extension-command':
        // Split layout handled separately
        return null

      case 'device':
        // Device category - show device list directly for selection (for map markers)
        return (
          <div className="space-y-1">
            {filteredDevices.length === 0 ? (
              <div className="p-4 text-center text-muted-foreground text-sm">{t('dataSource.noDevices')}</div>
            ) : (
              filteredDevices.map(device => {
                const devIsSelected = isSelected('device', device.id, 'location')

                return (
                  <button
                    key={device.id}
                    type="button"
                    onClick={() => handleSelectItem(createDeviceLocationDS(device.id))}
                    className={cn(
                      'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg border text-left transition-all duration-150',
                      devIsSelected
                        ? 'bg-muted border-border'
                        : 'bg-card border-border hover:bg-accent hover:border-border'
                    )}
                  >
                    {/* Check icon */}
                    <div className={cn(
                      'shrink-0 w-5 h-5 rounded-md flex items-center justify-center transition-colors',
                      devIsSelected
                        ? 'bg-primary text-primary-foreground'
                        : 'bg-muted text-muted-foreground'
                    )}>
                      <Check className={cn(
                        'h-4 w-4',
                        devIsSelected ? 'opacity-100' : 'opacity-0'
                      )} />
                    </div>

                    {/* Device info */}
                    <div className="flex-1 min-w-0">
                      <div className={cn(
                        'text-sm truncate',
                        devIsSelected ? 'font-medium text-foreground' : 'font-normal text-foreground'
                      )}>
                        {device.name || device.id}
                      </div>
                      <div className={cn(textNano, "text-muted-foreground truncate")}>
                        {device.device_type}
                      </div>
                    </div>

                    {/* Status indicator */}
                    <div className={cn(
                      'w-2 h-2 rounded-full',
                      device.online ? 'bg-success' : 'bg-muted-foreground'
                    )} />
                  </button>
                )
              })
            )}
          </div>
        )

      case 'system':
        // System metrics - show system metrics list
        return (
          <div className="space-y-1">
            {getSystemMetrics(t).map(metric => {
              const metricIsSelected = isSelected('system', 'neomind', metric.id)

              return (
                <button
                  key={metric.id}
                  type="button"
                  onClick={() => handleSelectItem(createSystemDS(metric.id))}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg border text-left transition-all duration-150',
                    metricIsSelected
                      ? 'bg-muted border-border'
                      : 'bg-card border-border hover:bg-accent hover:border-border'
                  )}
                >
                  {/* Check icon */}
                  <div className={cn(
                    'shrink-0 w-5 h-5 rounded-md flex items-center justify-center transition-colors',
                    metricIsSelected
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground'
                  )}>
                    <Check className={cn(
                      'h-4 w-4',
                      metricIsSelected ? 'opacity-100' : 'opacity-0'
                    )} />
                  </div>

                  {/* Metric info */}
                  <div className="flex-1 min-w-0">
                    <div className={cn(
                      'text-sm truncate',
                      metricIsSelected ? 'font-medium text-foreground' : 'font-normal text-foreground'
                    )}>
                      {metric.name}
                    </div>
                    <div className={cn(textNano, "text-muted-foreground truncate")}>
                      {metric.description}
                    </div>
                  </div>

                  {/* Data type indicator */}
                  <div className={cn("shrink-0", textNano, "px-1.5 py-0.5 rounded bg-muted text-muted-foreground")}>
                    {metric.unit || '—'}
                  </div>
                </button>
              )
            })}
          </div>
        )

      case 'transform':
        // Transform data sources - flat list layout
        return (
          <div className="space-y-1">
            {unifiedSourcesLoading ? (
              <div className="p-4 text-center text-muted-foreground text-sm flex items-center justify-center gap-2">
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading...
              </div>
            ) : transformSources.length === 0 ? (
              <div className="p-4 text-center text-muted-foreground text-sm">{t('dataSource.noTransforms')}</div>
            ) : (
              transformSources.map(source => {
                const tfIsSelected = isSelected('transform', source.source_name, source.field)
                return (
                  <button
                    key={source.id}
                    type="button"
                    onClick={() => handleSelectItem(createTransformDS(source.source_name, source.field))}
                    className={cn(
                      'w-full flex items-center gap-3 px-3 py-2.5 rounded-lg border text-left transition-all duration-150',
                      tfIsSelected
                        ? 'bg-muted border-border'
                        : 'bg-card border-border hover:bg-accent hover:border-border'
                    )}
                  >
                    <div className={cn(
                      'shrink-0 w-5 h-5 rounded-md flex items-center justify-center transition-colors',
                      tfIsSelected
                        ? 'bg-primary text-primary-foreground'
                        : 'bg-muted text-muted-foreground'
                    )}>
                      <Check className={cn(
                        'h-4 w-4',
                        tfIsSelected ? 'opacity-100' : 'opacity-0'
                      )} />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className={cn(
                        'text-sm truncate',
                        tfIsSelected ? 'font-medium text-foreground' : 'font-normal text-foreground'
                      )}>
                        {source.source_display_name} · {source.field_display_name}
                      </div>
                      <div className={cn(textNano, "text-muted-foreground truncate flex items-center gap-1.5")}>
                        <code className={cn("px-1 py-0.5 bg-muted rounded", textMicro, "font-mono")}>
                          {source.source_name}:{source.field}
                        </code>
                        {source.unit && source.unit !== '-' && (
                          <>
                            <span className="text-muted-foreground">·</span>
                            <span className="text-muted-foreground">{source.unit}</span>
                          </>
                        )}
                        {source.current_value !== undefined && source.current_value !== null && (
                          <>
                            <span className="text-muted-foreground">·</span>
                            <span className="text-foreground">{String(source.current_value)}</span>
                          </>
                        )}
                      </div>
                    </div>
                    <span className={cn("shrink-0", textNano, "px-1.5 py-0.5 rounded bg-accent-indigo-light text-accent-indigo border border-accent-indigo-light")}>
                      {t('dataSource.transform')}
                    </span>
                  </button>
                )
              })
            )}
          </div>
        )

      default:
        return null
    }
  }

  return (
    <>
      <div className={cn('flex flex-col h-full', className)}>
        {/* Selected items bar - compact single row */}
      {selectedDataSources.length > 0 && (
        <div className="px-3 py-2 border-b flex flex-wrap gap-2 items-center">
          <div className="flex items-center gap-1.5 text-xs font-medium text-primary">
            <Check className="h-4 w-4" />
            {t('dataSource.selectedItems', { count: selectedDataSources.length })}
          </div>
          <div className="h-4 w-px bg-border" />
          <div className="flex flex-wrap gap-1.5 flex-1 min-w-0">
            {selectedDataSources.slice(0, 3).map(ds => {
              const source = ds.source ?? ''
              const entityId = ds.id ?? ''
              const field = ds.field ?? ''

              // Resolve entity name
              let entityName = entityId
              if (source === 'extension') {
                const ext = extensions.find(e => e.id === entityId)
                entityName = ext?.name || entityId
              } else {
                const device = findDevice(devices, entityId)
                entityName = device?.name || entityId
              }

              // Icon and color based on DataSource type
              let TypeIcon = Info
              let iconColor = 'text-accent-emerald'
              let displayLabel = field
              let showSeparator = true

              if (ds.type === 'telemetry') {
                TypeIcon = Server
                iconColor = 'text-info'
              } else if (ds.type === 'command') {
                TypeIcon = Zap
                iconColor = 'text-warning'
              } else if (ds.type === 'device') {
                TypeIcon = MapPin
                iconColor = 'text-accent-purple'
                displayLabel = ''  // No label for device type, just device name
                showSeparator = false
              } else if (ds.type === 'extension') {
                TypeIcon = Puzzle
                iconColor = 'text-accent-cyan'
              } else if (ds.type === 'extension-command') {
                TypeIcon = Zap
                iconColor = 'text-accent-orange'
              } else if (ds.type === 'transform') {
                TypeIcon = Workflow
                iconColor = 'text-accent-indigo'
              } else if (ds.type === 'device-info') {
                TypeIcon = Server
                iconColor = 'text-info'
              } else if (ds.type === 'system') {
                TypeIcon = Activity
                iconColor = 'text-accent-emerald'
              }

              return (
                <div
                  key={dsIdentityKey(ds)}
                  className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-background border border-border text-xs group hover:border-border transition-all max-w-[140px]"
                >
                  <TypeIcon className={cn('h-4 w-4 shrink-0', iconColor)} />
                  <span className="max-w-[80px] truncate text-foreground" title={entityName}>{entityName}</span>
                  {showSeparator && <span className="text-muted-foreground">·</span>}
                  {displayLabel && <span className="truncate text-foreground" title={displayLabel}>{displayLabel}</span>}
                </div>
              )
            })}
            {selectedDataSources.length > 3 && (
              <div className="inline-flex items-center px-2 py-0.5 rounded-md bg-muted-50 text-xs text-muted-foreground">
                +{selectedDataSources.length - 3} {t('dataSource.more')}
              </div>
            )}
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClearSelection}
            className="h-7 px-2 text-xs hover:bg-muted hover:text-error shrink-0"
            title={t('dataSource.clearAllSelections')}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      )}

      {/* Category tabs */}
      {isMobile ? (
        <div className="shrink-0 px-3 pt-3 pb-2">
          <div className="grid grid-cols-4 gap-1.5">
            {availableCategories.map(cat => {
              const Icon = cat.icon
              const isActive = selectedCategory === cat.id
              return (
                <button
                  key={cat.id}
                  type="button"
                  onClick={() => {
                    setSelectedCategory(cat.id)
                    setSelectedDeviceId(null)
                    setSelectedExtensionId(null)
                  }}
                  className={cn(
                    'flex flex-col items-center justify-center gap-1 py-1.5 min-w-0 rounded-lg transition-colors',
                    isActive
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted-30 text-muted-foreground'
                  )}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  <span className="text-mini font-medium leading-none truncate w-full text-center">
                    {cat.name}
                  </span>
                </button>
              )
            })}
          </div>
        </div>
      ) : (
        <div className="flex border-b overflow-x-auto shrink-0 scrollbar-none [-webkit-overflow-scrolling:touch]">
          {availableCategories.map(cat => {
            const Icon = cat.icon
            const isActive = selectedCategory === cat.id
            return (
              <button
                key={cat.id}
                type="button"
                onClick={() => {
                  setSelectedCategory(cat.id)
                  setSelectedDeviceId(null)
                  setSelectedExtensionId(null)
                }}
                className={cn(
                  'flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 transition-colors shrink-0',
                  isActive
                    ? 'border-primary text-primary'
                    : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-muted-50'
                )}
              >
                <Icon className="h-4 w-4" />
                {cat.name}
              </button>
            )
          })}
        </div>
      )}

      {/* Content area */}
      {usesDeviceSplitLayout ? (
        // Split layout: device list on left, metrics/commands on right
        // Mobile: Show device list only, content opens in full screen
        isMobile ? (
          <div className="flex-1 flex overflow-hidden">
            <div className="flex-1 overflow-hidden flex flex-col">
              {renderDeviceList()}
            </div>
          </div>
        ) : (
          <div className="flex-1 flex overflow-hidden">
            {/* Left: Device list */}
            <div className="w-56 border-r shrink-0 overflow-hidden flex flex-col">
              {renderDeviceList()}
            </div>

            {/* Right: Device content */}
            <div className="flex-1 overflow-hidden flex flex-col">
              {renderDeviceContent()}
            </div>
          </div>
        )
      ) : usesExtensionSplitLayout ? (
        // Split layout: extension list on left, metrics/commands on right
        // Mobile: Show extension list only, content opens in full screen
        isMobile ? (
          <div className="flex-1 flex overflow-hidden">
            <div className="flex-1 overflow-hidden flex flex-col">
              {renderExtensionList()}
            </div>
          </div>
        ) : (
          <div className="flex-1 flex overflow-hidden">
            {/* Left: Extension list */}
            <div className="w-56 border-r shrink-0 overflow-hidden flex flex-col">
              {renderExtensionList()}
            </div>

            {/* Right: Extension content */}
            <div className="flex-1 overflow-hidden flex flex-col">
              {renderExtensionContent()}
            </div>
          </div>
        )
      ) : (
        // Single column layout
        <div className="flex-1 overflow-y-auto p-3">
          {renderCategoryContent()}
        </div>
      )}
    </div>

    {/* Mobile: Full-screen selector for device/extension content */}
    {isMobile && showMobileSelector && (
      <MobileItemSelector
        isOpen={showMobileSelector}
        onClose={() => setShowMobileSelector(false)}
        selectedDevice={selectedDevice}
        selectedExtension={selectedExtension}
        selectedCategory={selectedCategory}
        selectedItems={selectedKeys}
        onSelectItem={(itemKey: string) => {
          // Mobile components emit keys in unified format "source:id:field".
          // Route to the correct factory based on selectedCategory (only one category is visible at a time).
          const parts = itemKey.split(':')
          const id = parts[1] ?? ''
          switch (selectedCategory) {
            case 'device':
              handleSelectItem(createDeviceLocationDS(id))
              break
            case 'device-metric': {
              const field = parts.slice(2).join(':')
              // MobileMetricsList renders both metrics and device info properties with the same key format.
              // Check if this field is a known info property to use the correct factory.
              const isInfoProp = getDeviceInfoProperties(t).some(p => p.id === field)
              if (isInfoProp) {
                handleSelectItem(createDeviceInfoDS(id, field))
              } else {
                handleSelectItem(createMetricDS(id, field, suggestedMode))
              }
              break
            }
            case 'device-command': {
              const field = parts.slice(2).join(':')
              handleSelectItem(createCommandDS(id, field))
              break
            }
            case 'system': {
              handleSelectItem(createSystemDS(parts.slice(1).join(':')))
              break
            }
            case 'extension': {
              const field = parts.slice(2).join(':')
              const metricName = field.startsWith('produce:') ? field.slice(8) : field
              handleSelectItem(createExtensionMetricDS(id, metricName, suggestedMode))
              break
            }
            case 'extension-command': {
              const field = parts.slice(2).join(':')
              handleSelectItem(createExtensionCommandDS(id, field))
              break
            }
            case 'transform': {
              const field = parts.slice(2).join(':')
              handleSelectItem(createTransformDS(id, field))
              break
            }
          }
        }}
        deviceMetricsMap={deviceMetricsMap}
        deviceCommandsMap={deviceCommandsMap}
        extensionMetricsMap={extensionMetricsMap}
        devices={devices}
        extensions={extensions}
        summaries={summaries}
        availability={availability}
        checkingData={checkingData}
        getDeviceInfoProperties={getDeviceInfoProperties}
        t={t}
        insets={insets}
      />
    )}
    </>
  )
}

export default UnifiedDataSourceConfig
