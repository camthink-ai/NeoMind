/**
 * DashboardTabBar Component
 *
 * Horizontal tab bar shown inside the dashboard toolbar header as an alternative
 * to DashboardListSidebar. Renders scrollable tabs with a distinct active style,
 * a "+" create button + sidebar-toggle button on the LEFT, and a "more" dropdown
 * menu on the RIGHT for current-dashboard operations (rename / delete).
 */

import { useState, useRef, useEffect, useCallback, type ReactNode } from 'react'
import {
  Plus,
  Pencil,
  Trash2,
  Copy,
  Check,
  X,
  PanelLeft,
  MoreVertical,
  ChevronDown,
  ChevronUp,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useIsMobile } from '@/hooks/useMobile'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { confirm } from '@/hooks/use-confirm'
import type { Dashboard } from '@/types/dashboard'

export interface DashboardTabBarProps {
  dashboards: Dashboard[]
  currentDashboardId: string | null
  onSwitch: (id: string) => void
  onCreate: (name: string) => void
  onRename: (id: string, name: string) => void
  onDuplicate: (id: string) => void
  onDelete: (id: string) => void
  /** Persist a new manual order (array of dashboard IDs, index 0 = leftmost). */
  onReorder?: (newOrder: string[]) => void
  onSwitchToSidebar: () => void
  className?: string
}

export function DashboardTabBar({
  dashboards,
  currentDashboardId,
  onSwitch,
  onCreate,
  onRename,
  onDuplicate,
  onDelete,
  onReorder,
  onSwitchToSidebar,
  className,
}: DashboardTabBarProps) {
  const { t } = useTranslation('dashboardComponents')
  const isMobile = useIsMobile()
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingName, setEditingName] = useState('')
  const [showCreateInput, setShowCreateInput] = useState(false)
  const [newDashboardName, setNewDashboardName] = useState('')
  const [moreMenuOpen, setMoreMenuOpen] = useState(false)
  const [switcherOpen, setSwitcherOpen] = useState(false)
  const editInputRef = useRef<HTMLInputElement>(null)
  const createInputRef = useRef<HTMLInputElement>(null)
  const mobileCreateInputRef = useRef<HTMLInputElement>(null)
  const tabsViewportRef = useRef<HTMLDivElement>(null)

  // Focus create input when shown
  useEffect(() => {
    if (showCreateInput && createInputRef.current) {
      createInputRef.current.focus()
    }
    if (showCreateInput && isMobile && mobileCreateInputRef.current) {
      mobileCreateInputRef.current.focus()
    }
  }, [showCreateInput, isMobile])

  // Focus + select edit input when editing starts
  useEffect(() => {
    if (editingId && editInputRef.current) {
      editInputRef.current.focus()
      editInputRef.current.select()
    }
  }, [editingId])

  // Scroll the active tab into view when switching
  useEffect(() => {
    if (!currentDashboardId || !tabsViewportRef.current) return
    const viewport = tabsViewportRef.current
    const active = viewport.querySelector<HTMLButtonElement>(`[data-dashboard-id="${currentDashboardId}"]`)
    if (active) {
      active.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' })
    }
  }, [currentDashboardId])

  const handleDeleteCurrent = useCallback(async () => {
    if (!currentDashboardId) return
    const dashboard = dashboards.find(d => d.id === currentDashboardId)
    const name = dashboard?.name || ''
    const confirmed = await confirm({
      title: t('tabBar.deleteTitle'),
      description: t('tabBar.deleteDescription', { name }),
      confirmText: t('tabBar.delete'),
      cancelText: t('common.cancel'),
      variant: 'destructive',
    })
    if (confirmed) {
      onDelete(currentDashboardId)
    }
  }, [currentDashboardId, dashboards, t, onDelete])

  const handleRenameCurrent = useCallback(() => {
    if (!currentDashboardId) return
    const dashboard = dashboards.find(d => d.id === currentDashboardId)
    if (!dashboard) return
    setEditingId(dashboard.id)
    setEditingName(dashboard.name)
  }, [currentDashboardId, dashboards])

  const handleDuplicateCurrent = useCallback(() => {
    if (!currentDashboardId) return
    setMoreMenuOpen(false)
    onDuplicate(currentDashboardId)
  }, [currentDashboardId, onDuplicate])

  const commitCreate = useCallback(() => {
    const name = newDashboardName.trim()
    if (name) {
      onCreate(name)
      setNewDashboardName('')
      setShowCreateInput(false)
    }
  }, [newDashboardName, onCreate])

  const cancelCreate = useCallback(() => {
    setShowCreateInput(false)
    setNewDashboardName('')
  }, [])

  const commitRename = useCallback((id: string) => {
    const name = editingName.trim()
    if (name) {
      onRename(id, name)
      setEditingId(null)
      setEditingName('')
    }
  }, [editingName, onRename])

  const cancelRename = useCallback(() => {
    setEditingId(null)
    setEditingName('')
  }, [])

  // --- Reordering helpers (icon buttons in tab menus) ---
  const canReorder = !!onReorder && dashboards.length > 1

  const moveByOne = useCallback(
    (id: string, direction: -1 | 1) => {
      if (!onReorder) return
      const ids = dashboards.map((d) => d.id)
      const idx = ids.indexOf(id)
      const target = idx + direction
      if (idx < 0 || target < 0 || target >= ids.length) return
      const next = [...ids]
      next.splice(idx, 1)
      next.splice(target, 0, id)
      onReorder(next)
    },
    [onReorder, dashboards],
  )

  // Mobile: dropdown switcher. Tap the trigger to pick a dashboard, with
  // rename / delete / create surfaced as menu items — the desktop tab bar's
  // hover-reveal "more" trigger and double-click rename don't work on touch.
  if (isMobile) {
    const current = dashboards.find((d) => d.id === currentDashboardId)

    // Rename input replaces the trigger while editing
    if (editingId && current && editingId === current.id) {
      return (
        <div className={cn('flex items-center flex-1 min-w-0 gap-1', className)}>
          <Input
            ref={editInputRef}
            value={editingName}
            onChange={(e) => setEditingName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename(current.id)
              if (e.key === 'Escape') cancelRename()
            }}
            placeholder={t('tabBar.namePlaceholder')}
            className="h-8 flex-1 min-w-0 rounded-md text-sm"
          />
          <button
            type="button"
            className="h-7 w-7 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
            onClick={() => commitRename(current.id)}
            aria-label={t('common.confirm')}
          >
            <Check className="h-4 w-4" />
          </button>
          <button
            type="button"
            className="h-7 w-7 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
            onClick={cancelRename}
            aria-label={t('common.cancel')}
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      )
    }

    // Inline create input
    if (showCreateInput) {
      return (
        <div className={cn('flex items-center flex-1 min-w-0 gap-1', className)}>
          <Input
            ref={mobileCreateInputRef}
            value={newDashboardName}
            onChange={(e) => setNewDashboardName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitCreate()
              if (e.key === 'Escape') cancelCreate()
            }}
            placeholder={t('tabBar.namePlaceholder')}
            className="h-8 flex-1 min-w-0 rounded-md text-sm"
            autoFocus
          />
          <button
            type="button"
            className="h-7 w-7 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
            onClick={commitCreate}
            aria-label={t('common.confirm')}
          >
            <Check className="h-4 w-4" />
          </button>
          <button
            type="button"
            className="h-7 w-7 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
            onClick={cancelCreate}
            aria-label={t('common.cancel')}
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      )
    }

    return (
      <div className={cn('flex items-center flex-1 min-w-0 gap-1', className)}>
        <DropdownMenu open={switcherOpen} onOpenChange={setSwitcherOpen}>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              className="h-8 flex-1 min-w-0 justify-between gap-1 px-2 rounded-md text-sm font-medium"
            >
              <span className="truncate">
                {current?.name || t('tabBar.namePlaceholder')}
              </span>
              <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="min-w-[14rem] max-w-[20rem]">
            {dashboards.map((d) => {
              const active = d.id === currentDashboardId
              return (
                <DropdownMenuItem
                  key={d.id}
                  onClick={() => onSwitch(d.id)}
                  className="gap-2 justify-between"
                >
                  <span className="truncate">{d.name}</span>
                  {active && <Check className="h-4 w-4 shrink-0 text-primary" />}
                </DropdownMenuItem>
              )
            })}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={() => setShowCreateInput(true)}
              className="gap-2"
            >
              <Plus className="h-4 w-4" />
              {t('tabBar.newDashboard')}
            </DropdownMenuItem>
            {current && (
              <>
                {canReorder && (
                  <>
                    <DropdownMenuItem
                      onClick={() => moveByOne(current.id, -1)}
                      disabled={dashboards.findIndex((d) => d.id === current.id) === 0}
                      className="gap-2"
                    >
                      <ChevronUp className="h-4 w-4" />
                      {t('sidebar.moveUp')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onClick={() => moveByOne(current.id, 1)}
                      disabled={
                        dashboards.findIndex((d) => d.id === current.id) ===
                        dashboards.length - 1
                      }
                      className="gap-2"
                    >
                      <ChevronDown className="h-4 w-4" />
                      {t('sidebar.moveDown')}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                  </>
                )}
                <DropdownMenuItem
                  onClick={handleRenameCurrent}
                  className="gap-2"
                >
                  <Pencil className="h-4 w-4" />
                  {t('tabBar.rename')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={handleDuplicateCurrent}
                  className="gap-2"
                >
                  <Copy className="h-4 w-4" />
                  {t('sidebar.duplicateDashboard')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={handleDeleteCurrent}
                  className="gap-2 text-error focus:text-error"
                >
                  <Trash2 className="h-4 w-4" />
                  {t('tabBar.delete')}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    )
  }

  return (
    <div className={cn('flex items-center flex-1 min-w-0 gap-2', className)}>
      {/* LEFT: sidebar toggle + add button (add is to the RIGHT of the toggle) */}
      <TooltipProvider delayDuration={300}>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              onClick={onSwitchToSidebar}
              className="h-7 w-7 shrink-0"
              aria-label={t('tabBar.switchToSidebar')}
            >
              <PanelLeft className="h-4 w-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{t('tabBar.switchToSidebar')}</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setShowCreateInput(true)}
              className="h-7 w-7 shrink-0"
              aria-label={t('tabBar.newDashboard')}
              disabled={showCreateInput}
            >
              <Plus className="h-4 w-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{t('tabBar.newDashboard')}</TooltipContent>
        </Tooltip>
      </TooltipProvider>

      {/* Vertical separator */}
      <div className="h-5 w-px bg-border shrink-0" />

      {/* MIDDLE: scrollable tabs (more menu renders inline after the active tab).
          Native overflow-x (scrollbar-none = no visible bar, matches PageTabs).
          Radix ScrollArea WITHOUT a ScrollBar child disables scrolling entirely
          (Radix gates the viewport's overflow on ScrollBar presence) — so we
          use native scrolling instead. pr-7 keeps the rightmost tab's hover-
          reveal ⋮ (28px) inside the padded region instead of clipping. */}
      <div className="flex-1 min-w-0 h-11 overflow-x-auto scrollbar-none">
        <div
          ref={tabsViewportRef}
          className="flex items-center gap-0.5 py-1.5 pr-7"
        >
          {dashboards.flatMap((dashboard) => {
            const isActive = dashboard.id === currentDashboardId
            const isEditing = editingId === dashboard.id

            const elements: ReactNode[] = []

            if (isEditing) {
              elements.push(
                <div
                  key={dashboard.id}
                  className="flex items-center gap-1 shrink-0"
                  onClick={(e) => e.stopPropagation()}
                >
                  <Input
                    ref={editInputRef}
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') commitRename(dashboard.id)
                      if (e.key === 'Escape') cancelRename()
                    }}
                    placeholder={t('tabBar.namePlaceholder')}
                    className="h-8 w-40 rounded-md text-sm"
                  />
                  <button
                    type="button"
                    className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
                    onClick={() => commitRename(dashboard.id)}
                    title={t('common.confirm')}
                  >
                    <Check className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
                    onClick={cancelRename}
                    title={t('common.cancel')}
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
              )
            } else if (isActive) {
              // Active tab: name + elastically-expanding ⋮ trigger.
              const currentIndex = dashboards.findIndex((d) => d.id === dashboard.id)
              elements.push(
                <div
                  key={dashboard.id}
                  data-dashboard-id={dashboard.id}
                  className="group flex items-center bg-foreground text-background font-medium rounded-md shadow-sm shrink-0 h-8 overflow-hidden max-w-[200px]"
                >
                  <button
                    type="button"
                    onClick={() => onSwitch(dashboard.id)}
                    onDoubleClick={() => {
                      setEditingId(dashboard.id)
                      setEditingName(dashboard.name)
                    }}
                    className="px-3 h-8 text-sm truncate rounded-md transition-colors min-w-0"
                    title={dashboard.name}
                  >
                    {dashboard.name}
                  </button>
                  <div
                    className={cn(
                      "flex items-center overflow-hidden",
                      "max-w-0 opacity-0",
                      "transition-[max-width,opacity] duration-200",
                      "[transition-timing-function:cubic-bezier(0.34,1.56,0.64,1)]",
                      // 28px button + 4px right breathing room so the hover
                      // background clears the tab's rounded right edge.
                      "group-hover:max-w-[32px] group-hover:opacity-100",
                      "group-focus-within:max-w-[32px] group-focus-within:opacity-100",
                      moreMenuOpen && "max-w-[32px] opacity-100"
                    )}
                  >
                    <DropdownMenu open={moreMenuOpen} onOpenChange={setMoreMenuOpen}>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 rounded-md mr-1"
                          aria-label={t('common.actions')}
                          onClick={(e) => e.stopPropagation()}
                        >
                          <MoreVertical className="h-3.5 w-3.5" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        {canReorder && (
                          <>
                            <DropdownMenuItem
                              onClick={() => moveByOne(dashboard.id, -1)}
                              disabled={currentIndex === 0}
                            >
                              <ChevronUp className="h-4 w-4" />
                              {t('sidebar.moveUp')}
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onClick={() => moveByOne(dashboard.id, 1)}
                              disabled={currentIndex === dashboards.length - 1}
                            >
                              <ChevronDown className="h-4 w-4" />
                              {t('sidebar.moveDown')}
                            </DropdownMenuItem>
                            <DropdownMenuSeparator />
                          </>
                        )}
                        <DropdownMenuItem onClick={handleRenameCurrent}>
                          <Pencil className="h-4 w-4" />
                          {t('tabBar.rename')}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={handleDuplicateCurrent}>
                          <Copy className="h-4 w-4" />
                          {t('sidebar.duplicateDashboard')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onClick={handleDeleteCurrent}
                          className="text-error focus:text-error"
                        >
                          <Trash2 className="h-4 w-4" />
                          {t('tabBar.delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </div>
              )
            } else {
              elements.push(
                <button
                  key={dashboard.id}
                  type="button"
                  data-dashboard-id={dashboard.id}
                  onClick={() => onSwitch(dashboard.id)}
                  onDoubleClick={() => {
                    setEditingId(dashboard.id)
                    setEditingName(dashboard.name)
                  }}
                  className={cn(
                    'px-3 h-8 text-sm truncate rounded-md transition-colors shrink-0 max-w-[200px]',
                    'text-muted-foreground hover:text-foreground hover:bg-muted-30'
                  )}
                  title={dashboard.name}
                >
                  {dashboard.name}
                </button>
              )
            }

            return elements
          })}

          {/* Inline create input - rendered as rightmost tab */}
          {showCreateInput && (
            <div
              className="flex items-center gap-1 shrink-0 ml-1"
              onClick={(e) => e.stopPropagation()}
            >
              <Input
                ref={createInputRef}
                value={newDashboardName}
                onChange={(e) => setNewDashboardName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitCreate()
                  if (e.key === 'Escape') cancelCreate()
                }}
                placeholder={t('tabBar.namePlaceholder')}
                className="h-8 w-40 rounded-md text-sm"
              />
              <button
                type="button"
                className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
                onClick={commitCreate}
                title={t('common.confirm')}
              >
                <Check className="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
                onClick={cancelCreate}
                title={t('common.cancel')}
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
