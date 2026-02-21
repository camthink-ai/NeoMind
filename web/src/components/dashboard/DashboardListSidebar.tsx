/**
 * DashboardListSidebar Component
 *
 * Left sidebar for managing multiple dashboards.
 * - Desktop: Fixed sidebar with collapse toggle
 * - Mobile: Slide-out drawer with backdrop
 */

import { useState } from 'react'
import {
  LayoutDashboard,
  Plus,
  Trash2,
  Edit2,
  Check,
  X,
  ChevronLeft,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import type { Dashboard } from '@/types/dashboard'
import { confirm } from '@/hooks/use-confirm'

export interface DashboardListSidebarProps {
  dashboards: Dashboard[]
  currentDashboardId: string | null
  onSwitch: (id: string) => void
  onCreate: (name: string) => void
  onRename: (id: string, name: string) => void
  onDelete: (id: string) => void
  /** Open state: false = collapsed (desktop) or closed (mobile), true = expanded (desktop) or open drawer (mobile) */
  open?: boolean
  /** Open/close handler */
  onOpenChange?: (open: boolean) => void
  /** Is desktop mode (fixed sidebar) vs mobile (drawer) */
  isDesktop?: boolean
  className?: string
}

export function DashboardListSidebar({
  dashboards,
  currentDashboardId,
  onSwitch,
  onCreate,
  onRename,
  onDelete,
  open = true,
  onOpenChange,
  isDesktop = true,
  className,
}: DashboardListSidebarProps) {
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingName, setEditingName] = useState('')
  const [showCreateInput, setShowCreateInput] = useState(false)
  const [newDashboardName, setNewDashboardName] = useState('')

  const handleStartEdit = (dashboard: Dashboard) => {
    setEditingId(dashboard.id)
    setEditingName(dashboard.name)
  }

  const handleSaveEdit = () => {
    if (editingId && editingName.trim()) {
      onRename(editingId, editingName.trim())
    }
    setEditingId(null)
    setEditingName('')
  }

  const handleCancelEdit = () => {
    setEditingId(null)
    setEditingName('')
  }

  const handleCreate = () => {
    if (newDashboardName.trim()) {
      onCreate(newDashboardName.trim())
      setNewDashboardName('')
      setShowCreateInput(false)
    }
  }

  const handleDelete = async (id: string) => {
    const confirmed = await confirm({
      title: 'Delete Dashboard',
      description: 'Are you sure you want to delete this dashboard?',
      confirmText: 'Delete',
      cancelText: 'Cancel',
      variant: 'destructive'
    })
    if (confirmed) {
      onDelete(id)
    }
  }

  const handleSwitch = (id: string) => {
    onSwitch(id)
    // Close drawer on mobile after switching
    if (!isDesktop) {
      onOpenChange?.(false)
    }
  }

  // Sidebar content - shared between desktop and mobile
  const SidebarContent = () => (
    <>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border">
        <div className="flex items-center gap-2">
          <LayoutDashboard className="w-4 h-4 text-primary" />
          <h2 className="font-semibold text-sm">Dashboards</h2>
        </div>
        {isDesktop ? (
          // Desktop: collapse toggle
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={() => onOpenChange?.(false)}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
        ) : (
          // Mobile: close button
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={() => onOpenChange?.(false)}
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Dashboard List */}
      <div className="flex-1 overflow-y-auto p-3 space-y-1">
        {dashboards.map((dashboard) => {
          const isEditing = editingId === dashboard.id
          const isActive = dashboard.id === currentDashboardId
          const componentCount = dashboard.components?.length ?? 0

          return (
            <div
              key={dashboard.id}
              className={cn(
                'group rounded-lg border transition-all active:scale-95',
                isActive
                  ? 'bg-primary/10 border-primary/20'
                  : 'bg-background border-border hover:bg-muted/50'
              )}
            >
              {isEditing ? (
                // Edit Mode
                <div className="flex items-center gap-1 p-2">
                  <Input
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSaveEdit()
                      if (e.key === 'Escape') handleCancelEdit()
                    }}
                    className="h-7 text-sm flex-1"
                    autoFocus
                  />
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={handleSaveEdit}
                  >
                    <Check className="h-3.5 w-3.5 text-green-500" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={handleCancelEdit}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </div>
              ) : (
                // View Mode
                <div
                  role="button"
                  tabIndex={0}
                  onClick={() => handleSwitch(dashboard.id)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault()
                      handleSwitch(dashboard.id)
                    }
                  }}
                  className="w-full text-left p-2.5 cursor-pointer hover:bg-muted/50 rounded-md"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium truncate">{dashboard.name}</p>
                      <p className="text-xs text-muted-foreground">
                        {componentCount} {componentCount === 1 ? 'component' : 'components'}
                      </p>
                    </div>
                    <div
                      className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6"
                        onClick={() => handleStartEdit(dashboard)}
                        title="Rename"
                      >
                        <Edit2 className="h-3 w-3" />
                      </Button>
                      {dashboards.length > 1 && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 text-destructive hover:text-destructive hover:bg-destructive/10"
                          onClick={() => handleDelete(dashboard.id)}
                          title="Delete"
                        >
                          <Trash2 className="h-3 w-3" />
                        </Button>
                      )}
                    </div>
                  </div>
                </div>
              )}
            </div>
          )
        })}

        {/* Create New Dashboard */}
        {showCreateInput ? (
          <div className="rounded-lg border border-dashed border-border bg-muted/30 p-2">
            <Input
              value={newDashboardName}
              onChange={(e) => setNewDashboardName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleCreate()
                if (e.key === 'Escape') {
                  setShowCreateInput(false)
                  setNewDashboardName('')
                }
              }}
              placeholder="Dashboard name..."
              className="h-8 text-sm mb-1"
              autoFocus
            />
            <div className="flex gap-1">
              <Button
                size="sm"
                variant="ghost"
                className="h-7 px-2 text-xs flex-1"
                onClick={handleCreate}
              >
                <Check className="h-3 w-3 mr-1 text-green-500" />
                Create
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 w-7 p-0"
                onClick={() => {
                  setShowCreateInput(false)
                  setNewDashboardName('')
                }}
              >
                <X className="h-3 w-3" />
              </Button>
            </div>
          </div>
        ) : (
          <Button
            variant="outline"
            className="w-full justify-start border-dashed"
            onClick={() => setShowCreateInput(true)}
          >
            <Plus className="h-4 w-4 mr-2" />
            New Dashboard
          </Button>
        )}
      </div>

      {/* Footer Info */}
      <div className="p-3 border-t border-border">
        <p className="text-xs text-muted-foreground text-center">
          {dashboards.length} {dashboards.length === 1 ? 'dashboard' : 'dashboards'}
        </p>
      </div>
    </>
  )

  // Desktop mode: fixed sidebar with collapse
  if (isDesktop) {
    return (
      <div
        className={cn(
          'flex-shrink-0 flex flex-col bg-card border-r border-border transition-all duration-300',
          !open ? 'w-0 overflow-hidden' : 'w-64',
          className
        )}
      >
        <SidebarContent />
      </div>
    )
  }

  // Mobile mode: drawer with backdrop
  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/30 backdrop-blur-sm z-40 transition-opacity lg:hidden"
          onClick={() => onOpenChange?.(false)}
        />
      )}

      {/* Sidebar Drawer */}
      <div
        className={cn(
          'fixed top-0 left-0 h-full w-72 z-50 lg:hidden',
          'bg-background shadow-xl flex flex-col',
          'transform transition-transform duration-300 ease-out',
          open ? 'translate-x-0' : '-translate-x-full',
          className
        )}
      >
        <SidebarContent />
      </div>
    </>
  )
}
