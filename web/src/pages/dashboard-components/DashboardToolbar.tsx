/**
 * Dashboard Toolbar
 *
 * Top header bar containing dashboard tabs/name, edit/add/share/fullscreen buttons,
 * and the component library sidebar trigger.
 *
 * Pure presentational component — receives data and callbacks as props.
 */

import { Check, Settings2, Plus, Share2, Maximize } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { DashboardTabBar } from '@/components/dashboard/DashboardTabBar'
import { ComponentLibrarySidebar } from './ComponentLibrarySidebar'
import type { Dashboard } from '@/types/dashboard'
import type { ComponentCategory } from './componentLibraryUtils'
import type { MarketComponentEntry } from '@/types/frontend-component'

export interface DashboardToolbarProps {
  // Dashboard data
  sortedDashboards: Dashboard[]
  currentDashboardId: string | null
  layoutMode: 'sidebar' | 'tabs'

  // Dashboard handlers
  onDashboardSwitch: (id: string) => void
  onDashboardCreate: (name: string) => Promise<void>
  onDashboardRename: (id: string, name: string) => void
  onDashboardDuplicate: (id: string) => void
  onDashboardDelete: (id: string) => Promise<void>
  /** Persist a new manual order (array of dashboard IDs, index 0 = first). */
  onDashboardReorder?: (newOrder: string[]) => void
  onSwitchToSidebar: () => void

  // Edit mode
  editMode: boolean
  setEditMode: (mode: boolean) => void

  // Mobile state
  isMobile: boolean
  setMobileSelectedId: (id: string | null) => void
  setMobileEditBarOpen: (open: boolean) => void

  // Share
  onOpenShare: () => void

  // Fullscreen
  onToggleFullscreen: () => void

  // Component library sidebar
  componentLibraryOpen: boolean
  setComponentLibraryOpen: (open: boolean) => void
  libraryTab: 'components' | 'marketplace'
  onLibraryTabChange: (tab: 'components' | 'marketplace') => void
  librarySearch: string
  onLibrarySearchChange: (q: string) => void
  filteredLibrary: ComponentCategory[]
  onAddComponent: (componentType: string) => void

  // Marketplace
  marketComponents: MarketComponentEntry[]
  marketLoading: boolean
  installedComponents: { id: string; source?: 'local' | 'marketplace' }[]
  installingId: string | null
  onInstall: (id: string) => Promise<void>
  onUninstall: (id: string) => Promise<void>
  onRefreshComponent: (id: string) => Promise<void>
  onSetInstalling: (id: string | null) => void
  updatesAvailable: Record<string, { current: string; latest: string }>
  importDialogOpen: boolean
  onImportDialogOpenChange: (open: boolean) => void
}

export function DashboardToolbar(props: DashboardToolbarProps) {
  const {
    sortedDashboards,
    currentDashboardId,
    layoutMode,
    onDashboardSwitch,
    onDashboardCreate,
    onDashboardRename,
    onDashboardDuplicate,
    onDashboardDelete,
    onDashboardReorder,
    onSwitchToSidebar,
    editMode,
    setEditMode,
    isMobile,
    setMobileSelectedId,
    setMobileEditBarOpen,
    onOpenShare,
    onToggleFullscreen,
    componentLibraryOpen,
    setComponentLibraryOpen,
    libraryTab,
    onLibraryTabChange,
    librarySearch,
    onLibrarySearchChange,
    filteredLibrary,
    onAddComponent,
    marketComponents,
    marketLoading,
    installedComponents,
    installingId,
    onInstall,
    onUninstall,
    onRefreshComponent,
    onSetInstalling,
    updatesAvailable,
    importDialogOpen,
    onImportDialogOpenChange,
  } = props

  const { t } = useTranslation('dashboardComponents')

  // Sidebar mode on desktop: the action buttons sit directly under the
  // page title (left-aligned) so title + actions read as one header
  // block. Tabs/mobile keep them at the right edge opposite the tab bar.
  const actionsAlignLeft = layoutMode === 'sidebar' && !isMobile

  return (
    <header
      className={cn(
        'shrink-0 flex items-center px-4 sm:px-6 md:px-8 py-2 bg-background z-10',
        actionsAlignLeft ? 'justify-start' : 'justify-between',
      )}
    >
      {/* Mobile: always show the dropdown switcher regardless of layoutMode.
          Sidebar-mode's "open the list drawer" pattern has no trigger on
          touch devices, so we route through DashboardTabBar's mobile UI. */}
      {layoutMode === 'tabs' || isMobile ? (
        <DashboardTabBar
          dashboards={sortedDashboards}
          currentDashboardId={currentDashboardId}
          onSwitch={onDashboardSwitch}
          onCreate={onDashboardCreate}
          onRename={onDashboardRename}
          onDuplicate={onDashboardDuplicate}
          onDelete={onDashboardDelete}
          onReorder={onDashboardReorder}
          onSwitchToSidebar={onSwitchToSidebar}
        />
      ) : (
        /* Sidebar mode on desktop: the page title above already shows the
           current dashboard's name, so the left slot stays empty and only
           the action buttons remain, left-aligned under the title. */
        <span aria-hidden="true" />
      )}

      <TooltipProvider delayDuration={300}>
        <div className="flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant={editMode ? "default" : "outline"}
                size="icon"
                onClick={() => {
                  const nextMode = !editMode
                  setEditMode(nextMode)
                  if (!nextMode && isMobile) {
                    setMobileSelectedId(null)
                    setMobileEditBarOpen(false)
                  }
                }}
                className="h-8 w-8 rounded-lg"
              >
                {editMode ? (
                  <Check className="h-4 w-4" />
                ) : (
                  <Settings2 className="h-4 w-4" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              {editMode ? t('common.done') : t('common:editDashboard')}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8 rounded-lg"
                disabled={!editMode}
                onClick={() => editMode && setComponentLibraryOpen(true)}
              >
                <Plus className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{t('visualDashboard.addComponent')}</TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8 rounded-lg"
                onClick={() => onOpenShare()}
              >
                <Share2 className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{t('visualDashboard.share.title')}</TooltipContent>
          </Tooltip>

          <ComponentLibrarySidebar
            open={componentLibraryOpen}
            onOpenChange={setComponentLibraryOpen}
            libraryTab={libraryTab}
            onLibraryTabChange={onLibraryTabChange}
            librarySearch={librarySearch}
            onLibrarySearchChange={onLibrarySearchChange}
            filteredLibrary={filteredLibrary}
            onAddComponent={onAddComponent}
            marketComponents={marketComponents}
            marketLoading={marketLoading}
            installedComponents={installedComponents}
            installingId={installingId}
            onInstall={onInstall}
            onUninstall={onUninstall}
            onRefreshComponent={onRefreshComponent}
            onSetInstalling={onSetInstalling}
            updatesAvailable={updatesAvailable}
            importDialogOpen={importDialogOpen}
            onImportDialogOpenChange={onImportDialogOpenChange}
          />

          {/* Fullscreen toggle button */}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 rounded-lg"
                onClick={onToggleFullscreen}
              >
                <Maximize className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom">{t('visualDashboard.fullscreen')}</TooltipContent>
          </Tooltip>
        </div>

      </TooltipProvider>
    </header>
  )
}
