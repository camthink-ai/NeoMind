/**
 * ComponentConfigDialog Component
 *
 * Full-screen dialog for configuring dashboard components.
 * Layout:
 * - Desktop: Left = Preview (full height), Right = Style | Display | Data Source | Transform tabs
 * - Mobile: Full-screen with scrollable tiled sections
 *
 * Fully responsive with touch-friendly controls and safe area support.
 */

import { getPortalRoot } from '@/lib/portal'
import { useMemo, useState, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  Settings,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  X,
  Sparkles,
  Loader2,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Field } from '@/components/ui/field'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
} from '@/components/automation/dialog'
import { ConfigRenderer } from './ConfigRenderer'
import { ComponentPreview } from './ComponentPreview'
import ComponentRenderer from '@/components/dashboard/registry/ComponentRenderer'
import { UnifiedDataSourceConfig } from './UnifiedDataSourceConfig'
import { DataTransformConfig } from './DataTransformConfig'
import { getComponentMeta } from '@/components/dashboard/registry/registry'
import type { ComponentConfigSchema } from './ComponentConfigBuilder'
import type { DataSource, DataSourceOrList, DataSourceMode, ComponentPosition, ImplementedComponentType, DashboardComponent } from '@/types/dashboard'
import { normalizeDataSource, getSourceId } from '@/types/dashboard'
import { cn } from '@/lib/utils'
import { dialogHeader, responsiveCols } from '@/design-system/tokens/size'
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
  // Component position for preview sizing
  position?: ComponentPosition
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
  position,
}: ComponentConfigDialogProps) {
  const { t } = useTranslation('dashboardComponents')
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()
  const [saving, setSaving] = useState(false)

  // Extract config schema sections
  const dataSourceSections = configSchema?.dataSourceSections ?? []
  const styleSections = configSchema?.styleSections ?? []
  const displaySections = configSchema?.displaySections ?? []
  const advancedSections = configSchema?.advancedSections ?? []
  const allSections = configSchema?.sections ?? []

  const hasDataSource = dataSourceSections.length > 0 || allSections.some(s => s.type === 'data-source')
  const hasStyleConfig = styleSections.length > 0
  const hasDisplayConfig = showTitleInDisplay || displaySections.length > 0 || allSections.some(s => s.type !== 'data-source')
  const hasAdvancedConfig = advancedSections.length > 0

  // Desktop preview: build mock component for direct rendering
  const meta = getComponentMeta(componentType as ImplementedComponentType)

  // Compute suggestedMode for DataSource config based on component type
  const suggestedMode = useMemo((): DataSourceMode | undefined => {
    const latestTypes = ['led-indicator', 'value-card', 'progress-bar', 'markdown-display', 'image-display', 'video-display', 'web-display']
    const timeseriesTypes = ['line-chart', 'area-chart', 'bar-chart', 'pie-chart', 'sparkline', 'image-history', 'ai-analyst']
    const commandTypes = ['toggle-switch']
    const infoTypes = ['map-display', 'custom-layer']
    if (latestTypes.includes(componentType)) return 'latest'
    if (timeseriesTypes.includes(componentType)) return 'timeseries'
    if (commandTypes.includes(componentType)) return 'command'
    if (infoTypes.includes(componentType)) return 'info'
    return undefined
  }, [componentType])
  const previewGridW = position?.w ?? meta?.sizeConstraints.defaultW ?? 4
  const previewGridH = position?.h ?? meta?.sizeConstraints.defaultH ?? 3

  const previewMockComponent = useMemo((): DashboardComponent => ({
    id: 'preview',
    type: componentType as ImplementedComponentType,
    position: { x: 0, y: 0, w: previewGridW, h: previewGridH },
    title: title || (previewConfig?.label as string) || (previewConfig?.title as string) || '',
    config: { ...previewConfig, editMode: true },
    dataSource: previewDataSource,
  }), [componentType, previewGridW, previewGridH, title, previewConfig, previewDataSource])

  // Calculate the pixel-based aspect ratio matching react-grid-layout's actual rendering.
  // Grid unit ratio (4/3) is wrong because width scales with container width while height is fixed.
  const previewPixelAspectRatio = useMemo(() => {
    const GRID_ROW_HEIGHT = 60
    const MARGIN = 4
    const PADDING = 4
    const COLS = responsiveCols.lg  // 12
    const dashboardWidth = typeof window !== 'undefined' ? window.innerWidth : 1200
    const colWidth = (dashboardWidth - MARGIN * (COLS - 1) - PADDING * 2) / COLS
    const pixelW = colWidth * previewGridW + Math.max(0, previewGridW - 1) * MARGIN
    const pixelH = GRID_ROW_HEIGHT * previewGridH + Math.max(0, previewGridH - 1) * MARGIN
    return pixelW / pixelH
  }, [previewGridW, previewGridH])

  // Tab states
  const [mobileDataSourceTab, setMobileDataSourceTab] = useState<'datasource' | 'transform'>('datasource')
  const [configTabValue, setConfigTabValue] = useState<'style' | 'display' | 'datasource' | 'transform' | 'advanced'>('display')

  // Mobile collapsible sections state
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set(['dataSource', 'display']))

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setMobileDataSourceTab('datasource')
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
    return sources.map(s => {
      const tw = s.timeWindow ? `${s.timeWindow.type}:${s.timeWindow.startTime ?? ''}:${s.timeWindow.endTime ?? ''}` : ''
      return `${s.type}:${getSourceId(s) || ''}:${s.metricId || s.property || s.infoProperty || ''}:${s.command || ''}:${s.aggregateExt ?? ''}:${tw}:${s.limit ?? ''}`
    }).join('|')
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
          <div className="fixed inset-0 z-50 bg-background animate-in fade-in duration-200">
            <div className="flex h-full w-full flex-col">
              {/* Header */}
              <div
                className={dialogHeader}
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
                    isExpanded={expandedSections.has('preview')}
                    onToggle={() => toggleSection('preview')}
                  >
                    <div className="rounded-lg border bg-muted-20 p-4">
                      <ComponentPreview
                        key={previewKey}
                        componentType={componentType}
                        config={livePreviewConfig}
                        dataSource={livePreviewDataSource}
                        title={title}
                        showHeader={true}
                        maxContentHeight={280}
                      />
                    </div>
                  </MobileConfigCard>

                  {/* Data Source Card */}
                  {hasDataSource && (
                    <MobileConfigCard
                      title={t('componentConfig.dataSource')}
                      isExpanded={expandedSections.has('dataSource')}
                      onToggle={() => toggleSection('dataSource')}
                    >
                      {dataSourceSection ? (
                        // Legacy: has a proper data-source section
                        shouldShowDataTransform ? (
                          <div className="space-y-3">
                            {/* Inner tabs */}
                            <div className="flex gap-2 p-1 bg-muted-50 rounded-lg">
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
                                suggestedMode={suggestedMode}
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
                            suggestedMode={suggestedMode}
                          />
                        )
                      ) : (
                        // New: custom dataSourceSections (e.g., DualModeSourceField)
                        <ConfigRenderer sections={dataSourceSections} />
                      )}
                    </MobileConfigCard>
                  )}

                  {/* Transform Card (separate) */}
                  {shouldShowDataTransform && !hasDataSource && (
                    <MobileConfigCard
                      title={t('componentConfig.transform')}
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
                      isExpanded={expandedSections.has('style')}
                      onToggle={() => toggleSection('style')}
                    >
                      <ConfigRenderer sections={filteredStyleSections} />
                    </MobileConfigCard>
                  )}

                  {/* Advanced Card */}
                  {hasAdvancedConfig && (
                    <MobileConfigCard
                      title={t('componentConfig.advanced', 'Advanced')}
                      isExpanded={expandedSections.has('advanced')}
                      onToggle={() => toggleSection('advanced')}
                    >
                      <ConfigRenderer sections={advancedSections} />
                    </MobileConfigCard>
                  )}

                  {/* Display Card */}
                  {hasDisplayConfig && (
                    <MobileConfigCard
                      title={t('componentConfig.display')}
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
                className="flex items-center justify-end gap-3 px-4 py-4 border-t shrink-0"
                style={{ paddingBottom: `calc(1rem + ${insets.bottom}px)` }}
              >
                <Button variant="outline" onClick={onClose} className="min-w-[80px]">
                  {t('common.cancel')}
                </Button>
                <Button onClick={async () => { setSaving(true); try { await onSave() } finally { setSaving(false) } }} className="min-w-[80px]" disabled={saving}>
                  {saving && <Loader2 className="h-4 w-4 mr-1.5 animate-spin" />}
                  {t('common.saveChanges')}
                </Button>
              </div>
            </div>
          </div>
        )}
      </>, getPortalRoot()
    )
  }

  // For desktop: use FullScreenDialog
  return (
    <FullScreenDialog open={open} onOpenChange={(open) => !open && onClose()}>
      {/* Header */}
      <FullScreenDialogHeader
        icon={<Settings className="h-5 w-5" />}
        iconBg="bg-muted dark:bg-muted"
        iconColor="text-primary"
        title={t('componentConfig.editComponent')}
        subtitle={componentType.replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase())}
        onClose={onClose}
      />

      {/* Content Area */}
      <FullScreenDialogContent className="!p-0">
        <div className="h-full w-full flex">
          {/* Left: Preview */}
          <div className="flex-1 flex flex-col bg-muted-20 overflow-hidden border-r min-w-0">
            <div className="flex-1 flex items-center justify-center p-4 min-h-0">
              <div
                className="rounded-lg overflow-hidden border bg-background shadow-sm max-w-lg max-h-full"
                style={{ aspectRatio: `${previewPixelAspectRatio}`, width: '100%' }}
              >
                <ComponentRenderer
                  key={`${previewKey}-${previewGridW}x${previewGridH}`}
                  component={previewMockComponent}
                />
              </div>
            </div>
          </div>

          {/* Right: Config tabs */}
          <div className="flex-1 flex flex-col overflow-hidden bg-background min-w-0">
            {(hasStyleConfig || hasDisplayConfig || hasDataSource) ? (
              <Tabs value={configTabValue} onValueChange={(v) => setConfigTabValue(v as typeof configTabValue)} className="flex-1 flex flex-col min-h-0">
                <TabsList className="w-full justify-start shrink-0">
                  {hasStyleConfig && (
                    <TabsTrigger value="style">
                      {t('componentConfig.style')}
                    </TabsTrigger>
                  )}
                  {hasDisplayConfig && (
                    <TabsTrigger value="display">
                      {t('componentConfig.display')}
                    </TabsTrigger>
                  )}
                  {hasDataSource && (
                    <TabsTrigger value="datasource">
                      {t('componentConfig.dataSource')}
                    </TabsTrigger>
                  )}
                  {hasAdvancedConfig && (
                    <TabsTrigger value="advanced">
                      {t('componentConfig.advanced', 'Advanced')}
                    </TabsTrigger>
                  )}
                  {shouldShowDataTransform && (
                    <TabsTrigger value="transform">
                      {t('componentConfig.transform')}
                    </TabsTrigger>
                  )}
                </TabsList>

                {hasStyleConfig && (
                  <TabsContent value="style" className="flex-1 min-h-0 overflow-y-auto p-4">
                    <ConfigRenderer sections={filteredStyleSections} />
                  </TabsContent>
                )}

                {hasDisplayConfig && (
                  <TabsContent value="display" className="flex-1 min-h-0 overflow-y-auto p-4">
                    <ConfigRenderer sections={finalDisplaySections} />
                  </TabsContent>
                )}

                {hasDataSource && (
                  <TabsContent value="datasource" className="flex-1 min-h-0 overflow-y-auto">
                    {dataSourceSection ? (
                      // Legacy: first section is a data-source type → render UnifiedDataSourceConfig
                      <>
                        <UnifiedDataSourceConfig
                          value={previewDataSource}
                          onChange={handleDataSourceChange}
                          allowedTypes={dataSourceProps?.allowedTypes}
                          multiple={multiple}
                          maxSources={maxSources}
                          suggestedMode={suggestedMode}
                        />
                        {dataSourceSections.length > 1 && (
                          <div className="px-5 py-3 border-t">
                            <ConfigRenderer sections={dataSourceSections.slice(1)} />
                          </div>
                        )}
                      </>
                    ) : (
                      // New: custom dataSourceSections (e.g., DualModeSourceField)
                      <div className="p-4">
                        <ConfigRenderer sections={dataSourceSections} />
                      </div>
                    )}
                  </TabsContent>
                )}

                {hasAdvancedConfig && (
                  <TabsContent value="advanced" className="flex-1 min-h-0 overflow-y-auto p-4">
                    <ConfigRenderer sections={advancedSections} />
                  </TabsContent>
                )}

                {shouldShowDataTransform && (
                  <TabsContent value="transform" className="flex-1 min-h-0 overflow-y-auto p-4">
                    <DataTransformConfig
                      dataSource={previewDataSource}
                      onChange={handleDataTransformChange}
                      chartType={componentType.replace(/-chart$/, '') as any}
                    />
                  </TabsContent>
                )}
              </Tabs>
            ) : (
              !hasStyleConfig && !hasDisplayConfig && allSections.length > 0 && (
                <div className="flex-1 overflow-y-auto p-4">
                  <ConfigRenderer sections={allSections} />
                </div>
              )
            )}
          </div>
        </div>
      </FullScreenDialogContent>

      {/* Footer */}
      <FullScreenDialogFooter>
        <Button variant="outline" onClick={onClose}>
          {t('common.cancel')}
        </Button>
        <Button onClick={async () => { setSaving(true); try { await onSave() } finally { setSaving(false) } }} disabled={saving}>
          {saving ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : <Sparkles className="h-4 w-4 mr-2" />}
          {t('common.saveChanges')}
        </Button>
      </FullScreenDialogFooter>
    </FullScreenDialog>
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
  icon?: React.ComponentType<{ className?: string }>
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
    <div className="overflow-hidden rounded-lg border border-border bg-card">
      <button
        onClick={onToggle}
        className="w-full flex items-center justify-between px-4 py-4 bg-muted-30 hover:bg-muted active:bg-muted-50 transition-colors touch-action-manipulation"
      >
        <div className="flex items-center gap-3">
          {Icon && <Icon className="h-5 w-5 text-muted-foreground" />}
          <span className="font-semibold text-foreground">{title}</span>
        </div>
        <div className="flex items-center gap-2">
          {status === 'configured' && (
            <CheckCircle2 className="h-5 w-5 text-success shrink-0" />
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
        <div className="p-4 bg-background animate-in slide-in-from-top-2 duration-200 border-t border-border">
          {children}
        </div>
      )}
    </div>
  )
}
