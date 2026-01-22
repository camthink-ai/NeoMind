import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import { Play, Check, X, Zap, Bell, FileText, RefreshCw } from 'lucide-react'
import type { RuleAction, RuleCondition } from '@/types'

interface RuleFormData {
  name: string
  conditions: RuleCondition[]
  actions: RuleAction[]
}

interface RuleTesterProps {
  formData: RuleFormData
  dsl: string
  onTest: () => void
  testing: boolean
  testResult: {
    triggered: boolean
    message: string
    details?: Record<string, unknown>
  } | null
  resources?: {
    devices: Array<{ id: string; name: string; type: string }>
    metrics: string[]
    alertChannels: Array<{ id: string; name: string }>
  }
}

interface SimulatedValue {
  deviceId: string
  metric: string
  value: number
}

export function RuleTester({
  formData,
  dsl,
  onTest,
  testing,
  testResult,
  resources,
}: RuleTesterProps) {
  const { t } = useTranslation('automation')
  const [simulatedValues, setSimulatedValues] = useState<SimulatedValue[]>([])

  // Initialize simulated values based on conditions
  const initializeSimulatedValues = () => {
    const values: SimulatedValue[] = []
    formData.conditions.forEach((condition) => {
      if (condition.device_id && condition.metric) {
        // Use a value that won't trigger by default
        values.push({
          deviceId: condition.device_id,
          metric: condition.metric,
          value: 0,
        })
      }
    })
    setSimulatedValues(values)
  }

  const updateSimulatedValue = (index: number, value: number) => {
    const newValues = [...simulatedValues]
    newValues[index].value = value
    setSimulatedValues(newValues)
  }

  const checkCondition = (condition: RuleCondition, simulatedValue: number): boolean => {
    const threshold = condition.threshold ?? 0

    // Handle string threshold comparison
    if (typeof threshold === 'string') {
      const strValue = String(simulatedValue)
      switch (condition.operator) {
        case '==':
          return strValue === threshold
        case '!=':
          return strValue !== threshold
        case 'contains':
          return strValue.includes(threshold)
        case 'starts_with':
          return strValue.startsWith(threshold)
        case 'ends_with':
          return strValue.endsWith(threshold)
        case 'regex':
          try {
            return new RegExp(threshold).test(strValue)
          } catch {
            return false
          }
        default:
          // Numeric operators don't work with strings
          return false
      }
    }

    // Handle numeric threshold comparison
    switch (condition.operator) {
      case '>':
        return simulatedValue > threshold
      case '<':
        return simulatedValue < threshold
      case '>=':
        return simulatedValue >= threshold
      case '<=':
        return simulatedValue <= threshold
      case '==':
        return simulatedValue === threshold
      case '!=':
        return simulatedValue !== threshold
      default:
        return false
    }
  }

  const getConditionStatus = (condition: RuleCondition, index: number) => {
    if (simulatedValues.length === 0) return 'pending'

    const simulatedValue = simulatedValues[index]
    if (!simulatedValue || !condition.device_id || simulatedValue.deviceId !== condition.device_id) {
      return 'noValue'
    }

    const triggered = checkCondition(condition, simulatedValue.value)
    return triggered ? 'triggered' : 'notTriggered'
  }

  const getDeviceName = (deviceId: string): string => {
    const device = resources?.devices.find((d) => d.id === deviceId)
    return device?.name || deviceId
  }

  return (
    <div className="space-y-4">
      <Card className="p-4">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h3 className="font-semibold">{t('automation:ruleTest')}</h3>
            <p className="text-sm text-muted-foreground">{t('automation:ruleTestDesc')}</p>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={initializeSimulatedValues}
              disabled={formData.conditions.length === 0}
            >
              <RefreshCw className="h-4 w-4 mr-1" />
              {t('automation:initializeTest')}
            </Button>
            <Button onClick={onTest} disabled={testing || !dsl}>
              {testing ? (
                <>
                  <RefreshCw className="h-4 w-4 mr-2 animate-spin" />
                  {t('automation:testing')}
                </>
              ) : (
                <>
                  <Play className="h-4 w-4 mr-2" />
                  {t('automation:testRule')}
                </>
              )}
            </Button>
          </div>
        </div>

        {/* Conditions Simulation */}
        {formData.conditions.length > 0 ? (
          <div className="space-y-3">
            <Label>{t('automation:simulatedConditions')}</Label>
            {formData.conditions.map((condition, index) => {
              const status = getConditionStatus(condition, index)
              const simValue = simulatedValues[index]

              return (
                <div key={index} className="border rounded-md p-3">
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm font-medium">
                      {getDeviceName(condition.device_id || '')} - {condition.metric || 'value'}
                    </span>
                    <Badge
                      variant={
                        status === 'triggered'
                          ? 'default'
                          : status === 'notTriggered'
                            ? 'secondary'
                            : 'outline'
                      }
                    >
                      {status === 'triggered' && <Check className="h-3 w-3 mr-1" />}
                      {status === 'notTriggered' && <X className="h-3 w-3 mr-1" />}
                      {status === 'triggered'
                        ? t('automation:wouldTrigger')
                        : status === 'notTriggered'
                          ? t('automation:wouldNotTrigger')
                          : t('automation:pending')}
                    </Badge>
                  </div>
                  <div className="flex items-center gap-4">
                    <div className="flex-1">
                      <Label className="text-xs text-muted-foreground">
                        {t('automation:currentValue')}
                      </Label>
                      <Input
                        type="number"
                        value={simValue?.value ?? 0}
                        onChange={(e) => updateSimulatedValue(index, parseFloat(e.target.value) || 0)}
                        className="h-8"
                      />
                    </div>
                    <div className="flex items-center gap-2 text-sm">
                      <span className={simValue && typeof condition.threshold === 'number' && simValue.value > condition.threshold ? 'text-green-600 font-medium' : ''}>
                        {simValue?.value ?? 0}
                      </span>
                      <span className="text-muted-foreground">{condition.operator || '>'}</span>
                      <span className="font-medium">{condition.threshold ?? 0}</span>
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        ) : (
          <div className="text-center py-8 text-muted-foreground">
            <Zap className="h-12 w-12 mx-auto mb-2 opacity-50" />
            <p>{t('automation:addConditionsToTest')}</p>
          </div>
        )}
      </Card>

      {/* Test Result */}
      {testResult && (
        <Card className={`p-4 ${testResult.triggered ? 'border-green-500 bg-green-50 dark:bg-green-950/20' : 'border-yellow-500 bg-yellow-50 dark:bg-yellow-950/20'}`}>
          <div className="flex items-start gap-3">
            {testResult.triggered ? (
              <Check className="h-5 w-5 text-green-600 mt-0.5" />
            ) : (
              <X className="h-5 w-5 text-yellow-600 mt-0.5" />
            )}
            <div className="flex-1">
              <h4 className="font-semibold">
                {testResult.triggered ? t('automation:ruleWouldTrigger') : t('automation:ruleWouldNotTrigger')}
              </h4>
              <p className="text-sm text-muted-foreground mt-1">{testResult.message}</p>
              {testResult.details && (
                <pre className="mt-2 text-xs bg-background p-2 rounded overflow-x-auto">
                  {JSON.stringify(testResult.details, null, 2)}
                </pre>
              )}
            </div>
          </div>
        </Card>
      )}

      {/* Actions Preview */}
      {formData.actions.length > 0 && (
        <Card className="p-4">
          <Label className="mb-3 block">{t('automation:actionsPreview')}</Label>
          <div className="space-y-2">
            {formData.actions.map((action, index) => (
              <div key={index} className="flex items-center gap-2 p-2 bg-muted rounded-md">
                {action.type === 'Execute' && <Zap className="h-4 w-4 text-warning" />}
                {action.type === 'Notify' && <Bell className="h-4 w-4 text-primary" />}
                {action.type === 'Log' && <FileText className="h-4 w-4 text-muted-foreground" />}
                <span className="text-sm flex-1">
                  {action.type === 'Execute' && (
                    <>
                      {t('automation:execute')}: <strong>{getDeviceName(action.device_id)}</strong>
                      .{action.command}
                      {action.params && Object.keys(action.params).length > 0 && (
                        <span className="text-muted-foreground">
                          ({JSON.stringify(action.params)})
                        </span>
                      )}
                    </>
                  )}
                  {action.type === 'Notify' && <>{t('automation:sendNotification')}: {action.message}</>}
                  {action.type === 'Log' && (
                    <>
                      {t('automation:log')}: {action.message} <Badge variant="outline">{(action as any).level}</Badge>
                    </>
                  )}
                </span>
              </div>
            ))}
          </div>
        </Card>
      )}

      {/* DSL Preview */}
      <Card className="p-4">
        <Label className="mb-2 block">{t('automation:generatedDSL')}</Label>
        <pre className="bg-muted p-3 rounded-md text-sm font-mono overflow-x-auto">
          {dsl || '// No DSL generated yet'}
        </pre>
      </Card>
    </div>
  )
}
