/**
 * ComponentConfigDialog Component
 *
 * Unified dialog for configuring dashboard components.
 * Uses tabs to separate data source and style configuration.
 * Fully responsive with touch-friendly controls.
 */

import { useState, useEffect } from 'react'
import {
  Database,
  Palette,
  Settings2,
  ChevronRight,
  Layers,
  X,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { ConfigRenderer } from './ConfigRenderer'
import { DataSourceSelector } from './DataSourceSelector'
import type { ComponentConfigSchema } from './ComponentConfigBuilder'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { cn } from '@/lib/utils'

export interface ComponentConfigDialogProps {
  open: boolean
  onClose: () => void
  onSave: () => void
  title: string
  onTitleChange: (title: string) => void
  configSchema: ComponentConfigSchema | null
  componentType: string
}

export function ComponentConfigDialog({
  open,
  onClose,
  onSave,
  title,
  onTitleChange,
  configSchema,
  componentType,
}: ComponentConfigDialogProps) {
  const [activeTab, setActiveTab] = useState('data-source')
  const [showDataSourceSelector, setShowDataSourceSelector] = useState(false)
  const [currentDataSource, setCurrentDataSource] = useState<DataSourceOrList | undefined>(undefined)

  const dataSourceSections = configSchema?.dataSourceSections ?? []
  const styleSections = configSchema?.styleSections ?? []
  // Fallback for legacy configs
  const allSections = configSchema?.sections ?? []

  const hasDataSource = dataSourceSections.length > 0 || allSections.some(s => s.type === 'data-source')
  const hasStyleConfig = styleSections.length > 0 || allSections.some(s => s.type !== 'data-source')

  // Extract data source section props
  const dataSourceSection = [...dataSourceSections, ...allSections].find(s => s.type === 'data-source')
  const dataSourceProps = dataSourceSection?.type === 'data-source' ? dataSourceSection.props : null
  const initialDataSource = dataSourceProps?.dataSource
  const multiple = dataSourceProps?.multiple ?? false
  const maxSources = dataSourceProps?.maxSources

  // Sync currentDataSource when dialog opens or when dataSource changes
  useEffect(() => {
    if (open) {
      setCurrentDataSource(initialDataSource)
      // Reset to first tab that has content
      if (hasDataSource) {
        setActiveTab('data-source')
      } else if (hasStyleConfig) {
        setActiveTab('style')
      }
    }
  }, [open, initialDataSource, hasDataSource, hasStyleConfig])

  // Helper function to get a readable label for the current data source
  const getDataSourceLabel = (ds: DataSourceOrList | undefined): string => {
    if (!ds) return 'Select data source...'

    // Handle multiple data sources
    const dataSources = normalizeDataSource(ds)
    if (dataSources.length > 1) {
      return `${dataSources.length} data sources`
    }

    const dataSource = dataSources[0]
    if (!dataSource) return 'Select data source...'

    switch (dataSource.type) {
      case 'device':
        const device = dataSource.deviceId
        return device ? `Device: ${device}${dataSource.property ? ` (${dataSource.property})` : ''}` : 'Select device...'
      case 'metric':
        const metricNames: Record<string, string> = {
          'temperature-avg': 'Temperature Avg',
          'humidity-avg': 'Humidity Avg',
          'cpu-usage': 'CPU Usage',
          'memory-usage': 'Memory Usage',
          'device-count': 'Device Count',
        }
        return `Metric: ${metricNames[dataSource.metricId || ''] || dataSource.metricId || 'Select metric...'}`
      case 'command':
        const commandNames: Record<string, string> = {
          'toggle': 'Toggle',
          'setValue': 'Set Value',
          'setColor': 'Set Color',
          'setBrightness': 'Set Brightness',
          'setSpeed': 'Set Speed',
          'open': 'Open',
          'close': 'Close',
        }
        return `Command: ${dataSource.deviceId || ''} → ${commandNames[dataSource.command || ''] || dataSource.command || 'toggle'}`
      case 'api':
        return `API: ${dataSource.endpoint || 'Custom endpoint'}`
      case 'websocket':
        return `WebSocket: ${dataSource.endpoint || 'Live stream'}`
      case 'static':
        return `Static: ${JSON.stringify(dataSource.staticValue)?.slice(0, 20) || 'Value'}`
      case 'computed':
        return 'Computed'
      default:
        return 'Select data source...'
    }
  }

  const handleDataSourceChange = (dataSource: DataSourceOrList | DataSource | undefined) => {
    setCurrentDataSource(dataSource as DataSourceOrList | undefined)
    dataSourceProps?.onChange(dataSource as any)
  }

  // Update style sections to remove data-source section from legacy configs
  const filteredStyleSections = styleSections.length > 0
    ? styleSections
    : allSections.filter(s => s.type !== 'data-source')

  // Get component icon based on type
  const getComponentIcon = () => {
    const type = componentType.toLowerCase()
    if (type.includes('chart') || type.includes('gauge') || type.includes('sparkline') || type.includes('progress')) {
      return '📈'
    }
    if (type.includes('led') || type.includes('status')) {
      return '🔴'
    }
    if (type.includes('button') || type.includes('toggle') || type.includes('dropdown') || type.includes('input')) {
      return '🎛️'
    }
    if (type.includes('table') || type.includes('list') || type.includes('log')) {
      return '📋'
    }
    if (type.includes('card') || type.includes('counter') || type.includes('value')) {
      return '🔢'
    }
    return '📊'
  }

  return (
    <>
      <Dialog open={open} onOpenChange={onClose}>
        <DialogContent className="sm:max-w-[520px] md:max-w-[580px] p-0 gap-0 max-h-[90vh] md:max-h-[85vh] overflow-hidden flex flex-col">
          {/* Header */}
          <DialogHeader className="px-4 md:px-6 py-4 border-b shrink-0 bg-gradient-to-r from-muted/50 to-background">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-xl bg-primary/10 flex items-center justify-center text-lg">
                  {getComponentIcon()}
                </div>
                <div>
                  <DialogTitle className="text-base md:text-lg font-semibold p-0 h-auto">
                    Edit Component
                  </DialogTitle>
                  <p className="text-xs md:text-sm text-muted-foreground mt-0.5">
                    {componentType.replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}
                  </p>
                </div>
              </div>
            </div>
          </DialogHeader>

          {/* Content */}
          <div className="px-4 md:px-6 py-4 flex-1 overflow-y-auto">
            {/* Title Input */}
            <div className="space-y-2 mb-6">
              <Label htmlFor="component-title" className="text-sm font-medium">Display Title</Label>
              <div className="relative">
                <Input
                  id="component-title"
                  value={title}
                  onChange={(e) => onTitleChange(e.target.value)}
                  placeholder="Enter component title..."
                  className="h-10 pr-8 border-muted focus:border-primary transition-colors"
                />
                {title && (
                  <button
                    onClick={() => onTitleChange('')}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors p-1 rounded-md hover:bg-muted"
                  >
                    <X className="h-4 w-4" />
                  </button>
                )}
              </div>
            </div>

            {/* Tabs */}
            {hasDataSource || hasStyleConfig ? (
              <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
                <TabsList className="grid w-full grid-cols-2 h-10 bg-muted/50 p-1 rounded-lg">
                  <TabsTrigger
                    value="data-source"
                    disabled={!hasDataSource}
                    className="gap-2 data-[state=active]:bg-background data-[state=active]:shadow-sm rounded-md transition-all"
                  >
                    <Database className="h-4 w-4" />
                    <span className="hidden sm:inline">Data</span>
                    <span className="sm:hidden">Data</span>
                  </TabsTrigger>
                  <TabsTrigger
                    value="style"
                    disabled={!hasStyleConfig}
                    className="gap-2 data-[state=active]:bg-background data-[state=active]:shadow-sm rounded-md transition-all"
                  >
                    <Palette className="h-4 w-4" />
                    <span className="hidden sm:inline">Style</span>
                    <span className="sm:hidden">Style</span>
                  </TabsTrigger>
                </TabsList>

                {/* Data Source Tab */}
                <TabsContent value="data-source" className="mt-6 space-y-4">
                  {hasDataSource ? (
                    <>
                      {/* Current Data Source Display */}
                      <div className="space-y-2">
                        <Label className="text-sm font-medium flex items-center gap-2">
                          <Database className="h-4 w-4 text-muted-foreground" />
                          Data Source{multiple ? 's' : ''}
                        </Label>
                        <button
                          onClick={() => setShowDataSourceSelector(true)}
                          className="w-full flex items-center justify-between p-4 rounded-xl border-2 border-dashed border-border bg-card/50 hover:bg-accent/50 hover:border-primary/50 hover:shadow-sm transition-all group min-h-[60px]"
                        >
                          <div className="flex items-center gap-3">
                            <div className="w-10 h-10 rounded-lg bg-primary/5 flex items-center justify-center group-hover:bg-primary/10 transition-colors">
                              {multiple ? (
                                <Layers className="h-5 w-5 text-muted-foreground group-hover:text-primary transition-colors" />
                              ) : (
                                <Database className="h-5 w-5 text-muted-foreground group-hover:text-primary transition-colors" />
                              )}
                            </div>
                            <div className="text-left">
                              <span className="text-sm font-medium block">
                                {currentDataSource
                                  ? getDataSourceLabel(currentDataSource)
                                  : `Select data source${multiple ? 's' : ''}`
                                }
                              </span>
                              {!currentDataSource && (
                                <span className="text-xs text-muted-foreground">Choose where to get data from</span>
                              )}
                            </div>
                          </div>
                          <ChevronRight className="h-5 w-5 text-muted-foreground group-hover:text-primary transition-colors" />
                        </button>
                      </div>

                      {/* Other data source sections */}
                      {dataSourceSections.length > 1 && (
                        <ConfigRenderer sections={dataSourceSections.slice(1)} />
                      )}
                    </>
                  ) : (
                    <div className="text-center py-12 text-muted-foreground">
                      <div className="w-16 h-16 rounded-2xl bg-muted/30 flex items-center justify-center mx-auto mb-4">
                        <Database className="h-8 w-8 opacity-40" />
                      </div>
                      <p className="text-sm">No data source options available</p>
                    </div>
                  )}
                </TabsContent>

                {/* Style Tab */}
                <TabsContent value="style" className="mt-6 space-y-4">
                  {filteredStyleSections.length > 0 ? (
                    <ConfigRenderer sections={filteredStyleSections} />
                  ) : (
                    <div className="text-center py-12 text-muted-foreground">
                      <div className="w-16 h-16 rounded-2xl bg-muted/30 flex items-center justify-center mx-auto mb-4">
                        <Palette className="h-8 w-8 opacity-40" />
                      </div>
                      <p className="text-sm">No style options available</p>
                    </div>
                  )}
                </TabsContent>
              </Tabs>
            ) : allSections.length > 0 ? (
              // No tabs, show all sections
              <div className="space-y-4">
                <ConfigRenderer sections={allSections} />
              </div>
            ) : (
              <div className="text-center py-12 text-muted-foreground">
                <div className="w-16 h-16 rounded-2xl bg-muted/30 flex items-center justify-center mx-auto mb-4">
                  <Settings2 className="h-8 w-8 opacity-40" />
                </div>
                <p className="text-sm">No configuration options available</p>
              </div>
            )}
          </div>

          {/* Footer */}
          <div className="px-4 md:px-6 py-4 border-t flex justify-end gap-2 md:gap-3 shrink-0 bg-muted/20">
            <Button
              variant="outline"
              onClick={onClose}
              className="h-10 px-4 md:px-6 flex-1 sm:flex-initial"
            >
              Cancel
            </Button>
            <Button
              onClick={onSave}
              className="h-10 px-4 md:px-6 flex-1 sm:flex-initial shadow-sm"
            >
              Save Changes
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Data Source Selector Dialog */}
      <DataSourceSelector
        open={showDataSourceSelector}
        onClose={() => setShowDataSourceSelector(false)}
        onSelect={handleDataSourceChange}
        currentDataSource={currentDataSource}
        allowedTypes={dataSourceProps?.allowedTypes}
        multiple={multiple}
        maxSources={maxSources}
      />
    </>
  )
}
