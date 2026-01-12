import { ReactNode } from 'react'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface TabAction {
  label: string
  icon?: ReactNode
  variant?: 'default' | 'destructive' | 'outline' | 'ghost' | 'secondary' | 'link'
  onClick: () => void
  disabled?: boolean
  loading?: boolean
}

export interface TabConfig {
  value: string
  label: string
  icon?: ReactNode
  disabled?: boolean
}

export interface PageTabsProps {
  tabs: TabConfig[]
  activeTab: string
  onTabChange: (value: string) => void
  actions?: TabAction[]
  className?: string
  tabsClassName?: string
  children: ReactNode
  contentClassName?: string
}

// Unified page tabs component with action buttons
export function PageTabs({
  tabs,
  activeTab,
  onTabChange,
  actions = [],
  className,
  tabsClassName,
  children,
}: PageTabsProps) {
  return (
    <Tabs value={activeTab} onValueChange={onTabChange} className={className}>
      {/* Tabs + Actions Bar */}
      <div className="flex items-center justify-between mb-4 shrink-0">
        <TabsList className={tabsClassName}>
          {tabs.map((tab) => (
            <TabsTrigger key={tab.value} value={tab.value} disabled={tab.disabled}>
              {tab.icon && <span className="mr-2">{tab.icon}</span>}
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        {actions.length > 0 && (
          <div className="flex gap-2">
            {actions.map((action, index) => (
              <Button
                key={index}
                variant={action.variant || 'default'}
                size="sm"
                onClick={action.onClick}
                disabled={action.disabled || action.loading}
              >
                {action.loading ? (
                  <span className="mr-2 h-4 w-4 animate-spin">⟳</span>
                ) : (
                  action.icon && <span className="mr-2">{action.icon}</span>
                )}
                {action.label}
              </Button>
            ))}
          </div>
        )}
      </div>

      {children}
    </Tabs>
  )
}

export interface PageTabsContentProps {
  value: string
  activeTab: string
  children: ReactNode
  className?: string
}

/**
 * Tab content wrapper with consistent spacing and scroll support
 */
export function PageTabsContent({ value, activeTab, children, className }: PageTabsContentProps) {
  if (value !== activeTab) return null

  return (
    <TabsContent
      value={value}
      className={cn(
        'space-y-4 min-h-0 overflow-auto',
        className
      )}
    >
      {children}
    </TabsContent>
  )
}

// Tabs with grid layout (for icon-based tabs like automation page)
export interface PageTabsGridProps extends Omit<PageTabsProps, 'tabsClassName'> {
  gridCols?: 2 | 3 | 4 | 5 | 6
}

export function PageTabsGrid({
  tabs,
  activeTab,
  onTabChange,
  actions = [],
  className,
  gridCols = 3,
  children,
}: PageTabsGridProps) {
  return (
    <Tabs value={activeTab} onValueChange={onTabChange} className={className}>
      <div className="flex items-center justify-between mb-4 shrink-0">
        <TabsList className={`grid w-full max-w-md grid-cols-${gridCols}`}>
          {tabs.map((tab) => (
            <TabsTrigger key={tab.value} value={tab.value} disabled={tab.disabled}>
              {tab.icon && <span className="mr-2">{tab.icon}</span>}
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        {actions.length > 0 && (
          <div className="flex gap-2">
            {actions.map((action, index) => (
              <Button
                key={index}
                variant={action.variant || 'default'}
                size="sm"
                onClick={action.onClick}
                disabled={action.disabled || action.loading}
              >
                {action.loading ? (
                  <span className="mr-2 h-4 w-4 animate-spin">⟳</span>
                ) : (
                  action.icon && <span className="mr-2">{action.icon}</span>
                )}
                {action.label}
              </Button>
            ))}
          </div>
        )}
      </div>

      {children}
    </Tabs>
  )
}
