/**
 * Data Source Configuration Component
 *
 * Allows users to bind dashboard components to data sources.
 */

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

const dataSourceTypes = [
  { value: 'static', label: 'Static Value', icon: Calculator, description: 'Fixed value' },
  { value: 'api', label: 'API Endpoint', icon: Server, description: 'Fetch from API' },
  { value: 'websocket', label: 'WebSocket', icon: Globe, description: 'Real-time updates' },
  { value: 'computed', label: 'Computed', icon: Calculator, description: 'Calculate from others' },
]

export function DataSourceConfig({ dataSource, onChange, className }: DataSourceConfigProps) {
  const currentType = dataSource?.type || 'static'

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
        <Label>Data Source</Label>
        {dataSource && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onChange(undefined)}
            className="h-7 text-xs text-muted-foreground"
          >
            <Unplug className="w-3 h-3 mr-1" />
            Unbind
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
          Bind to Data Source
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
              <Label>Static Value</Label>
              <Input
                type="number"
                value={String(dataSource.staticValue ?? 0)}
                onChange={(e) => updateField('staticValue', parseFloat(e.target.value) || 0)}
                placeholder="Enter a fixed value"
              />
            </div>
          </TabsContent>

          {/* API Endpoint */}
          <TabsContent value="api" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label>API Endpoint</Label>
              <Input
                value={dataSource.endpoint || ''}
                onChange={(e) => updateField('endpoint', e.target.value)}
                placeholder="/api/devices/telemetry"
              />
              <p className="text-xs text-muted-foreground">
                Enter the API path to fetch data from
              </p>
            </div>
            <div className="space-y-2">
              <Label>Refresh Interval (seconds)</Label>
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
              <Label>Event Topic</Label>
              <Input
                value={dataSource.endpoint || ''}
                onChange={(e) => updateField('endpoint', e.target.value)}
                placeholder="device.metric"
              />
              <p className="text-xs text-muted-foreground">
                Subscribe to real-time event updates
              </p>
            </div>
            <div className="space-y-2">
              <Label>Refresh Interval (seconds)</Label>
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
              <Label>Expression</Label>
              <Input
                value={(dataSource.params?.expression as string) || ''}
                onChange={(e) => updateField('params', { ...dataSource.params, expression: e.target.value })}
                placeholder="data1 + data2 * 0.5"
              />
              <p className="text-xs text-muted-foreground">
                Simple math expression (e.g., 10 + 20)
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
  return (
    <div className={cn('space-y-2', className)}>
      <Label>Data Source</Label>
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
          <SelectValue placeholder="Select data source" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="static">
            <div className="flex items-center gap-2">
              <Calculator className="w-4 h-4" />
              <span>Static Value</span>
            </div>
          </SelectItem>
          <SelectItem value="api">
            <div className="flex items-center gap-2">
              <Server className="w-4 h-4" />
              <span>API Endpoint</span>
            </div>
          </SelectItem>
          <SelectItem value="websocket">
            <div className="flex items-center gap-2">
              <Globe className="w-4 h-4" />
              <span>WebSocket</span>
            </div>
          </SelectItem>
        </SelectContent>
      </Select>

      {/* Show endpoint input for API/WebSocket */}
      {value && (value.type === 'api' || value.type === 'websocket') && (
        <div className="space-y-2">
          <Label>Endpoint / Topic</Label>
          <Input
            value={value.endpoint || ''}
            onChange={(e) => onChange({ ...value, endpoint: e.target.value })}
            placeholder={value.type === 'api' ? '/api/devices/telemetry' : 'device.metric'}
          />
          <div className="flex items-center gap-2">
            <Label className="text-xs">Refresh (s)</Label>
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
          <Label>Value</Label>
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
