import { ReactNode, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useIsMobile } from '@/hooks/useMobile'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  MoreHorizontal,
  Plus,
  Upload,
  Download,
  RefreshCw,
  Pencil,
  Trash2,
  Settings,
  Filter,
  Loader2,
  Search,
  Cloud,
  Share2,
  Play,
  Pause,
  Save,
  Check,
  X,
  Copy,
  ExternalLink,
  type LucideIcon,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { textMini } from "@/design-system/tokens/typography"
import { useMobileHeaderActionsRegistrar } from '@/components/layout/MobileHeaderActionsContext'

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
  /** Primary actions, rendered as visible buttons (desktop) / icon+overflow (mobile). */
  actions?: TabAction[]
  /**
   * Secondary actions, collapsed into a `…` overflow menu on BOTH desktop
   * and mobile. Use for low-frequency page-level operations (Import, Export
   * all, etc.) so they don't compete with the primary "+ Add" action.
   */
  secondaryActions?: TabAction[]
  actionsExtra?: ReactNode
  tabsClassName?: string
  maxWidth?: 'md' | 'lg' | 'xl' | '2xl' | 'full'
}

export function PageTabsBar({
  tabs,
  activeTab,
  onTabChange,
  actions = [],
  secondaryActions = [],
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

  // On mobile, lift all actions into the MobilePageHeader via context.
  // secondaryActions are appended so MobileTabActionsCompact naturally
  // tucks them into the MoreHorizontal overflow alongside any extra
  // primary actions.
  if (isMobile) {
    return (
      <MobilePageTabsActionsLift
        actions={[...actions, ...secondaryActions]}
        actionsExtra={actionsExtra}
      />
    )
  }

  // Desktop: page toolbar — the SAME shape as the dashboard toolbar
  // (h-11 chrome row with bottom border): tabs capsule left, page actions
  // center-right, global entry points at the far right. The tab strip is a
  // SINGLE scrollable row (no wrapping): overflowing tabs scroll
  // horizontally at natural size instead of squeezing labels together.
  return (
    // py-2 + border-b: the row keeps its bottom divider (tab capsule's edge
    // line) AND breathing room above the content area. px matches the title
    // row (md:px-8) so tabs left-align with the title above them.
    <div className="flex shrink-0 items-center gap-3 bg-background px-4 py-2 sm:px-6 md:px-8">
      <div
        className={cn(
          'flex min-w-0 max-w-full items-center overflow-x-auto scrollbar-none rounded-lg border border-border bg-card p-1 gap-1',
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
                    'inline-flex shrink-0 items-center justify-start gap-2 rounded-sm px-3 py-1.5 h-9 text-sm font-medium whitespace-nowrap transition-all',
                    isActive
                      ? 'bg-foreground text-background shadow-sm'
                      : 'text-muted-foreground hover:text-foreground'
                  )}
                >
                  {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                  <span>{tab.label}</span>
                </button>
              )
            })}
          </div>

          {(actions.length > 0 || secondaryActions.length > 0 || actionsExtra) && (
            <div className="ml-auto flex shrink-0 flex-wrap items-center gap-2">
              {actions.map((action) => (
                <Button
                  key={action.label}
                  variant={action.variant || 'default'}
                  size="sm"
                  onClick={action.onClick}
                  disabled={action.disabled || action.loading}
                >
                  {action.loading ? (
                    <span className="mr-2 shrink-0 h-4 w-4 flex items-center justify-center">{action.icon || '⟳'}</span>
                  ) : (
                    action.icon && <span className="mr-2 shrink-0 h-4 w-4">{action.icon}</span>
                  )}
                  <span className="whitespace-nowrap">{action.label}</span>
                </Button>
              ))}
              {secondaryActions.length > 0 && (
                <TabActionsOverflow actions={secondaryActions} trigger="more" />
              )}
              {actionsExtra}
            </div>
          )}

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
    <div className="fixed bottom-[var(--keyboard-offset,0px)] left-0 right-0 z-50 bg-bg-95 backdrop-blur-sm border-t border-border safe-bottom">
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
                  'shrink-0 h-5 w-5 transition-opacity',
                  isActive ? 'opacity-100' : 'opacity-60'
                )}>
                  {Icon}
                </span>
              )}
              <span className={cn(textMini, "font-medium leading-tight truncate w-full text-center")}>
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
                  'inline-flex w-auto flex-wrap overflow-visible rounded-lg border border-border bg-card p-1',
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
                        'inline-flex items-center justify-start gap-2 rounded-sm px-3 py-1.5 h-9 text-sm font-medium whitespace-nowrap transition-all',
                        isActive
                          ? 'bg-foreground text-background shadow-sm'
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
                      className=""
                    >
                      {action.loading ? (
                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
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
        {isMobile && actions.length > 0 && (
          <div className="flex shrink-0 flex-wrap items-center gap-1.5">
            {actions.map((action) => (
              <Button
                key={action.label}
                variant={action.variant || 'default'}
                size="sm"
                onClick={action.onClick}
                disabled={action.disabled || action.loading}
              >
                {action.loading ? (
                  <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                ) : action.icon ? (
                  <span className="mr-1 shrink-0 h-3.5 w-3.5">{action.icon}</span>
                ) : null}
                <span className="whitespace-nowrap">{action.label}</span>
              </Button>
            ))}
            {actionsExtra}
          </div>
        )}

        {/* Content area - add bottom padding on mobile for bottom nav */}
        <div className="md:pb-0 pb-16">
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
              'flex w-full flex-nowrap overflow-x-auto rounded-lg border border-border bg-card p-1',
              '[-webkit-overflow-scrolling:touch]',
              /* desktop: inline tabs with full text */
              'md:inline-flex md:w-auto md:flex-wrap md:overflow-visible md:rounded-md md:border-0 md:bg-card md:p-0.5',
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
                    isActive && 'md:bg-foreground md:text-background md:shadow-sm'
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
                    <span className="mr-2 shrink-0 h-4 w-4 flex items-center justify-center">{action.icon || '⟳'}</span>
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
    <div className={cn('md:mt-3', className)}>
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
              'grid w-full shrink-0 grid-cols-2 overflow-x-auto rounded-lg border border-border bg-card p-1',
              '[-webkit-overflow-scrolling:touch]',
              /* desktop: compact inline grid */
              'md:w-auto md:overflow-visible md:rounded-md md:border-0 md:bg-card',
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
                    isActive && 'md:bg-foreground md:text-background md:shadow-sm'
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
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
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

/**
 * MobilePageTabsActionsLift - renders nothing on its own. Instead, it
 * registers the tab actions into two host slots via MobileHeaderActionsContext:
 *
 *   - primary actions (Add/Refresh/…) → MobilePageHeader's actions slot
 *     (compact icon button + overflow dropdown).
 *   - actionsExtra (search input / filter popover / wide controls) → the
 *     sticky content toolbar rendered by PageLayout above the scroll area.
 *     These controls don't fit in an icon-only header slot, so they stay
 *     with the content.
 *
 * Used internally by PageTabsBar's mobile branch.
 */
function MobilePageTabsActionsLift({
  actions,
  actionsExtra,
}: {
  actions: TabAction[]
  actionsExtra?: ReactNode
}) {
  const ctx = useMobileHeaderActionsRegistrar()

  // Header slot — primary actions only.
  useEffect(() => {
    if (!ctx) return
    if (actions.length === 0) return
    const node = <MobileTabActionsCompact actions={actions} />
    return ctx.register('header', 'PageTabsBar', node)
  }, [ctx, actions])

  // Content slot — actionsExtra (search/filter/complex controls).
  useEffect(() => {
    if (!ctx) return
    if (!actionsExtra) return
    return ctx.register('content', 'PageTabsBarExtra', actionsExtra)
  }, [ctx, actionsExtra])

  return null
}

/**
 * Derive a likely icon from an action's label when the caller didn't provide
 * one. Matches common action verbs/nouns in English and Chinese so most page
 * actions get a recognizable icon on the mobile header without each page
 * having to pass an explicit `icon` prop.
 *
 * Returns null when no keyword matches — caller then falls back to a compact
 * text button.
 */
function deriveActionIcon(label: string): LucideIcon | null {
  const lower = label.toLowerCase()
  // Order matters: more specific keywords first.
  if (/(create|new|\badd\b|新增|新建|添加|创建)/.test(lower)) return Plus
  if (/(import|upload|导入|上传)/.test(lower)) return Upload
  if (/(export|download|导出|下载)/.test(lower)) return Download
  if (/(refresh|reload|刷新)/.test(lower)) return RefreshCw
  if (/(rename|edit|重命名|编辑|修改)/.test(lower)) return Pencil
  if (/(delete|remove|删除|移除)/.test(lower)) return Trash2
  if (/(config|setting|配置|设置)/.test(lower)) return Settings
  if (/(filter|筛选|过滤)/.test(lower)) return Filter
  if (/(search|查找|搜索)/.test(lower)) return Search
  if (/(cloud|云端|云)/.test(lower)) return Cloud
  if (/(share|分享|共享)/.test(lower)) return Share2
  if (/(save|保存)/.test(lower)) return Save
  if (/(copy|复制|duplicate)/.test(lower)) return Copy
  if (/(run|start|execute|运行|启动|执行)/.test(lower)) return Play
  if (/(pause|stop|暂停|停止)/.test(lower)) return Pause
  if (/(open|external|打开|外部)/.test(lower)) return ExternalLink
  if (/(done|confirm|complete|完成|确定)/.test(lower)) return Check
  if (/(cancel|close|取消|关闭)/.test(lower)) return X
  return null
}

/**
 * Resolve the effective icon for a tab action: explicit prop wins, otherwise
 * derive from the label.
 */
function resolveActionIcon(action: TabAction): ReactNode | null {
  if (action.icon) return action.icon
  const Derived = deriveActionIcon(action.label)
  if (!Derived) return null
  return <Derived className="h-5 w-5" />
}

/**
 * Shared overflow dropdown for TabAction lists. Renders a single trigger
 * button (icon-only "MoreHorizontal" on mobile, outline "More" button with
 * label on desktop) that opens a DropdownMenu with all actions.
 *
 * Used by:
 *   - MobileTabActionsCompact for actions[1..] (mobile header overflow)
 *   - PageTabsBar desktop rendering for `secondaryActions` (Import/Export/etc)
 *
 * Variant coloring + icon derivation are applied so the same TabAction looks
 * consistent across the two surfaces.
 */
function TabActionsOverflow({
  actions,
  trigger,
}: {
  actions: TabAction[]
  /** "icon" = square icon button (mobile header); "more" = outline button with label (desktop) */
  trigger: 'icon' | 'more'
}) {
  const { t } = useTranslation('common')
  if (actions.length === 0) return null
  const moreText = t('actions.more')
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {trigger === 'icon' ? (
          <Button
            variant="ghost"
            size="icon"
            className="h-9 w-9 shrink-0"
            aria-label={moreText}
          >
            <MoreHorizontal className="h-5 w-5" />
          </Button>
        ) : (
          <Button variant="outline" size="sm" className="gap-1.5">
            <MoreHorizontal className="h-4 w-4" />
            <span className="whitespace-nowrap">{moreText}</span>
          </Button>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[12rem]">
        {actions.map((action) => {
          const icon = resolveActionIcon(action)
          const isDestructive = action.variant === 'destructive'
          return (
            <DropdownMenuItem
              key={action.label}
              onClick={action.onClick}
              disabled={action.disabled || action.loading}
              className={cn(
                'gap-2',
                isDestructive && 'text-error focus:text-error'
              )}
            >
              {icon && (
                <span
                  className={cn(
                    'flex h-4 w-4 shrink-0 items-center justify-center',
                    isDestructive && 'text-error'
                  )}
                >
                  {icon}
                </span>
              )}
              <span className="whitespace-nowrap">{action.label}</span>
            </DropdownMenuItem>
          )
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * MobileTabActionsCompact - compact rendering for the MobilePageHeader's
 * actions slot. The first action is an icon button (h-9 w-9, matching the
 * hamburger tap target); the rest collapse into a MoreHorizontal dropdown.
 *
 * Note: `actionsExtra` (search/filter/wide controls) is NOT handled here —
 * those are routed to the sticky content toolbar via the "content" slot of
 * MobileHeaderActionsContext. The header slot is for icon-button actions only.
 *
 * Icon rules (in priority order):
 *   1. action.icon present        → use it (icon-only button)
 *   2. label keyword match        → derived icon (icon-only button)
 *   3. nothing matches            → small ghost text button with label
 */
function MobileTabActionsCompact({
  actions,
}: {
  actions: TabAction[]
}) {
  if (actions.length === 0) return null

  const primary = actions[0]
  const overflowActions = actions.length > 1 ? actions.slice(1) : []
  const primaryIcon = primary ? resolveActionIcon(primary) : null

  return (
    <>
      {primary && (
        <Button
          variant={primary.variant || 'ghost'}
          size={primaryIcon ? 'icon' : 'sm'}
          onClick={primary.onClick}
          disabled={primary.disabled || primary.loading}
          className={primaryIcon ? 'h-9 w-9 shrink-0' : 'h-9 shrink-0 px-2 text-xs'}
          aria-label={primary.label}
          title={primary.label}
        >
          {primaryIcon ? (
            <span className="flex h-5 w-5 items-center justify-center [&>svg]:h-5 [&>svg]:w-5">{primaryIcon}</span>
          ) : (
            <span className="whitespace-nowrap">{primary.label}</span>
          )}
        </Button>
      )}

      <TabActionsOverflow actions={overflowActions} trigger="icon" />
    </>
  )
}
