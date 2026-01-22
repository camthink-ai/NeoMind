import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Trash2, GripVertical, ArrowDown } from 'lucide-react'
import type { RuleCondition } from '@/types'

interface ConditionBuilderProps {
  condition: RuleCondition
  onChange: (condition: RuleCondition) => void
  onRemove: () => void
  resources?: {
    devices: Array<{ id: string; name: string; type: string }>
    metrics: string[]
    alertChannels: Array<{ id: string; name: string }>
  }
  showOperator?: boolean
  operator?: 'AND' | 'OR'
  onOperatorChange?: (operator: 'AND' | 'OR') => void
  canRemove?: boolean
}

// Comparison operators with i18n support
const getOperators = (t: (key: string) => string) => [
  { value: '>', label: '>', description: t('automation:operators.greaterThan') },
  { value: '<', label: '<', description: t('automation:operators.lessThan') },
  { value: '>=', label: '>=', description: t('automation:operators.greaterThanOrEqual') },
  { value: '<=', label: '<=', description: t('automation:operators.lessThanOrEqual') },
  { value: '==', label: '==', description: t('automation:operators.equal') },
  { value: '!=', label: '!=', description: t('automation:operators.notEqual') },
]

const COMMON_METRICS = [
  'temperature',
  'humidity',
  'pressure',
  'motion',
  'power',
  'voltage',
  'current',
  'luminance',
  'presence',
  'status',
]

export function ConditionBuilder({
  condition,
  onChange,
  onRemove,
  resources,
  showOperator,
  operator,
  onOperatorChange,
  canRemove = true,
}: ConditionBuilderProps) {
  const { t } = useTranslation('automation')

  const devices = resources?.devices || []
  const metrics = resources?.metrics || COMMON_METRICS
  const OPERATORS = getOperators(t)

  const updateCondition = (updates: Partial<RuleCondition>) => {
    onChange({ ...condition, ...updates })
  }

  const selectedDevice = devices.find((d) => d.id === condition.device_id)

  return (
    <Card className="p-3">
      <div className="flex items-start gap-3">
        {/* Drag Handle */}
        <div className="flex items-center gap-1 pt-1">
          <GripVertical className="h-4 w-4 text-muted-foreground cursor-grab" />
        </div>

        {/* Operator Badge */}
        {showOperator && onOperatorChange && (
          <div className="flex items-center gap-2 pt-1">
            <Badge
              variant="outline"
              className="cursor-pointer hover:bg-accent"
              onClick={() => onOperatorChange(operator === 'AND' ? 'OR' : 'AND')}
            >
              {operator}
            </Badge>
            <ArrowDown className="h-4 w-4 text-muted-foreground" />
          </div>
        )}

        {/* Condition Fields */}
        <div className="flex-1 grid grid-cols-1 md:grid-cols-4 gap-3">
          {/* Device Selector */}
          <div>
            <Label className="text-xs text-muted-foreground">{t('automation:device')}</Label>
            <Select
              value={condition.device_id}
              onValueChange={(value) => updateCondition({ device_id: value })}
            >
              <SelectTrigger className="h-9">
                <SelectValue placeholder={t('automation:selectDevice')} />
              </SelectTrigger>
              <SelectContent>
                {devices.map((device) => (
                  <SelectItem key={device.id} value={device.id}>
                    <div className="flex items-center gap-2">
                      <span>{device.name}</span>
                      <Badge variant="outline" className="text-xs">
                        {device.type}
                      </Badge>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Metric Selector */}
          <div>
            <Label className="text-xs text-muted-foreground">{t('automation:metric')}</Label>
            <Select
              value={condition.metric}
              onValueChange={(value) => updateCondition({ metric: value })}
            >
              <SelectTrigger className="h-9">
                <SelectValue placeholder={t('automation:selectMetric')} />
              </SelectTrigger>
              <SelectContent>
                {metrics.map((metric) => (
                  <SelectItem key={metric} value={metric}>
                    {metric}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Operator Selector */}
          <div>
            <Label className="text-xs text-muted-foreground">{t('automation:operator')}</Label>
            <Select
              value={condition.operator}
              onValueChange={(value) => updateCondition({ operator: value })}
            >
              <SelectTrigger className="h-9">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {OPERATORS.map((op) => (
                  <SelectItem key={op.value} value={op.value}>
                    <span className="flex items-center gap-2">
                      <span className="font-mono">{op.label}</span>
                      <span className="text-xs text-muted-foreground">{op.description}</span>
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Threshold Input */}
          <div>
            <Label className="text-xs text-muted-foreground">{t('automation:threshold')}</Label>
            <div className="flex items-center gap-2">
              <Input
                type="number"
                value={condition.threshold}
                onChange={(e) => updateCondition({ threshold: parseFloat(e.target.value) || 0 })}
                className="h-9"
                placeholder="0"
              />
              {selectedDevice && (
                <span className="text-xs text-muted-foreground whitespace-nowrap">
                  {getUnitForMetric(condition.metric || 'value')}
                </span>
              )}
            </div>
          </div>
        </div>

        {/* Remove Button */}
        {canRemove && (
          <div className="pt-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 text-destructive hover:text-destructive"
              onClick={onRemove}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        )}
      </div>

      {/* Preview */}
      {condition.device_id && condition.metric && (
        <div className="mt-3 pt-3 border-t">
          <p className="text-xs text-muted-foreground">
            <span className="font-medium">{t('automation:preview')}:</span>{' '}
            <code className="bg-muted px-1 py-0.5 rounded">
              device.{condition.device_id}.{condition.metric} {condition.operator} {condition.threshold}
            </code>
          </p>
        </div>
      )}
    </Card>
  )
}

function getUnitForMetric(metric: string): string {
  const units: Record<string, string> = {
    temperature: '°C',
    humidity: '%',
    pressure: 'hPa',
    power: 'W',
    voltage: 'V',
    current: 'A',
    luminance: 'lx',
    motion: '',
    presence: '',
    status: '',
  }
  return units[metric] || ''
}
