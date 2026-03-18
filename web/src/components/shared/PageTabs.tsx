import { ReactNode } from 'react'
import { useIsMobile } from '@/hooks/useMobile'
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

/**
 * PageTabsBar - Fixed tab bar component for use with PageLayout.headerContent
 * This component is rendered outside the scroll container for fixed positioning.
 * Only renders on desktop; mobile uses bottom navigation.
 */
export interface PageTabsBarProps {
  tabs: TabConfig[]
  activeTab: string
  onTabChange: (value: string) => void
  actions?: TabAction[]
  actionsExtra?: ReactNode
  tabsClassName?: string
  maxWidth?: 'md' | 'lg' | 'xl' | '2xl' | 'full'
}

export function PageTabsBar({
  tabs,
  activeTab,
  onTabChange,
  actions = [],
  actionsExtra,
  tabsClassName,
  maxWidth = 'full',
}: PageTabsBarProps) {
  const isMobile = useIsMobile()

  const maxWidthClass = {
    md: 'max-w-4xl',
    lg: 'max-w-6xl',
    xl: 'max-w-7xl',
    '2xl': 'max-w-7xl',
    full: 'max-w-full',
  }

  // On mobile, only show actions bar (tabs are in bottom nav)
  if (isMobile) {
    return (
      <div className="px-4 sm:px-6 md:px-8 py-2">
        <div className="mb-3 flex shrink-0 flex-wrap justify-start gap-2">
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
      </div>
    )
  }

  // Desktop: Show full tabs bar
  return (
    <div className="px-4 sm:px-6 md:px-8 py-2">
      <div className={cn('mx-auto', maxWidthClass[maxWidth])}>
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div
            className={cn(
              'inline-flex w-auto flex-wrap overflow-visible rounded-md bg-muted p-0.5',
              tabsClassName
            )}
          >
            {tabs.map((tab) => {
              const isActive = activeTab === tab.value
              return (
                <button
                  key={tab.value}
                  disabled={tab.disabled}
                  onClick={() => onTabChange(tab.value)}
                  className={cn(
                    'inline-flex items-center justify-center gap-2 rounded-sm px-4 py-1.5 h-9 text-sm font-medium whitespace-nowrap transition-all',
                    isActive
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:text-foreground'
                  )}
                >
                  {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                  <span>{tab.label}</span>
                </button>
              )
            })}
          </div>

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
      </div>
    </div>
  )
}

/**
 * PageTabsBottomNav - Mobile bottom navigation bar
 * Rendered separately to be fixed at bottom of screen
 */
export interface PageTabsBottomNavProps {
  tabs: TabConfig[]
  activeTab: string
  onTabChange: (value: string) => void
}

export function PageTabsBottomNav({ tabs, activeTab, onTabChange }: PageTabsBottomNavProps) {
  const isMobile = useIsMobile()

  if (!isMobile) return null

  return (
    <div className="fixed bottom-0 left-0 right-0 z-50 bg-background/95 backdrop-blur-sm border-t border-border">
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
  )
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
  const isMobile = useIsMobile()

  // Mobile bottom navigation variant
  if (mobileBottomNav) {
    return (
      <div className={className}>
        {/* Desktop: Top tabs bar */}
        {!isMobile && (
          <div className="mb-4">
            <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div
                className={cn(
                  'inline-flex w-auto flex-wrap overflow-visible rounded-md bg-muted p-0.5',
                  tabsClassName
                )}
              >
                {tabs.map((tab) => {
                  const isActive = activeTab === tab.value
                  return (
                    <button
                      key={tab.value}
                      disabled={tab.disabled}
                      onClick={() => onTabChange(tab.value)}
                      className={cn(
                        'inline-flex items-center justify-center gap-2 rounded-sm px-4 py-1.5 h-9 text-sm font-medium whitespace-nowrap transition-all',
                        isActive
                          ? 'bg-background text-foreground shadow-sm'
                          : 'text-muted-foreground hover:text-foreground'
                      )}
                    >
                      {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                      <span>{tab.label}</span>
                    </button>
                  )
                })}
              </div>

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
          </div>
        )}

        {/* Mobile: Top actions bar */}
        {isMobile && (
          <div className="mb-3 flex shrink-0 flex-wrap justify-start gap-2">
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
        )}

        {/* Content area - add bottom padding on mobile for bottom nav */}
        <div className="md:mb-0 mb-16">
          {children}
        </div>

        {/* Mobile: Bottom navigation bar */}
        <PageTabsBottomNav
          tabs={tabs}
          activeTab={activeTab}
          onTabChange={onTabChange}
        />
      </div>
    )
  }

  // Original inline tabs variant (non-bottom-nav)
  return (
    <div className={className}>
      {/* Tabs + Actions Bar - mobile: segmented control, desktop: inline tabs */}
      <div className="mb-4">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div
            className={cn(
              /* mobile: full-width segmented control, horizontal scroll */
              'flex w-full flex-nowrap overflow-x-auto rounded-xl border border-border bg-muted/30 p-1',
              '[-webkit-overflow-scrolling:touch]',
              /* desktop: inline tabs with full text */
              'md:inline-flex md:w-auto md:flex-wrap md:overflow-visible md:rounded-md md:border-0 md:bg-muted md:p-0.5',
              tabsClassName
            )}
          >
            {tabs.map((tab) => {
              const isActive = activeTab === tab.value
              return (
                <button
                  key={tab.value}
                  disabled={tab.disabled}
                  onClick={() => onTabChange(tab.value)}
                  className={cn(
                    /* mobile: large touch targets, segmented active style */
                    'flex min-h-11 min-w-[5rem] shrink-0 items-center justify-center gap-2 rounded-lg px-4 py-2.5 text-sm font-medium transition-all',
                    isActive
                      ? 'bg-foreground text-background'
                      : 'text-muted-foreground',
                    /* desktop: full text display, centered, fixed height */
                    'md:inline-flex md:h-9 md:min-w-max md:shrink-0 md:rounded-sm md:px-4 md:py-1.5 md:whitespace-nowrap',
                    !isActive && 'md:hover:text-foreground',
                    isActive && 'md:bg-background md:text-foreground md:shadow-sm'
                  )}
                >
                  {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                  <span>{tab.label}</span>
                </button>
              )
            })}
          </div>

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
      </div>

      {children}
    </div>
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
    <div className={cn('mt-3', className)}>
      {children}
    </div>
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
    <div className={className}>
      {/* mobile: segmented control grid, desktop: compact grid */}
      <div className="mb-4">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div
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
            {tabs.map((tab) => {
              const isActive = activeTab === tab.value
              return (
                <button
                  key={tab.value}
                  disabled={tab.disabled}
                  onClick={() => onTabChange(tab.value)}
                  className={cn(
                    /* mobile: large touch targets, segmented active style */
                    'flex min-h-11 items-center justify-center gap-1.5 rounded-lg px-4 py-2.5 text-sm font-medium transition-all',
                    isActive
                      ? 'bg-foreground text-background'
                      : 'text-muted-foreground',
                    /* desktop: compact, Radix-style active */
                    'md:min-h-0 md:rounded-sm md:px-3 md:py-1.5',
                    isActive && 'md:bg-background md:text-foreground md:shadow-sm'
                  )}
                >
                  {tab.icon && <span className="shrink-0">{tab.icon}</span>}
                  <span className="truncate">{tab.label}</span>
                </button>
              )
            })}
          </div>

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
      </div>

      {children}
    </div>
  )
}
