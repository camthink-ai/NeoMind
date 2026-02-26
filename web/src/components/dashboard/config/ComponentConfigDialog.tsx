/**
 * ComponentConfigDialog Component
 *
 * Modern unified dialog for configuring dashboard components.
 * Layout:
 * - Desktop: Two-column (Preview + Config) with Dialog
 * - Mobile: Full-screen with Portal + scrollable tiled sections
 *
 * Fully responsive with touch-friendly controls and safe area support.
 */

import { useMemo, useState, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  Settings,
  CheckCircle2,
  Eye,
  ChevronDown,
  ChevronUp,
  X,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Field } from '@/components/ui/field'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { ConfigRenderer } from './ConfigRenderer'
import { ComponentPreview } from './ComponentPreview'
import { UnifiedDataSourceConfig } from './UnifiedDataSourceConfig'
import { DataTransformConfig } from './DataTransformConfig'
import type { ComponentConfigSchema } from './ComponentConfigBuilder'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { cn } from '@/lib/utils'
import { useIsMobile, useSafeAreaInsets } from '@/hooks/useMobile'
import { useMobileBodyScrollLock } from '@/hooks/useBodyScrollLock'

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
  // Show title in display section instead of separate input
  showTitleInDisplay?: boolean
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
  showTitleInDisplay = true,
}: ComponentConfigDialogProps) {
  const { t } = useTranslation('dashboardComponents')
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Extract config schema sections
  const dataSourceSections = configSchema?.dataSourceSections ?? []
  const styleSections = configSchema?.styleSections ?? []
  const displaySections = configSchema?.displaySections ?? []
  const allSections = configSchema?.sections ?? []

  const hasDataSource = dataSourceSections.length > 0 || allSections.some(s => s.type === 'data-source')
  const hasStyleConfig = styleSections.length > 0
  const hasDisplayConfig = showTitleInDisplay || displaySections.length > 0 || allSections.some(s => s.type !== 'data-source')

  // Tab states
  const [mobileDataSourceTab, setMobileDataSourceTab] = useState<'datasource' | 'transform'>('datasource')
  const [rightDataSourceTab, setRightDataSourceTab] = useState<'datasource' | 'transform'>('datasource')
  const [configTabValue, setConfigTabValue] = useState<'style' | 'display'>('display')

  // Mobile collapsible sections state
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set(['dataSource', 'display']))

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setMobileDataSourceTab('datasource')
      setRightDataSourceTab('datasource')
      setConfigTabValue(hasStyleConfig ? 'style' : 'display')
      setExpandedSections(new Set(['dataSource', 'display']))
    }
  }, [open, hasStyleConfig])

  // Toggle mobile section expansion
  const toggleSection = useCallback((sectionKey: string) => {
    setExpandedSections(prev => {
      const newSet = new Set(prev)
      if (newSet.has(sectionKey)) {
        newSet.delete(sectionKey)
      } else {
        newSet.add(sectionKey)
      }
      return newSet
    })
  }, [])

  // Lock body scroll when mobile dialog is open to prevent layout shift
  useMobileBodyScrollLock(isMobile && open)

  // Extract data source section props
  const dataSourceSection = [...dataSourceSections, ...allSections].find(s => s.type === 'data-source')
  const dataSourceProps = dataSourceSection?.type === 'data-source' ? dataSourceSection.props : null
  const multiple = dataSourceProps?.multiple ?? false
  const maxSources = dataSourceProps?.maxSources

  // Check data source status
  const normalizedSources = previewDataSource ? normalizeDataSource(previewDataSource) : []
  const hasConfiguredDataSource = normalizedSources.length > 0
  const hasDeviceInfoOnly = useMemo(() => {
    return normalizedSources.length > 0 && normalizedSources.every((ds: DataSource) => ds.type === 'device-info')
  }, [normalizedSources])

  // Transform support
  const supportsDataTransform = useMemo(() => {
    const transformCapableTypes = ['line-chart', 'area-chart', 'bar-chart', 'pie-chart', 'value-card', 'sparkline', 'progress-bar']
    return transformCapableTypes.includes(componentType)
  }, [componentType])

  const shouldShowDataTransform = hasConfiguredDataSource && supportsDataTransform && !hasDeviceInfoOnly

  const handleDataSourceChange = (dataSource: DataSourceOrList | DataSource | undefined) => {
    dataSourceProps?.onChange(dataSource as any)
  }

  // Preview key
  const [previewKey, setPreviewKey] = useState<string>('preview-no-ds')
  const coreIdentifier = useMemo(() => {
    if (!previewDataSource) return 'preview-no-ds'
    const sources = normalizeDataSource(previewDataSource)
    return sources.map(s => `${s.type}:${s.deviceId || ''}:${s.metricId || s.property || s.infoProperty || ''}:${s.command || ''}`).join('|')
  }, [previewDataSource])

  useEffect(() => {
    setPreviewKey(coreIdentifier)
  }, [coreIdentifier])

  const handleDataTransformChange = (updates: Partial<DataSource>) => {
    if (!previewDataSource) return
    const sources = normalizeDataSource(previewDataSource)
    const updatedSources = sources.map(source => ({ ...source, ...updates }))
    const result = Array.isArray(previewDataSource) ? updatedSources : updatedSources[0]
    dataSourceProps?.onChange(result as any)
  }

  // Sections
  const filteredStyleSections = styleSections.length > 0 ? styleSections : allSections.filter(s => s.type !== 'data-source')

  const livePreviewConfig = useMemo(() => ({ ...previewConfig, title }), [previewConfig, title])
  const livePreviewDataSource = useMemo(() => previewDataSource, [previewDataSource])

  const titleSection = useMemo(() => showTitleInDisplay ? [{
    type: 'custom' as const,
    render: () => (
      <Field>
        <Label htmlFor="component-title-display">{t('componentConfig.displayTitle')}</Label>
        <Input
          id="component-title-display"
          value={title}
          onChange={(e) => onTitleChange(e.target.value)}
          placeholder={t('componentConfig.titlePlaceholder')}
          className="h-10"
        />
      </Field>
    ),
  }] : [], [showTitleInDisplay, title, onTitleChange, t])

  const enhancedDisplaySections = useMemo(() => {
    if (showTitleInDisplay && displaySections.length > 0) {
      return [...titleSection, ...displaySections]
    }
    return displaySections
  }, [showTitleInDisplay, titleSection, displaySections])

  const finalDisplaySections = showTitleInDisplay ? enhancedDisplaySections : displaySections

  // For mobile: render full-screen portal
  if (isMobile) {
    return createPortal(
      <>
        {open && (
          <div className="fixed inset-0 z-[100] bg-background animate-in fade-in duration-200">
            <div className="flex h-full w-full flex-col">
              {/* Header */}
              <div
                className="flex items-center justify-between px-4 py-4 border-b shrink-0 bg-background"
                style={{ paddingTop: `calc(1rem + ${insets.top}px)` }}
              >
                <div className="flex items-center gap-3 min-w-0 flex-1">
                  <Settings className="h-5 w-5 text-muted-foreground shrink-0" />
                  <div className="min-w-0 flex-1">
                    <h1 className="text-base font-semibold truncate">{t('componentConfig.editComponent')}</h1>
                    <p className="text-xs text-muted-foreground truncate">
                      {componentType.replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}
                    </p>
                  </div>
                </div>
                <Button variant="ghost" size="icon" onClick={onClose} className="shrink-0">
                  <X className="h-5 w-5" />
                </Button>
              </div>

              {/* Scrollable Content */}
              <div className="flex-1 overflow-y-auto overflow-x-hidden">
                <div className="p-4 space-y-4">
                  {/* Preview Card */}
                  <MobileConfigCard
                    title={t('componentConfig.preview')}
                    icon={Eye}
                    isExpanded={expandedSections.has('preview')}
                    onToggle={() => toggleSection('preview')}
                  >
                    <div className="rounded-xl border bg-muted/20 p-4">
                      <ComponentPreview
                        key={previewKey}
                        componentType={componentType}
                        config={livePreviewConfig}
                        dataSource={livePreviewDataSource}
                        title={title}
                        showHeader={true}
                      />
                    </div>
                  </MobileConfigCard>

                  {/* Data Source Card */}
                  {hasDataSource && (
                    <MobileConfigCard
                      title={t('componentConfig.dataSource')}
                      icon={Settings}
                      isExpanded={expandedSections.has('dataSource')}
                      onToggle={() => toggleSection('dataSource')}
                      status={hasConfiguredDataSource ? 'configured' : 'empty'}
                    >
                      {shouldShowDataTransform ? (
                        <div className="space-y-3">
                          {/* Inner tabs */}
                          <div className="flex gap-2 p-1 bg-muted/50 rounded-xl">
                            <button
                              onClick={() => setMobileDataSourceTab('datasource')}
                              className={`flex-1 py-2 px-3 text-sm font-medium rounded-lg transition-all ${
                                mobileDataSourceTab === 'datasource'
                                  ? 'bg-background text-foreground shadow-sm'
                                  : 'text-muted-foreground'
                              }`}
                            >
                              {t('componentConfig.dataSource')}
                            </button>
                            <button
                              onClick={() => setMobileDataSourceTab('transform')}
                              className={`flex-1 py-2 px-3 text-sm font-medium rounded-lg transition-all ${
                                mobileDataSourceTab === 'transform'
                                  ? 'bg-background text-foreground shadow-sm'
                                  : 'text-muted-foreground'
                              }`}
                            >
                              {t('componentConfig.transform')}
                            </button>
                          </div>

                          {mobileDataSourceTab === 'datasource' && (
                            <UnifiedDataSourceConfig
                              value={previewDataSource}
                              onChange={handleDataSourceChange}
                              allowedTypes={dataSourceProps?.allowedTypes}
                              multiple={multiple}
                              maxSources={maxSources}
                            />
                          )}

                          {mobileDataSourceTab === 'transform' && (
                            <DataTransformConfig
                              dataSource={previewDataSource}
                              onChange={handleDataTransformChange}
                              chartType={componentType.replace(/-chart$/, '') as any}
                            />
                          )}
                        </div>
                      ) : (
                        <UnifiedDataSourceConfig
                          value={previewDataSource}
                          onChange={handleDataSourceChange}
                          allowedTypes={dataSourceProps?.allowedTypes}
                          multiple={multiple}
                          maxSources={maxSources}
                        />
                      )}
                    </MobileConfigCard>
                  )}

                  {/* Transform Card (separate) */}
                  {shouldShowDataTransform && !hasDataSource && (
                    <MobileConfigCard
                      title={t('componentConfig.transform')}
                      icon={Settings}
                      isExpanded={expandedSections.has('transform')}
                      onToggle={() => toggleSection('transform')}
                    >
                      <DataTransformConfig
                        dataSource={previewDataSource}
                        onChange={handleDataTransformChange}
                        chartType={componentType.replace(/-chart$/, '') as any}
                      />
                    </MobileConfigCard>
                  )}

                  {/* Style Card */}
                  {hasStyleConfig && (
                    <MobileConfigCard
                      title={t('componentConfig.style')}
                      icon={Settings}
                      isExpanded={expandedSections.has('style')}
                      onToggle={() => toggleSection('style')}
                    >
                      <ConfigRenderer sections={filteredStyleSections} />
                    </MobileConfigCard>
                  )}

                  {/* Display Card */}
                  {hasDisplayConfig && (
                    <MobileConfigCard
                      title={t('componentConfig.display')}
                      icon={Settings}
                      isExpanded={expandedSections.has('display')}
                      onToggle={() => toggleSection('display')}
                    >
                      <ConfigRenderer sections={finalDisplaySections} />
                    </MobileConfigCard>
                  )}

                  {/* Legacy fallback */}
                  {!hasDataSource && !hasStyleConfig && !displaySections.length && allSections.length > 0 && (
                    <MobileConfigCard
                      title={t('componentConfig.configOptions')}
                      icon={Settings}
                      isExpanded={true}
                      onToggle={() => {}}
                    >
                      <ConfigRenderer sections={allSections} />
                    </MobileConfigCard>
                  )}
                </div>
              </div>

              {/* Footer */}
              <div
                className="flex items-center justify-end gap-3 px-4 py-4 border-t shrink-0 bg-background"
                style={{ paddingBottom: `calc(1rem + ${insets.bottom}px)` }}
              >
                <Button variant="outline" onClick={onClose} className="min-w-[80px]">
                  {t('common.cancel')}
                </Button>
                <Button onClick={onSave} className="min-w-[80px]">
                  {t('common.saveChanges')}
                </Button>
              </div>
            </div>
          </div>
        )}
      </>,
      document.body
    )
  }

  // For desktop/tablet: use original Dialog
  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-4xl p-0 sm:p-0 gap-0 flex flex-col">
        {/* Header */}
        <DialogHeader className="px-4 py-3 border-b shrink-0">
          <div className="flex items-center gap-2.5">
            <Settings className="h-4 w-4 text-muted-foreground" />
            <div>
              <DialogTitle className="text-sm font-medium">
                {t('componentConfig.editComponent')}
              </DialogTitle>
              <p className="text-xs text-muted-foreground">
                {componentType.replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}
              </p>
            </div>
          </div>
        </DialogHeader>

        {/* Content Area */}
        <div className="flex-1 flex flex-col lg:flex-row overflow-hidden">
          {/* Tablet (md-lg): Tab-based layout */}
          <div className="flex-1 flex flex-col lg:hidden overflow-hidden">
            <Tabs defaultValue="preview" className="flex-1 flex flex-col">
              <TabsList className="grid w-full grid-cols-2 h-10 bg-muted/50 p-1 rounded-lg mx-3 mt-3">
                <TabsTrigger value="preview" className="gap-1.5 rounded data-[state=active]:bg-background data-[state=active]:shadow-sm">
                  <Eye className="h-3.5 w-3.5" />
                  <span className="text-sm">{t('componentConfig.preview')}</span>
                </TabsTrigger>
                <TabsTrigger value="config" className="gap-1.5 rounded data-[state=active]:bg-background data-[state=active]:shadow-sm">
                  <Settings className="h-3.5 w-3.5" />
                  <span className="text-sm">{t('componentConfig.config')}</span>
                </TabsTrigger>
              </TabsList>

              <TabsContent value="preview" className="flex-1 min-h-0 overflow-y-auto px-3 pb-3 mt-2">
                <div className="rounded-lg border bg-muted/20 p-3">
                  <ComponentPreview
                    key={previewKey}
                    componentType={componentType}
                    config={livePreviewConfig}
                    dataSource={livePreviewDataSource}
                    title={title}
                    showHeader={true}
                  />
                </div>
              </TabsContent>

              <TabsContent value="config" className="flex-1 min-h-0 overflow-y-auto flex flex-col">
                {hasDataSource && (
                  <div className="rounded-lg border bg-card overflow-hidden mx-3 mt-2 shrink-0">
                    {shouldShowDataTransform ? (
                      <>
                        <div className="flex items-center gap-2 px-3 py-2.5 border-b bg-muted/30">
                          <div className="flex gap-1">
                            <button
                              onClick={() => setMobileDataSourceTab('datasource')}
                              className={`px-3 h-7 text-sm rounded-none transition-colors border-b-2 ${
                                mobileDataSourceTab === 'datasource'
                                  ? 'border-primary text-foreground'
                                  : 'border-transparent text-muted-foreground hover:text-foreground'
                              }`}
                            >
                              {t('componentConfig.dataSource')}
                            </button>
                            <button
                              onClick={() => setMobileDataSourceTab('transform')}
                              className={`px-3 h-7 text-sm rounded-none transition-colors border-b-2 ${
                                mobileDataSourceTab === 'transform'
                                  ? 'border-primary text-foreground'
                                  : 'border-transparent text-muted-foreground hover:text-foreground'
                              }`}
                            >
                              {t('componentConfig.transform')}
                            </button>
                          </div>
                          {hasConfiguredDataSource && <CheckCircle2 className="h-3.5 w-3.5 text-green-500 ml-auto" />}
                        </div>

                        {mobileDataSourceTab === 'datasource' && (
                          <div className="p-3">
                            <UnifiedDataSourceConfig
                              value={previewDataSource}
                              onChange={handleDataSourceChange}
                              allowedTypes={dataSourceProps?.allowedTypes}
                              multiple={multiple}
                              maxSources={maxSources}
                            />
                          </div>
                        )}

                        {mobileDataSourceTab === 'transform' && (
                          <div className="p-3">
                            <DataTransformConfig
                              dataSource={previewDataSource}
                              onChange={handleDataTransformChange}
                              chartType={componentType.replace(/-chart$/, '') as any}
                            />
                          </div>
                        )}
                      </>
                    ) : (
                      <>
                        <div className="flex items-center gap-2 px-3 py-2.5 border-b bg-muted/30">
                          <Settings className="h-4 w-4 text-primary" />
                          <span className="text-sm font-semibold">{t('componentConfig.dataSourceConfig')}</span>
                          {hasConfiguredDataSource && <CheckCircle2 className="h-4 w-4 text-green-500 ml-auto" />}
                        </div>
                        <div className="p-3">
                          <UnifiedDataSourceConfig
                            value={previewDataSource}
                            onChange={handleDataSourceChange}
                            allowedTypes={dataSourceProps?.allowedTypes}
                            multiple={multiple}
                            maxSources={maxSources}
                          />
                        </div>
                      </>
                    )}
                  </div>
                )}

                {(hasStyleConfig || hasDisplayConfig) && (
                  <Tabs value={configTabValue} onValueChange={(v) => setConfigTabValue(v as 'style' | 'display')} className="flex-1 flex flex-col min-h-0 mx-3 mt-2 overflow-hidden">
                    <TabsList className="w-auto justify-start bg-muted/50 p-1 rounded-lg h-10 shrink-0">
                      {hasStyleConfig && (
                        <TabsTrigger value="style" className="gap-2 data-[state=active]:bg-background data-[state=active]:shadow-sm rounded-lg">
                          {t('componentConfig.style')}
                        </TabsTrigger>
                      )}
                      {hasDisplayConfig && (
                        <TabsTrigger value="display" className="gap-2 data-[state=active]:bg-background data-[state=active]:shadow-sm rounded-lg">
                          {t('componentConfig.display')}
                        </TabsTrigger>
                      )}
                    </TabsList>

                    {hasStyleConfig && (
                      <TabsContent value="style" className="flex-1 min-h-0 overflow-y-auto mt-3 data-[state=active]:flex data-[state=active]:flex-col">
                        <ConfigRenderer sections={filteredStyleSections} />
                      </TabsContent>
                    )}

                    {hasDisplayConfig && (
                      <TabsContent value="display" className="flex-1 min-h-0 overflow-y-auto mt-3 data-[state=active]:flex data-[state=active]:flex-col">
                        <ConfigRenderer sections={finalDisplaySections} />
                      </TabsContent>
                    )}
                  </Tabs>
                )}

                {!hasDataSource && !hasStyleConfig && !displaySections.length && allSections.length > 0 && (
                  <div className="rounded-lg border bg-card overflow-hidden mx-3 mt-2">
                    <div className="flex items-center gap-2 px-3 py-2.5 border-b bg-muted/30">
                      <Settings className="h-3.5 w-3.5 text-muted-foreground" />
                      <span className="text-sm font-medium">{t('componentConfig.configOptions')}</span>
                    </div>
                    <div className="p-3">
                      <ConfigRenderer sections={allSections} />
                    </div>
                  </div>
                )}
              </TabsContent>
            </Tabs>
          </div>

          {/* Desktop (lg+): Two-column layout */}
          <div className="hidden lg:flex flex-1 overflow-hidden">
            {/* Left: Preview + Style/Display Config */}
            <div className={cn(hasDataSource ? "w-1/2 border-r" : "w-full", "flex flex-col bg-background overflow-hidden")}>
              <div className="shrink-0 border-b overflow-hidden">
                <ComponentPreview
                  key={previewKey}
                  componentType={componentType}
                  config={livePreviewConfig}
                  dataSource={livePreviewDataSource}
                  title={title}
                  showHeader={true}
                />
              </div>

              {(hasStyleConfig || hasDisplayConfig) && (
                <Tabs value={configTabValue} onValueChange={(v) => setConfigTabValue(v as 'style' | 'display')} className="flex-1 flex flex-col min-h-0">
                  <TabsList className="w-auto justify-start bg-muted/50 p-1 rounded-lg px-3 h-10 shrink-0">
                    {hasStyleConfig && (
                      <TabsTrigger value="style" className="data-[state=active]:bg-background data-[state=active]:shadow-sm rounded">
                        {t('componentConfig.style')}
                      </TabsTrigger>
                    )}
                    {hasDisplayConfig && (
                      <TabsTrigger value="display" className="data-[state=active]:bg-background data-[state=active]:shadow-sm rounded">
                        {t('componentConfig.display')}
                      </TabsTrigger>
                    )}
                  </TabsList>

                  {hasStyleConfig && (
                    <TabsContent value="style" className="min-h-0 overflow-y-auto p-3">
                      <ConfigRenderer sections={filteredStyleSections} />
                    </TabsContent>
                  )}

                  {hasDisplayConfig && (
                    <TabsContent value="display" className="min-h-0 overflow-y-auto p-3">
                      <ConfigRenderer sections={finalDisplaySections} />
                    </TabsContent>
                  )}
                </Tabs>
              )}

              {!hasStyleConfig && !hasDisplayConfig && allSections.length > 0 && !hasDataSource && (
                <div className="flex-1 overflow-y-auto p-3">
                  <ConfigRenderer sections={allSections} />
                </div>
              )}
            </div>

            {/* Right: Data Source + Transform Config */}
            {hasDataSource && (
              <div className="w-1/2 flex flex-col overflow-hidden bg-background">
                <div className="flex-1 min-h-0 flex flex-col">
                  <div className="flex items-center gap-2 px-3 py-2 bg-muted/30 border-b shrink-0">
                    <div className="flex gap-1">
                      <button
                        onClick={() => setRightDataSourceTab('datasource')}
                        className={`px-3 h-8 text-sm rounded-none transition-colors border-b-2 ${
                          rightDataSourceTab === 'datasource'
                            ? 'border-primary text-foreground'
                            : 'border-transparent text-muted-foreground hover:text-foreground'
                        }`}
                      >
                        {t('componentConfig.dataSource')}
                      </button>
                      {shouldShowDataTransform && (
                        <button
                          onClick={() => setRightDataSourceTab('transform')}
                          className={`px-3 h-8 text-sm rounded-none transition-colors border-b-2 ${
                            rightDataSourceTab === 'transform'
                              ? 'border-primary text-foreground'
                              : 'border-transparent text-muted-foreground hover:text-foreground'
                          }`}
                        >
                          {t('componentConfig.transform')}
                        </button>
                      )}
                    </div>
                    {hasConfiguredDataSource && <CheckCircle2 className="h-4 w-4 text-green-500 ml-auto" />}
                  </div>

                  <div className="flex-1 min-h-0 overflow-hidden flex flex-col">
                    {rightDataSourceTab === 'datasource' ? (
                      <UnifiedDataSourceConfig
                        value={previewDataSource}
                        onChange={handleDataSourceChange}
                        allowedTypes={dataSourceProps?.allowedTypes}
                        multiple={multiple}
                        maxSources={maxSources}
                      />
                    ) : (
                      <div className="flex-1 overflow-y-auto p-3">
                        <DataTransformConfig
                          dataSource={previewDataSource}
                          onChange={handleDataTransformChange}
                          chartType={componentType.replace(/-chart$/, '') as any}
                        />
                      </div>
                    )}
                  </div>

                  {dataSourceSections.length > 1 && (
                    <div className="px-5 py-3 border-t shrink-0">
                      <ConfigRenderer sections={dataSourceSections.slice(1)} />
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Desktop Footer */}
        <DialogFooter className="px-4 py-3 border-t bg-muted/30">
          <Button variant="outline" size="sm" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button size="sm" onClick={onSave}>
            {t('common.saveChanges')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/**
 * MobileConfigCard Component
 *
 * A card-style collapsible section for mobile configuration.
 * Optimized for touch with smooth animations.
 */
interface MobileConfigCardProps {
  title: string
  icon: React.ComponentType<{ className?: string }>
  isExpanded: boolean
  onToggle: () => void
  children: React.ReactNode
  status?: 'configured' | 'empty'
}

function MobileConfigCard({
  title,
  icon: Icon,
  isExpanded,
  onToggle,
  children,
  status,
}: MobileConfigCardProps) {
  return (
    <div className="overflow-hidden rounded-2xl border border-border/50 bg-card">
      <button
        onClick={onToggle}
        className="w-full flex items-center justify-between px-4 py-4 bg-muted/30 hover:bg-muted/40 active:bg-muted/50 transition-colors touch-action-manipulation"
      >
        <div className="flex items-center gap-3">
          <Icon className="h-5 w-5 text-muted-foreground" />
          <span className="font-semibold text-foreground">{title}</span>
        </div>
        <div className="flex items-center gap-2">
          {status === 'configured' && (
            <CheckCircle2 className="h-5 w-5 text-green-500 shrink-0" />
          )}
          <div className="h-8 w-8 rounded-full bg-background flex items-center justify-center shrink-0">
            {isExpanded ? (
              <ChevronUp className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </div>
      </button>
      {isExpanded && (
        <div className="p-4 bg-background animate-in slide-in-from-top-2 duration-200 border-t border-border/30">
          {children}
        </div>
      )}
    </div>
  )
}
