/**
 * Component Library (Refactored)
 *
 * Displays available dashboard components organized by category.
 * Uses the component registry for metadata.
 */

import { useState } from 'react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Input } from '@/components/ui/input'
import { groupComponentsByCategory, getCategoryInfo, type ComponentMeta } from '@/components/dashboard/registry'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'

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
}

// ============================================================================
// Component Item
// ============================================================================

function ComponentItem({ meta, onAdd }: ComponentItemProps) {
  const Icon = meta.icon

  return (
    <button
      onClick={onAdd}
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
        <h4 className="text-sm font-medium text-foreground truncate">{meta.name}</h4>
        <p className="text-xs text-muted-foreground truncate">{meta.description}</p>
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
}

function CategorySection({ category, components, onAddComponent }: CategorySectionProps) {
  const categoryInfo = getCategoryInfo(category as any)
  const CategoryIcon = categoryInfo.icon

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 px-1">
        <CategoryIcon className="w-4 h-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-muted-foreground">{categoryInfo.name}</h3>
        <span className="text-xs text-muted-foreground/60">({components.length})</span>
      </div>

      <div className="space-y-1">
        {components.map((meta) => (
          <ComponentItem
            key={meta.type}
            meta={meta}
            onAdd={() => onAddComponent(meta.type)}
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
  const [searchQuery, setSearchQuery] = useState('')

  // Get grouped components
  const groupedComponents = groupComponentsByCategory({
    searchQuery: searchQuery || undefined,
  })

  return (
    <div className={cn('flex flex-col h-full', className)}>
      {/* Header */}
      <div className="px-4 py-3 border-b">
        <h2 className="text-sm font-semibold text-foreground mb-3">Components</h2>

        {/* Search */}
        <div className="relative">
          <Input
            placeholder="Search components..."
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
            />
          ))}

          {/* No results */}
          {groupedComponents.length === 0 && (
            <div className="text-center py-8">
              <p className="text-sm text-muted-foreground">No components found</p>
              <p className="text-xs text-muted-foreground/60 mt-1">
                Try a different search term
              </p>
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}
