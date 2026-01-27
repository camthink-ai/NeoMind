/**
 * SimpleRuleBuilderSplit Component
 *
 * Step-by-step dialog for creating/editing automation rules.
 * Following the same pattern as DeviceTypeDialog.
 *
 * @module automation
 */

import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import {
  Plus,
  X,
  Zap,
  Bell,
  FileText,
  Save,
  Lightbulb,
  Clock,
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  Check,
  Settings,
  Eye,
  Globe,
  Timer,
  Code,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType } from '@/types'

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
  }
}

type Step = 'basic' | 'condition' | 'action' | 'review'
type ConditionType = 'simple' | 'range' | 'and' | 'or' | 'not'

// ============================================================================
// UI Condition Types
// ============================================================================

interface UICondition {
  id: string
  type: ConditionType
  device_id?: string
  metric?: string
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
}

// ============================================================================
// Helper Functions
// ============================================================================

const NUMERIC_OPERATORS = [
  { value: '>', label: 'Greater than', symbol: '>' },
  { value: '<', label: 'Less than', symbol: '<' },
  { value: '>=', label: 'Greater or equal', symbol: '≥' },
  { value: '<=', label: 'Less or equal', symbol: '≤' },
]

const STRING_OPERATORS = [
  { value: '==', label: 'Equals', symbol: '=' },
  { value: '!=', label: 'Not equals', symbol: '≠' },
  { value: 'contains', label: 'Contains', symbol: '∋' },
  { value: 'starts_with', label: 'Starts with', symbol: 'a*' },
  { value: 'ends_with', label: 'Ends with', symbol: '*z' },
  { value: 'regex', label: 'Regex', symbol: '.*' },
]

const BOOLEAN_OPERATORS = [
  { value: '==', label: 'Equals', symbol: '=' },
  { value: '!=', label: 'Not equals', symbol: '≠' },
]

const getComparisonOperators = (_t: (key: string) => string, dataType?: string) => {
  if (dataType === 'string') return [...NUMERIC_OPERATORS, ...STRING_OPERATORS]
  if (dataType === 'boolean') return BOOLEAN_OPERATORS
  return [...NUMERIC_OPERATORS, { value: '==', label: 'Equals', symbol: '=' }, { value: '!=', label: 'Not equals', symbol: '≠' }]
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

function uiConditionToRuleCondition(cond: UICondition): RuleCondition {
  switch (cond.type) {
    case 'simple': {
      let thresholdValue: number | string
      if (cond.threshold_value !== undefined) {
        const parsed = Number(cond.threshold_value)
        if (!isNaN(parsed) && cond.operator !== 'contains' && cond.operator !== 'starts_with' && cond.operator !== 'ends_with' && cond.operator !== 'regex') {
          thresholdValue = parsed
        } else {
          thresholdValue = cond.threshold_value
        }
      } else {
        thresholdValue = cond.threshold ?? 0
      }
      return {
        device_id: cond.device_id || '',
        metric: cond.metric || 'value',
        operator: cond.operator || '>',
        threshold: thresholdValue,
      }
    }
    case 'range':
      return {
        device_id: cond.device_id || '',
        metric: cond.metric || 'value',
        operator: 'between',
        threshold: cond.range_max || 0,
        range_min: cond.range_min,
      } as RuleCondition
    case 'and':
      return {
        operator: 'and',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    case 'or':
      return {
        operator: 'or',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    case 'not':
      return {
        operator: 'not',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    default:
      return {
        device_id: '',
        metric: 'value',
        operator: '>',
        threshold: 0,
      }
  }
}

function ruleConditionToUiCondition(
  ruleCond?: RuleCondition,
  devices?: Array<{ id: string; name: string; device_type?: string }>,
  dsl?: string
): UICondition {
  if (!ruleCond) {
    return {
      id: crypto.randomUUID(),
      type: 'simple',
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
        id: crypto.randomUUID(),
        type: op,
        conditions: ((ruleCond as any).conditions || []).map((c: RuleCondition) => ruleConditionToUiCondition(c, devices, dsl)),
      }
    }
    if (op === 'not') {
      return {
        id: crypto.randomUUID(),
        type: 'not',
        conditions: [(ruleCond as any).conditions?.[0]].map((c: RuleCondition) => ruleConditionToUiCondition(c, devices, dsl)).filter(Boolean),
      }
    }
  }

  // Check for range condition (has range_min)
  if ('range_min' in ruleCond && (ruleCond as any).range_min !== undefined) {
    const thresholdVal = ruleCond.threshold
    const rangeMax = typeof thresholdVal === 'number' ? thresholdVal :
                     typeof thresholdVal === 'string' ? parseFloat(thresholdVal) || 0 : 0
    let deviceId = ruleCond.device_id || ''
    let metric = ruleCond.metric || 'value'

    // Try to reconstruct device_id from DSL if missing
    if (!deviceId && dsl && devices) {
      const reconstructed = reconstructDeviceIdFromCondition(ruleCond, dsl, devices)
      deviceId = reconstructed.device_id
      metric = reconstructed.metric || metric
    }

    return {
      id: crypto.randomUUID(),
      type: 'range',
      device_id: deviceId,
      metric: metric,
      range_min: (ruleCond as any).range_min,
      range_max: rangeMax,
    }
  }

  // Simple condition
  const thresholdValue = ruleCond.threshold
  const isStringThreshold = typeof thresholdValue === 'string'
  let deviceId = ruleCond.device_id || ''
  let metric = ruleCond.metric || 'value'

  // Try to reconstruct device_id from DSL if missing
  if (!deviceId && dsl && devices) {
    const reconstructed = reconstructDeviceIdFromCondition(ruleCond, dsl, devices)
    deviceId = reconstructed.device_id
    metric = reconstructed.metric || metric
  }

  return {
    id: crypto.randomUUID(),
    type: 'simple',
    device_id: deviceId,
    metric: metric,
    operator: ruleCond.operator || '>',
    threshold: isStringThreshold ? undefined : typeof thresholdValue === 'number' ? thresholdValue : 0,
    threshold_value: isStringThreshold ? thresholdValue : undefined,
  }
}

// Helper to reconstruct device_id and metric from DSL
// The backend parses DSL but may lose device_id, so we need to match device names back to IDs
function reconstructDeviceIdFromCondition(
  ruleCond: RuleCondition,
  dsl: string,
  devices: Array<{ id: string; name: string; device_type?: string }>
): { device_id: string; metric?: string } {
  // If we already have device_id, return it
  if (ruleCond.device_id) {
    return { device_id: ruleCond.device_id, metric: ruleCond.metric }
  }

  // The DSL format is: "DeviceName.metric operator threshold"
  // or for range: "DeviceName.metric BETWEEN min AND max"
  const dslLines = dsl.split('\n')
  const whenLine = dslLines.find(line => line.trim().startsWith('WHEN'))
  if (!whenLine) return { device_id: '' }

  // Extract the condition part after "WHEN"
  const conditionPart = whenLine.replace(/^WHEN\s+/i, '').trim()

  // Try to parse the condition to extract device name and metric
  // Format: "DeviceName.metric operator threshold" or "(conditions) operator (conditions)"
  // For range: "DeviceName.metric BETWEEN min AND max"

  // Try range format first
  const rangeMatch = conditionPart.match(/(\S+)\s+BETWEEN\s+(\d+)\s+AND\s+(\d+)/i)
  if (rangeMatch) {
    const [_, path, min, max] = rangeMatch
    const result = parseDeviceMetricPath(path, devices)
    return { device_id: result.device_id, metric: result.metric }
  }

  // Try simple format: DeviceName.metric operator threshold
  const simpleMatch = conditionPart.match(/(\S+\.\S+)\s*([<>=!]+)\s*(.+)/)
  if (simpleMatch) {
    const [_, path, operator, threshold] = simpleMatch
    const result = parseDeviceMetricPath(path, devices)
    return { device_id: result.device_id, metric: result.metric }
  }

  return { device_id: '' }
}

// Parse device.metric path and match to device_id
function parseDeviceMetricPath(
  path: string,
  devices: Array<{ id: string; name: string; device_type?: string }>
): { device_id: string; metric: string } {
  const parts = path.split('.')
  if (parts.length < 2) return { device_id: '', metric: 'value' }

  const deviceName = parts[0]
  const metric = parts.slice(1).join('.')

  // Try exact name match first
  const exactMatch = devices.find(d => d.name === deviceName)
  if (exactMatch) {
    return { device_id: exactMatch.id, metric }
  }

  // Try case-insensitive match
  const caseMatch = devices.find(d => d.name.toLowerCase() === deviceName.toLowerCase())
  if (caseMatch) {
    return { device_id: caseMatch.id, metric }
  }

  // Try matching device_type if device_name is a type name
  const typeMatch = devices.find(d => d.device_type?.toLowerCase() === deviceName.toLowerCase())
  if (typeMatch) {
    return { device_id: typeMatch.id, metric }
  }

  // Try partial match (device name contains the DSL name)
  const partialMatch = devices.find(d => d.name.toLowerCase().includes(deviceName.toLowerCase()))
  if (partialMatch) {
    return { device_id: partialMatch.id, metric }
  }

  // Try reverse: DSL name contains device name
  const reverseMatch = devices.find(d => deviceName.toLowerCase().includes(d.name.toLowerCase()))
  if (reverseMatch) {
    return { device_id: reverseMatch.id, metric }
  }

  return { device_id: '', metric }
}

// Helper to get device name from ID
function getDeviceNameById(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>
): string {
  const device = devices.find(d => d.id === deviceId)
  return device?.name || deviceId
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

function generateRuleDSL(
  name: string,
  condition: RuleCondition,
  actions: RuleAction[],
  devices: Array<{ id: string; name: string; device_type?: string }>,
  forDuration?: number,
  forUnit?: 'seconds' | 'minutes' | 'hours'
): string {
  const lines: string[] = []
  lines.push(`RULE "${name}"`)
  lines.push(`WHEN ${conditionToDSL(condition, devices)}`)
  if (forDuration && forDuration > 0) {
    const unit = forUnit === 'seconds' ? 'seconds' : forUnit === 'hours' ? 'hours' : 'minutes'
    lines.push(`FOR ${forDuration} ${unit}`)
  }
  lines.push('DO')
  for (const action of actions) {
    lines.push(`    ${actionToDSL(action, devices)}`)
  }
  lines.push('END')
  return lines.join('\n')
}

function parseForClauseFromDSL(dsl?: string): { duration: number; unit: 'seconds' | 'minutes' | 'hours' } | null {
  if (!dsl) return null
  const forMatch = dsl.match(/^FOR\s+(\d+)\s+(seconds|minutes|hours)$/m)
  if (forMatch) {
    const duration = parseInt(forMatch[1], 10)
    const unit = forMatch[2] as 'seconds' | 'minutes' | 'hours'
    return { duration, unit }
  }
  return null
}

function conditionToDSL(
  cond: RuleCondition,
  devices: Array<{ id: string; name: string; device_type?: string }>
): string {
  const op = (cond as any).operator
  if (op === 'and' || op === 'or') {
    const subConds = ((cond as any).conditions || []) as RuleCondition[]
    if (subConds.length === 0) return 'true'
    const parts = subConds.map(c => conditionToDSL(c, devices))
    return `(${parts.join(`) ${op.toUpperCase()} (`)})`
  }
  if (op === 'not') {
    const subConds = ((cond as any).conditions || []) as RuleCondition[]
    if (subConds.length === 0) return 'false'
    return `NOT (${conditionToDSL(subConds[0], devices)})`
  }
  if ('range_min' in cond && (cond as any).range_min !== undefined) {
    const deviceName = getDeviceNameById(cond.device_id || '', devices)
    const metric = getMetricPath(cond.metric || 'value', cond.device_id || '', devices)
    const min = (cond as any).range_min ?? 0
    // Use range_max if available (from UI), otherwise fall back to threshold
    const max = 'range_max' in cond ? ((cond as any).range_max ?? 100) :
                typeof cond.threshold === 'number' ? cond.threshold : 100
    return `${deviceName}.${metric} BETWEEN ${min} AND ${max}`
  }
  const deviceName = getDeviceNameById(cond.device_id || '', devices)
  const metric = getMetricPath(cond.metric || 'value', cond.device_id || '', devices)
  const operator = cond.operator || '>'
  let threshold = cond.threshold ?? 0
  if (typeof threshold === 'string') {
    threshold = `"${threshold}"`
  }
  return `${deviceName}.${metric} ${operator} ${threshold}`
}

function actionToDSL(
  action: RuleAction,
  devices: Array<{ id: string; name: string; device_type?: string }>
): string {
  switch (action.type) {
    case 'Notify': return `NOTIFY "${action.message}"`
    case 'Execute':
      const deviceName = getDeviceNameById(action.device_id || '', devices)
      const params = action.params && Object.keys(action.params).length > 0
        ? Object.entries(action.params).map(([k, v]) => `${k}=${v}`).join(', ')
        : ''
      return params ? `EXECUTE ${deviceName}.${action.command}(${params})` : `EXECUTE ${deviceName}.${action.command}`
    case 'Log': return `LOG ${action.level || 'info'}, "${action.message}"`
    case 'Set':
      const setDeviceName = getDeviceNameById(action.device_id || '', devices)
      const value = typeof action.value === 'string' ? `"${action.value}"` : String(action.value)
      return `SET ${setDeviceName}.${action.property} = ${value}`
    case 'Delay': return `DELAY ${action.duration}ms`
    case 'CreateAlert': return `ALERT "${action.title}" "${action.message}" ${action.severity || 'info'}`
    case 'HttpRequest': return `HTTP ${action.method} ${action.url}`
    default: return '// Unknown action'
  }
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
  const { t } = useTranslation(['automation', 'common'])
  const tBuilder = (key: string) => t(`automation:ruleBuilder.${key}`)
  const isEditMode = !!rule

  // Step state
  const [currentStep, setCurrentStep] = useState<Step>('basic')
  const [completedSteps, setCompletedSteps] = useState<Set<Step>>(new Set())

  // Form data
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [condition, setCondition] = useState<UICondition | null>(null)
  const [forDuration, setForDuration] = useState<number>(0)
  const [forUnit, setForUnit] = useState<'seconds' | 'minutes' | 'hours'>('minutes')
  const [actions, setActions] = useState<RuleAction[]>([])
  const [saving, setSaving] = useState(false)
  const [formErrors, setFormErrors] = useState<FormErrors>({})

  // Reset when dialog opens or rule changes
  useEffect(() => {
    if (open) {
      setCurrentStep('basic')
      setCompletedSteps(new Set())

      if (rule) {
        console.log('[DEBUG] Loading rule into form:', rule)
        console.log('[DEBUG] Rule condition:', rule.condition)
        console.log('[DEBUG] Rule source:', (rule as any).source)
        console.log('[DEBUG] Rule actions:', rule.actions)
        setName(rule.name || '')
        setDescription(rule.description || '')
        setEnabled(rule.enabled ?? true)
        setFormErrors({})

        // Try to restore from source.uiCondition first (exact restoration)
        const sourceUiCond = (rule as any).source?.uiCondition
        if (sourceUiCond) {
          console.log('[DEBUG] Restoring from source.uiCondition:', sourceUiCond)
          setCondition(sourceUiCond)
        } else if (rule.condition) {
          // Fall back to converting the condition
          // Pass devices and dsl to help reconstruct device_id if missing
          const uiCond = ruleConditionToUiCondition(rule.condition, resources.devices, rule.dsl)
          console.log('[DEBUG] Converted UI condition from rule.condition:', uiCond)
          setCondition(uiCond)
        } else {
          console.log('[DEBUG] No condition in rule, setting to null')
          setCondition(null)
        }

        // Restore actions - prefer source.uiActions for exact restoration
        const sourceUiActions = (rule as any).source?.uiActions
        if (sourceUiActions && sourceUiActions.length > 0) {
          console.log('[DEBUG] Restoring from source.uiActions:', sourceUiActions)
          setActions(sourceUiActions)
        } else if (rule.actions && rule.actions.length > 0) {
          // Validate and clean up actions to ensure correct structure
          const cleanedActions: RuleAction[] = rule.actions.map(action => {
            // Ensure action has correct structure based on type
            switch (action.type) {
              case 'Log':
                return { type: 'Log', level: (action as any).level || 'info', message: (action as any).message || 'Rule triggered' } as RuleAction
              case 'Notify':
                return { type: 'Notify', message: (action as any).message || '' } as RuleAction
              case 'Execute':
                return { type: 'Execute', device_id: (action as any).device_id || '', command: (action as any).command || '', params: (action as any).params || {} } as RuleAction
              case 'CreateAlert':
                return { type: 'CreateAlert', title: (action as any).title || '', message: (action as any).message || '', severity: (action as any).severity || 'info' } as RuleAction
              case 'Set':
                return { type: 'Set', device_id: (action as any).device_id || '', property: (action as any).property || 'state', value: (action as any).value ?? true } as RuleAction
              case 'Delay':
                return { type: 'Delay', duration: (action as any).duration || 1000 } as RuleAction
              case 'HttpRequest':
                return { type: 'HttpRequest', method: (action as any).method || 'GET', url: (action as any).url || '' } as RuleAction
              default:
                // Unknown action type, default to Log
                return { type: 'Log', level: 'info', message: 'Rule triggered' } as RuleAction
            }
          })
          setActions(cleanedActions)
        } else {
          setActions([])
        }

        // Restore forDuration and forUnit - prefer source values
        const sourceForDuration = (rule as any).source?.forDuration
        const sourceForUnit = (rule as any).source?.forUnit
        if (sourceForDuration !== undefined && sourceForUnit !== undefined) {
          setForDuration(sourceForDuration)
          setForUnit(sourceForUnit)
        } else {
          const forClause = parseForClauseFromDSL(rule.dsl)
          if (forClause) {
            setForDuration(forClause.duration)
            setForUnit(forClause.unit)
          } else {
            setForDuration(0)
            setForUnit('minutes')
          }
        }
      } else {
        resetForm()
      }
    }
  }, [open, rule, resources.devices, resources.deviceTypes])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setCondition(null)
    setForDuration(0)
    setForUnit('minutes')
    // Use a fixed default message instead of translation to avoid issues
    setActions([{ type: 'Log', level: 'info', message: 'Rule triggered' }])
    setFormErrors({})
  }, [])

  const createDefaultCondition = useCallback((): UICondition => {
    const firstDevice = resources.devices[0]
    if (!firstDevice) {
      return {
        id: crypto.randomUUID(),
        type: 'simple',
        device_id: '',
        metric: 'value',
        operator: '>',
        threshold: 0,
      }
    }
    const metrics = getDeviceMetrics(firstDevice.id, resources.devices, resources.deviceTypes)
    return {
      id: crypto.randomUUID(),
      type: 'simple',
      device_id: firstDevice.id,
      metric: metrics[0]?.name || 'value',
      operator: '>',
      threshold: 0,
    }
  }, [resources.devices, resources.deviceTypes])

  // Validate current step
  const validateStep = (step: Step): boolean => {
    const errors: FormErrors = {}

    if (step === 'basic') {
      if (!name.trim()) {
        errors.name = tBuilder('ruleNameRequired')
      }
    }

    if (step === 'condition') {
      if (!condition) {
        errors.condition = [tBuilder('addTriggerCondition')]
      } else {
        const validateCondition = (cond: UICondition): string[] => {
          const errs: string[] = []
          if (cond.type === 'simple' || cond.type === 'range') {
            if (!cond.device_id) errs.push(tBuilder('selectDevice'))
            if (!cond.metric) errs.push(tBuilder('selectMetric'))
            if (cond.type === 'simple') {
              const hasValue = cond.threshold !== undefined || cond.threshold_value !== undefined
              if (!hasValue) errs.push(tBuilder('enterThreshold'))
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

  // Navigate to next step
  const handleNext = () => {
    if (!validateStep(currentStep)) return

    const newCompleted = new Set(completedSteps)
    newCompleted.add(currentStep)
    setCompletedSteps(newCompleted)

    const steps: Step[] = ['basic', 'condition', 'action', 'review']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex < steps.length - 1) {
      setCurrentStep(steps[currentIndex + 1])
    }
  }

  // Navigate to previous step
  const handlePrevious = () => {
    const steps: Step[] = ['basic', 'condition', 'action', 'review']
    const currentIndex = steps.indexOf(currentStep)
    if (currentIndex > 0) {
      setCurrentStep(steps[currentIndex - 1])
    }
  }

  // Save
  const handleSave = async () => {
    if (!name.trim() || !condition) return

    setSaving(true)
    try {
      const finalCondition = uiConditionToRuleCondition(condition)
      const dsl = generateRuleDSL(name, finalCondition, actions, resources.devices, forDuration, forUnit)
      const ruleData: Partial<Rule> = {
        name,
        description,
        enabled,
        trigger: { type: 'device_state' } as RuleTrigger,
        condition: finalCondition,
        actions: actions.length > 0 ? actions : undefined,
        dsl,
        // Store original UI state in source field for proper restoration on edit
        source: {
          condition: finalCondition,
          uiCondition: condition, // Store the UI condition for exact restoration
          uiActions: actions, // Store the UI actions for exact restoration
          forDuration,
          forUnit,
        },
      }
      if (rule?.id) ruleData.id = rule.id
      await onSave(ruleData)
    } finally {
      setSaving(false)
    }
  }

  // Step config
  const steps: { key: Step; label: string; icon: React.ReactNode }[] = [
    { key: 'basic', label: tBuilder('steps.basic'), icon: <Settings className="h-4 w-4" /> },
    { key: 'condition', label: tBuilder('steps.condition'), icon: <Lightbulb className="h-4 w-4" /> },
    { key: 'action', label: tBuilder('steps.action'), icon: <Zap className="h-4 w-4" /> },
    { key: 'review', label: tBuilder('steps.review'), icon: <Eye className="h-4 w-4" /> },
  ]

  const stepIndex = steps.findIndex(s => s.key === currentStep)
  const isFirstStep = currentStep === 'basic'

  // Generate preview DSL
  const previewDSL = condition ? generateRuleDSL(name || tBuilder('name'), uiConditionToRuleCondition(condition), actions, resources.devices, forDuration, forUnit) : ''

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[90vh] max-h-[90vh] flex flex-col p-0 overflow-hidden [&>[data-radix-dialog-close]]:right-6 [&>[data-radix-dialog-close]]:top-5">
        {/* Header */}
        <DialogHeader className="px-6 pt-4 pb-4 border-b">
          <DialogTitle className="text-xl flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-purple-500/10 flex items-center justify-center">
              <Zap className="h-5 w-5 text-purple-500" />
            </div>
            {isEditMode ? t('automation:edit') : t('automation:createRule')}
          </DialogTitle>
        </DialogHeader>

        {/* Step Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Step Indicator */}
          <div className="flex items-center justify-center gap-2">
            {steps.map((step, index) => {
              const isCompleted = completedSteps.has(step.key)
              const isCurrent = step.key === currentStep
              const isPast = index < stepIndex

              return (
                <div key={step.key} className="flex items-center gap-2">
                  <div
                    className={cn(
                      "w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-colors shrink-0",
                      isCompleted && "bg-green-500 text-white",
                      isCurrent && "bg-primary text-primary-foreground ring-4 ring-primary/20",
                      !isCompleted && !isCurrent && "bg-muted text-muted-foreground"
                    )}
                  >
                    {isCompleted ? <Check className="h-4 w-4" /> : step.icon}
                  </div>
                  <span
                    className={cn(
                      "text-xs font-medium whitespace-nowrap",
                      isCurrent ? "text-foreground" : "text-muted-foreground"
                    )}
                  >
                    {step.label}
                  </span>
                  {index < steps.length - 1 && (
                    <div
                      className={cn(
                        "w-8 h-0.5 transition-colors",
                        isPast ? "bg-primary" : "bg-muted"
                      )}
                    />
                  )}
                </div>
              )
            })}
          </div>

          {/* Step 1: Basic Info */}
          {currentStep === 'basic' && (
            <BasicInfoStep
              name={name}
              onNameChange={setName}
              description={description}
              onDescriptionChange={setDescription}
              enabled={enabled}
              onEnabledChange={setEnabled}
              errors={formErrors}
              t={t}
              tBuilder={tBuilder}
            />
          )}

          {/* Step 2: Condition */}
          {currentStep === 'condition' && (
            <ConditionStep
              condition={condition}
              onConditionChange={setCondition}
              onAddCondition={createDefaultCondition}
              devices={resources.devices}
              deviceTypes={resources.deviceTypes}
              forDuration={forDuration}
              onForDurationChange={setForDuration}
              forUnit={forUnit}
              onForUnitChange={setForUnit}
              errors={formErrors}
              t={t}
              tBuilder={tBuilder}
            />
          )}

          {/* Step 3: Actions */}
          {currentStep === 'action' && (
            <ActionStep
              actions={actions}
              onActionsChange={setActions}
              devices={resources.devices}
              deviceTypes={resources.deviceTypes}
              t={t}
              tBuilder={tBuilder}
            />
          )}

          {/* Step 4: Review */}
          {currentStep === 'review' && (
            <ReviewStep
              name={name}
              description={description}
              enabled={enabled}
              condition={condition}
              actions={actions}
              forDuration={forDuration}
              forUnit={forUnit}
              previewDSL={previewDSL}
              t={t}
              tBuilder={tBuilder}
            />
          )}
        </div>

        {/* Footer Navigation */}
        <DialogFooter className="px-6 pb-4 pt-4 border-t gap-2">
          {!isFirstStep && (
            <Button variant="outline" onClick={handlePrevious}>
              <ChevronLeft className="h-4 w-4 mr-1" />
              {tBuilder('previous')}
            </Button>
          )}

          <div className="flex-1" />

          {currentStep === 'review' ? (
            <Button onClick={handleSave} disabled={saving}>
              {saving ? tBuilder('saving') : tBuilder('save')}
            </Button>
          ) : (
            <Button onClick={handleNext}>
              {tBuilder('next')}
              <ChevronRight className="h-4 w-4 ml-1" />
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// Step 1: Basic Info
// ============================================================================

interface BasicInfoStepProps {
  name: string
  onNameChange: (v: string) => void
  description: string
  onDescriptionChange: (v: string) => void
  enabled: boolean
  onEnabledChange: (v: boolean) => void
  errors: FormErrors
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function BasicInfoStep({ name, onNameChange, description, onDescriptionChange, enabled, onEnabledChange, errors, t, tBuilder }: BasicInfoStepProps) {
  return (
    <div className="space-y-6 max-w-2xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('steps.basic')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('steps.basicDesc')}</p>
      </div>

      <div className="space-y-2">
        <Label className="text-sm font-medium">
          {tBuilder('ruleName')} <span className="text-destructive">*</span>
        </Label>
        <Input
          value={name}
          onChange={e => onNameChange(e.target.value)}
          placeholder={tBuilder('ruleNamePlaceholder')}
          className={cn(errors.name && "border-destructive")}
        />
        {errors.name && (
          <p className="text-xs text-destructive">{errors.name}</p>
        )}
      </div>

      <div className="space-y-2">
        <Label className="text-sm font-medium">{tBuilder('description')}</Label>
        <Input
          value={description}
          onChange={e => onDescriptionChange(e.target.value)}
          placeholder={tBuilder('descriptionPlaceholder')}
        />
      </div>

      <div className="flex items-center gap-3">
        <input
          type="checkbox"
          id="rule-enabled"
          checked={enabled}
          onChange={e => onEnabledChange(e.target.checked)}
          className="h-4 w-4"
        />
        <Label htmlFor="rule-enabled" className="text-sm font-medium cursor-pointer">
          {tBuilder('enabled')}
        </Label>
      </div>
    </div>
  )
}

// ============================================================================
// Step 2: Condition
// ============================================================================

interface ConditionStepProps {
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
  forDuration: number
  onForDurationChange: (v: number) => void
  forUnit: 'seconds' | 'minutes' | 'hours'
  onForUnitChange: (v: 'seconds' | 'minutes' | 'hours') => void
  errors: FormErrors
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ConditionStep({
  condition,
  onConditionChange,
  onAddCondition,
  devices,
  deviceTypes,
  forDuration,
  onForDurationChange,
  forUnit,
  onForUnitChange,
  errors,
  t,
  tBuilder,
}: ConditionStepProps) {
  return (
    <div className="space-y-4 py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('steps.condition')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('steps.conditionDesc')}</p>
      </div>

      {/* Condition Type Selector */}
      {!condition && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3 max-w-3xl mx-auto">
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
        </div>
      )}

      {/* Condition Editor */}
      {condition && (
        <div className="max-w-3xl mx-auto">
          <ConditionEditor
            condition={condition}
            onChange={onConditionChange}
            devices={devices}
            deviceTypes={deviceTypes}
            t={t}
            tBuilder={tBuilder}
          />

          {/* Duration */}
          <div className="mt-6 flex items-center gap-3 p-4 bg-blue-500/10 rounded-lg border border-blue-500/20">
            <Clock className="h-4 w-4 text-blue-500 shrink-0" />
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
            <div className="mt-4 p-3 bg-destructive/10 border border-destructive/20 rounded-lg">
              {errors.condition.map((err, i) => (
                <p key={i} className="text-sm text-destructive">• {err}</p>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function ConditionTypeButton({ label, icon, onClick }: { label: string; icon: React.ReactNode; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="p-4 rounded-lg border-2 border-muted hover:border-primary/50 hover:bg-primary/5 transition-all text-left"
    >
      <div className="flex items-center gap-3">
        <div className="p-2 rounded-lg bg-muted">{icon}</div>
        <span className="font-medium">{label}</span>
      </div>
    </button>
  )
}

// ============================================================================
// Step 3: Actions
// ============================================================================

interface ActionStepProps {
  actions: RuleAction[]
  onActionsChange: (actions: RuleAction[]) => void
  devices: Array<{
    id: string
    name: string
    device_type: string
    commands?: Array<{ name: string; description: string }>
  }>
  deviceTypes?: DeviceType[]
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ActionStep({ actions, onActionsChange, devices, deviceTypes, t, tBuilder }: ActionStepProps) {
  const addAction = useCallback((type: 'Notify' | 'Execute' | 'Log' | 'Set' | 'Delay' | 'CreateAlert' | 'HttpRequest') => {
    // Create a properly typed action based on the type
    let newAction: RuleAction
    switch (type) {
      case 'Notify':
        newAction = { type: 'Notify', message: '' }
        break
      case 'Execute': {
        const firstDevice = devices[0]
        const commands = firstDevice ? getDeviceCommands(firstDevice.id, devices, deviceTypes) : []
        newAction = {
          type: 'Execute',
          device_id: firstDevice?.id || '',
          command: commands[0]?.name || 'turn_on',
          params: {},
        }
        break
      }
      case 'Set':
        newAction = {
          type: 'Set',
          device_id: devices[0]?.id || '',
          property: 'state',
          value: true,
        }
        break
      case 'Delay':
        newAction = { type: 'Delay', duration: 5000 }
        break
      case 'CreateAlert':
        newAction = { type: 'CreateAlert', title: '', message: '', severity: 'info' }
        break
      case 'HttpRequest':
        newAction = { type: 'HttpRequest', method: 'GET', url: '' }
        break
      case 'Log':
      default:
        newAction = { type: 'Log', level: 'info', message: '' }
        break
    }
    onActionsChange([...actions, newAction])
  }, [actions, devices, deviceTypes, onActionsChange])

  const updateAction = useCallback((index: number, data: Partial<RuleAction>) => {
    onActionsChange(actions.map((a, i) => {
      if (i !== index) return a

      // Ensure type integrity - only allow updates to fields that belong to this action type
      const updated = { ...a, ...data } as RuleAction

      // Verify the action maintains its correct structure based on type
      switch (updated.type) {
        case 'Log':
          return { type: 'Log', level: (updated as any).level || 'info', message: (updated as any).message || '' }
        case 'Notify':
          return { type: 'Notify', message: (updated as any).message || '' }
        case 'Execute':
          return { type: 'Execute', device_id: (updated as any).device_id || '', command: (updated as any).command || '', params: (updated as any).params || {} }
        case 'CreateAlert':
          return { type: 'CreateAlert', title: (updated as any).title || '', message: (updated as any).message || '', severity: (updated as any).severity || 'info' }
        case 'Set':
          return { type: 'Set', device_id: (updated as any).device_id || '', property: (updated as any).property || '', value: (updated as any).value ?? true }
        case 'Delay':
          return { type: 'Delay', duration: (updated as any).duration || 1000 }
        case 'HttpRequest':
          return { type: 'HttpRequest', method: (updated as any).method || 'GET', url: (updated as any).url || '' }
        default:
          return updated
      }
    }))
  }, [actions, onActionsChange])

  const removeAction = useCallback((index: number) => {
    onActionsChange(actions.filter((_, i) => i !== index))
  }, [actions, onActionsChange])

  return (
    <div className="space-y-4 py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('steps.action')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('steps.actionDesc')}</p>
      </div>

      {/* Action Type Buttons */}
      <div className="flex flex-wrap justify-center gap-2 mb-4">
        <ActionTypeButton label={tBuilder('executeCommand')} icon={<Zap className="h-4 w-4" />} onClick={() => addAction('Execute')} />
        <ActionTypeButton label={tBuilder('sendNotification')} icon={<Bell className="h-4 w-4" />} onClick={() => addAction('Notify')} />
        <ActionTypeButton label={tBuilder('logRecord')} icon={<FileText className="h-4 w-4" />} onClick={() => addAction('Log')} />
        <ActionTypeButton label={tBuilder('writeValue')} icon={<Globe className="h-4 w-4" />} onClick={() => addAction('Set')} />
        <ActionTypeButton label={tBuilder('delay')} icon={<Timer className="h-4 w-4" />} onClick={() => addAction('Delay')} />
        <ActionTypeButton label={tBuilder('createAlert')} icon={<AlertTriangle className="h-4 w-4" />} onClick={() => addAction('CreateAlert')} />
        <ActionTypeButton label={tBuilder('httpRequest')} icon={<Globe className="h-4 w-4" />} onClick={() => addAction('HttpRequest')} />
      </div>

      {/* Actions List */}
      <div className="max-w-3xl mx-auto space-y-2">
        {actions.map((action, i) => (
          <ActionEditorCompact
            key={i}
            action={action}
            index={i}
            devices={devices}
            deviceTypes={deviceTypes}
            t={t}
            tBuilder={tBuilder}
            onUpdate={(data) => updateAction(i, data)}
            onRemove={() => removeAction(i)}
          />
        ))}
        {actions.length === 0 && (
          <div className="text-center py-12 border-2 border-dashed rounded-lg bg-muted/20">
            <Zap className="h-10 w-10 mx-auto text-muted-foreground/50 mb-3" />
            <p className="text-sm text-muted-foreground">{tBuilder('noActionsHint')}</p>
          </div>
        )}
      </div>
    </div>
  )
}

function ActionTypeButton({ label, icon, onClick }: { label: string; icon: React.ReactNode; onClick: () => void }) {
  return (
    <Button variant="outline" size="sm" onClick={onClick} className="gap-1.5">
      {icon}
      {label}
    </Button>
  )
}

// ============================================================================
// Step 4: Review
// ============================================================================

interface ReviewStepProps {
  name: string
  description: string
  enabled: boolean
  condition: UICondition | null
  actions: RuleAction[]
  forDuration: number
  forUnit: 'seconds' | 'minutes' | 'hours'
  previewDSL: string
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ReviewStep({ name, description, enabled, condition, actions, forDuration, forUnit, previewDSL, t, tBuilder }: ReviewStepProps) {
  return (
    <div className="space-y-6 max-w-3xl mx-auto py-4">
      <div className="text-center mb-6">
        <h3 className="text-lg font-semibold">{tBuilder('review.title')}</h3>
        <p className="text-sm text-muted-foreground">{tBuilder('review.description')}</p>
      </div>

      {/* Summary Cards */}
      <div className="grid grid-cols-3 gap-4">
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold text-purple-500">{condition ? 1 : 0}</div>
          <div className="text-xs text-muted-foreground">{tBuilder('review.triggerCondition')}</div>
        </div>
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold text-green-500">{actions.length}</div>
          <div className="text-xs text-muted-foreground">{tBuilder('review.executeAction')}</div>
        </div>
        <div className="rounded-lg border bg-card p-4 text-center">
          <div className="text-2xl font-bold">{enabled ? tBuilder('review.enabled') : tBuilder('review.disabled')}</div>
          <div className="text-xs text-muted-foreground">{tBuilder('review.status')}</div>
        </div>
      </div>

      {/* Basic Info */}
      <div className="rounded-lg border bg-card p-4">
        <h4 className="font-medium flex items-center gap-2 mb-3">
          <Settings className="h-4 w-4" />
          {tBuilder('review.basicInfo')}
        </h4>
        <div className="grid grid-cols-2 gap-4 text-sm">
          <div>
            <span className="text-muted-foreground">{tBuilder('review.name')}:</span>
            <span className="ml-2 font-medium">{name || '-'}</span>
          </div>
          <div>
            <span className="text-muted-foreground">{tBuilder('review.status')}:</span>
            <span className="ml-2 font-medium">{enabled ? tBuilder('review.enabled') : tBuilder('review.disabled')}</span>
          </div>
          <div className="col-span-2">
            <span className="text-muted-foreground">{tBuilder('review.desc')}:</span>
            <span className="ml-2">{description || '-'}</span>
          </div>
        </div>
      </div>

      {/* DSL Preview */}
      <div className="rounded-lg border bg-card p-4">
        <h4 className="font-medium flex items-center gap-2 mb-3">
          <Code className="h-4 w-4" />
          {tBuilder('review.ruleDSL')}
        </h4>
        <pre className="text-sm font-mono bg-muted/30 p-3 rounded overflow-x-auto whitespace-pre-wrap">
          {previewDSL || '// No DSL generated'}
        </pre>
      </div>
    </div>
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
  t: (key: string) => string
  tBuilder: (key: string) => string
}

function ConditionEditor({ condition, onChange, devices, deviceTypes, t, tBuilder }: ConditionEditorProps) {
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

  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name }))

  // Render simple condition
  const renderSimpleCondition = (cond: UICondition) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)
    const metricDataType = cond.metric && cond.device_id
      ? getMetricDataType(cond.metric, cond.device_id, devices, deviceTypes)
      : 'float'
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
            disabled={!cond.device_id}
          />
        )
      }

      return (
        <Input
          type="number"
          value={cond.threshold ?? ''}
          onChange={e => updateField('threshold', parseFloat(e.target.value) || 0)}
          className="w-24 h-9"
          disabled={!cond.device_id}
        />
      )
    }

    return (
      <div className="p-3 bg-gradient-to-r from-purple-500/10 to-transparent rounded-lg border border-purple-500/20">
        <div className="flex flex-wrap items-center gap-2">
          <Select value={cond.device_id} onValueChange={(v) => {
            const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
            updateField('device_id', v)
            updateField('metric', newMetrics[0]?.name || 'value')
          }}>
            <SelectTrigger className="w-36 h-9 text-sm"><SelectValue placeholder={tBuilder('selectDevice')} /></SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
            </SelectContent>
          </Select>
          {cond.device_id && metrics.length > 0 ? (
            <Select value={cond.metric} onValueChange={(v) => updateField('metric', v)}>
              <SelectTrigger className="w-32 h-9 text-sm"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => (
                  <SelectItem key={m.name} value={m.name}>
                    {m.display_name || m.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : (
            <span className="text-xs text-muted-foreground italic">{tBuilder('selectDeviceFirst')}</span>
          )}
          <Select value={cond.operator} onValueChange={(v) => updateField('operator', v)} disabled={!cond.device_id}>
            <SelectTrigger className="w-20 h-9 text-sm"><SelectValue /></SelectTrigger>
            <SelectContent>
              {getComparisonOperators((k) => k, metricDataType).map(o => <SelectItem key={o.value} value={o.value}>{o.symbol}</SelectItem>)}
            </SelectContent>
          </Select>
          {renderValueInput()}
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => onChange(null as any)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    )
  }

  // Render range condition
  const renderRangeCondition = (cond: UICondition) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)

    return (
      <div className="p-3 bg-gradient-to-r from-blue-500/10 to-transparent rounded-lg border border-blue-500/20">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className="text-xs bg-blue-500/20 text-blue-500 border-blue-500/30">BETWEEN</Badge>
          <Select value={cond.device_id} onValueChange={(v) => {
            const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
            updateField('device_id', v)
            updateField('metric', newMetrics[0]?.name || 'value')
          }}>
            <SelectTrigger className="w-36 h-9 text-sm"><SelectValue placeholder={tBuilder('selectDevice')} /></SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
            </SelectContent>
          </Select>
          {cond.device_id && metrics.length > 0 ? (
            <Select value={cond.metric} onValueChange={(v) => updateField('metric', v)}>
              <SelectTrigger className="w-32 h-9 text-sm"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>)}
              </SelectContent>
            </Select>
          ) : (
            <span className="text-xs text-muted-foreground italic">{tBuilder('selectDeviceFirst')}</span>
          )}
          <span className="text-xs font-medium text-muted-foreground px-1">BETWEEN</span>
          <Input
            type="number"
            value={cond.range_min}
            onChange={e => updateField('range_min', parseFloat(e.target.value) || 0)}
            className="w-20 h-9"
            placeholder="Min"
            disabled={!cond.device_id}
          />
          <span className="text-xs text-muted-foreground">AND</span>
          <Input
            type="number"
            value={cond.range_max}
            onChange={e => updateField('range_max', parseFloat(e.target.value) || 0)}
            className="w-20 h-9"
            placeholder="Max"
            disabled={!cond.device_id}
          />
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => onChange(null as any)}>
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    )
  }

  // Render logical condition (AND/OR/NOT)
  const renderLogicalCondition = () => {
    const label = condition.type.toUpperCase()
    const badgeClass = condition.type === 'and'
      ? 'bg-green-500/20 text-green-500 border-green-500/30'
      : condition.type === 'or'
      ? 'bg-amber-500/20 text-amber-500 border-amber-500/30'
      : 'bg-red-500/20 text-red-500 border-red-500/30'

    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2 p-2.5 bg-muted/40 rounded-t-lg border">
          <Badge variant="outline" className={cn('text-xs px-2.5 py-1', badgeClass)}>{label}</Badge>
          <span className="text-xs text-muted-foreground flex-1">
            {condition.type === 'and' ? tBuilder('allConditionsMustMeet') : condition.type === 'or' ? tBuilder('anyConditionMustMeet') : tBuilder('conditionNotMet')}
          </span>
          <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => onChange(null as any)}>
            <X className="h-3 w-3" />
          </Button>
        </div>

        <div className="p-3 bg-background border border-t-0 rounded-b-lg space-y-3">
          {condition.conditions?.map((subCond, i) => (
            <div key={subCond.id} className="relative group">
              {i > 0 && (
                <div className="flex items-center justify-start -mb-2 mt-1">
                  <span className={cn(
                    'text-xs font-semibold px-2.5 py-1 rounded-full',
                    condition.type === 'and' ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' : 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'
                  )}>
                    {condition.type.toUpperCase()}
                  </span>
                </div>
              )}
              <div className="relative pr-8">
                <div className="rounded-lg bg-muted/30">
                  <ConditionEditor
                    condition={subCond}
                    onChange={(c) => updateNestedCondition(i, c)}
                    devices={devices}
                    deviceTypes={deviceTypes}
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
                  >
                    <X className="h-3 w-3" />
                  </Button>
                )}
              </div>
            </div>
          ))}

          <div className="pt-2 border-t border-border/50">
            <Button
              variant="outline"
              size="sm"
              className="w-full border-dashed h-9"
              onClick={() => {
                const newCond: UICondition = {
                  id: crypto.randomUUID(),
                  type: 'simple',
                  device_id: devices[0]?.id || '',
                  metric: getDeviceMetrics(devices[0]?.id || '', devices, deviceTypes)[0]?.name || 'value',
                  operator: '>',
                  threshold: 0,
                }
                onChange({
                  ...condition,
                  conditions: [...(condition.conditions || []), newCond]
                })
              }}
            >
              <Plus className="h-3.5 w-3.5 mr-1" />{tBuilder('addCondition')}
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
  }>
  deviceTypes?: DeviceType[]
  t: (key: string) => string
  tBuilder: (key: string) => string
  onUpdate: (data: Partial<RuleAction>) => void
  onRemove: () => void
}

function ActionEditorCompact({ action, devices, deviceTypes, t, tBuilder, onUpdate, onRemove }: ActionEditorCompactProps) {
  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name }))

  const getActionIcon = () => {
    switch (action.type) {
      case 'Execute': return <Zap className="h-4 w-4 text-yellow-500" />
      case 'Notify': return <Bell className="h-4 w-4 text-blue-500" />
      case 'Log': return <FileText className="h-4 w-4 text-gray-500" />
      case 'Set': return <Globe className="h-4 w-4 text-purple-500" />
      case 'Delay': return <Timer className="h-4 w-4 text-orange-500" />
      case 'CreateAlert': return <AlertTriangle className="h-4 w-4 text-red-500" />
      case 'HttpRequest': return <Globe className="h-4 w-4 text-green-500" />
      default: return <Zap className="h-4 w-4" />
    }
  }

  const getActionBadgeClass = (): string => {
    switch (action.type) {
      case 'Execute': return 'bg-yellow-500/20 text-yellow-500 border-yellow-500/30'
      case 'Notify': return 'bg-blue-500/20 text-blue-500 border-blue-500/30'
      case 'Log': return 'bg-gray-500/20 text-gray-500 border-gray-500/30'
      case 'Set': return 'bg-purple-500/20 text-purple-500 border-purple-500/30'
      case 'Delay': return 'bg-orange-500/20 text-orange-500 border-orange-500/30'
      case 'CreateAlert': return 'bg-red-500/20 text-red-500 border-red-500/30'
      case 'HttpRequest': return 'bg-green-500/20 text-green-500 border-green-500/30'
      default: return 'bg-muted'
    }
  }

  const getActionLabel = (): string => {
    switch (action.type) {
      case 'Execute': return tBuilder('executeCommand')
      case 'Notify': return tBuilder('sendNotification')
      case 'Log': return tBuilder('logRecord')
      case 'Set': return tBuilder('writeValue')
      case 'Delay': return tBuilder('delay')
      case 'CreateAlert': return tBuilder('createAlert')
      case 'HttpRequest': return tBuilder('httpRequest')
    }
    return (action as any).type
  }

  return (
    <div className="group flex items-center gap-2.5 p-3 bg-gradient-to-r from-green-500/10 to-transparent rounded-lg border border-green-500/20 hover:border-green-500/40 transition-colors">
      <div className="p-1.5 bg-background rounded shadow-sm flex-shrink-0">
        {getActionIcon()}
      </div>
      <Badge variant="outline" className={cn('text-xs px-2 py-0.5 flex-shrink-0', getActionBadgeClass())}>
        {getActionLabel()}
      </Badge>

      <div className="flex items-center gap-2 flex-wrap flex-1 min-w-0">
        {action.type === 'Execute' && (
          <>
            <Select
              value={action.device_id}
              onValueChange={(v) => {
                const commands = getDeviceCommands(v, devices, deviceTypes)
                onUpdate({ device_id: v, command: commands[0]?.name || 'turn_on' })
              }}
            >
              <SelectTrigger className="w-32 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Select
              value={action.command}
              onValueChange={(v) => onUpdate({ command: v })}
            >
              <SelectTrigger className="w-28 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {getDeviceCommands(action.device_id, devices, deviceTypes).map(c => (
                  <SelectItem key={c.name} value={c.name}>{c.display_name || c.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        )}

        {action.type === 'Notify' && (
          <Input
            value={action.message}
            onChange={(e) => onUpdate({ message: e.target.value })}
            placeholder={tBuilder('notificationContentPlaceholder')}
            className="flex-1 min-w-[120px] h-9 text-sm"
          />
        )}

        {action.type === 'Log' && (
          <>
            <Select value={action.level} onValueChange={(v: any) => onUpdate({ level: v })}>
              <SelectTrigger className="w-16 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="debug">Debug</SelectItem>
                <SelectItem value="info">Info</SelectItem>
                <SelectItem value="warn">Warn</SelectItem>
                <SelectItem value="error">Error</SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={action.message}
              onChange={(e) => onUpdate({ message: e.target.value })}
              placeholder={tBuilder('logContentPlaceholder')}
              className="flex-1 min-w-[120px] h-9 text-sm"
            />
          </>
        )}

        {action.type === 'Set' && (
          <>
            <Select value={action.device_id} onValueChange={(v) => onUpdate({ device_id: v })}>
              <SelectTrigger className="w-32 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Input
              value={action.property}
              onChange={(e) => onUpdate({ property: e.target.value })}
              placeholder={tBuilder('propertyNamePlaceholder')}
              className="w-24 h-9 text-sm flex-shrink-0"
            />
            <span className="text-muted-foreground text-sm flex-shrink-0">=</span>
            <Input
              value={String(action.value ?? '')}
              onChange={(e) => onUpdate({ value: e.target.value })}
              placeholder={tBuilder('valuePlaceholder')}
              className="w-24 h-9 text-sm flex-shrink-0"
            />
          </>
        )}

        {action.type === 'Delay' && (
          <>
            <Input
              type="number"
              value={(action.duration || 0) / 1000}
              onChange={(e) => onUpdate({ duration: (parseInt(e.target.value) || 0) * 1000 })}
              className="w-16 h-9 text-sm flex-shrink-0"
            />
            <span className="text-xs text-muted-foreground flex-shrink-0">{tBuilder('seconds')}</span>
          </>
        )}

        {action.type === 'CreateAlert' && (
          <>
            <Input
              value={action.title}
              onChange={(e) => onUpdate({ title: e.target.value })}
              placeholder={tBuilder('alertTitlePlaceholder')}
              className="w-28 h-9 text-sm flex-shrink-0"
            />
            <Input
              value={action.message}
              onChange={(e) => onUpdate({ message: e.target.value })}
              placeholder={tBuilder('alertMessagePlaceholder')}
              className="flex-1 min-w-[80px] h-9 text-sm"
            />
            <Select value={action.severity} onValueChange={(v: any) => onUpdate({ severity: v })}>
              <SelectTrigger className="w-20 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="info">Info</SelectItem>
                <SelectItem value="warning">Warning</SelectItem>
                <SelectItem value="error">Error</SelectItem>
                <SelectItem value="critical">Critical</SelectItem>
              </SelectContent>
            </Select>
          </>
        )}

        {action.type === 'HttpRequest' && (
          <>
            <Select value={action.method} onValueChange={(v: any) => onUpdate({ method: v })}>
              <SelectTrigger className="w-20 h-9 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="GET">GET</SelectItem>
                <SelectItem value="POST">POST</SelectItem>
                <SelectItem value="PUT">PUT</SelectItem>
                <SelectItem value="DELETE">DELETE</SelectItem>
                <SelectItem value="PATCH">PATCH</SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={action.url}
              onChange={(e) => onUpdate({ url: e.target.value })}
              placeholder="https://example.com/api"
              className="flex-1 min-w-[100px] h-9 text-sm"
            />
          </>
        )}
      </div>

      <Button variant="ghost" size="icon" className="h-7 w-7 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity" onClick={onRemove}>
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  )
}
