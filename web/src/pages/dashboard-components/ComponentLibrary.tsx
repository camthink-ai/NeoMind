/**
 * Component Library (Refactored)
 *
 * Displays available dashboard components organized by category.
 * Uses the component registry for metadata.
 */

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Input } from '@/components/ui/input'
import { groupComponentsByCategory, getCategoryInfo, type ComponentMeta } from '@/components/dashboard/registry'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'

// Translation key mapping for component types
const COMPONENT_TRANSLATION_KEYS: Record<string, { name: string; description: string }> = {
  'value-card': { name: 'componentLibrary.valueCard', description: 'componentLibrary.valueCardDesc' },
  'led-indicator': { name: 'componentLibrary.ledIndicator', description: 'componentLibrary.ledIndicatorDesc' },
  'sparkline': { name: 'componentLibrary.sparkline', description: 'componentLibrary.sparklineDesc' },
  'progress-bar': { name: 'componentLibrary.progressBar', description: 'componentLibrary.progressBarDesc' },
  'line-chart': { name: 'componentLibrary.lineChart', description: 'componentLibrary.lineChartDesc' },
  'area-chart': { name: 'componentLibrary.areaChart', description: 'componentLibrary.areaChartDesc' },
  'bar-chart': { name: 'componentLibrary.barChart', description: 'componentLibrary.barChartDesc' },
  'pie-chart': { name: 'componentLibrary.pieChart', description: 'componentLibrary.pieChartDesc' },
  'toggle-switch': { name: 'componentLibrary.toggleSwitch', description: 'componentLibrary.toggleSwitchDesc' },
  'image-display': { name: 'componentLibrary.imageDisplay', description: 'componentLibrary.imageDisplayDesc' },
  'image-history': { name: 'componentLibrary.imageHistory', description: 'componentLibrary.imageHistoryDesc' },
  'web-display': { name: 'componentLibrary.webDisplay', description: 'componentLibrary.webDisplayDesc' },
  'markdown-display': { name: 'componentLibrary.markdownDisplay', description: 'componentLibrary.markdownDisplayDesc' },
  'map-display': { name: 'componentLibrary.mapDisplay', description: 'componentLibrary.mapDisplayDesc' },
  'video-display': { name: 'componentLibrary.videoDisplay', description: 'componentLibrary.videoDisplayDesc' },
  'custom-layer': { name: 'componentLibrary.customLayer', description: 'componentLibrary.customLayerDesc' },
  'agent-status-card': { name: 'componentLibrary.agentStatus', description: 'componentLibrary.agentStatusDesc' },
  'agent-monitor-widget': { name: 'componentLibrary.agentMonitor', description: 'componentLibrary.agentMonitorDesc' },
  'decision-list': { name: 'componentLibrary.decisionList', description: 'componentLibrary.decisionListDesc' },
  'device-control': { name: 'componentLibrary.deviceControl', description: 'componentLibrary.deviceControlDesc' },
  'rule-status-grid': { name: 'componentLibrary.ruleStatusGrid', description: 'componentLibrary.ruleStatusGridDesc' },
  'transform-list': { name: 'componentLibrary.transformList', description: 'componentLibrary.transformListDesc' },
}

// Translation key mapping for categories
const CATEGORY_TRANSLATION_KEYS: Record<string, string> = {
  indicators: 'componentLibrary.indicators',
  charts: 'componentLibrary.charts',
  controls: 'componentLibrary.controls',
  display: 'componentLibrary.display',
  spatial: 'componentLibrary.spatial',
  business: 'componentLibrary.business',
  custom: 'componentLibrary.custom',
}

// ============================================================================
// Types
// ============================================================================

interface ComponentLibraryProps {
  onAddComponent: (type: string) => void
  onClose?: () => void
  className?: string
}

interface ComponentItemProps {
  meta: ComponentMeta
  onAdd: () => void
  t: (key: string) => string
}

// ============================================================================
// Component Item
// ============================================================================

function ComponentItem({ meta, onAdd, t }: ComponentItemProps) {
  const Icon = meta.icon

  // Get translation keys for this component
  const translationKeys = COMPONENT_TRANSLATION_KEYS[meta.type]
  const name = translationKeys ? t(translationKeys.name) : meta.name
  const description = translationKeys ? t(translationKeys.description) : meta.description

  // Unified handler for both mouse and touch
  const handleAdd = () => {
    onAdd()
  }

  // Mouse click handler - don't prevent default to allow normal click behavior
  const handleMouseClick = () => {
    handleAdd()
  }

  // Touch end handler for mobile - prevent default to avoid double-firing with click
  const handleTouchEnd = (e: React.TouchEvent) => {
    e.preventDefault()
    handleAdd()
  }

  return (
    <button
      onClick={handleMouseClick}
      onTouchEnd={handleTouchEnd}
      onTouchStart={() => {}}
      className={cn(
        'w-full flex items-center gap-3 p-3 rounded-lg',
        'bg-card/50 hover:bg-accent/50',
        'border border-transparent hover:border-border/50',
        'transition-all duration-200',
        'text-left'
      )}
    >
      <div className="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center flex-shrink-0">
        <Icon className="w-5 h-5 text-primary" />
      </div>

      <div className="flex-1 min-w-0">
        <h4 className="text-sm font-medium text-foreground truncate">{name}</h4>
        <p className="text-xs text-muted-foreground truncate">{description}</p>
      </div>

      <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        <span className="text-xs text-muted-foreground">
          {meta.sizeConstraints.defaultW}×{meta.sizeConstraints.defaultH}
        </span>
      </div>
    </button>
  )
}

// ============================================================================
// Category Section
// ============================================================================

interface CategorySectionProps {
  category: string
  components: ComponentMeta[]
  onAddComponent: (type: string) => void
  t: (key: string) => string
}

function CategorySection({ category, components, onAddComponent, t }: CategorySectionProps) {
  const categoryInfo = getCategoryInfo(category as any)
  const CategoryIcon = categoryInfo.icon

  // Get translated category name
  const translationKey = CATEGORY_TRANSLATION_KEYS[category]
  const categoryName = translationKey ? t(translationKey) : categoryInfo.name

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 px-1">
        <CategoryIcon className="w-4 h-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-muted-foreground">{categoryName}</h3>
      </div>

      <div className="space-y-1">
        {components.map((meta) => (
          <ComponentItem
            key={meta.type}
            meta={meta}
            onAdd={() => onAddComponent(meta.type)}
            t={t}
          />
        ))}
      </div>
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function ComponentLibrary({ onAddComponent, onClose, className }: ComponentLibraryProps) {
  const { t } = useTranslation('dashboardComponents')
  const [searchQuery, setSearchQuery] = useState('')

  // Get grouped components
  const groupedComponents = groupComponentsByCategory({
    searchQuery: searchQuery || undefined,
  })

  return (
    <div className={cn('flex flex-col h-full', className)}>
      {/* Header */}
      <div className="px-4 py-3 border-b">
        <h2 className="text-sm font-semibold text-foreground mb-3">{t('componentLibrary.components')}</h2>

        {/* Search */}
        <div className="relative">
          <Input
            placeholder={t('componentLibrary.searchPlaceholder')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>
      </div>

      {/* Component List */}
      <ScrollArea className="flex-1 px-4 py-4">
        <div className="space-y-6">
          {groupedComponents.map((section) => (
            <CategorySection
              key={section.category}
              category={section.category}
              components={section.components}
              onAddComponent={onAddComponent}
              t={t}
            />
          ))}

          {/* No results */}
          {groupedComponents.length === 0 && (
            <div className="text-center py-8">
              <p className="text-sm text-muted-foreground">{t('componentLibrary.noResults')}</p>
              <p className="text-xs text-muted-foreground/60 mt-1">
                {t('componentLibrary.noResultsHint')}
              </p>
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}
