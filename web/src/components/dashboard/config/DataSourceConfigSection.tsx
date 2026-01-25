/**
 * DataSourceConfigSection Component
 *
 * Standardized data source configuration section.
 * All components with data binding use this consistent UI.
 */

import { Button } from '@/components/ui/button'
import { ConfigSection } from './ConfigSection'
import { Database, Zap, Settings } from 'lucide-react'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource as normalizeDs } from '@/types/dashboard'

export interface DataSourceConfigSectionProps {
  dataSource?: DataSourceOrList
  onChange: (dataSource: DataSourceOrList | DataSource | undefined) => void
  collapsible?: boolean
  multiple?: boolean
  maxSources?: number
  // Allowed data source types
  allowedTypes?: Array<'device' | 'metric' | 'command'>
}

export function DataSourceConfigSection({
  dataSource,
  onChange,
  collapsible = true,
  // Reserved for future multi-select functionality
  multiple: _multiple = false,
  // Reserved for future multi-select functionality
  maxSources: _maxSources,
  allowedTypes = ['device', 'metric', 'command'],
}: DataSourceConfigSectionProps) {
  const dataSources = normalizeDs(dataSource)
  const isBound = dataSources.length > 0

  const getDisplayText = () => {
    if (dataSources.length === 0) return 'Data Source'
    if (dataSources.length === 1) {
      const ds = dataSources[0]
      if (ds.type === 'device') return `Device: ${ds.deviceId}${ds.property ? `.${ds.property}` : ''}`
      if (ds.type === 'metric') return `Metric: ${ds.metricId}`
      if (ds.type === 'command') return `Command: ${ds.deviceId} → ${ds.command}`
      return 'Data Source'
    }
    return `${dataSources.length} Data Sources`
  }

  const handleBindDevice = () => {
    // TODO: Open device/data source selector dialog
    const newDataSource = {
      type: 'device',
      deviceId: 'device-1',
      property: 'temperature',
    } as unknown as DataSource
    onChange(newDataSource)
  }

  const handleBindMetric = () => {
    // TODO: Open metric selector dialog
    const newDataSource = {
      type: 'metric',
      metricId: 'temperature-avg',
    } as unknown as DataSource
    onChange(newDataSource)
  }

  const handleBindCommand = () => {
    // TODO: Open command selector dialog (select device + command)
    const newDataSource = {
      type: 'command',
      deviceId: 'device-1',
      command: 'toggle',
    } as unknown as DataSource
    onChange(newDataSource)
  }

  const handleClearBinding = () => {
    onChange(undefined)
  }

  return (
    <ConfigSection
      title="Data Source"
      bordered
      collapsible={collapsible}
      defaultCollapsed={!isBound}
    >
      {isBound ? (
        <div className="space-y-2">
          <div className="flex items-center justify-between p-2 bg-muted/50 rounded-md">
            <div className="flex items-center gap-2 text-sm">
              <Database className="h-4 w-4 text-primary" />
              <span className="text-foreground">{getDisplayText()}</span>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleClearBinding}
              className="h-7 text-xs"
            >
              Clear
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">
            Bind this component to a data source for real-time updates
          </p>
          <div className="grid grid-cols-3 gap-2">
            {allowedTypes.includes('device') && (
              <Button
                variant="outline"
                size="sm"
                onClick={handleBindDevice}
                className="flex-col gap-1 h-auto py-3"
              >
                <Database className="h-4 w-4" />
                <span className="text-xs">Device</span>
              </Button>
            )}
            {allowedTypes.includes('metric') && (
              <Button
                variant="outline"
                size="sm"
                onClick={handleBindMetric}
                className="flex-col gap-1 h-auto py-3"
              >
                <Zap className="h-4 w-4" />
                <span className="text-xs">Metric</span>
              </Button>
            )}
            {allowedTypes.includes('command') && (
              <Button
                variant="outline"
                size="sm"
                onClick={handleBindCommand}
                className="flex-col gap-1 h-auto py-3"
              >
                <Settings className="h-4 w-4" />
                <span className="text-xs">Command</span>
              </Button>
            )}
          </div>
        </div>
      )}
    </ConfigSection>
  )
}
