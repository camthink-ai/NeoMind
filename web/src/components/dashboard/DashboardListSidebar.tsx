/**
 * DashboardListSidebar Component
 *
 * Left sidebar for managing multiple dashboards.
 * - Desktop: Separate fixed column (always expanded)
 * - Mobile: Slide-out drawer with backdrop
 */

import { useState, useRef, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { getPortalRoot } from '@/lib/portal'
import {
  LayoutDashboard,
  Plus,
  Trash2,
  Pencil,
  Copy,
  Check,
  X,
  PanelTop,
  ChevronUp,
  ChevronDown,
  MoreVertical,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
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
import { textNano } from '@/design-system/tokens/typography'
import type { Dashboard } from '@/types/dashboard'
import { confirm } from '@/hooks/use-confirm'

export interface DashboardListSidebarProps {
  dashboards: Dashboard[]
  currentDashboardId: string | null
  onSwitch: (id: string) => void
  onCreate: (name: string) => void
  onRename: (id: string, name: string) => void
  onDuplicate: (id: string) => void
  onDelete: (id: string) => void
  /** Persist a new manual order (array of dashboard IDs, index 0 = top). */
  onReorder?: (newOrder: string[]) => void
  /** Open state: false = collapsed (desktop) or closed (mobile), true = expanded (desktop) or open drawer (mobile) */
  open?: boolean
  /** Open/close handler */
  onOpenChange?: (open: boolean) => void
  /** Is desktop mode (fixed sidebar) vs mobile (drawer) */
  isDesktop?: boolean
  /** Optional callback to switch from sidebar layout to tab bar layout */
  onSwitchToTabs?: () => void
  className?: string
}

/** Shared content for both desktop and mobile sidebar layouts */
function DashboardSidebarContent({
  dashboards,
  currentDashboardId,
  onSwitch,
  onCreate,
  onRename,
  onDuplicate,
  onDelete,
  onReorder,
  onOpenChange,
  isDesktop,
  onSwitchToTabs,
}: Omit<DashboardListSidebarProps, 'open' | 'className'>) {
  const { t } = useTranslation('dashboardComponents')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingName, setEditingName] = useState('')
  const [showCreateInput, setShowCreateInput] = useState(false)
  const [newDashboardName, setNewDashboardName] = useState('')
  const editInputRef = useRef<HTMLInputElement>(null)
  const createInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (showCreateInput && createInputRef.current) {
      createInputRef.current.focus()
    }
  }, [showCreateInput])

  useEffect(() => {
    if (editingId && editInputRef.current) {
      editInputRef.current.focus()
      editInputRef.current.select()
    }
  }, [editingId])

  const handleDelete = async (id: string) => {
    const dashboard = dashboards.find(d => d.id === id)
    const name = dashboard?.name || ''
    const confirmed = await confirm({
      title: t('sidebar.deleteTitle'),
      description: t('sidebar.deleteDescription', { name }),
      confirmText: t('sidebar.delete'),
      cancelText: t('common.cancel'),
      variant: 'destructive'
    })
    if (confirmed) {
      onDelete(id)
    }
  }

  const handleSwitch = (id: string) => {
    onSwitch(id)
    if (!isDesktop) {
      onOpenChange?.(false)
    }
  }

  // --- Reordering helpers (arrow buttons) ---
  // Produces a new ID order array and delegates to onReorder.
  // No-op when onReorder isn't provided (parent didn't wire it).
  const idsInOrder = () => dashboards.map(d => d.id)

  const moveByOne = (id: string, direction: -1 | 1) => {
    if (!onReorder) return
    const ids = idsInOrder()
    const idx = ids.indexOf(id)
    const target = idx + direction
    if (idx < 0 || target < 0 || target >= ids.length) return
    const next = [...ids]
    next.splice(idx, 1)
    next.splice(target, 0, id)
    onReorder(next)
  }

  return (
    <>
      {/* Header */}
      <div className="flex items-center justify-between px-3 h-11 border-b border-border">
        <h2 className="text-sm font-semibold">{t('sidebar.title')}</h2>
        <div className="flex items-center gap-0.5">
          {isDesktop && onSwitchToTabs && (
            <TooltipProvider delayDuration={300}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={onSwitchToTabs}
                    className="h-6 w-6 rounded-lg"
                    aria-label={t('sidebar.switchToTabs')}
                  >
                    <PanelTop className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom">{t('sidebar.switchToTabs')}</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          {!isDesktop && (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => onOpenChange?.(false)}
              className="h-6 w-6 rounded-lg"
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>

      {/* New Dashboard Button */}
      <div className="p-3 pb-2">
        {showCreateInput ? (
          <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
            <Input
              ref={createInputRef}
              value={newDashboardName}
              onChange={(e) => setNewDashboardName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && newDashboardName.trim()) {
                  onCreate(newDashboardName.trim())
                  setNewDashboardName('')
                  setShowCreateInput(false)
                }
                if (e.key === 'Escape') {
                  setShowCreateInput(false)
                  setNewDashboardName('')
                }
              }}
              placeholder={t('sidebar.namePlaceholder')}
              className="h-8 flex-1 rounded-lg placeholder:text-mini"
              autoFocus
            />
            <button
              aria-label={t('common:confirm')}
              className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
              onClick={() => {
                if (newDashboardName.trim()) {
                  onCreate(newDashboardName.trim())
                  setNewDashboardName('')
                  setShowCreateInput(false)
                }
              }}
            >
              <Check className="h-3.5 w-3.5" />
            </button>
            <button
              aria-label={t('common:cancel')}
              className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
              onClick={() => { setShowCreateInput(false); setNewDashboardName('') }}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        ) : (
          <Button
            onClick={() => setShowCreateInput(true)}
            variant="outline"
            className="w-full h-8 text-sm rounded-lg"
          >
            <Plus className="h-4 w-4 mr-1.5" />
            {t('sidebar.newDashboard')}
          </Button>
        )}
      </div>

      {/* Dashboard List */}
      <ScrollArea className="flex-1 min-h-0">
        <div className="px-2 pb-2 space-y-0.5">
          {dashboards.length === 0 ? (
            <div className="py-8 text-center">
              <LayoutDashboard className="h-8 w-8 mx-auto text-muted-foreground mb-2" />
              <p className={cn(textNano, "text-muted-foreground")}>
                {t('sidebar.newDashboard')}
              </p>
            </div>
          ) : (
            dashboards.map((dashboard, index) => {
              const isActive = dashboard.id === currentDashboardId
              const isEditing = editingId === dashboard.id
              const count = dashboard.components?.length ?? 0
              const canReorder = !!onReorder && dashboards.length > 1

              return (
                <div
                  key={dashboard.id}
                  onClick={() => !isEditing && handleSwitch(dashboard.id)}
                  className={cn(
                    "group relative flex items-start gap-2 p-2 rounded-lg cursor-pointer transition-all",
                    isActive
                      ? "bg-muted"
                      : "hover:bg-muted-50",
                    isEditing && "bg-muted"
                  )}
                >
                  {isEditing ? (
                    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                      <Input
                        ref={editInputRef}
                        value={editingName}
                        onChange={(e) => setEditingName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' && editingName.trim()) {
                            onRename(dashboard.id, editingName.trim())
                            setEditingId(null)
                            setEditingName('')
                          }
                          if (e.key === 'Escape') {
                            setEditingId(null)
                            setEditingName('')
                          }
                        }}
                        className="h-7 text-sm flex-1 rounded-md"
                        autoFocus
                      />
                      <button
                        className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md text-success hover:bg-success-light transition-colors"
                        onClick={() => {
                          if (editingName.trim()) {
                            onRename(dashboard.id, editingName.trim())
                            setEditingId(null)
                            setEditingName('')
                          }
                        }}
                      >
                        <Check className="h-3.5 w-3.5" />
                      </button>
                      <button
                        className="h-6 w-6 shrink-0 flex items-center justify-center rounded-md hover:bg-muted transition-colors"
                        onClick={() => { setEditingId(null); setEditingName('') }}
                      >
                        <X className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  ) : (
                    <>
                      <div className="flex items-start gap-2 min-w-0 flex-1">
                        <LayoutDashboard className={cn(
                          "h-4 w-4 mt-0.5 shrink-0",
                          isActive ? "text-foreground" : "text-muted-foreground"
                        )} />
                        <div className="min-w-0 flex-1">
                          <h4 className={cn(
                            "text-sm truncate",
                            isActive ? "text-foreground font-medium" : "text-muted-foreground"
                          )}>
                            {dashboard.name}
                          </h4>
                          <div className={cn("flex items-center gap-1 mt-0.5 overflow-hidden", textNano, "text-muted-foreground")}>
                            <span className="truncate">{t('sidebar.componentCount', { count })}</span>
                          </div>
                        </div>
                      </div>

                      {/* Action menu */}
                      <div className="absolute right-1.5 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity">
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <button
                              type="button"
                              className="h-6 w-6 flex items-center justify-center rounded hover:bg-muted transition-colors"
                              onClick={(e) => e.stopPropagation()}
                              aria-label={t('sidebar.moreActions')}
                              title={t('sidebar.moreActions')}
                            >
                              <MoreVertical className="h-3 w-3" />
                            </button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            {canReorder && (
                              <>
                                <DropdownMenuItem
                                  onClick={(e) => { e.stopPropagation(); moveByOne(dashboard.id, -1) }}
                                  disabled={index === 0}
                                >
                                  <ChevronUp className="h-4 w-4" />
                                  {t('sidebar.moveUp')}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  onClick={(e) => { e.stopPropagation(); moveByOne(dashboard.id, 1) }}
                                  disabled={index === dashboards.length - 1}
                                >
                                  <ChevronDown className="h-4 w-4" />
                                  {t('sidebar.moveDown')}
                                </DropdownMenuItem>
                                <DropdownMenuSeparator />
                              </>
                            )}
                            <DropdownMenuItem
                              onClick={(e) => { e.stopPropagation(); setEditingId(dashboard.id); setEditingName(dashboard.name) }}
                            >
                              <Pencil className="h-4 w-4" />
                              {t('sidebar.rename')}
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onClick={(e) => { e.stopPropagation(); onDuplicate(dashboard.id) }}
                            >
                              <Copy className="h-4 w-4" />
                              {t('sidebar.duplicateDashboard')}
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onClick={(e) => { e.stopPropagation(); handleDelete(dashboard.id) }}
                              className="text-error focus:text-error"
                            >
                              <Trash2 className="h-4 w-4" />
                              {t('sidebar.delete')}
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>
                    </>
                  )}
                </div>
              )
            })
          )}
        </div>
      </ScrollArea>

      {/* Footer */}
      <div className="p-2 border-t border-border">
        <p className={cn(textNano, "text-muted-foreground text-center")}>
          {t('sidebar.dashboardCount', { count: dashboards.length })}
        </p>
      </div>
    </>
  )
}

export function DashboardListSidebar({
  dashboards,
  currentDashboardId,
  onSwitch,
  onCreate,
  onRename,
  onDuplicate,
  onDelete,
  onReorder,
  open = true,
  onOpenChange,
  isDesktop = true,
  onSwitchToTabs,
  className,
}: DashboardListSidebarProps) {
  // Desktop mode: separate fixed-width column
  if (isDesktop) {
    return (
      <div
        className={cn(
          // bg-popover: opaque, unified with SessionSidebar and all other
          // popups/drawers.
          "h-full w-64 bg-popover border-r border-border flex flex-col",
          className
        )}
      >
        <DashboardSidebarContent
          dashboards={dashboards}
          currentDashboardId={currentDashboardId}
          onSwitch={onSwitch}
          onCreate={onCreate}
          onRename={onRename}
          onDuplicate={onDuplicate}
          onDelete={onDelete}
          onReorder={onReorder}
          onOpenChange={onOpenChange}
          isDesktop={true}
          onSwitchToTabs={onSwitchToTabs}
        />
      </div>
    )
  }

  // Mobile mode: drawer — portaled to the dialog root so the fixed overlay
  // escapes any in-page stacking context and stacks at the base dialog tier
  // (z-50) like every other drawer (MobileNav Sheet, SessionSidebar). Full
  // height, covering the chrome, no --topnav-height geometric coupling.
  return createPortal(
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-overlay-light backdrop-blur-sm z-50 transition-opacity lg:hidden"
          onClick={() => onOpenChange?.(false)}
        />
      )}

      {/* Sidebar Drawer */}
      <div
        className={cn(
          'fixed top-0 left-0 h-full w-72 z-50 lg:hidden safe-top',
          // bg-popover matches desktop persistent sidebar and all other
          // drawers. Previously bg-background (dark-mode /97% alpha) which
          // produced a visible dark tint vs the topnav chrome above.
          'bg-popover shadow-xl flex flex-col',
          'transform transition-transform duration-300 ease-out',
          open ? 'translate-x-0' : '-translate-x-full',
          className
        )}
      >
        <DashboardSidebarContent
          dashboards={dashboards}
          currentDashboardId={currentDashboardId}
          onSwitch={onSwitch}
          onCreate={onCreate}
          onRename={onRename}
          onDuplicate={onDuplicate}
          onDelete={onDelete}
          onReorder={onReorder}
          onOpenChange={onOpenChange}
          isDesktop={false}
        />
      </div>
    </>,
    getPortalRoot()
  )
}
