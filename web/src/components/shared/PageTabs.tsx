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
  mobileIcon?: ReactNode  // Icon specifically for mobile bottom nav
  disabled?: boolean
}

export interface PageTabsProps {
  tabs: TabConfig[]
  activeTab: string
  onTabChange: (value: string) => void
  actions?: TabAction[]
  actionsExtra?: ReactNode
  className?: string
  tabsClassName?: string
  children: ReactNode
  contentClassName?: string
  /**
   * Whether to use bottom navigation on mobile
   * @default true
   */
  mobileBottomNav?: boolean
}

// Unified page tabs component with action buttons
// Desktop: Top tabs with full text labels
// Mobile: Bottom navigation bar (app-style)
export function PageTabs({
  tabs,
  activeTab,
  onTabChange,
  actions = [],
  actionsExtra,
  className,
  tabsClassName,
  children,
  mobileBottomNav = true,
}: PageTabsProps) {
  // Mobile bottom navigation variant
  if (mobileBottomNav) {
    return (
      <Tabs value={activeTab} onValueChange={onTabChange} className={className}>
        {/* Desktop: Top tabs bar */}
        <div className="hidden md:flex mb-4 shrink-0 flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <TabsList
            className={cn(
              'inline-flex w-auto flex-wrap overflow-visible rounded-md bg-muted p-0.5',
              tabsClassName
            )}
          >
            {tabs.map((tab) => (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                disabled={tab.disabled}
                className={cn(
                  'inline-flex items-center justify-center gap-2 rounded-sm px-4 py-1.5 h-9 text-sm font-medium whitespace-nowrap transition-all',
                  'data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm',
                  'data-[state=inactive]:text-muted-foreground hover:text-foreground'
                )}
              >
                {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                <span>{tab.label}</span>
              </TabsTrigger>
            ))}
          </TabsList>

          {actions.length > 0 && (
            <div className="flex shrink-0 flex-wrap gap-2">
              {actions.map((action) => (
                <Button
                  key={action.label}
                  variant={action.variant || 'default'}
                  size="sm"
                  onClick={action.onClick}
                  disabled={action.disabled || action.loading}
                  className="h-9"
                >
                  {action.loading ? (
                    <span className="mr-2 h-4 w-4 animate-spin">⟳</span>
                  ) : (
                    action.icon && <span className="mr-2 shrink-0 h-4 w-4">{action.icon}</span>
                  )}
                  <span className="whitespace-nowrap">{action.label}</span>
                </Button>
              ))}
              {actionsExtra}
            </div>
          )}
        </div>

        {/* Mobile: Top actions bar */}
        <div className="md:hidden mb-3 flex shrink-0 flex-wrap justify-start gap-2">
          {actions.map((action) => (
            <Button
              key={action.label}
              variant={action.variant || 'default'}
              size="sm"
              onClick={action.onClick}
              disabled={action.disabled || action.loading}
              className="h-9 text-xs px-2.5 sm:px-3"
            >
              {action.loading ? (
                <span className="mr-1.5 h-4 w-4 animate-spin">⟳</span>
              ) : action.icon ? (
                <span className="mr-1.5 shrink-0 h-4 w-4">{action.icon}</span>
              ) : null}
              <span className="whitespace-nowrap">{action.label}</span>
            </Button>
          ))}
          {actionsExtra}
        </div>

        {/* Content area - add bottom padding on mobile for bottom nav */}
        <div className="md:mb-0 mb-16">
          {children}
        </div>

        {/* Mobile: Bottom navigation bar */}
        <div className="md:hidden fixed bottom-0 left-0 right-0 z-50 bg-background/95 backdrop-blur-sm border-t border-border">
          <div className="flex items-center justify-around px-2 py-1">
            {tabs.map((tab) => {
              const isActive = activeTab === tab.value
              const Icon = tab.mobileIcon || tab.icon

              return (
                <button
                  key={tab.value}
                  onClick={() => onTabChange(tab.value)}
                  disabled={tab.disabled}
                  className={cn(
                    'flex flex-col items-center justify-center gap-0.5 py-2 px-3 rounded-lg transition-all min-w-0 flex-1',
                    'active:scale-95',
                    isActive
                      ? 'text-foreground'
                      : 'text-muted-foreground hover:text-foreground'
                  )}
                >
                  {Icon && (
                    <span className={cn(
                      'shrink-0 transition-all',
                      isActive ? 'h-5 w-5' : 'h-4.5 w-4.5 opacity-70'
                    )}>
                      {Icon}
                    </span>
                  )}
                  <span className={cn(
                    'text-[10px] font-medium leading-tight truncate w-full text-center',
                    isActive ? 'text-[11px]' : 'text-[10px] opacity-80'
                  )}>
                    {tab.label}
                  </span>
                </button>
              )
            })}
          </div>
        </div>
      </Tabs>
    )
  }

  // Original inline tabs variant (non-bottom-nav)
  return (
    <Tabs value={activeTab} onValueChange={onTabChange} className={className}>
      {/* Tabs + Actions Bar - mobile: segmented control, desktop: inline tabs */}
      <div className="mb-2 flex shrink-0 flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <TabsList
          className={cn(
            /* mobile: full-width segmented control, horizontal scroll */
            'flex w-full flex-nowrap overflow-x-auto rounded-xl border border-border bg-muted/30 p-1',
            '[-webkit-overflow-scrolling:touch]',
            /* desktop: inline tabs with full text */
            'md:inline-flex md:w-auto md:flex-wrap md:overflow-visible md:rounded-md md:border-0 md:bg-muted md:p-0.5',
            tabsClassName
          )}
        >
          {tabs.map((tab) => (
            <TabsTrigger
              key={tab.value}
              value={tab.value}
              disabled={tab.disabled}
              className={cn(
                /* mobile: large touch targets, segmented active style */
                'flex min-h-11 min-w-[5rem] shrink-0 items-center justify-center gap-2 rounded-lg px-4 py-2.5 text-sm font-medium transition-all',
                'data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=inactive]:text-muted-foreground',
                /* desktop: full text display, centered, fixed height */
                'md:inline-flex md:h-9 md:min-w-max md:shrink-0 md:rounded-sm md:px-4 md:py-1.5 md:whitespace-nowrap',
                'md:data-[state=active]:bg-background md:data-[state=active]:text-foreground md:data-[state=active]:shadow-sm',
                'md:data-[state=inactive]:text-muted-foreground md:hover:text-foreground'
              )}
            >
              {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
              <span>{tab.label}</span>
            </TabsTrigger>
          ))}
        </TabsList>

        {actions.length > 0 && (
          <div className="flex shrink-0 flex-wrap gap-2">
            {actions.map((action) => (
              <Button
                key={action.label}
                variant={action.variant || 'default'}
                size="sm"
                onClick={action.onClick}
                disabled={action.disabled || action.loading}
                className="min-h-11 shrink-0 px-4 md:min-h-9"
              >
                {action.loading ? (
                  <span className="mr-2 h-4 w-4 animate-spin">⟳</span>
                ) : (
                  action.icon && <span className="mr-2 shrink-0 h-4 w-4">{action.icon}</span>
                )}
                <span className="whitespace-nowrap">{action.label}</span>
              </Button>
            ))}
            {actionsExtra}
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
        'min-h-0 overflow-auto mt-6',
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
  actionsExtra,
  className,
  gridCols = 3,
  children,
}: PageTabsGridProps) {
  // Map gridCols to Tailwind classes - md: for desktop layout
  const gridColsClass: Record<2 | 3 | 4 | 5 | 6, string> = {
    2: 'md:grid-cols-2',
    3: 'md:grid-cols-3',
    4: 'md:grid-cols-4',
    5: 'md:grid-cols-5',
    6: 'md:grid-cols-6',
  }

  // Adjust max-width based on number of columns (desktop only)
  const maxWidthClass = gridCols >= 5 ? 'md:max-w-2xl' : gridCols === 4 ? 'md:max-w-xl' : 'md:max-w-md'

  return (
    <Tabs value={activeTab} onValueChange={onTabChange} className={className}>
      {/* mobile: segmented control grid, desktop: compact grid */}
      <div className="mb-2 flex shrink-0 flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <TabsList
          className={cn(
            /* mobile: 2-col grid, segmented style */
            'grid w-full shrink-0 grid-cols-2 overflow-x-auto rounded-xl border border-border bg-muted/30 p-1',
            '[-webkit-overflow-scrolling:touch]',
            /* desktop: compact inline grid */
            'md:w-auto md:overflow-visible md:rounded-md md:border-0 md:bg-muted',
            gridColsClass[gridCols],
            maxWidthClass
          )}
        >
          {tabs.map((tab) => (
            <TabsTrigger
              key={tab.value}
              value={tab.value}
              disabled={tab.disabled}
              className={cn(
                /* mobile: large touch targets, segmented active style */
                'flex min-h-11 items-center justify-center gap-1.5 rounded-lg px-4 py-2.5 text-sm font-medium transition-all',
                'data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=inactive]:text-muted-foreground',
                /* desktop: compact, Radix-style active */
                'md:min-h-0 md:rounded-sm md:px-3 md:py-1.5 md:data-[state=active]:bg-background md:data-[state=active]:text-foreground md:data-[state=active]:shadow-sm'
              )}
            >
              {tab.icon && <span className="shrink-0">{tab.icon}</span>}
              <span className="truncate">{tab.label}</span>
            </TabsTrigger>
          ))}
        </TabsList>

        {actions.length > 0 && (
          <div className="flex shrink-0 flex-wrap gap-2">
            {actions.map((action) => (
              <Button
                key={action.label}
                variant={action.variant || 'default'}
                size="sm"
                onClick={action.onClick}
                disabled={action.disabled || action.loading}
                className="min-h-11 shrink-0 px-4 md:min-h-9"
              >
                {action.loading ? (
                  <span className="mr-2 h-4 w-4 animate-spin">⟳</span>
                ) : (
                  action.icon && <span className="mr-2 shrink-0">{action.icon}</span>
                )}
                <span className="whitespace-nowrap">{action.label}</span>
              </Button>
            ))}
            {actionsExtra}
          </div>
        )}
      </div>

      {children}
    </Tabs>
  )
}
