/**
 * Data Source Configuration Component
 *
 * Allows users to bind dashboard components to data sources.
 */

import { useTranslation } from 'react-i18next'
import { Server, Globe, Calculator, Plug, Unplug } from 'lucide-react'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
} from '@/components/ui/tabs'
import { cn } from '@/lib/utils'
import type { DataSource } from '@/types/dashboard'

export interface DataSourceConfigProps {
  dataSource: DataSource | undefined
  onChange: (dataSource: DataSource | undefined) => void
  className?: string
}

export function DataSourceConfig({ dataSource, onChange, className }: DataSourceConfigProps) {
  const { t } = useTranslation('dashboardComponents')
  const currentType = dataSource?.type || 'static'

  const dataSourceTypes = [
    { value: 'static', label: t('dataSource.staticValue', 'Static Value'), icon: Calculator, description: t('dataSource.fixedValue', 'Fixed value') },
    { value: 'api', label: t('dataSource.apiEndpoint', 'API Endpoint'), icon: Server, description: t('dataSource.fetchFromApi', 'Fetch from API') },
    { value: 'websocket', label: t('dataSource.websocket', 'WebSocket'), icon: Globe, description: t('dataSource.realtimeUpdates', 'Real-time updates') },
    { value: 'computed', label: t('dataSource.computed', 'Computed'), icon: Calculator, description: t('dataSource.calculateFromOthers', 'Calculate from others') },
  ]

  const handleTypeChange = (type: DataSource['type']) => {
    if (type === 'static') {
      onChange({ type: 'static', staticValue: 0 })
    } else if (type === 'api') {
      onChange({ type: 'api', endpoint: '', refresh: 30 })
    } else if (type === 'websocket') {
      onChange({ type: 'websocket', endpoint: '', refresh: 5 })
    } else if (type === 'computed') {
      onChange({ type: 'computed', params: { expression: '0' } })
    }
  }

  const updateField = <K extends keyof DataSource>(field: K, value: DataSource[K]) => {
    onChange({ ...dataSource, [field]: value } as DataSource)
  }

  return (
    <div className={cn('space-y-4', className)}>
      {/* Data Source Toggle */}
      <div className="flex items-center justify-between">
        <Label>{t('dataSource.title')}</Label>
        {dataSource && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onChange(undefined)}
            className="h-7 text-xs text-muted-foreground"
          >
            <Unplug className="w-3 h-3 mr-1" />
            {t('dataSource.unbind', 'Unbind')}
          </Button>
        )}
      </div>

      {!dataSource && (
        <Button
          variant="outline"
          className="w-full justify-start"
          onClick={() => onChange({ type: 'static', staticValue: 0 })}
        >
          <Plug className="w-4 h-4 mr-2 text-muted-foreground" />
          {t('dataSource.bindToDataSource', 'Bind to Data Source')}
        </Button>
      )}

      {dataSource && (
        <Tabs value={currentType} onValueChange={(value) => handleTypeChange(value as DataSource['type'])}>
          <TabsList className="grid w-full grid-cols-4">
            {dataSourceTypes.map((type) => {
              const Icon = type.icon
              return (
                <TabsTrigger key={type.value} value={type.value} className="flex items-center gap-1">
                  <Icon className="w-3.5 h-3.5" />
                  <span className="hidden sm:inline">{type.label}</span>
                </TabsTrigger>
              )
            })}
          </TabsList>

          {/* Static Value */}
          <TabsContent value="static" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label>{t('dataSource.staticValue')}</Label>
              <Input
                type="number"
                value={String(dataSource.staticValue ?? 0)}
                onChange={(e) => updateField('staticValue', parseFloat(e.target.value) || 0)}
                placeholder={t('dataSourceConfig.fixedValue')}
              />
            </div>
          </TabsContent>

          {/* API Endpoint */}
          <TabsContent value="api" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label>{t('dataSource.apiEndpoint')}</Label>
              <Input
                value={dataSource.endpoint || ''}
                onChange={(e) => updateField('endpoint', e.target.value)}
                placeholder={t('dataSourceConfig.apiUrl')}
              />
              <p className="text-xs text-muted-foreground">
                {t('dataSourceConfig.apiEndpointHint', 'Enter the API path to fetch data from')}
              </p>
            </div>
            <div className="space-y-2">
              <Label>{t('dataSourceConfig.refreshInterval', 'Refresh Interval (seconds)')}</Label>
              <Input
                type="number"
                min={1}
                max={3600}
                value={String(dataSource.refresh ?? 30)}
                onChange={(e) => updateField('refresh', parseInt(e.target.value) || 30)}
              />
            </div>
          </TabsContent>

          {/* WebSocket */}
          <TabsContent value="websocket" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label>{t('dataSourceConfig.eventTopic', 'Event Topic')}</Label>
              <Input
                value={dataSource.endpoint || ''}
                onChange={(e) => updateField('endpoint', e.target.value)}
                placeholder={t('dataSourceConfig.metricPath')}
              />
              <p className="text-xs text-muted-foreground">
                {t('dataSourceConfig.subscribeHint', 'Subscribe to real-time event updates')}
              </p>
            </div>
            <div className="space-y-2">
              <Label>{t('dataSourceConfig.refreshInterval', 'Refresh Interval (seconds)')}</Label>
              <Input
                type="number"
                min={1}
                max={60}
                value={String(dataSource.refresh ?? 5)}
                onChange={(e) => updateField('refresh', parseInt(e.target.value) || 5)}
              />
            </div>
          </TabsContent>

          {/* Computed */}
          <TabsContent value="computed" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label>{t('dataSourceConfig.expression', 'Expression')}</Label>
              <Input
                value={(dataSource.params?.expression as string) || ''}
                onChange={(e) => updateField('params', { ...dataSource.params, expression: e.target.value })}
                placeholder={t('dataSourceConfig.transformExpression')}
              />
              <p className="text-xs text-muted-foreground">
                {t('dataSourceConfig.expressionHint', 'Simple math expression (e.g., 10 + 20)')}
              </p>
            </div>
          </TabsContent>
        </Tabs>
      )}
    </div>
  )
}

/**
 * Quick Data Source Selector
 *
 * Simplified version for quick binding
 */
export interface QuickDataSourceSelectorProps {
  value: DataSource | undefined
  onChange: (value: DataSource | undefined) => void
  availableSources?: Array<{ id: string; name: string; type: string }>
  className?: string
}

export function QuickDataSourceSelector({
  value,
  onChange,
  availableSources: _availableSources = [],
  className,
}: QuickDataSourceSelectorProps) {
  const { t } = useTranslation('dashboardComponents')

  return (
    <div className={cn('space-y-2', className)}>
      <Label>{t('dataSource.title')}</Label>
      <Select
        value={value?.type || 'none'}
        onValueChange={(type) => {
          if (type === 'none') {
            onChange(undefined)
          } else if (type === 'static') {
            onChange({ type: 'static', staticValue: 0 })
          } else if (type === 'api') {
            onChange({ type: 'api', endpoint: '/api/stats', refresh: 30 })
          } else if (type === 'websocket') {
            onChange({ type: 'websocket', endpoint: 'device.metric', refresh: 5 })
          }
        }}
      >
        <SelectTrigger>
          <SelectValue placeholder={t('dataSourceConfig.selectPlaceholder')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="static">
            <div className="flex items-center gap-2">
              <Calculator className="w-4 h-4" />
              <span>{t('dataSource.staticValue')}</span>
            </div>
          </SelectItem>
          <SelectItem value="api">
            <div className="flex items-center gap-2">
              <Server className="w-4 h-4" />
              <span>{t('dataSource.apiEndpoint')}</span>
            </div>
          </SelectItem>
          <SelectItem value="websocket">
            <div className="flex items-center gap-2">
              <Globe className="w-4 h-4" />
              <span>{t('DataSource.websocket')}</span>
            </div>
          </SelectItem>
        </SelectContent>
      </Select>

      {/* Show endpoint input for API/WebSocket */}
      {value && (value.type === 'api' || value.type === 'websocket') && (
        <div className="space-y-2">
          <Label>{t('dataSourceConfig.endpointOrTopic', 'Endpoint / Topic')}</Label>
          <Input
            value={value.endpoint || ''}
            onChange={(e) => onChange({ ...value, endpoint: e.target.value })}
            placeholder={value.type === 'api' ? t('dataSourceConfig.apiUrl') : t('dataSourceConfig.metricPath')}
          />
          <div className="flex items-center gap-2">
            <Label className="text-xs">{t('dataSourceConfig.refreshShort', 'Refresh (s)')}</Label>
            <Input
              type="number"
              min={1}
              max={300}
              value={String(value.refresh || 30)}
              onChange={(e) => onChange({ ...value, refresh: parseInt(e.target.value) || 30 })}
              className="h-8 w-20"
            />
          </div>
        </div>
      )}

      {/* Show static value input */}
      {value && value.type === 'static' && (
        <div className="space-y-2">
          <Label>{t('dataSourceConfig.value', 'Value')}</Label>
          <Input
            type="number"
            value={String(value.staticValue ?? 0)}
            onChange={(e) => onChange({ ...value, staticValue: parseFloat(e.target.value) || 0 })}
          />
        </div>
      )}
    </div>
  )
}
