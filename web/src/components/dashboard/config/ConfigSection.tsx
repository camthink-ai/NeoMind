/**
 * ConfigSection Component
 *
 * Reusable section component for configuration panels.
 * Provides consistent styling and layout for grouped configuration options.
 */

import { cn } from '@/lib/utils'
import React from 'react'

export interface ConfigSectionProps {
  title?: string
  children: React.ReactNode
  className?: string
  bordered?: boolean
  collapsible?: boolean
  defaultCollapsed?: boolean
}

export function ConfigSection({
  title,
  children,
  className,
  bordered = false,
  collapsible = false,
  defaultCollapsed = false,
}: ConfigSectionProps) {
  const [collapsed, setCollapsed] = React.useState(defaultCollapsed)

  const headerContent = title && (
    <div
      className={cn(
        'flex items-center justify-between cursor-pointer',
        collapsible && 'hover:bg-muted/50 -mx-2 px-2 py-1 rounded transition-colors'
      )}
      onClick={() => collapsible && setCollapsed(!collapsed)}
    >
      <h4 className="text-sm font-medium text-foreground">{title}</h4>
      {collapsible && (
        <span className="text-xs text-muted-foreground">
          {collapsed ? '+' : '-'}
        </span>
      )}
    </div>
  )

  return (
    <div className={cn('space-y-3', className)}>
      {headerContent}
      {!collapsed && (
        <div className={cn('space-y-3', bordered && 'pt-3 border-t')}>
          {children}
        </div>
      )}
    </div>
  )
}
