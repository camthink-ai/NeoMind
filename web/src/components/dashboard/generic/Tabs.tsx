/**
 * Tabs Component
 *
 * Tabbed content interface.
 * Supports horizontal tabs with different variants.
 */

import { useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export interface TabItem {
  id: string
  label: string
  content?: string
  icon?: string
}

export interface TabsProps {
  dataSource?: DataSource
  tabs?: TabItem[]
  variant?: 'default' | 'line' | 'pills' | 'underline'
  position?: 'top' | 'bottom' | 'left' | 'right'
  defaultTab?: string
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const sizeClasses = {
  sm: { padding: 'px-3 py-1.5', text: 'text-sm' },
  md: { padding: 'px-4 py-2', text: 'text-base' },
  lg: { padding: 'px-6 py-3', text: 'text-lg' },
}

export function Tabs({
  dataSource,
  tabs: propTabs,
  variant = 'default',
  position = 'top',
  defaultTab,
  size = 'md',
  showCard = true,
  className,
}: TabsProps) {
  const { data } = useDataSource<TabItem[]>(dataSource, { fallback: propTabs })
  const tabs = data ?? propTabs ?? []
  const sizes = sizeClasses[size]

  const [activeTab, setActiveTab] = useState(defaultTab || tabs[0]?.id || '')

  const activeTabData = tabs.find(t => t.id === activeTab)

  const renderTabs = () => {
    const isVertical = position === 'left' || position === 'right'

    const tabClasses = {
      default: cn(
        'inline-flex items-center gap-2 rounded-t-lg border-b-2 transition-colors',
        'data-[state=active]:border-primary data-[state=active]:bg-muted',
        'data-[state=inactive]:border-transparent data-[state=inactive]:hover:bg-muted/50'
      ),
      line: cn(
        'inline-flex items-center gap-2 border-b-2 transition-colors',
        'data-[state=active]:border-primary data-[state=active]:text-foreground',
        'data-[state=inactive]:border-transparent data-[state=inactive]:text-muted-foreground'
      ),
      pills: cn(
        'inline-flex items-center gap-2 rounded-full transition-colors',
        'data-[state=active]:bg-primary data-[state=active]:text-primary-foreground',
        'data-[state=inactive]:bg-muted data-[state=inactive]:hover:bg-muted/70'
      ),
      underline: cn(
        'inline-flex items-center gap-2 border-b transition-colors',
        'data-[state=active]:border-primary data-[state=active]:text-foreground',
        'data-[state=inactive]:border-transparent data-[state=inactive]:text-muted-foreground'
      ),
    }

    return (
      <div
        role="tablist"
        className={cn(
          'flex gap-1',
          isVertical ? 'flex-col' : 'flex-row',
          position === 'bottom' && 'flex-row-reverse',
          position === 'right' && 'flex-col'
        )}
      >
        {tabs.map((tab) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              tabClasses[variant],
              sizes.padding,
              sizes.text,
              activeTab === tab.id ? 'data-[state=active]' : 'data-[state=inactive]'
            )}
          >
            {tab.icon && <span className="text-sm">{tab.icon}</span>}
            <span>{tab.label}</span>
          </button>
        ))}
      </div>
    )
  }

  const renderContent = () => {
    if (!activeTabData) return null

    return (
      <div
        role="tabpanel"
        className={cn('p-4', sizes.text)}
      >
        {activeTabData.content || (
          <div className="text-muted-foreground">
            Content for "{activeTabData.label}"
          </div>
        )}
      </div>
    )
  }

  const isVertical = position === 'left' || position === 'right'

  const content = (
    <div className={cn('flex', isVertical ? 'flex-row' : 'flex-col', className)}>
      {position !== 'bottom' && renderTabs()}
      {renderContent()}
      {position === 'bottom' && renderTabs()}
    </div>
  )

  if (showCard && tabs.length > 0) {
    return (
      <DashboardComponentWrapper
        title={tabs[0]?.label}
        showCard={true}
        padding="none"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

