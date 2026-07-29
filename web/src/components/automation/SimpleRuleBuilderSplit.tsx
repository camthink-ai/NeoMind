/**
 * SimpleRuleBuilderSplit Component
 *
 * Full-screen dialog for creating/editing automation rules.
 * Using unified FullScreenDialog components with glassmorphism style.
 *
 * @module automation
 */

import React, { useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { generateId } from '@/lib/id'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import {
  Plus,
  X,
  Zap,
  Bell,
  Bot,
  Lightbulb,
  Clock,
  AlertTriangle,
  Check,
  Globe,
  Timer,
  Calendar,
  Play,
  Loader2,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { cardPadded } from '@/design-system/tokens/size'
import { textNano } from "@/design-system/tokens/typography"
import { useIsMobile } from '@/hooks/useMobile'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType, Extension, ExtensionDataSourceInfo, ExtensionCommandDescriptor, TransformDataSourceInfo } from '@/types'
// Unified dialog components
import { BuilderShell } from './dialog/BuilderShell'
import { WorkspaceSegmentedControl } from './dialog/WorkspaceSegmentedControl'
import { Field, FieldLabel, FieldMessage } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'

// ============================================================================
// Utility Functions
// ============================================================================

// Check if an ID is an extension ID (format: extension:extension_id)
// DEPRECATED: Use source_type field instead
function isExtensionId(id: string): boolean {
  return id.startsWith('extension:')
}

// Get extension ID from the formatted ID
// DEPRECATED: Use extension_id field instead
function getExtensionId(formattedId: string): string {
  return formattedId.replace('extension:', '')
}

// Get commands for a resource (device or extension)
function getCommandsForResource(
  id: string,
  devices: Array<{ id: string; name: string; device_type: string; commands?: Array<{ name: string; description: string; parameters?: Array<{ name: string; param_type: string; required: boolean; default_value?: unknown }> }> }>,
  deviceTypes?: DeviceType[],
  extensions?: Extension[]
): Array<{ name: string; description: string; display_name?: string; parameters?: Array<{ name: string; param_type: string; required: boolean; default_value?: unknown }> }> {
  if (isExtensionId(id)) {
    const extId = getExtensionId(id)
    const ext = extensions?.find((e: Extension) => e.id === extId)
    return ext?.commands?.map((c: ExtensionCommandDescriptor) => ({
      name: c.id,
      description: c.description,
      display_name: c.display_name
    })) || []
  }

  const device = devices?.find((d: any) => d.id === id)
  if (device?.commands) return device.commands

  // Check device type for commands
  if (device && deviceTypes) {
    const dt = deviceTypes.find((t: any) => t.name === device.device_type)
    return dt?.commands?.map((c: any) => ({ name: c.name, description: c.description || '', display_name: c.display_name, parameters: c.parameters })) || []
  }

  return []
}

// Get metrics for a resource (device or extension)
function getMetricsForResource(
  id: string,
  devices: Array<{ id: string; name: string; device_type: string; metrics?: Array<{ name: string; data_type: string; unit?: string | null }> }>,
  deviceTypes?: DeviceType[],
  extensions?: Extension[],
  extensionDataSources?: ExtensionDataSourceInfo[]
): Array<{ name: string; data_type: string; unit?: string | null; display_name?: string }> {
  if (isExtensionId(id)) {
    const extId = getExtensionId(id)
    // Return extension data sources as metrics
    return extensionDataSources
      ?.filter((ds: ExtensionDataSourceInfo) => ds.extension_id === extId)
      .map((ds: ExtensionDataSourceInfo) => ({
        name: `${ds.command}.${ds.field}`,
        data_type: ds.data_type,
        unit: ds.unit,
        display_name: ds.display_name,
        extension_id: ds.extension_id,
        command: ds.command,
        field: ds.field,
      })) || []
  }

  const device = devices?.find((d: any) => d.id === id)
  if (device?.metrics) return device.metrics

  // Check device type for metrics
  if (device && deviceTypes) {
    const dt = deviceTypes.find((t: any) => t.name === device.device_type)
    return dt?.metrics?.map((m: any) => ({
      name: m.name,
      data_type: m.data_type || 'string',
      unit: m.unit,
      display_name: m.display_name || m.name
    })) || []
  }

  return []
}

// Get metric data type for a resource
function getMetricDataTypeForResource(
  resourceId: string,
  metricName: string,
  devices: Array<{ id: string; name: string; device_type: string; metrics?: Array<{ name: string; data_type: string }> }>,
  deviceTypes?: DeviceType[],
  extensions?: Extension[],
  extensionDataSources?: ExtensionDataSourceInfo[]
): string {
  // Virtual metrics emitted by the rule engine itself — always numeric age in seconds.
  if (metricName === '__last_seen_age_secs') return 'float'

  if (isExtensionId(resourceId)) {
    const extId = getExtensionId(resourceId)
    const ds = extensionDataSources?.find((d: ExtensionDataSourceInfo) =>
      d.extension_id === extId && `${d.command}.${d.field}` === metricName
    )
    if (ds) return ds.data_type
    return 'string'
  }

  const device = devices?.find((d: any) => d.id === resourceId)
  if (device?.metrics) {
    const metric = device.metrics.find((m: any) => m.name === metricName)
    if (metric) return metric.data_type
  }

  // Check device type
  if (device && deviceTypes) {
    const dt = deviceTypes.find((t: any) => t.name === device.device_type)
    const metric = dt?.metrics?.find((m: any) => m.name === metricName)
    if (metric) return metric.data_type || 'string'
  }

  return 'string'
}

// ============================================================================
// Types
// ============================================================================

interface RuleBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  rule?: Rule
  onSave: (rule: Partial<Rule>) => Promise<void>
  resources?: {
    devices: Array<{
      id: string
      name: string
      device_type: string
      metrics?: Array<{ name: string; data_type: string; unit?: string | null }>
      commands?: Array<{ name: string; description: string }>
      online?: boolean
    }>
    deviceTypes?: DeviceType[]
    extensions?: Extension[]
    extensionDataSources?: ExtensionDataSourceInfo[]
    transformDataSources?: TransformDataSourceInfo[]
    messageChannels?: Array<{ name: string; type: string; enabled: boolean }>
    agents?: Array<{ id: string; name: string }>
  }
}

// ============================================================================
// UI Condition Types
// ============================================================================

type ConditionType = 'simple' | 'range' | 'and' | 'or' | 'not'
type DataSourceType = 'device' | 'extension' | 'transform'

interface UICondition {
  id: string
  type: ConditionType
  source_type?: DataSourceType  // 'device', 'extension', or 'transform'
  device_id?: string  // Device ID only
  extension_id?: string  // Extension ID only
  transform_id?: string  // Transform ID only
  metric?: string  // Metric name (for both devices and extensions)
  operator?: string
  threshold?: number
  threshold_value?: string
  range_min?: number
  range_max?: number
  conditions?: UICondition[]
}

interface FormErrors {
  name?: string
  condition?: string[]
  actions?: Record<number, string>
  cron?: string
}

// ============================================================================
// Trigger Types
// ============================================================================

type TriggerType = 'data_change' | 'schedule' | 'manual'

interface CronTemplate {
  id: string
  label: string
  expression: string
  description: string
  icon: React.ReactNode
}

const CRON_TEMPLATES: CronTemplate[] = [
  { id: 'every_minute', label: '每分钟', expression: '* * * * *', description: '每分钟执行', icon: <Timer className="h-4 w-4" /> },
  { id: 'every_5min', label: '每5分钟', expression: '*/5 * * * *', description: '每5分钟执行', icon: <Timer className="h-4 w-4" /> },
  { id: 'every_15min', label: '每15分钟', expression: '*/15 * * * *', description: '每15分钟执行', icon: <Timer className="h-4 w-4" /> },
  { id: 'every_30min', label: '每30分钟', expression: '*/30 * * * *', description: '每30分钟执行', icon: <Timer className="h-4 w-4" /> },
  { id: 'hourly', label: '每小时', expression: '0 * * * *', description: '每小时的第0分钟', icon: <Clock className="h-4 w-4" /> },
  { id: 'daily_midnight', label: '每天午夜', expression: '0 0 * * *', description: '每天00:00', icon: <Calendar className="h-4 w-4" /> },
  { id: 'daily_morning', label: '每天早上', expression: '0 8 * * *', description: '每天08:00', icon: <Calendar className="h-4 w-4" /> },
  { id: 'daily_evening', label: '每天晚上', expression: '0 20 * * *', description: '每天20:00', icon: <Calendar className="h-4 w-4" /> },
  { id: 'weekly_monday', label: '每周一', expression: '0 0 * * 1', description: '每周一00:00', icon: <Calendar className="h-4 w-4" /> },
  { id: 'monthly', label: '每月1号', expression: '0 0 1 * *', description: '每月1号00:00', icon: <Calendar className="h-4 w-4" /> },
  { id: 'workdays_morning', label: '工作日早上', expression: '0 8 * * 1-5', description: '周一至周五08:00', icon: <Calendar className="h-4 w-4" /> },
]


// Calculate next execution time from cron expression
function getNextExecutionTime(cronExpression: string): Date | null {
  try {
    const parts = cronExpression.trim().split(/\s+/)
    if (parts.length !== 5) return null

    const [minute, hour, dayOfMonth, month, dayOfWeek] = parts

    const now = new Date()
    const next = new Date(now)

    // Simple calculation for common patterns
    // For a robust implementation, consider using a cron parser library
    if (minute === '*' && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
      // Every minute
      next.setSeconds(next.getSeconds() + 60)
      return next
    }

    if (minute.startsWith('*/')) {
      const interval = parseInt(minute.slice(2))
      next.setMinutes(next.getMinutes() + (interval - (next.getMinutes() % interval)))
    } else if (minute !== '*') {
      const targetMin = parseInt(minute)
      if (next.getMinutes() >= targetMin) {
        next.setHours(next.getHours() + 1)
      }
      next.setMinutes(targetMin)
    }

    if (hour !== '*' && !minute.startsWith('*/')) {
      const targetHour = parseInt(hour)
      if (next.getHours() >= targetHour) {
        next.setDate(next.getDate() + 1)
      }
      next.setHours(targetHour)
    }

    return next
  } catch {
    return null
  }
}

// ============================================================================
// Helper Functions
// ============================================================================

/** True if the condition tree references the `__last_seen_age_secs` virtual metric
 *  anywhere. Used to surface the stricter cooldown hint in the UI. */
function conditionUsesVirtualMetric(condition: UICondition | null): boolean {
  if (!condition) return false
  if (condition.metric === '__last_seen_age_secs') return true
  return (condition.conditions || []).some(c => conditionUsesVirtualMetric(c))
}

const getNumericOperators = (t: (key: string) => string) => [
  { value: '>', label: t('automation:operators.greaterThan') },
  { value: '<', label: t('automation:operators.lessThan') },
  { value: '>=', label: t('automation:operators.greaterThanOrEqual') },
  { value: '<=', label: t('automation:operators.lessThanOrEqual') },
]

const getStringOperators = (t: (key: string) => string) => [
  { value: '==', label: t('automation:operators.equal') },
  { value: '!=', label: t('automation:operators.notEqual') },
  { value: 'contains', label: t('automation:operators.contains') },
  { value: 'starts_with', label: t('automation:operators.startsWith') },
  { value: 'ends_with', label: t('automation:operators.endsWith') },
  { value: 'regex', label: t('automation:operators.regex') },
]

const getBooleanOperators = (t: (key: string) => string) => [
  { value: '==', label: t('automation:operators.equal') },
  { value: '!=', label: t('automation:operators.notEqual') },
]

const getComparisonOperators = (t: (key: string) => string, dataType?: string) => {
  if (dataType === 'string') return getStringOperators(t)
  if (dataType === 'boolean') return getBooleanOperators(t)
  return [...getNumericOperators(t),
    { value: '==', label: t('automation:operators.equal') },
    { value: '!=', label: t('automation:operators.notEqual') },
  ]
}

function getDeviceType(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): string {
  const device = devices.find(d => d.id === deviceId)
  return device?.device_type || deviceTypes?.[0]?.device_type || ''
}

function getDeviceMetrics(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string; metrics?: unknown }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string; data_type?: string }> {
  const device = devices.find(d => d.id === deviceId)
  if (!device) return []

  // If device has metrics directly (from rule resources), use them
  if (device.metrics && Array.isArray(device.metrics)) {
    return device.metrics as Array<{ name: string; display_name?: string; data_type?: string }>
  }

  // Otherwise, look up by device_type
  const deviceTypeName = device.device_type || deviceTypes?.[0]?.device_type || ''
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)

  // Return metrics or default fallback
  return deviceType?.metrics || [{ name: 'value', display_name: 'Value', data_type: 'float' }]
}

function getMetricDataType(
  metricName: string,
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string; metrics?: unknown }>,
  deviceTypes?: DeviceType[]
): string {
  const metrics = getDeviceMetrics(deviceId, devices, deviceTypes)
  const metric = metrics.find(m => m.name === metricName)
  return metric?.data_type || 'float'
}

function getDeviceCommands(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string; commands?: unknown }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string }> {
  // If device has commands directly (from rule resources), use them
  const device = devices.find(d => d.id === deviceId)
  if (device?.commands && Array.isArray(device.commands)) {
    return device.commands as Array<{ name: string; display_name?: string }>
  }

  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.commands || []
}

// ============================================================================
// Extension Helper Functions (V2 - decoupled from devices)
// ============================================================================

// Get extension metrics by extension ID
function getExtensionMetrics(
  extensionId: string,
  extensions: Extension[],
  extensionDataSources: ExtensionDataSourceInfo[]
): Array<{ name: string; display_name?: string; data_type?: string; unit?: string }> {
  const ext = extensions.find(e => e.id === extensionId)
  if (!ext) return []

  // Get data sources for this extension
  const dataSources = extensionDataSources.filter(ds => ds.extension_id === extensionId)

  // Also include metrics from the extension's metadata if available
  const metricsFromDataSources = dataSources.map(ds => ({
    name: ds.field,
    display_name: ds.display_name,
    data_type: ds.data_type,
    unit: ds.unit,
  }))

  // If extension has metrics metadata, include those too
  if (ext.metrics && ext.metrics.length > 0) {
    const extMetrics = ext.metrics.map(m => ({
      name: m.name,
      display_name: m.display_name || m.name,
      data_type: m.data_type,
      unit: m.unit,
    }))
    // Merge, removing duplicates by name
    const merged = new Map()
    metricsFromDataSources.forEach(m => merged.set(m.name, m))
    extMetrics.forEach(m => merged.set(m.name, m))
    return Array.from(merged.values())
  }

  return metricsFromDataSources
}

function getExtensionDataType(
  metricName: string,
  extensionId: string,
  extensions: Extension[],
  extensionDataSources: ExtensionDataSourceInfo[]
): string {
  const metrics = getExtensionMetrics(extensionId, extensions, extensionDataSources)
  const metric = metrics.find(m => m.name === metricName)
  return metric?.data_type || 'float'
}

// Get transform metrics by transform ID
function getTransformMetrics(
  transformId: string,
  transformDataSources: TransformDataSourceInfo[]
): Array<{ name: string; display_name?: string; data_type?: string; unit?: string }> {
  return transformDataSources
    .filter(ds => ds.transform_id === transformId)
    .map(ds => ({
      name: ds.metric_name,
      display_name: ds.display_name,
      data_type: ds.data_type,
      unit: ds.unit,
    }))
}

function getExtensionCommands(
  extensionId: string,
  extensions: Extension[]
): ExtensionCommandDescriptor[] {
  const ext = extensions.find(e => e.id === extensionId)
  return ext?.commands || []
}

function uiConditionToRuleCondition(cond: UICondition): RuleCondition {
  switch (cond.type) {
    case 'simple': {
      const op = cond.operator || '>'
      const stringOnlyOps = ['contains', 'starts_with', 'ends_with', 'regex']
      const isStringOp = stringOnlyOps.includes(op)

      // Build source from source_type, id, and metric
      const sourceId = cond.source_type === 'extension'
        ? `extension:${cond.extension_id || ''}:${cond.metric || 'value'}`
        : cond.source_type === 'transform'
        ? `transform:${cond.transform_id || ''}:${cond.metric || 'value'}`
        : `device:${cond.device_id || ''}:${cond.metric || 'value'}`

      // For string-only operators, always use threshold_value
      // For ==/!=, use threshold_value if the value is a non-numeric string
      if (isStringOp) {
        return {
          condition_type: 'comparison',
          source: sourceId,
          operator: op,
          threshold: 0,
          threshold_value: cond.threshold_value ?? '',
        }
      }

      // For ==/!= with a string threshold_value that isn't numeric, pass as string
      const strVal = cond.threshold_value
      if ((op === '==' || op === '!=') && strVal !== undefined && strVal !== '' && isNaN(Number(strVal))) {
        return {
          condition_type: 'comparison',
          source: sourceId,
          operator: op,
          threshold: 0,
          threshold_value: strVal,
        }
      }

      // Default: numeric threshold
      let thresholdValue: number
      if (cond.threshold_value !== undefined) {
        thresholdValue = Number(cond.threshold_value) || 0
      } else {
        thresholdValue = cond.threshold ?? 0
      }

      return {
        condition_type: 'comparison',
        source: sourceId,
        operator: op,
        threshold: thresholdValue,
      }
    }
    case 'range': {
      // Build source from source_type, id, and metric
      const sourceId = cond.source_type === 'extension'
        ? `extension:${cond.extension_id || ''}:${cond.metric || 'value'}`
        : cond.source_type === 'transform'
        ? `transform:${cond.transform_id || ''}:${cond.metric || 'value'}`
        : `device:${cond.device_id || ''}:${cond.metric || 'value'}`

      return {
        condition_type: 'range',
        source: sourceId,
        min: cond.range_min,
        max: cond.range_max,
      }
    }
    case 'and':
      return {
        condition_type: 'logical',
        operator: 'and',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      }
    case 'or':
      return {
        condition_type: 'logical',
        operator: 'or',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      }
    case 'not':
      return {
        condition_type: 'logical',
        operator: 'not',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      }
    default:
      return {
        condition_type: 'comparison',
        source: 'device::value',
        operator: '>',
        threshold: 0,
      }
  }
}

function ruleConditionToUiCondition(
  ruleCond?: RuleCondition,
  devices?: Array<{ id: string; name: string; device_type?: string }>,
  dslPreview?: string
): UICondition {
  if (!ruleCond) {
    return {
      id: generateId(),
      type: 'simple',
      source_type: 'device',
      device_id: '',
      metric: 'value',
      operator: '>',
      threshold: 0,
    }
  }

  // Check for logical conditions first (they have 'conditions' array)
  if ('conditions' in ruleCond && Array.isArray((ruleCond as any).conditions)) {
    const op = (ruleCond as any).operator
    if (op === 'and' || op === 'or') {
      return {
        id: generateId(),
        type: op,
        source_type: undefined,
        conditions: ((ruleCond as any).conditions || []).map((c: RuleCondition) => ruleConditionToUiCondition(c, devices, dslPreview)),
      }
    }
    if (op === 'not') {
      return {
        id: generateId(),
        type: 'not',
        source_type: undefined,
        conditions: [(ruleCond as any).conditions?.[0]].map((c: RuleCondition) => ruleConditionToUiCondition(c, devices, dslPreview)).filter(Boolean),
      }
    }
  }

  // Parse source field "device:sensor1:temperature" or "extension:weather:temp" or "transform:t1:field"
  const sourceStr = ruleCond.source || ''
  const sourceParts = sourceStr.split(':')
  const sourceType: DataSourceType = sourceParts[0] === 'extension' ? 'extension' : sourceParts[0] === 'transform' ? 'transform' : 'device'
  const sourceId = sourceParts[1] || ''
  const sourceField = sourceParts.length >= 3 ? sourceParts.slice(2).join(':') : 'value'

  // Check for range condition (has min/max)
  if (ruleCond.condition_type === 'range' || ('min' in ruleCond && (ruleCond as any).min !== undefined)) {
    return {
      id: generateId(),
      type: 'range',
      source_type: sourceType,
      ...(sourceType === 'extension'
        ? { extension_id: sourceId }
        : sourceType === 'transform'
        ? { transform_id: sourceId }
        : { device_id: sourceId }),
      metric: sourceField,
      range_min: (ruleCond as any).min,
      range_max: (ruleCond as any).max,
    }
  }

  // Simple/comparison condition
  const thresholdValue = ruleCond.threshold
  const isStringThreshold = typeof thresholdValue === 'string'
  const apiThresholdValue = (ruleCond as any).threshold_value as string | undefined

  return {
    id: generateId(),
    type: 'simple',
    source_type: sourceType,
    ...(sourceType === 'extension'
      ? { extension_id: sourceId }
      : sourceType === 'transform'
      ? { transform_id: sourceId }
      : { device_id: sourceId }),
    metric: sourceField,
    operator: ruleCond.operator || '>',
    threshold: isStringThreshold ? undefined : typeof thresholdValue === 'number' ? thresholdValue : 0,
    threshold_value: apiThresholdValue ?? (isStringThreshold ? thresholdValue : undefined),
  }
}

// Helper to get device name from ID
function getDeviceNameById(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>
): string {
  const device = devices.find(d => d.id === deviceId)
  return device?.name || deviceId
}

// Collect all data sources from a condition tree (for data_change trigger sources)
function collectSourcesFromCondition(cond: RuleCondition): string[] {
  const sources: string[] = []
  if (cond.source) {
    sources.push(cond.source)
  }
  if (cond.conditions) {
    for (const sub of cond.conditions) {
      sources.push(...collectSourcesFromCondition(sub))
    }
  }
  return sources
}

// Helper to get extension name from ID
function getExtensionNameById(
  extensionId: string,
  extensions: Extension[]
): string {
  const ext = extensions.find(e => e.id === extensionId)
  return ext?.name || extensionId
}

// Helper to get the base metric name without duplicate prefix
// Transform-generated metrics have format "prefix.name" (e.g., "ai_result.poses")
// If the device type prefix matches the metric prefix, strip it to avoid duplication
function getMetricPath(
  metric: string,
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>
): string {
  if (!metric) return 'value'

  // If metric contains a dot, it might have a prefix from transform output
  const parts = metric.split('.')
  if (parts.length > 1) {
    const prefix = parts[0]
    const device = devices.find(d => d.id === deviceId)

    // Common prefixes that should be stripped (MQTT/device standard prefixes)
    const commonPrefixes = ['values', 'value', 'data', 'telemetry', 'metrics', 'state']
    if (commonPrefixes.includes(prefix)) {
      return parts.slice(1).join('.')
    }

    // Check if the metric prefix matches the device_type
    // If so, strip the prefix to avoid: device_type.device_type.metric_name
    if (device?.device_type && prefix === device.device_type) {
      // Strip the prefix, return just the suffix
      return parts.slice(1).join('.')
    }

    // Also check against device name (in case name is used as prefix)
    const deviceName = device?.name?.toLowerCase().replace(/\s+/g, '_')
    if (deviceName && prefix === deviceName) {
      return parts.slice(1).join('.')
    }
  }

  // Return metric as-is
  return metric
}

// ============================================================================
// Local Canvas Components for RuleWorkspace
// ============================================================================

interface ConditionCanvasProps {
  triggerType: TriggerType
  cronExpression: string
  onCronExpressionChange: (v: string) => void
  selectedCronTemplate: string
  onSelectedCronTemplateChange: (v: string) => void
  condition: UICondition | null
  onConditionChange: (c: UICondition) => void
  onAddCondition: () => UICondition
  devices: Array<{
    id: string
    name: string
    device_type: string
    metrics?: Array<{ name: string; data_type: string; unit?: string | null }>
  }>
  deviceTypes?: DeviceType[]
  extensions?: Extension[]
  extensionDataSources?: ExtensionDataSourceInfo[]
  transformDataSources?: TransformDataSourceInfo[]
  forDuration: number
  onForDurationChange: (v: number) => void
  forUnit: 'seconds' | 'minutes' | 'hours'
  onForUnitChange: (v: 'seconds' | 'minutes' | 'hours') => void
  errors: FormErrors
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ConditionCanvas({
  triggerType,
  cronExpression,
  onCronExpressionChange,
  selectedCronTemplate,
  onSelectedCronTemplateChange,
  condition,
  onConditionChange,
  onAddCondition,
  devices,
  deviceTypes,
  extensions,
  extensionDataSources,
  transformDataSources,
  forDuration,
  onForDurationChange,
  forUnit,
  onForUnitChange,
  errors,
  t,
  tBuilder,
}: ConditionCanvasProps) {
  const [showCustomCron, setShowCustomCron] = useState(false)

  const handleCronTemplateSelect = (templateId: string) => {
    if (templateId === 'custom') {
      setShowCustomCron(true)
      onSelectedCronTemplateChange('custom')
    } else {
      setShowCustomCron(false)
      const template = CRON_TEMPLATES.find(t => t.id === templateId)
      if (template) {
        onCronExpressionChange(template.expression)
        onSelectedCronTemplateChange(templateId)
      }
    }
  }

  const nextExecution = useMemo(() => {
    if (triggerType === 'schedule') {
      return getNextExecutionTime(cronExpression)
    }
    return null
  }, [triggerType, cronExpression])

  return (
    <div className="space-y-4 p-4 rounded-lg border border-border">
      {triggerType === 'data_change' && (
        <>
          <div className="flex items-center gap-2 pb-4 border-b">
            <div className="p-2 rounded-full bg-accent-indigo-light">
              <Lightbulb className="h-5 w-5 text-accent-indigo" />
            </div>
            <div>
              <h4 className="text-sm font-medium">{tBuilder('triggerDevice') || 'Device Trigger'}</h4>
              <p className="text-xs text-muted-foreground">{tBuilder('deviceTriggerDesc') || 'Trigger when device state meets conditions'}</p>
            </div>
          </div>

          {!condition && (
            <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] md:grid-cols-5 gap-3">
              <ConditionTypeButton
                label={tBuilder('simpleCondition')}
                icon={<Lightbulb className="h-5 w-5" />}
                onClick={() => onConditionChange(onAddCondition())}
              />
              <ConditionTypeButton
                label={tBuilder('rangeCondition')}
                icon={<Globe className="h-5 w-5" />}
                onClick={() => {
                  const c = onAddCondition()
                  c.type = 'range'
                  c.range_min = 0
                  c.range_max = 100
                  onConditionChange(c)
                }}
              />
              <ConditionTypeButton
                label={tBuilder('andCombination')}
                icon={<Check className="h-5 w-5" />}
                onClick={() => {
                  const c = onAddCondition()
                  c.type = 'and'
                  c.conditions = [onAddCondition(), onAddCondition()]
                  onConditionChange(c)
                }}
              />
              <ConditionTypeButton
                label={tBuilder('orCombination')}
                icon={<AlertTriangle className="h-5 w-5" />}
                onClick={() => {
                  const c = onAddCondition()
                  c.type = 'or'
                  c.conditions = [onAddCondition(), onAddCondition()]
                  onConditionChange(c)
                }}
              />
              <ConditionTypeButton
                label={tBuilder('notCondition') || 'NOT'}
                icon={<X className="h-5 w-5" />}
                onClick={() => {
                  const c = onAddCondition()
                  c.type = 'not'
                  c.conditions = [onAddCondition()]
                  onConditionChange(c)
                }}
              />
            </div>
          )}

          {condition && (
            <div className="space-y-4">
              <ConditionEditor
                condition={condition}
                onChange={onConditionChange}
                devices={devices}
                deviceTypes={deviceTypes}
                extensions={extensions}
                extensionDataSources={extensionDataSources}
                transformDataSources={transformDataSources}
                t={t}
                tBuilder={tBuilder}
              />

              <div className="flex items-center gap-3 p-4 rounded-lg border">
                <Clock className="h-4 w-4 text-muted-foreground shrink-0" />
                <Label className="text-sm font-medium">{tBuilder('duration')}</Label>
                <Input
                  type="number"
                  min={0}
                  value={forDuration}
                  onChange={e => onForDurationChange(parseInt(e.target.value) || 0)}
                  className="w-24 h-9"
                />
                <Select value={forUnit} onValueChange={(v: any) => onForUnitChange(v)}>
                  <SelectTrigger className="w-28 h-9">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="seconds">{tBuilder('seconds')}</SelectItem>
                    <SelectItem value="minutes">{tBuilder('minutes')}</SelectItem>
                    <SelectItem value="hours">{tBuilder('hours')}</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {errors.condition && errors.condition.length > 0 && (
                <div className="p-3 bg-error-light border border-error rounded-lg">
                  {errors.condition.map((err, i) => (
                    <p key={i} className="text-sm text-error">• {err}</p>
                  ))}
                </div>
              )}
            </div>
          )}
        </>
      )}

      {triggerType === 'schedule' && (
        <>
          <div className="flex items-center gap-2 pb-4 border-b">
            <div className="p-2 rounded-full bg-accent-indigo-light">
              <Clock className="h-5 w-5 text-accent-indigo" />
            </div>
            <div>
              <h4 className="text-sm font-medium">{tBuilder('triggerSchedule') || 'Schedule Trigger'}</h4>
              <p className="text-xs text-muted-foreground">{tBuilder('scheduleTriggerDesc') || 'Execute on a schedule'}</p>
            </div>
          </div>

          <div className="space-y-4">
            <div>
              <Label className="text-xs text-muted-foreground">{tBuilder('cronTemplate') || 'Cron Template'}</Label>
              <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] sm:grid-cols-4 gap-2 mt-2">
                {CRON_TEMPLATES.slice(0, 8).map(template => (
                  <button
                    key={template.id}
                    type="button"
                    onClick={() => handleCronTemplateSelect(template.id)}
                    className={cn(
                      "flex flex-col items-center gap-1 p-2 rounded-md border text-xs transition-all",
                      selectedCronTemplate === template.id
                        ? "border-accent-indigo bg-accent-indigo-light text-accent-indigo"
                        : "border-border hover:border-accent-indigo"
                    )}
                  >
                    {template.icon}
                    <span>{template.label}</span>
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between mb-2">
                <Label className="text-xs text-muted-foreground">
                  {tBuilder('cronExpression')}
                  <span className="text-error ml-0.5">*</span>
                </Label>
                <button
                  type="button"
                  onClick={() => {
                    setShowCustomCron(!showCustomCron)
                    if (showCustomCron) {
                      const template = CRON_TEMPLATES.find(t => t.expression === cronExpression)
                      if (template) {
                        onSelectedCronTemplateChange(template.id)
                      } else {
                        onSelectedCronTemplateChange('custom')
                      }
                    } else {
                      onSelectedCronTemplateChange('custom')
                    }
                  }}
                  className="text-xs text-accent-indigo hover:text-accent-indigo"
                >
                  {showCustomCron ? (tBuilder('useTemplate') || 'Use Template') : (tBuilder('customCron') || 'Custom')}
                </button>
              </div>
              {showCustomCron ? (
                <Input
                  type="text"
                  value={cronExpression}
                  onChange={e => onCronExpressionChange(e.target.value)}
                  placeholder="* * * * *"
                  aria-invalid={!!errors.cron}
                  className={cn(
                    "font-mono text-sm h-9",
                    errors.cron && "border-error"
                  )}
                />
              ) : (
                <div className="p-3 rounded-lg border">
                  <code className="text-sm font-mono">{cronExpression}</code>
                </div>
              )}
              <p className="text-xs text-muted-foreground mt-1">
                {tBuilder('cronFormat')}
              </p>
              {errors.cron && <FieldMessage>{errors.cron}</FieldMessage>}
            </div>

            {nextExecution && (
              <div className="flex items-center gap-2 p-3 rounded-lg border">
                <Calendar className="h-4 w-4 text-success" />
                <span className="text-xs text-muted-foreground">
                  {tBuilder('nextExecution') || 'Next execution'}: {nextExecution.toLocaleString('zh-CN', {
                    month: 'short',
                    day: 'numeric',
                    hour: '2-digit',
                    minute: '2-digit'
                  })}
                </span>
              </div>
            )}
          </div>
        </>
      )}

      {triggerType === 'manual' && (
        <>
          <div className="flex items-center gap-2 pb-4 border-b">
            <div className="p-2 rounded-full bg-accent-indigo-light">
              <Play className="h-5 w-5 text-accent-indigo" />
            </div>
            <div>
              <h4 className="text-sm font-medium">{tBuilder('triggerManual') || 'Manual Trigger'}</h4>
              <p className="text-xs text-muted-foreground">{tBuilder('manualTriggerDesc') || 'This rule must be triggered manually'}</p>
            </div>
          </div>

          <div className="space-y-3">
            <div className="flex items-center gap-3 p-3 rounded-lg border">
              <div className="w-6 h-6 shrink-0 rounded-full bg-success-light flex items-center justify-center">
                <span className="text-xs font-medium text-success">1</span>
              </div>
              <p className="text-sm text-muted-foreground">{tBuilder('manualStep1') || 'Click execute button in rule list'}</p>
            </div>
            <div className="flex items-center gap-3 p-3 rounded-lg border">
              <div className="w-6 h-6 shrink-0 rounded-full bg-success-light flex items-center justify-center">
                <span className="text-xs font-medium text-success">2</span>
              </div>
              <p className="text-sm text-muted-foreground">{tBuilder('manualStep2') || 'Or call execution API'}</p>
            </div>
          </div>
        </>
      )}
    </div>
  )
}

interface ActionCanvasProps {
  actions: RuleAction[]
  onActionsChange: (actions: RuleAction[]) => void
  devices: Array<{
    id: string
    name: string
    device_type: string
    commands?: Array<{ name: string; description: string }>
    metrics?: Array<{ name: string; data_type: string; unit?: string | null }>
  }>
  deviceTypes?: DeviceType[]
  extensions?: Extension[]
  messageChannels?: Array<{ name: string; type: string; enabled: boolean }>
  agents?: Array<{ id: string; name: string }>
  errors?: FormErrors
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ActionCanvas({ actions, onActionsChange, devices, deviceTypes, extensions, messageChannels, agents, errors, t, tBuilder }: ActionCanvasProps) {
  return (
    <div className="space-y-4 p-4 rounded-lg border border-border">
      {/* Header — emerald accent to differentiate from Condition (indigo) */}
      <div className="flex items-center gap-2 pb-3 border-b">
        <div className="p-2 rounded-full bg-accent-emerald-light">
          <Zap className="h-5 w-5 text-accent-emerald" />
        </div>
        <div>
          <h4 className="text-sm font-medium">{tBuilder('executeActions')}</h4>
          <p className="text-xs text-muted-foreground">{tBuilder('actionsDesc')}</p>
        </div>
      </div>

      {/* Action type buttons */}
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" onClick={() => {
          const firstDevice = devices[0]
          const commands = firstDevice ? getCommandsForResource(firstDevice.id, devices, deviceTypes, extensions) : []
          onActionsChange([...actions, { type: 'execute', target: firstDevice?.id || '', target_type: 'device' as const, command: commands[0]?.name || 'turn_on', params: {}, _key: generateId() } as any])
        }}>
          <Zap className="h-4 w-4 mr-1" />
          {tBuilder('executeCommand') || 'Execute'}
        </Button>
        <Button size="sm" variant="outline" onClick={() => onActionsChange([...actions, { type: 'notify', message: '', severity: 'info' as const, _key: generateId() } as any])}>
          <Bell className="h-4 w-4 mr-1" />
          {tBuilder('sendNotification') || 'Notify'}
        </Button>
        <Button size="sm" variant="outline" onClick={() => onActionsChange([...actions, { type: 'trigger_agent', agent_id: '', _key: generateId() } as any])}>
          <Bot className="h-4 w-4 mr-1" />
          {tBuilder('triggerAgent') || 'Trigger Agent'}
        </Button>
      </div>

      {/* Actions list — rendered via ActionEditorCompact (handles all 7 types) */}
      <div className="space-y-2">
        {actions.length === 0 ? (
          <div className="text-center py-8 text-muted-foreground">
            <Zap className="h-8 w-8 mx-auto mb-2 opacity-50" />
            <p className="text-sm">{tBuilder('noActionsHint') || 'No actions yet, click buttons above to add'}</p>
          </div>
        ) : (
          actions.map((action, index) => (
            <ActionEditorCompact
              key={(action as any)._key || index}
              index={index}
              action={action}
              devices={devices}
              deviceTypes={deviceTypes}
              extensions={extensions}
              messageChannels={messageChannels}
              agents={agents}
              error={errors?.actions?.[index]}
              t={t}
              tBuilder={tBuilder}
              onUpdate={(updates) => onActionsChange(actions.map((a, i) => i === index ? ({ ...a, ...updates } as RuleAction) : a))}
              onRemove={() => onActionsChange(actions.filter((_, i) => i !== index))}
            />
          ))
        )}
      </div>
    </div>
  )
}

function ConditionTypeButton({ label, icon, onClick }: { label: string; icon: React.ReactNode; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="p-4 rounded-lg border-2 border-border hover:border-accent-indigo hover:transition-all text-left min-w-0"
    >
      <div className="flex items-center gap-3 min-w-0">
        <div className="p-2 rounded-lg shrink-0">{icon}</div>
        <span className="font-medium min-w-0 break-words">{label}</span>
      </div>
    </button>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function SimpleRuleBuilderSplit({
  open,
  onOpenChange,
  rule,
  onSave,
  resources = { devices: [], deviceTypes: [] },
}: RuleBuilderProps) {
  const { t } = useTranslation(['automation', 'common', 'dashboardComponents'])
  const tBuilder = (key: string) => t(`automation:ruleBuilder.${key}`)
  const isEditMode = !!rule
  const isMobile = useIsMobile()

  // Workspace state
  const [workspaceTab, setWorkspaceTab] = useState<'form' | 'dsl'>('form')

  // Form data
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [tags, setTags] = useState<string[]>([])
  const [tagInput, setTagInput] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [triggerType, setTriggerType] = useState<TriggerType>('data_change')
  const [cronExpression, setCronExpression] = useState('0 0 * * *') // Default: daily at midnight
  const [selectedCronTemplate, setSelectedCronTemplate] = useState('daily_midnight')
  const [condition, setCondition] = useState<UICondition | null>(null)
  const [forDuration, setForDuration] = useState<number>(0)
  const [forUnit, setForUnit] = useState<'seconds' | 'minutes' | 'hours'>('minutes')
  const [cooldownValue, setCooldownValue] = useState<number>(1)
  const [cooldownUnit, setCooldownUnit] = useState<'seconds' | 'minutes' | 'hours'>('minutes')
  const [actions, setActions] = useState<RuleAction[]>([])
  const [saving, setSaving] = useState(false)
  const [formErrors, setFormErrors] = useState<FormErrors>({})

  // Reset when dialog opens or rule changes
  useEffect(() => {
    if (open) {
      if (rule) {
        setName(rule.name || '')
        setDescription(rule.description || '')
        setEnabled(rule.enabled ?? true)
        setTags(rule.tags || (rule as any).source?.tags || [])
        setFormErrors({})

        // Restore trigger type - check trigger field or saved source
        const savedTriggerType = (rule as any).source?.triggerType as TriggerType
        const savedCronExpression = (rule as any).source?.cronExpression as string

        if (rule.trigger?.trigger_type === 'schedule' || savedTriggerType === 'schedule') {
          setTriggerType('schedule')
          setCronExpression(savedCronExpression || (rule.trigger as any)?.cron || '0 0 * * *')
          // Find matching template
          const matchingTemplate = CRON_TEMPLATES.find(t => t.expression === (savedCronExpression || (rule.trigger as any)?.cron))
          setSelectedCronTemplate(matchingTemplate?.id || 'custom')
        } else if (rule.trigger?.trigger_type === 'manual' || savedTriggerType === 'manual') {
          setTriggerType('manual')
        } else {
          setTriggerType('data_change')
        }

        // Try to restore from source.uiCondition first (exact restoration)
        const sourceUiCond = (rule as any).source?.uiCondition
        if (sourceUiCond) {
          setCondition(sourceUiCond)
        } else if (rule.condition) {
          // Fall back to converting the condition from the new source-based format
          const uiCond = ruleConditionToUiCondition(rule.condition, resources.devices)
          setCondition(uiCond)
        } else {
          setCondition(null)
        }

        // Restore actions - prefer source.uiActions for exact restoration
        const sourceUiActions = (rule as any).source?.uiActions
        if (sourceUiActions && sourceUiActions.length > 0) {
          setActions(sourceUiActions)
        } else if (rule.actions && rule.actions.length > 0) {
          // Validate and clean up actions to ensure correct structure
          const cleanedActions: RuleAction[] = rule.actions.map(action => {
            // Ensure action has correct structure based on type
            switch (action.type) {
              case 'notify':
                return { type: 'notify', message: (action as any).message || '', severity: (action as any).severity || 'info' } as RuleAction
              case 'execute':
                return { type: 'execute', target: (action as any).target || (action as any).device_id || '', target_type: (action as any).target_type || 'device', command: (action as any).command || '', params: (action as any).params || {} } as RuleAction
              case 'trigger_agent':
                return { type: 'trigger_agent', agent_id: (action as any).agent_id || '', input: (action as any).input, data: (action as any).data } as RuleAction
              default:
                // Unknown action type, default to notify
                return { type: 'notify', message: 'Rule triggered', severity: 'info' } as RuleAction
            }
          })
          setActions(cleanedActions)
        } else {
          setActions([])
        }

        // Restore forDuration and forUnit - prefer source values, then for_duration field
        const sourceForDuration = (rule as any).source?.forDuration
        const sourceForUnit = (rule as any).source?.forUnit
        if (sourceForDuration !== undefined && sourceForUnit !== undefined) {
          setForDuration(sourceForDuration)
          setForUnit(sourceForUnit)
        } else if (rule.for_duration) {
          // Convert ms to appropriate unit
          const ms = rule.for_duration
          if (ms >= 3600000 && ms % 3600000 === 0) {
            setForDuration(ms / 3600000)
            setForUnit('hours')
          } else if (ms >= 60000 && ms % 60000 === 0) {
            setForDuration(ms / 60000)
            setForUnit('minutes')
          } else {
            setForDuration(Math.round(ms / 1000))
            setForUnit('seconds')
          }
        } else {
          setForDuration(0)
          setForUnit('minutes')
        }

        // Restore cooldown - prefer source values, then cooldown field
        const sourceCooldownValue = (rule as any).source?.cooldownValue
        const sourceCooldownUnit = (rule as any).source?.cooldownUnit
        if (sourceCooldownValue !== undefined && sourceCooldownUnit !== undefined) {
          setCooldownValue(sourceCooldownValue)
          setCooldownUnit(sourceCooldownUnit)
        } else if (rule.cooldown) {
          const ms = rule.cooldown
          if (ms >= 3600000 && ms % 3600000 === 0) {
            setCooldownValue(ms / 3600000)
            setCooldownUnit('hours')
          } else if (ms >= 60000 && ms % 60000 === 0) {
            setCooldownValue(ms / 60000)
            setCooldownUnit('minutes')
          } else {
            setCooldownValue(Math.round(ms / 1000))
            setCooldownUnit('seconds')
          }
        } else {
          setCooldownValue(1)
          setCooldownUnit('minutes')
        }
      } else {
        resetForm()
      }
    }
  }, [open, rule, resources.devices, resources.deviceTypes])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setTags([])
    setTagInput('')
    setEnabled(true)
    setTriggerType('data_change')
    setCronExpression('0 0 * * *')
    setSelectedCronTemplate('daily_midnight')
    setCondition(null)
    setForDuration(0)
    setForUnit('minutes')
    setCooldownValue(1)
    setCooldownUnit('minutes')
    // Use a fixed default message instead of translation to avoid issues
    setActions([{ type: 'notify', message: 'Rule triggered', severity: 'info' }])
    setFormErrors({})
  }, [])

  const createDefaultCondition = useCallback((): UICondition => {
    // Try devices first, then extensions
    const firstDevice = resources.devices[0]
    const firstExtension = resources.extensions?.[0]

    if (!firstDevice && !firstExtension) {
      return {
        id: generateId(),
        type: 'simple',
        source_type: 'device',
        metric: 'value',
        operator: '>',
        threshold: 0,
      }
    }

    // Use first available resource (prefer device over extension)
    if (firstDevice) {
      const metrics = getDeviceMetrics(firstDevice.id, resources.devices, resources.deviceTypes)
      return {
        id: generateId(),
        type: 'simple',
        source_type: 'device',
        device_id: firstDevice.id,
        metric: metrics[0]?.name || 'value',
        operator: '>',
        threshold: 0,
      }
    } else {
      const metrics = getExtensionMetrics(firstExtension!.id, resources.extensions || [], resources.extensionDataSources || [])
      return {
        id: generateId(),
        type: 'simple',
        source_type: 'extension',
        extension_id: firstExtension!.id,
        metric: metrics[0]?.name || 'value',
        operator: '>',
        threshold: 0,
      }
    }
  }, [resources.devices, resources.deviceTypes, resources.extensions, resources.extensionDataSources])

  // Validate form
  const validate = (): boolean => {
    const errors: FormErrors = {}

    if (!name.trim()) {
      errors.name = tBuilder('ruleNameRequired')
    }

    // Validate cron expression for schedule trigger
    if (triggerType === 'schedule') {
      const cron = cronExpression.trim()
      if (!cron) {
        errors.cron = tBuilder('cronRequired')
      } else {
        const parts = cron.split(/\s+/)
        if (parts.length !== 5) {
          errors.cron = tBuilder('cronInvalid')
        } else {
          // Validate each field contains only valid cron characters
          const validField = /^[\d*/,-]+$/
          for (let i = 0; i < 5; i++) {
            if (!validField.test(parts[i])) {
              errors.cron = tBuilder('cronInvalid')
              break
            }
          }
        }
      }
    }

    // Validate actions — each action's required fields must be populated
    const actionErrors: Record<number, string> = {}
    actions.forEach((action, index) => {
      let incomplete = false
      switch (action.type) {
        case 'execute':
          incomplete = !action.target || !action.command
          break
        case 'notify':
          incomplete = !action.message.trim()
          break
        case 'trigger_agent':
          incomplete = !action.agent_id
          break
      }
      if (incomplete) actionErrors[index] = tBuilder('actionIncomplete')
    })
    if (Object.keys(actionErrors).length > 0) {
      errors.actions = actionErrors
    }

    // Only validate condition for data_change trigger type
    if (triggerType === 'data_change') {
      if (!condition) {
        errors.condition = [tBuilder('addTriggerCondition')]
      } else {
        const validateCondition = (cond: UICondition): string[] => {
          const errs: string[] = []
          if (cond.type === 'simple' || cond.type === 'range') {
            const hasSourceId = cond.source_type === 'extension' ? !!cond.extension_id : !!cond.device_id
            if (!hasSourceId) errs.push(cond.source_type === 'extension' ? (tBuilder('selectExtension') || 'Select extension') : tBuilder('selectDevice'))
            if (!cond.metric) errs.push(tBuilder('selectMetric'))
            if (cond.type === 'simple') {
              const stringOnlyOps = ['contains', 'starts_with', 'ends_with', 'regex']
              const isStringOp = stringOnlyOps.includes(cond.operator || '')
              if (isStringOp) {
                if (!cond.threshold_value || cond.threshold_value.trim() === '') errs.push(tBuilder('enterThreshold'))
              } else {
                const hasValue = cond.threshold !== undefined || cond.threshold_value !== undefined
                if (!hasValue) errs.push(tBuilder('enterThreshold'))
              }
            }
          } else if (cond.type === 'and' || cond.type === 'or' || cond.type === 'not') {
            if (!cond.conditions || cond.conditions.length === 0) {
              errs.push(tBuilder('addSubConditions'))
            } else {
              cond.conditions.forEach((sub) => {
                errs.push(...validateCondition(sub))
              })
            }
          }
          return errs
        }
        const conditionErrors = validateCondition(condition)
        if (conditionErrors.length > 0) {
          errors.condition = conditionErrors
        }
      }
    }

    setFormErrors(errors)
    return Object.keys(errors).length === 0
  }

  // Save
  const handleSave = async () => {
    if (!validate()) return

    setSaving(true)
    try {
      let finalCondition: RuleCondition | undefined = undefined
      if (triggerType === 'data_change' && condition) {
        finalCondition = uiConditionToRuleCondition(condition)
      }

      // Build trigger based on type
      let trigger: RuleTrigger
      if (triggerType === 'schedule') {
        trigger = { trigger_type: 'schedule', cron: cronExpression }
      } else if (triggerType === 'manual') {
        trigger = { trigger_type: 'manual' }
      } else {
        // Collect sources from conditions for data_change trigger
        const sources = finalCondition ? collectSourcesFromCondition(finalCondition) : []
        trigger = { trigger_type: 'data_change', sources }
      }

      // Calculate for_duration in ms
      let forDurationMs: number | undefined
      if (forDuration > 0) {
        const multiplier = forUnit === 'seconds' ? 1000 : forUnit === 'hours' ? 3600000 : 60000
        forDurationMs = forDuration * multiplier
      }

      // Calculate cooldown in ms
      const cooldownMs = cooldownValue > 0
        ? cooldownValue * (cooldownUnit === 'seconds' ? 1000 : cooldownUnit === 'hours' ? 3600000 : 60000)
        : 0

      const ruleData: Partial<Rule> = {
        name,
        description,
        enabled,
        tags: tags.length > 0 ? tags : undefined,
        trigger,
        condition: finalCondition,
        actions: actions.length > 0 ? actions.map(({ _key, ...rest }: any) => rest) as RuleAction[] : undefined,
        for_duration: forDurationMs,
        cooldown: cooldownMs,
        // Store original UI state in source field for proper restoration on edit
        source: {
          condition: finalCondition,
          uiCondition: condition,
          uiActions: actions,
          forDuration,
          forUnit,
          tags,
          triggerType,
          cronExpression,
          cooldownValue,
          cooldownUnit,
        } as any,
      }
      if (rule?.id) ruleData.id = rule.id
      await onSave(ruleData)
    } finally {
      setSaving(false)
    }
  }

  // Generate preview JSON instead of DSL
  const previewJSON = useMemo(() => {
    const finalCondition = triggerType === 'data_change' && condition ? uiConditionToRuleCondition(condition) : null
    let trigger: RuleTrigger
    if (triggerType === 'schedule') {
      trigger = { trigger_type: 'schedule', cron: cronExpression }
    } else if (triggerType === 'manual') {
      trigger = { trigger_type: 'manual' }
    } else {
      const sources = finalCondition ? collectSourcesFromCondition(finalCondition) : []
      trigger = { trigger_type: 'data_change', sources }
    }
    let forDurationMs: number | undefined
    if (forDuration > 0) {
      const multiplier = forUnit === 'seconds' ? 1000 : forUnit === 'hours' ? 3600000 : 60000
      forDurationMs = forDuration * multiplier
    }
    const cooldownMs = cooldownValue > 0
      ? cooldownValue * (cooldownUnit === 'seconds' ? 1000 : cooldownUnit === 'hours' ? 3600000 : 60000)
      : 0
    return JSON.stringify({
      name: name || tBuilder('name'),
      description,
      enabled,
      tags: tags.length > 0 ? tags : undefined,
      trigger,
      condition: finalCondition,
      actions,
      for_duration: forDurationMs,
      cooldown: cooldownMs,
    }, null, 2)
  }, [name, condition, actions, resources.devices, resources.extensions, forDuration, forUnit, cooldownValue, cooldownUnit, tags, triggerType, cronExpression, tBuilder])

  // Local workspace content (plain variable, NOT a component — avoids remount on every state change)
  const ruleWorkspaceContent = (
      <div className="space-y-4">
        <WorkspaceSegmentedControl
          accent="indigo"
          segments={[
            { value: 'form', label: tBuilder('form') || 'Form' },
            { value: 'dsl', label: tBuilder('dsl') || 'JSON' },
          ]}
          value={workspaceTab}
          onChange={(v) => setWorkspaceTab(v as 'form' | 'dsl')}
        />

        {workspaceTab === 'form' && (
          <div className="space-y-6">
            <ConditionCanvas
              triggerType={triggerType}
              cronExpression={cronExpression}
              onCronExpressionChange={setCronExpression}
              selectedCronTemplate={selectedCronTemplate}
              onSelectedCronTemplateChange={setSelectedCronTemplate}
              condition={condition}
              onConditionChange={setCondition}
              onAddCondition={createDefaultCondition}
              devices={resources.devices}
              deviceTypes={resources.deviceTypes}
              extensions={resources.extensions}
              extensionDataSources={resources.extensionDataSources}
              transformDataSources={resources.transformDataSources}
              forDuration={forDuration}
              onForDurationChange={setForDuration}
              forUnit={forUnit}
              onForUnitChange={setForUnit}
              errors={formErrors}
              t={t}
              tBuilder={tBuilder}
            />
            <ActionCanvas
              actions={actions}
              onActionsChange={setActions}
              devices={resources.devices}
              deviceTypes={resources.deviceTypes}
              extensions={resources.extensions}
              messageChannels={resources.messageChannels}
              agents={resources.agents}
              errors={formErrors}
              t={t}
              tBuilder={tBuilder}
            />
          </div>
        )}

        {workspaceTab === 'dsl' && (
          <div className="rounded-lg border border-border p-4">
            <pre className={cn(textNano, "font-mono overflow-x-auto whitespace-pre-wrap break-all")}>
              {previewJSON || tBuilder('noPreview')}
            </pre>
          </div>
        )}
      </div>
  )

  return (
    <BuilderShell
      open={open}
      onOpenChange={onOpenChange}
      accent="indigo"
      mobileConfigLabel={t('automation:ruleBuilder.config')}
      title={rule ? t('automation:ruleBuilder.editRule') : t('automation:newRule')}
      subtitle={t('automation:ruleBuilder.subtitle')}
      icon={<Zap className="h-5 w-5" />}
      statusIndicator={
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span className={cn('h-1.5 w-1.5 rounded-full', enabled ? 'bg-success' : 'bg-muted-foreground opacity-40')} />
          {enabled ? t('automation:ruleBuilder.enabled') : t('automation:ruleBuilder.disabled')}
        </span>
      }
      config={
        <div className="space-y-3.5">
          {/* Name */}
          <Field>
            <FieldLabel htmlFor="rule-name">
              {t('automation:ruleBuilder.ruleName')}
              <span className="text-error ml-0.5">*</span>
            </FieldLabel>
            <Input
              id="rule-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-invalid={!!formErrors.name}
              className={formErrors.name ? "border-error" : undefined}
            />
            {formErrors.name && <FieldMessage>{formErrors.name}</FieldMessage>}
          </Field>

          {/* Description */}
          <Field>
            <FieldLabel htmlFor="rule-desc">{t('automation:ruleBuilder.description')}</FieldLabel>
            <Textarea id="rule-desc" value={description} onChange={(e) => setDescription(e.target.value)} rows={3} />
          </Field>

          {/* Trigger type select */}
          <Field>
            <FieldLabel>{t('automation:ruleBuilder.triggerType')}</FieldLabel>
            <Select value={triggerType} onValueChange={(v) => setTriggerType(v as typeof triggerType)}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="data_change">{t('automation:ruleBuilder.triggerDevice')}</SelectItem>
                <SelectItem value="schedule">{t('automation:ruleBuilder.triggerSchedule')}</SelectItem>
                <SelectItem value="manual">{t('automation:ruleBuilder.triggerManual')}</SelectItem>
              </SelectContent>
            </Select>
          </Field>

          {/* Tags editor - lifted from BasicInfoStep */}
          <Field>
            <FieldLabel>{t('automation:ruleBuilder.tags') || 'Tags'}</FieldLabel>
            <div className="flex flex-wrap gap-2 p-2 border border-input bg-card rounded-md min-h-[42px] focus-within:ring-1 focus-within:ring-ring transition-all">
              {tags.map(tag => (
                <Badge key={tag} variant="secondary" className="gap-1 pl-2">
                  {tag}
                  <button
                    type="button"
                    onClick={() => setTags(tags.filter(t => t !== tag))}
                    aria-label={t('automation:ruleBuilder.removeTag') + ': ' + tag}
                    className="rounded-full p-0 hover:bg-muted"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </Badge>
              ))}
              <Input
                type="text"
                value={tagInput}
                onChange={e => setTagInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    const trimmed = tagInput.trim()
                    if (trimmed && !tags.includes(trimmed)) {
                      setTags([...tags, trimmed])
                      setTagInput('')
                    }
                  } else if (e.key === 'Backspace' && !tagInput && tags.length > 0) {
                    setTags(tags.slice(0, -1))
                  }
                }}
                placeholder={tags.length === 0 ? (tBuilder('addTag') || 'Add tag...') : ''}
                className="flex-1 min-w-[80px] border-0 bg-transparent px-0 shadow-none focus-visible:ring-0"
              />
            </div>
          </Field>

          {/* Cooldown */}
          <Field>
            <FieldLabel>{tBuilder('cooldown')}</FieldLabel>
            <div className="flex items-center gap-2">
              <Input
                type="number"
                min={0}
                value={cooldownValue}
                onChange={(e) => setCooldownValue(Math.max(0, parseInt(e.target.value) || 0))}
                className="flex-1"
              />
              <Select value={cooldownUnit} onValueChange={(v) => setCooldownUnit(v as typeof cooldownUnit)}>
                <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="seconds">{tBuilder('seconds')}</SelectItem>
                  <SelectItem value="minutes">{tBuilder('minutes')}</SelectItem>
                  <SelectItem value="hours">{tBuilder('hours')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {conditionUsesVirtualMetric(condition)
                ? tBuilder('cooldownHintVirtualMetric') || 'Min 60s (emitter tick). 5+ min recommended to avoid alert fatigue.'
                : tBuilder('cooldownHint')}
            </p>
          </Field>

          {/* Enabled switch */}
          <div className="flex items-center gap-3">
            <Switch
              id="rule-enabled"
              checked={enabled}
              onCheckedChange={(checked) => setEnabled(!!checked)}
            />
            <Label htmlFor="rule-enabled" className="text-sm font-medium cursor-pointer">
              {tBuilder('enabled')}
            </Label>
          </div>
        </div>
      }
      workspace={ruleWorkspaceContent}
      footer={
        <Button onClick={handleSave} disabled={saving} className="ml-auto">
          {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {t('automation:ruleBuilder.save')}
        </Button>
      }
    />
  )
}

// ============================================================================
// Condition Editor Component
// ============================================================================

interface ConditionEditorProps {
  condition: UICondition
  onChange: (c: UICondition) => void
  devices: Array<{
    id: string
    name: string
    device_type: string
    metrics?: Array<{ name: string; data_type: string; unit?: string | null }>
    commands?: Array<{ name: string; description: string }>
    online?: boolean
  }>
  deviceTypes?: DeviceType[]
  extensions?: Extension[]
  extensionDataSources?: ExtensionDataSourceInfo[]
  transformDataSources?: TransformDataSourceInfo[]
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ConditionEditor({ condition, onChange, devices, deviceTypes, extensions, extensionDataSources, transformDataSources, t, tBuilder }: ConditionEditorProps) {
  const updateField = <K extends keyof UICondition>(field: K, value: UICondition[K]) => {
    onChange({ ...condition, [field]: value })
  }

  const updateNestedCondition = (index: number, updates: Partial<UICondition>) => {
    if (!condition.conditions) return
    const newConditions = [...condition.conditions]
    newConditions[index] = { ...newConditions[index], ...updates }
    onChange({ ...condition, conditions: newConditions })
  }

  const removeNestedCondition = (index: number) => {
    if (!condition.conditions) return
    onChange({ ...condition, conditions: condition.conditions.filter((_, i) => i !== index) })
  }

  // Build device options - only devices
  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name, type: 'device' as const }))

  // Build extension options - only extensions
  const extensionOptions = (extensions || []).map(ext => ({
    value: ext.id,
    label: `${ext.name} (${tBuilder('extension') || 'Extension'})`,
    type: 'extension' as const
  }))

  // Build transform options - unique transforms from data sources
  const transformOptions = (() => {
    const seen = new Map<string, string>()
    for (const ds of transformDataSources || []) {
      if (!seen.has(ds.transform_id)) {
        seen.set(ds.transform_id, ds.transform_name)
      }
    }
    return Array.from(seen.entries()).map(([id, name]) => ({
      value: id,
      label: `${name} (${tBuilder('transform') || 'Transform'})`,
      type: 'transform' as const
    }))
  })()

  // Get current source type (default to device if not set)
  const currentSourceType = condition.source_type || 'device'

  // Render simple condition
  const renderSimpleCondition = (cond: UICondition) => {
    // Get metrics based on source type
    const metrics = cond.source_type === 'extension' && cond.extension_id
      ? getExtensionMetrics(cond.extension_id, extensions || [], extensionDataSources || [])
      : cond.source_type === 'transform' && cond.transform_id
      ? getTransformMetrics(cond.transform_id, transformDataSources || [])
      : cond.source_type === 'device' && cond.device_id
      ? getDeviceMetrics(cond.device_id, devices, deviceTypes)
      : []

    // Resolve the data type for a given metric name under the current source selection.
    // Used both for rendering the operator set and for validating the operator when
    // the user switches metrics (a previously-chosen string operator like "contains"
    // is invalid after switching to a numeric metric).
    const resolveMetricDataType = (metricName: string | undefined): string => {
      if (!metricName) return 'float'
      if (cond.source_type === 'extension' && cond.extension_id) {
        return extensionDataSources?.find((ds: ExtensionDataSourceInfo) =>
          ds.extension_id === cond.extension_id && ds.field === metricName
        )?.data_type || 'float'
      }
      if (cond.source_type === 'transform' && cond.transform_id) {
        return transformDataSources?.find(ds =>
          ds.transform_id === cond.transform_id && ds.metric_name === metricName
        )?.data_type || 'float'
      }
      if (cond.source_type === 'device' && cond.device_id) {
        return getMetricDataTypeForResource(cond.device_id, metricName, devices, deviceTypes, extensions, extensionDataSources)
      }
      return 'float'
    }

    const metricDataType = resolveMetricDataType(cond.metric)

    const isStringType = metricDataType === 'string'
    const isBooleanType = metricDataType === 'boolean'
    const isNumericType = ['integer', 'float'].includes(metricDataType)

    const renderValueInput = () => {
      if (isBooleanType) {
        return (
          <Select
            value={cond.threshold_value ?? String(cond.threshold ?? '')}
            onValueChange={(v) => {
              const boolVal = v === 'true'
              updateField('threshold', boolVal ? 1 : 0)
              updateField('threshold_value', v)
            }}
          >
            <SelectTrigger className="w-20 h-9"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="true">true</SelectItem>
              <SelectItem value="false">false</SelectItem>
            </SelectContent>
          </Select>
        )
      }

      if (isStringType || !isNumericType) {
        return (
          <Input
            type="text"
            value={cond.threshold_value ?? String(cond.threshold ?? '')}
            onChange={e => {
              updateField('threshold_value', e.target.value)
            }}
            className="w-28 h-9"
            disabled={!(cond.source_type === 'extension' ? cond.extension_id : cond.device_id)}
          />
        )
      }

      return (
        <Input
          type="number"
          value={cond.threshold ?? ''}
          onChange={e => updateField('threshold', parseFloat(e.target.value) || 0)}
          className="w-24 h-9"
          disabled={!(cond.source_type === 'extension' ? cond.extension_id : cond.device_id)}
        />
      )
    }

    return (
      <div className="p-3 bg-accent-purple-light rounded-lg border border-accent-purple">
        <div className="flex flex-wrap items-center gap-2">
          {/* Source Type Selector */}
          <Select
            value={cond.source_type || 'device'}
            onValueChange={(v: 'device' | 'extension' | 'transform') => {
              // Clear all IDs when changing source type
              onChange({ ...condition, source_type: v, device_id: undefined, extension_id: undefined, transform_id: undefined, metric: undefined })
            }}
          >
            <SelectTrigger className="w-28 h-9 text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="device">{tBuilder('device') || 'Device'}</SelectItem>
              <SelectItem value="extension">{tBuilder('extension') || 'Extension'}</SelectItem>
              {transformOptions.length > 0 && (
                <SelectItem value="transform">{tBuilder('transform') || 'Transform'}</SelectItem>
              )}
            </SelectContent>
          </Select>

          {/* Device/Extension/Transform Selector */}
          <Select
            value={cond.source_type === 'extension' ? cond.extension_id : cond.source_type === 'transform' ? cond.transform_id : cond.device_id}
            onValueChange={(v) => {
              const st = cond.source_type || 'device'
              const newMetrics = st === 'extension'
                ? getExtensionMetrics(v, extensions || [], extensionDataSources || [])
                : st === 'transform'
                ? getTransformMetrics(v, transformDataSources || [])
                : getDeviceMetrics(v, devices, deviceTypes)

              if (st === 'extension') {
                onChange({ ...condition, extension_id: v, device_id: undefined, transform_id: undefined, metric: newMetrics[0]?.name || 'value' })
              } else if (st === 'transform') {
                onChange({ ...condition, transform_id: v, device_id: undefined, extension_id: undefined, metric: newMetrics[0]?.name || 'value' })
              } else {
                onChange({ ...condition, device_id: v, extension_id: undefined, transform_id: undefined, metric: newMetrics[0]?.name || 'value' })
              }
            }}
          >
            <SelectTrigger className="w-36 h-9 text-sm">
              <SelectValue placeholder={
                cond.source_type === 'extension' ? tBuilder('selectExtension')
                : cond.source_type === 'transform' ? tBuilder('selectTransform')
                : tBuilder('selectDevice')
              } />
            </SelectTrigger>
            <SelectContent>
              {(cond.source_type === 'extension' ? extensionOptions : cond.source_type === 'transform' ? transformOptions : deviceOptions).map(d => (
                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* Metric Selector */}
          {((cond.source_type === 'extension' && cond.extension_id) || (cond.source_type === 'device' && cond.device_id) || (cond.source_type === 'transform' && cond.transform_id)) && metrics.length > 0 ? (
            <Select value={cond.metric} onValueChange={(v) => {
              const newDataType = resolveMetricDataType(v)
              const allowedOps = getComparisonOperators(t, newDataType).map(o => o.value)
              onChange({
                ...condition,
                metric: v,
                operator: (cond.operator && allowedOps.includes(cond.operator) ? cond.operator : allowedOps[0])!,
              })
            }}>
              <SelectTrigger className="w-32 h-9 text-sm"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => (
                  <SelectItem key={m.name} value={m.name}>
                    {m.display_name || m.name}
                  </SelectItem>
                ))}
                {cond.source_type === 'device' && (
                  <SelectGroup>
                    <SelectLabel className="text-muted-foreground">{tBuilder('systemMetrics')}</SelectLabel>
                    <SelectItem value="__last_seen_age_secs">Seconds since last data</SelectItem>
                  </SelectGroup>
                )}
              </SelectContent>
            </Select>
          ) : (
            <span className="text-xs text-muted-foreground italic">
              {cond.source_type === 'extension' ? tBuilder('selectExtensionFirst')
                : cond.source_type === 'transform' ? tBuilder('selectTransformFirst')
                : tBuilder('selectDeviceFirst')}
            </span>
          )}

          {/* Operator Selector */}
          <Select
            value={cond.operator}
            onValueChange={(v) => updateField('operator', v)}
            disabled={!((cond.source_type === 'extension' && cond.extension_id) || (cond.source_type === 'transform' && cond.transform_id) || (cond.source_type === 'device' && cond.device_id))}
          >
            <SelectTrigger className="w-32 h-9 text-sm"><SelectValue /></SelectTrigger>
            <SelectContent>
              {getComparisonOperators(t, metricDataType).map(o => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
            </SelectContent>
          </Select>

          {renderValueInput()}
          <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => onChange(null as any)} aria-label={tBuilder('removeCondition')}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>
    )
  }

  // Render range condition
  const renderRangeCondition = (cond: UICondition) => {
    // Get metrics based on source type
    const metrics = cond.source_type === 'extension' && cond.extension_id
      ? getExtensionMetrics(cond.extension_id, extensions || [], extensionDataSources || [])
      : cond.source_type === 'transform' && cond.transform_id
      ? getTransformMetrics(cond.transform_id, transformDataSources || [])
      : cond.source_type === 'device' && cond.device_id
      ? getDeviceMetrics(cond.device_id, devices, deviceTypes)
      : []

    const hasValidId = (cond.source_type === 'extension' && cond.extension_id) || (cond.source_type === 'device' && cond.device_id) || (cond.source_type === 'transform' && cond.transform_id)

    return (
      <div className="p-3 bg-info-light rounded-lg border border-info">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className="text-xs bg-info-light text-info border-info">{tBuilder('between')}</Badge>

          {/* Source Type Selector */}
          <Select
            value={cond.source_type || 'device'}
            onValueChange={(v: 'device' | 'extension' | 'transform') => {
              onChange({ ...condition, source_type: v, device_id: undefined, extension_id: undefined, transform_id: undefined, metric: undefined })
            }}
          >
            <SelectTrigger className="w-28 h-9 text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="device">{tBuilder('device') || 'Device'}</SelectItem>
              <SelectItem value="extension">{tBuilder('extension') || 'Extension'}</SelectItem>
              {transformOptions.length > 0 && (
                <SelectItem value="transform">{tBuilder('transform') || 'Transform'}</SelectItem>
              )}
            </SelectContent>
          </Select>

          {/* Device/Extension/Transform Selector */}
          <Select
            value={cond.source_type === 'extension' ? cond.extension_id : cond.source_type === 'transform' ? cond.transform_id : cond.device_id}
            onValueChange={(v) => {
              const st = cond.source_type || 'device'
              const newMetrics = st === 'extension'
                ? getExtensionMetrics(v, extensions || [], extensionDataSources || [])
                : st === 'transform'
                ? getTransformMetrics(v, transformDataSources || [])
                : getDeviceMetrics(v, devices, deviceTypes)

              if (st === 'extension') {
                onChange({ ...condition, extension_id: v, device_id: undefined, transform_id: undefined, metric: newMetrics[0]?.name || 'value' })
              } else if (st === 'transform') {
                onChange({ ...condition, transform_id: v, device_id: undefined, extension_id: undefined, metric: newMetrics[0]?.name || 'value' })
              } else {
                onChange({ ...condition, device_id: v, extension_id: undefined, transform_id: undefined, metric: newMetrics[0]?.name || 'value' })
              }
            }}
          >
            <SelectTrigger className="w-36 h-9 text-sm">
              <SelectValue placeholder={
                cond.source_type === 'extension' ? tBuilder('selectExtension')
                : cond.source_type === 'transform' ? tBuilder('selectTransform')
                : tBuilder('selectDevice')
              } />
            </SelectTrigger>
            <SelectContent>
              {(cond.source_type === 'extension' ? extensionOptions : cond.source_type === 'transform' ? transformOptions : deviceOptions).map(d => (
                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* Metric Selector */}
          {hasValidId && metrics.length > 0 ? (
            <Select value={cond.metric} onValueChange={(v) => {
              onChange({
                ...condition,
                metric: v,
              })
            }}>
              <SelectTrigger className="w-32 h-9 text-sm"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>)}
                {cond.source_type === 'device' && (
                  <SelectGroup>
                    <SelectLabel className="text-muted-foreground">{tBuilder('systemMetrics')}</SelectLabel>
                    <SelectItem value="__last_seen_age_secs">Seconds since last data</SelectItem>
                  </SelectGroup>
                )}
              </SelectContent>
            </Select>
          ) : (
            <span className="text-xs text-muted-foreground italic">
              {cond.source_type === 'extension' ? tBuilder('selectExtensionFirst')
                : cond.source_type === 'transform' ? tBuilder('selectTransformFirst')
                : tBuilder('selectDeviceFirst')}
            </span>
          )}

          <span className="text-xs font-medium text-muted-foreground px-1">{tBuilder('between')}</span>
          <Input
            type="number"
            value={cond.range_min}
            onChange={e => updateField('range_min', parseFloat(e.target.value) || 0)}
            className="w-20 h-9"
            placeholder={tBuilder('minPlaceholder')}
            disabled={!hasValidId}
          />
          <span className="text-xs text-muted-foreground">{tBuilder('and')}</span>
          <Input
            type="number"
            value={cond.range_max}
            onChange={e => updateField('range_max', parseFloat(e.target.value) || 0)}
            className="w-20 h-9"
            placeholder={tBuilder('maxPlaceholder')}
            disabled={!hasValidId}
          />
          <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => onChange(null as any)} aria-label={tBuilder('removeCondition')}>
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>
    )
  }

  // Render logical condition (AND/OR/NOT)
  const renderLogicalCondition = () => {
    const label = condition.type.toUpperCase()
    const badgeClass = condition.type === 'and'
      ? 'bg-success-light text-success border-success-light'
      : condition.type === 'or'
      ? 'bg-warning-light text-warning border-warning'
      : 'bg-error-light text-error border-error'

    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2 p-2.5 rounded-t-lg border">
          <Badge variant="outline" className={cn('text-xs px-2.5 py-1', badgeClass)}>{label}</Badge>
          <span className="text-xs text-muted-foreground flex-1">
            {condition.type === 'and' ? tBuilder('allConditionsMustMeet') : condition.type === 'or' ? tBuilder('anyConditionMustMeet') : tBuilder('conditionNotMet')}
          </span>
          <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => onChange(null as any)} aria-label={tBuilder('removeCondition')}>
            <X className="h-4 w-4" />
          </Button>
        </div>

        <div className="p-3 border border-t-0 rounded-b-lg space-y-3">
          {condition.conditions?.map((subCond, i) => (
            <div key={subCond.id} className="relative group">
              {i > 0 && (
                <div className="flex items-center justify-start -mb-2 mt-1">
                  <span className={cn(
                    'text-xs font-semibold px-2.5 py-1 rounded-full',
                    condition.type === 'and' ? 'bg-success-light text-success dark:bg-success-light dark:text-success' : 'bg-warning-light text-warning'
                  )}>
                    {condition.type.toUpperCase()}
                  </span>
                </div>
              )}
              <div className="relative pr-8">
                <div className="rounded-lg">
                  <ConditionEditor
                    condition={subCond}
                    onChange={(c) => updateNestedCondition(i, c)}
                    devices={devices}
                    deviceTypes={deviceTypes}
                    extensions={extensions}
                    extensionDataSources={extensionDataSources}
                    transformDataSources={transformDataSources}
                    t={t}
                    tBuilder={tBuilder}
                  />
                </div>
                {condition.conditions && condition.conditions.length > 1 && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6 absolute right-0 top-2 opacity-0 group-hover:opacity-100"
                    onClick={() => removeNestedCondition(i)}
                    aria-label={tBuilder('removeCondition')}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                )}
              </div>
            </div>
          ))}

          <div className="pt-2 border-t border-border">
            <Button
              variant="outline"
              size="sm"
              className="w-full border-dashed h-9"
              onClick={() => {
                // Try devices first, then extensions
                const firstDevice = devices[0]
                const firstExtension = extensions?.[0]

                let newCond: UICondition

                if (firstDevice) {
                  const metrics = getDeviceMetrics(firstDevice.id, devices, deviceTypes)
                  newCond = {
                    id: generateId(),
                    type: 'simple',
                    source_type: 'device',
                    device_id: firstDevice.id,
                    metric: metrics[0]?.name || 'value',
                    operator: '>',
                    threshold: 0,
                  }
                } else if (firstExtension) {
                  const metrics = getExtensionMetrics(firstExtension.id, extensions || [], extensionDataSources || [])
                  newCond = {
                    id: generateId(),
                    type: 'simple',
                    source_type: 'extension',
                    extension_id: firstExtension.id,
                    metric: metrics[0]?.name || 'value',
                    operator: '>',
                    threshold: 0,
                  }
                } else {
                  // Fallback - create empty condition
                  newCond = {
                    id: generateId(),
                    type: 'simple',
                    source_type: 'device',
                    operator: '>',
                    threshold: 0,
                  }
                }

                onChange({
                  ...condition,
                  conditions: [...(condition.conditions || []), newCond]
                })
              }}
            >
              <Plus className="h-4 w-4 mr-1" />{tBuilder('addCondition')}
            </Button>
          </div>
        </div>
      </div>
    )
  }

  switch (condition.type) {
    case 'simple': return renderSimpleCondition(condition)
    case 'range': return renderRangeCondition(condition)
    case 'and':
    case 'or':
    case 'not': return renderLogicalCondition()
    default: return null
  }
}

// ============================================================================
// Helper Functions for Set Action
// ============================================================================

function getAvailableMetricsForDevice(
  deviceId: string,
  devices: Array<{ id: string; name: string; metrics?: Array<{ name: string; data_type: string; unit?: string | null }> }>,
  extensions?: Extension[]
): Array<{ name: string; data_type: string; unit?: string | null }> {
  // Find device metrics
  const device = devices.find(d => d.id === deviceId)
  if (device?.metrics) {
    return device.metrics
  }

  // Check if it's an extension
  if (extensions && deviceId.startsWith('extension:')) {
    const extId = deviceId.replace('extension:', '').split(':')[0]
    const extension = extensions.find(e => e.id === extId)
    if (extension?.metrics) {
      return extension.metrics.map(m => ({
        name: m.name,
        data_type: m.data_type,
        unit: m.unit
      }))
    }
  }

  return []
}

function getMetricDataTypeForSet(
  deviceId: string,
  propertyName: string,
  devices: Array<{ id: string; name: string; metrics?: Array<{ name: string; data_type: string; unit?: string | null }> }>,
  extensions?: Extension[]
): string {
  const metrics = getAvailableMetricsForDevice(deviceId, devices, extensions)
  const metric = metrics.find(m => m.name === propertyName)
  return metric?.data_type || 'string'
}

interface MetricValueInputProps {
  deviceId: string
  propertyName: string
  value: unknown
  devices: Array<{ id: string; name: string; metrics?: Array<{ name: string; data_type: string; unit?: string | null }> }>
  extensions?: Extension[]
  onChange: (value: unknown) => void
  tBuilder: (key: string) => string
}

function MetricValueInput({ deviceId, propertyName, value, devices, extensions, onChange, tBuilder }: MetricValueInputProps) {
  const dataType = propertyName
    ? getMetricDataTypeForSet(deviceId, propertyName, devices, extensions)
    : 'string'

  if (dataType === 'boolean' || dataType === 'bool') {
    return (
      <Select
        value={value === undefined ? '' : String(value)}
        onValueChange={(v) => onChange(v === 'true')}
      >
        <SelectTrigger className="w-20 h-9 text-sm flex-shrink-0">
          <SelectValue placeholder={tBuilder('valuePlaceholder')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="true">true</SelectItem>
          <SelectItem value="false">false</SelectItem>
        </SelectContent>
      </Select>
    )
  }

  if (dataType === 'number' || dataType === 'integer' || dataType === 'float') {
    return (
      <Input
        type="number"
        value={String(value ?? '')}
        onChange={(e) => onChange(parseFloat(e.target.value) || 0)}
        placeholder={tBuilder('valuePlaceholder')}
        className="w-24 h-9 text-sm flex-shrink-0"
      />
    )
  }

  // Default: string input
  return (
    <Input
      type="text"
      value={String(value ?? '')}
      onChange={(e) => onChange(e.target.value)}
      placeholder={tBuilder('valuePlaceholder')}
      className="w-24 h-9 text-sm flex-shrink-0"
    />
  )
}

// ============================================================================
// Action Editor Component
// ============================================================================

interface ActionEditorCompactProps {
  action: RuleAction
  index: number
  devices: Array<{
    id: string
    name: string
    device_type: string
    commands?: Array<{ name: string; description: string }>
    metrics?: Array<{ name: string; data_type: string; unit?: string | null }>
  }>
  deviceTypes?: DeviceType[]
  extensions?: Extension[]
  messageChannels?: Array<{ name: string; type: string; enabled: boolean }>
  agents?: Array<{ id: string; name: string }>
  t: (key: string) => string
  tBuilder: (key: string) => string
  onUpdate: (data: Partial<RuleAction>) => void
  onRemove: () => void
  error?: string
}

function ActionEditorCompact({ action, devices, deviceTypes, extensions, messageChannels, agents, t, tBuilder, onUpdate, onRemove, error }: ActionEditorCompactProps) {
  // Build device/extension options for Execute action
  const deviceOptions = [
    ...devices.map(d => ({ value: d.id, label: d.name, type: 'device' as const })),
    ...(extensions || []).map(ext => ({
      value: `extension:${ext.id}`,
      label: `${ext.name} (Extension)`,
      type: 'extension' as const
    }))
  ]

  const renderActionContent = () => {
    switch (action.type) {
      case 'execute': {
        const executeAction = action as { type: 'execute'; target: string; target_type: 'device' | 'extension'; command: string; params: Record<string, unknown> }
        const commands = getCommandsForResource(executeAction.target || '', devices, deviceTypes, extensions)
        const isExt = executeAction.target && isExtensionId(executeAction.target)
        const selectedCmd = commands.find(c => c.name === executeAction.command)
        const cmdParams = selectedCmd?.parameters || []
        const currentParams = executeAction.params || {}

        return (
          <div className="space-y-2 w-full">
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground w-20">
                {isExt ? t('extensions:extension') : t('automation:device')}:
              </span>
              <Select
                value={executeAction.target}
                onValueChange={(v) => {
                  const cmds = getCommandsForResource(v, devices, deviceTypes, extensions)
                  const targetType = v && isExtensionId(v) ? 'extension' as const : 'device' as const
                  onUpdate({ target: v, target_type: targetType, command: cmds[0]?.name || 'turn_on', params: {} })
                }}
              >
                <SelectTrigger className="h-8 text-sm flex-1 max-w-xs">
                  <SelectValue placeholder={isExt ? tBuilder('selectExtension') : t('automation:selectDevice')} />
                </SelectTrigger>
                <SelectContent>
                  {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground w-20">{t('dashboardComponents:ruleBuilder.command')}:</span>
              <Select value={executeAction.command} onValueChange={(v) => onUpdate({ command: v, params: {} })}>
                <SelectTrigger className="h-8 text-sm flex-1 max-w-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {commands.map(c => (
                    <SelectItem key={c.name} value={c.name}>{c.display_name || c.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {/* Command Parameters */}
            {cmdParams.length > 0 && (
              <div className="space-y-1.5 pl-2 border-l-2 border-muted ml-1">
                {cmdParams.map(param => {
                  const paramType = param.param_type?.toLowerCase() || 'string'
                  const isBool = paramType.includes('bool')
                  const isNum = paramType.includes('int') || paramType.includes('float') || paramType.includes('number')
                  return (
                    <div key={param.name} className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground w-20 truncate" title={param.name}>
                        {param.name}:
                      </span>
                      {isBool ? (
                        <Select
                          value={String(currentParams[param.name] ?? param.default_value ?? 'false')}
                          onValueChange={(v) => {
                            const newParams = { ...currentParams, [param.name]: v === 'true' }
                            onUpdate({ params: newParams })
                          }}
                        >
                          <SelectTrigger className="h-8 text-sm flex-1 max-w-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="true">true</SelectItem>
                            <SelectItem value="false">false</SelectItem>
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          type={isNum ? 'number' : 'text'}
                          value={String(currentParams[param.name] ?? param.default_value ?? (isNum ? 0 : ''))}
                          onChange={(e) => {
                            const val = isNum ? (parseFloat(e.target.value) || 0) : e.target.value
                            const newParams = { ...currentParams, [param.name]: val }
                            onUpdate({ params: newParams })
                          }}
                          className="h-8 text-sm flex-1 max-w-xs"
                          placeholder={param.name}
                        />
                      )}
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        )
      }

      case 'notify': {
        const notifyAction = action as { type: 'notify'; message: string; severity: 'info' | 'warning' | 'critical' | 'emergency' }
        return (
          <div className="space-y-2 w-full">
            <Input
              value={notifyAction.message}
              onChange={(e) => onUpdate({ message: e.target.value })}
              placeholder={tBuilder('notificationContentPlaceholder')}
              className="h-8 text-sm"
            />
            <Select value={notifyAction.severity} onValueChange={(v) => onUpdate({ severity: v as 'info' | 'warning' | 'critical' | 'emergency' })}>
              <SelectTrigger className="w-28 h-8 text-sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="info">{t('dashboardComponents:ruleBuilder.severity.info')}</SelectItem>
                <SelectItem value="warning">{t('dashboardComponents:ruleBuilder.severity.warning')}</SelectItem>
                <SelectItem value="critical">{t('dashboardComponents:ruleBuilder.severity.critical')}</SelectItem>
                <SelectItem value="emergency">{t('dashboardComponents:ruleBuilder.severity.emergency') || 'Emergency'}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        )
      }

      case 'trigger_agent': {
        const agentAction = action as { type: 'trigger_agent'; agent_id: string; input?: string }
        const availableAgents = agents || []
        return (
          <div className="space-y-2 w-full">
            <Select
              value={agentAction.agent_id}
              onValueChange={(value) => onUpdate({ agent_id: value })}
            >
              <SelectTrigger className="h-8 text-sm">
                <SelectValue placeholder={t('automation:selectAgent') || 'Select Agent'} />
              </SelectTrigger>
              <SelectContent>
                {availableAgents.map((agent) => (
                  <SelectItem key={agent.id} value={agent.id}>
                    {agent.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {availableAgents.length === 0 && (
              <p className="text-xs text-muted-foreground">{t('automation:noAgentsAvailable') || 'No agents available'}</p>
            )}
            <Input
              value={agentAction.input || ''}
              onChange={(e) => onUpdate({ input: e.target.value })}
              placeholder={t('automation:agentInputPlaceholder') || 'Optional input'}
              className="h-8 text-sm"
            />
          </div>
        )
      }

      default:
        return null
    }
  }

  const getActionIcon = () => {
    switch (action.type) {
      case 'execute': return <Zap className="h-4 w-4" />
      case 'notify': return <Bell className="h-4 w-4" />
      case 'trigger_agent': return <Bot className="h-4 w-4" />
      default: return <Zap className="h-4 w-4" />
    }
  }

  const getActionLabel = (): string => {
    switch (action.type) {
      case 'execute': return tBuilder('executeCommand')
      case 'notify': return tBuilder('sendNotification')
      case 'trigger_agent': return tBuilder('triggerAgent') || 'Trigger Agent'
    }
  }

  const getActionColor = (): string => {
    switch (action.type) {
      case 'execute': return 'text-warning bg-warning-light border-warning'
      case 'notify': return 'text-info bg-info-light border-info'
      case 'trigger_agent': return 'text-accent-purple bg-accent-purple-light border-accent-purple-light'
      default: return 'text-muted-foreground bg-muted border-border'
    }
  }

  return (
    <div className={cn(
      "group rounded-lg border bg-card p-3 shadow-sm transition-all hover:shadow-md",
      getActionColor()
    )}>
      <div className="flex items-start gap-3">
        {/* Action Icon */}
        <div className={cn(
          "flex items-center justify-center w-8 h-8 rounded-full flex-shrink-0",
          getActionColor().replace('text-', 'bg-opacity-10 ')
        )}>
          {getActionIcon()}
        </div>

        {/* Action Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between mb-1">
            <span className="text-sm font-medium">{getActionLabel()}</span>
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
              onClick={onRemove}
              aria-label={tBuilder('removeAction')}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
          {renderActionContent()}
          {error && (
            <p className="text-xs text-error mt-2">{error}</p>
          )}
        </div>
      </div>
    </div>
  )
}

