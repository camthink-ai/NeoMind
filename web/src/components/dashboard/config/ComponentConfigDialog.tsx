/**
 * ComponentConfigDialog Component
 *
 * Unified dialog for configuring dashboard components.
 * Layout: Two-column (Preview + Config) with inline expandable data source selector.
 * Fully responsive with touch-friendly controls.
 */

import { useState, useMemo, useEffect } from 'react'
import {
  Settings,
  X,
  CheckCircle2,
  Database,
  Eye,
  ChevronDown,
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
import { Accordion, AccordionItem, AccordionTrigger, AccordionContent } from '@/components/ui/accordion'
import { ConfigRenderer } from './ConfigRenderer'
import { ComponentPreview } from './ComponentPreview'
import { DataSourceSelectorContent } from './DataSourceSelectorContent'
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
  // Preview props
  previewDataSource?: DataSource
  previewConfig?: Record<string, unknown>
}

export function ComponentConfigDialog({
  open,
  onClose,
  onSave,
  title,
  onTitleChange,
  configSchema,
  componentType,
  previewDataSource,
  previewConfig = {},
}: ComponentConfigDialogProps) {
  const [dataSourceExpanded, setDataSourceExpanded] = useState(false)

  // Reset expanded state when dialog opens
  useEffect(() => {
    if (open) {
      setDataSourceExpanded(false)
    }
  }, [open])

  // Live preview config - combines initial config with current title
  const livePreviewConfig = useMemo(() => ({
    ...previewConfig,
    title,  // Title changes in real-time
  }), [previewConfig, title])

  const livePreviewDataSource = useMemo(() => {
    // Use previewDataSource from props (which comes from componentConfig and updates live)
    // This ensures the preview updates as soon as config changes
    return previewDataSource
  }, [previewDataSource])

  const dataSourceSections = configSchema?.dataSourceSections ?? []
  const styleSections = configSchema?.styleSections ?? []
  const displaySections = configSchema?.displaySections ?? []
  // Fallback for legacy configs
  const allSections = configSchema?.sections ?? []

  const hasDataSource = dataSourceSections.length > 0 || allSections.some(s => s.type === 'data-source')
  const hasStyleConfig = styleSections.length > 0
  const hasDisplayConfig = displaySections.length > 0 || allSections.some(s => s.type !== 'data-source')
  const hasAnyConfig = hasDataSource || hasStyleConfig || hasDisplayConfig || allSections.length > 0

  // Extract data source section props
  const dataSourceSection = [...dataSourceSections, ...allSections].find(s => s.type === 'data-source')
  const dataSourceProps = dataSourceSection?.type === 'data-source' ? dataSourceSection.props : null
  const multiple = dataSourceProps?.multiple ?? false
  const maxSources = dataSourceProps?.maxSources

  // Check if data source is configured and count for multiple sources
  const normalizedSources = previewDataSource ? normalizeDataSource(previewDataSource) : []
  const hasConfiguredDataSource = normalizedSources.length > 0
  const dataSourceCount = normalizedSources.length

  const handleDataSourceChange = (dataSource: DataSourceOrList | DataSource | undefined) => {
    dataSourceProps?.onChange(dataSource as any)
  }

  // Helper function to get a readable label for the current data source
  const getDataSourceLabel = (ds: DataSourceOrList | undefined): string => {
    if (!ds) return '点击选择数据源'

    const dataSources = normalizeDataSource(ds)
    const count = dataSources.length

    if (count === 0) return '点击选择数据源'
    if (count === 1) {
      const dataSource = dataSources[0]
      if (!dataSource) return '点击选择数据源'

      switch (dataSource.type) {
        case 'device':
          return `设备: ${dataSource.deviceId}${dataSource.property ? ` (${dataSource.property})` : ''}`
        case 'device-info':
          return `信息: ${dataSource.deviceId}${dataSource.infoProperty ? ` (${dataSource.infoProperty})` : ''}`
        case 'telemetry':
          return `遥测: ${dataSource.deviceId} / ${dataSource.metricId || 'raw'}`
        case 'metric':
          return `指标: ${dataSource.metricId || '未指定'}`
        case 'command':
          return `指令: ${dataSource.deviceId} → ${dataSource.command || 'toggle'}`
        case 'api':
          return `API: ${dataSource.endpoint || '自定义'}`
        case 'websocket':
          return `WebSocket: ${dataSource.endpoint || '实时'}`
        case 'static':
          return `静态值`
        default:
          return '已配置'
      }
    }

    // Multiple data sources
    return `已选择 ${count} 个数据源`
  }

  // Update style sections to remove data-source section from legacy configs
  const filteredStyleSections = styleSections.length > 0
    ? styleSections
    : allSections.filter(s => s.type !== 'data-source')

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="
        max-w-[95vw] w-[1000px]
        p-0 gap-0 max-h-[95vh] overflow-hidden flex flex-col
        [&>[data-radix-dialog-close]]:right-4 [&>[data-radix-dialog-close]]:top-5
      ">
        {/* Header */}
        <DialogHeader className="px-4 py-3 border-b shrink-0 bg-gradient-to-r from-muted/50 to-background">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-9 h-9 rounded-lg bg-primary/10 flex items-center justify-center">
                <Settings className="h-4.5 w-4.5 text-primary" />
              </div>
              <div>
                <DialogTitle className="text-base font-semibold p-0 h-auto">
                  编辑组件
                </DialogTitle>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {componentType.replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}
                </p>
              </div>
            </div>
          </div>
        </DialogHeader>

        {/* Content Area */}
        {/* Large screens (>= 1024px): Three-column layout */}
        {/* Small screens (< 1024px): Tab-based layout */}
        <div className="flex-1 flex flex-col lg:flex-row overflow-hidden">
          {/* Title Input (always at top on small screens, in config panel on large screens) */}
          <div className="lg:hidden px-4 py-3 border-b bg-background/50">
            <div className="space-y-1.5">
              <Label htmlFor="component-title-mobile" className="text-xs font-medium">显示标题</Label>
              <div className="relative">
                <Input
                  id="component-title-mobile"
                  value={title}
                  onChange={(e) => onTitleChange(e.target.value)}
                  placeholder="输入组件标题..."
                  className="h-9 pr-8 text-sm"
                />
                {title && (
                  <button
                    onClick={() => onTitleChange('')}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded hover:bg-muted"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>
            </div>
          </div>

          {/* Small screen: Tab-based layout */}
          <div className="flex-1 flex flex-col lg:hidden overflow-hidden">
            <Tabs defaultValue="preview" className="flex-1 flex flex-col">
              <TabsList className="grid w-full grid-cols-2 h-10 bg-muted/50 p-1 rounded-lg m-3">
                <TabsTrigger value="preview" className="gap-2">
                  <Eye className="h-4 w-4" />
                  <span>预览</span>
                </TabsTrigger>
                <TabsTrigger value="config" className="gap-2">
                  <Settings className="h-4 w-4" />
                  <span>配置</span>
                </TabsTrigger>
              </TabsList>

              {/* Preview Tab */}
              <TabsContent value="preview" className="flex-1 overflow-y-auto px-3 pb-3">
                <ComponentPreview
                  componentType={componentType}
                  config={livePreviewConfig}
                  dataSource={livePreviewDataSource}
                  title={title}
                  showHeader={true}
                />
              </TabsContent>

              {/* Config Tab */}
              <TabsContent value="config" className="flex-1 overflow-y-auto px-3 pb-3">
                <Accordion type="multiple" defaultValue={['data-source']} className="space-y-3">
                  {/* Data Source Section */}
                  {hasDataSource && (
                    <AccordionItem value="data-source" className="border rounded-lg px-3">
                      <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                        <div className="flex items-center gap-2">
                          <span>数据源</span>
                          {hasConfiguredDataSource && (
                            <CheckCircle2 className="h-3.5 w-3.5 text-green-500" />
                          )}
                        </div>
                      </AccordionTrigger>
                      <AccordionContent className="pt-0 pb-3 space-y-2">
                        {/* Summary button - toggles selector visibility */}
                        <Button
                          variant="outline"
                          className={cn(
                            "w-full justify-start h-auto px-3 py-2.5 text-left transition-colors",
                            dataSourceExpanded && "bg-muted/50 border-primary/50"
                          )}
                          onClick={() => setDataSourceExpanded(!dataSourceExpanded)}
                        >
                          <Database className="h-4 w-4 mr-2 shrink-0 text-muted-foreground" />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center justify-between">
                              <span className="text-sm font-medium">{getDataSourceLabel(previewDataSource)}</span>
                              {dataSourceCount > 0 && (
                                <span className="text-xs bg-primary/10 text-primary px-1.5 py-0.5 rounded-full">
                                  {dataSourceCount}
                                </span>
                              )}
                            </div>
                          </div>
                          <ChevronDown className={cn(
                            "h-3.5 w-3.5 ml-2 shrink-0 text-muted-foreground transition-transform",
                            dataSourceExpanded && "rotate-180"
                          )} />
                        </Button>

                        {/* Inline selector - expands when button is clicked */}
                        {dataSourceExpanded && (
                          <div className="border rounded-lg overflow-hidden bg-background/50">
                            <div className="max-h-[300px] overflow-y-auto">
                              <DataSourceSelectorContent
                                onSelect={handleDataSourceChange}
                                currentDataSource={previewDataSource}
                                allowedTypes={dataSourceProps?.allowedTypes}
                                multiple={multiple}
                                maxSources={maxSources}
                              />
                            </div>
                          </div>
                        )}

                        {dataSourceSections.length > 1 && (
                          <ConfigRenderer sections={dataSourceSections.slice(1)} />
                        )}
                      </AccordionContent>
                    </AccordionItem>
                  )}

                  {/* Style Options Section */}
                  {hasStyleConfig && (
                    <AccordionItem value="style" className="border rounded-lg px-3">
                      <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                        样式选项
                      </AccordionTrigger>
                      <AccordionContent className="pt-0 pb-3">
                        <ConfigRenderer sections={filteredStyleSections} />
                      </AccordionContent>
                    </AccordionItem>
                  )}

                  {/* Display Config Section */}
                  {hasDisplayConfig && displaySections.length > 0 && (
                    <AccordionItem value="display" className="border rounded-lg px-3">
                      <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                        显示配置
                      </AccordionTrigger>
                      <AccordionContent className="pt-0 pb-3">
                        <ConfigRenderer sections={displaySections} />
                      </AccordionContent>
                    </AccordionItem>
                  )}

                  {/* Legacy sections fallback */}
                  {!hasDataSource && !hasStyleConfig && !displaySections.length && allSections.length > 0 && (
                    <AccordionItem value="legacy" className="border rounded-lg px-3">
                      <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                        配置选项
                      </AccordionTrigger>
                      <AccordionContent className="pt-0 pb-3">
                        <ConfigRenderer sections={allSections} />
                      </AccordionContent>
                    </AccordionItem>
                  )}
                </Accordion>
              </TabsContent>
            </Tabs>
          </div>

          {/* Large screens: Two-column layout */}
          <div className="hidden lg:flex flex-1 overflow-hidden">
            {/* Left: Preview Panel (35%) */}
            <div className="w-[35%] min-w-[300px] border-r flex flex-col bg-muted/10">
              <ComponentPreview
                componentType={componentType}
                config={livePreviewConfig}
                dataSource={livePreviewDataSource}
                title={title}
                showHeader={true}
              />
            </div>

            {/* Right: Configuration Panel (65%) */}
            <div className="flex-1 min-w-[400px] flex flex-col overflow-hidden">
              {/* Title Input */}
              <div className="px-4 py-3 border-b bg-background/50">
                <div className="space-y-1.5">
                  <Label htmlFor="component-title" className="text-xs font-medium">显示标题</Label>
                  <div className="relative">
                    <Input
                      id="component-title"
                      value={title}
                      onChange={(e) => onTitleChange(e.target.value)}
                      placeholder="输入组件标题..."
                      className="h-9 pr-8 text-sm"
                    />
                    {title && (
                      <button
                        onClick={() => onTitleChange('')}
                        className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded hover:bg-muted"
                      >
                        <X className="h-3.5 w-3.5" />
                      </button>
                    )}
                  </div>
                </div>
              </div>

              {/* Configuration Accordion */}
              <div className="flex-1 overflow-y-auto px-4 py-3">
                {hasAnyConfig ? (
                  <Accordion type="multiple" defaultValue={['data-source', 'style']} className="space-y-3">
                    {/* Data Source Section */}
                    {hasDataSource && (
                      <AccordionItem value="data-source" className="border rounded-lg px-3">
                        <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                          <div className="flex items-center gap-2">
                            <span>数据源</span>
                            {hasConfiguredDataSource && (
                              <CheckCircle2 className="h-3.5 w-3.5 text-green-500" />
                            )}
                          </div>
                        </AccordionTrigger>
                        <AccordionContent className="pt-0 pb-3 space-y-2">
                          {/* Summary button - toggles selector visibility */}
                          <Button
                            variant="outline"
                            className={cn(
                              "w-full justify-start h-auto px-3 py-2.5 text-left transition-colors",
                              dataSourceExpanded && "bg-muted/50 border-primary/50"
                            )}
                            onClick={() => setDataSourceExpanded(!dataSourceExpanded)}
                          >
                            <Database className="h-4 w-4 mr-2 shrink-0 text-muted-foreground" />
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center justify-between">
                                <span className="text-sm font-medium">{getDataSourceLabel(previewDataSource)}</span>
                                {dataSourceCount > 0 && (
                                  <span className="text-xs bg-primary/10 text-primary px-1.5 py-0.5 rounded-full">
                                    {dataSourceCount}
                                  </span>
                                )}
                              </div>
                            </div>
                            <ChevronDown className={cn(
                              "h-3.5 w-3.5 ml-2 shrink-0 text-muted-foreground transition-transform",
                              dataSourceExpanded && "rotate-180"
                            )} />
                          </Button>

                          {/* Inline selector - expands when button is clicked */}
                          {dataSourceExpanded && (
                            <div className="border rounded-lg overflow-hidden bg-background/50">
                              <div className="max-h-[300px] overflow-y-auto">
                                <DataSourceSelectorContent
                                  onSelect={handleDataSourceChange}
                                  currentDataSource={previewDataSource}
                                  allowedTypes={dataSourceProps?.allowedTypes}
                                  multiple={multiple}
                                  maxSources={maxSources}
                                />
                              </div>
                            </div>
                          )}

                          {/* Other data source related sections */}
                          {dataSourceSections.length > 1 && (
                            <ConfigRenderer sections={dataSourceSections.slice(1)} />
                          )}
                        </AccordionContent>
                      </AccordionItem>
                    )}

                    {/* Style Options Section */}
                    {hasStyleConfig && (
                      <AccordionItem value="style" className="border rounded-lg px-3">
                        <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                          样式选项
                        </AccordionTrigger>
                        <AccordionContent className="pt-0 pb-3">
                          <ConfigRenderer sections={filteredStyleSections} />
                        </AccordionContent>
                      </AccordionItem>
                    )}

                    {/* Display Config Section */}
                    {hasDisplayConfig && displaySections.length > 0 && (
                      <AccordionItem value="display" className="border rounded-lg px-3">
                        <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                          显示配置
                        </AccordionTrigger>
                        <AccordionContent className="pt-0 pb-3">
                          <ConfigRenderer sections={displaySections} />
                        </AccordionContent>
                      </AccordionItem>
                    )}

                    {/* Legacy sections fallback */}
                    {!hasDataSource && !hasStyleConfig && !displaySections.length && allSections.length > 0 && (
                      <AccordionItem value="legacy" className="border rounded-lg px-3">
                        <AccordionTrigger className="py-3 text-sm font-medium hover:no-underline">
                          配置选项
                        </AccordionTrigger>
                        <AccordionContent className="pt-0 pb-3">
                          <ConfigRenderer sections={allSections} />
                        </AccordionContent>
                      </AccordionItem>
                    )}
                  </Accordion>
                ) : (
                  <div className="text-center py-12 text-muted-foreground">
                    <div className="w-12 h-12 rounded-xl bg-muted/30 flex items-center justify-center mx-auto mb-3">
                      <Settings className="h-6 w-6 opacity-40" />
                    </div>
                    <p className="text-sm">此组件无可用的配置选项</p>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="px-4 py-3 border-t flex justify-end gap-2 shrink-0 bg-muted/20">
          <Button
            variant="outline"
            onClick={onClose}
            className="h-9 px-4"
          >
            取消
          </Button>
          <Button
            onClick={onSave}
            className="h-9 px-4"
          >
            保存更改
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
