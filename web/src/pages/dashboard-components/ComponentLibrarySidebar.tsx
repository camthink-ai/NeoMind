/**
 * Component Library Sidebar
 *
 * Full-screen dialog split into a left navigation rail (source switch
 * between built-in components / marketplace, plus category filtering)
 * and a right content pane (search bar sitting directly above the grid
 * it filters). Mobile keeps a compact tab strip instead of the rail.
 * Extracted from VisualDashboard to reduce its file size and improve
 * maintainability.
 */

import { memo, useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { dynamicIconMap } from '@/lib/dynamicIcons'
import {
  LayoutGrid, Store as StoreIcon, Search, Boxes,
  Box, Check, Trash2, Download, Loader2, PackagePlus, RefreshCw, ArrowDownCircle,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  FullScreenDialog, FullScreenDialogHeader, FullScreenDialogContent,
  FullScreenDialogSidebar,
} from '@/components/automation/dialog'
import { EmptyState } from '@/components/shared/EmptyState'
import { notifySuccess, notifyError } from '@/lib/notify'
import { useIsMobile } from '@/hooks/useMobile'
import type { MarketComponentEntry } from '@/types/frontend-component'
import type { ComponentCategory } from './componentLibraryUtils'
import { InstallComponentDialog } from './InstallComponentDialog'

export interface ComponentLibrarySidebarProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  libraryTab: 'components' | 'marketplace'
  onLibraryTabChange: (tab: 'components' | 'marketplace') => void
  librarySearch: string
  onLibrarySearchChange: (search: string) => void
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
  /** Map of component id → { current, latest } for components with a newer marketplace version. */
  updatesAvailable: Record<string, { current: string; latest: string }>

  // Import dialog
  importDialogOpen: boolean
  onImportDialogOpenChange: (open: boolean) => void
}

export const ComponentLibrarySidebar = memo(function ComponentLibrarySidebar({
  open,
  onOpenChange,
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
}: ComponentLibrarySidebarProps) {
  const { t, i18n } = useTranslation('dashboardComponents')
  const isMobile = useIsMobile()

  // Highlight newly installed community component
  const [highlightedId, setHighlightedId] = useState<string | null>(null)
  // Category picked in the left rail ('all' = every category)
  const [selectedCategory, setSelectedCategory] = useState<string | 'all'>('all')

  const highlightedRef = useCallback((node: HTMLDivElement | null) => {
    if (node) {
      node.scrollIntoView({ behavior: 'smooth', block: 'center' })
    }
  }, [])

  useEffect(() => {
    if (!highlightedId) return
    const timer = setTimeout(() => setHighlightedId(null), 2500)
    return () => clearTimeout(timer)
  }, [highlightedId])

  // Reset the rail selection whenever the dialog closes
  useEffect(() => {
    if (!open) setSelectedCategory('all')
  }, [open])

  // Search always matches across every category; otherwise the rail's
  // selection scopes the grid (mobile has no rail, so it shows all).
  const searching = librarySearch.trim().length > 0
  const showAllCategories = searching || selectedCategory === 'all' || isMobile
  const visibleCategories = showAllCategories
    ? filteredLibrary
    : filteredLibrary.filter((c) => c.category === selectedCategory)
  const totalComponentCount = filteredLibrary.reduce((n, c) => n + c.items.length, 0)

  const resetOnClose = () => {
    onOpenChange(false)
    onLibrarySearchChange('')
    onLibraryTabChange('components')
  }

  const searchInput = (
    <div className="relative w-full max-w-sm">
      <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
      <Input
        value={librarySearch}
        onChange={(e) => onLibrarySearchChange(e.target.value)}
        placeholder={t('componentLibrary.searchPlaceholder')}
        className="h-9 pl-8"
      />
    </div>
  )

  const componentsPane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl">
        {visibleCategories.length === 0 ? (
          <div className="flex h-full min-h-[320px]">
            <EmptyState
              icon="search"
              title={t('componentLibrary.noResults')}
              description={t('componentLibrary.noResultsHint')}
            />
          </div>
        ) : (
          visibleCategories.map((category) => (
            <section key={category.category} className="pt-5 first:pt-1">
              {showAllCategories && (
                <div className="flex items-center gap-2 pb-2.5">
                  <span className={`flex h-6 w-6 items-center justify-center rounded-md ${category.categoryColor}`}>
                    <category.categoryIcon className="h-3.5 w-3.5" />
                  </span>
                  <span className="text-sm font-medium text-foreground">{category.categoryLabel}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground bg-muted rounded-full px-1.5 py-0.5 leading-none">
                    {category.items.length}
                  </span>
                </div>
              )}
              <div className="grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-3">
                {category.items.map((item) => {
                  const Icon = item.icon
                  const installedComp = installedComponents.find(c => c.id === item.id)
                  const isCommunity = !!installedComp
                  const update = updatesAvailable[item.id]
                  const isHighlighted = highlightedId === item.id
                  return (
                    <div
                      key={item.id}
                      ref={isHighlighted ? highlightedRef : undefined}
                      className="relative group"
                    >
                      {update && (
                        <span
                          className="absolute top-2 right-2 z-10 h-2 w-2 rounded-full bg-info ring-2 ring-background transition-opacity duration-normal group-hover:opacity-0"
                          title={t('componentLibrary.updateAvailable', { version: update.latest })}
                        />
                      )}
                      <button
                        type="button"
                        onClick={() => onAddComponent(item.id)}
                        className={`w-full h-[76px] flex items-center gap-3 py-2 px-3 rounded-lg border hover:shadow-sm transition-all duration-normal cursor-pointer active:scale-[0.98] text-left ${isHighlighted ? 'border-primary shadow-sm ring-2 ring-primary animate-[fadeHighlight_2s_ease-out_forwards]' : 'border-border'}`}
                      >
                        <span className="w-9 h-9 rounded-lg bg-muted text-foreground flex items-center justify-center shrink-0">
                          <Icon className="h-4 w-4 shrink-0" />
                        </span>
                        <div className="flex-1 min-w-0">
                          <span className="text-sm font-medium block truncate">{item.name}</span>
                          <p className="text-xs text-muted-foreground mt-0.5 line-clamp-2 leading-snug">{item.description}</p>
                        </div>
                      </button>
                      {isCommunity && (
                        <div className="absolute top-1 right-1 flex gap-0.5 rounded-md bg-background/90 backdrop-blur-sm p-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                          <Button
                            variant="ghost"
                            size="icon"
                            className={`h-6 w-6 ${update ? 'text-info' : 'text-muted-foreground hover:text-info'}`}
                            disabled={installingId === item.id}
                            title={update
                              ? t('componentLibrary.updateAvailable', { version: update.latest })
                              : t('componentLibrary.reinstall')}
                            aria-label={update
                              ? t('componentLibrary.updateAvailable', { version: update.latest })
                              : t('componentLibrary.reinstall')}
                            onClick={async (e) => {
                              e.stopPropagation()
                              onSetInstalling(item.id)
                              try {
                                await onRefreshComponent(item.id)
                                notifySuccess(t('componentLibrary.reinstallSuccess'))
                              } catch {
                                notifyError(t('componentLibrary.installError'))
                              } finally {
                                onSetInstalling(null)
                              }
                            }}
                          >
                            {installingId === item.id
                              ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              : update
                                ? <ArrowDownCircle className="h-3.5 w-3.5" />
                                : <RefreshCw className="h-3.5 w-3.5" />}
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6 text-muted-foreground hover:text-error"
                            disabled={installingId === item.id}
                            aria-label={t('componentLibrary.uninstall')}
                            onClick={async (e) => {
                              e.stopPropagation()
                              onSetInstalling(item.id)
                              try {
                                await onUninstall(item.id)
                                notifySuccess(t('componentLibrary.uninstallSuccess'))
                              } catch {
                                notifyError(t('componentLibrary.installError'))
                              } finally {
                                onSetInstalling(null)
                              }
                            }}
                          >
                            {installingId === item.id
                              ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              : <Trash2 className="h-3.5 w-3.5" />}
                          </Button>
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            </section>
          ))
        )}
      </div>
    </div>
  )

  const marketplacePane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl pt-4">
        {marketLoading ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="rounded-lg border border-border p-3.5 flex flex-col gap-2">
                <div className="flex items-start gap-2.5">
                  <div className="w-9 h-9 rounded-lg bg-muted animate-pulse shrink-0" />
                  <div className="flex-1 pt-1 space-y-1.5">
                    <div className="h-3.5 bg-muted rounded w-2/3 animate-pulse" />
                    <div className="h-2.5 bg-muted rounded w-1/2 animate-pulse" />
                  </div>
                </div>
                <div className="h-2.5 bg-muted rounded w-full animate-pulse" />
                <div className="h-7 bg-muted rounded-md animate-pulse" />
              </div>
            ))}
          </div>
        ) : marketComponents.length === 0 ? (
          <div className="flex h-full min-h-[320px]">
            <EmptyState
              icon="plugin"
              title={t('componentLibrary.marketplaceEmpty')}
            />
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
            {marketComponents.map((mc: MarketComponentEntry) => {
              const isInstalled = installedComponents.some(c => c.id === mc.id)
              const McIcon = dynamicIconMap[mc.icon || 'Box'] || Box
              const mcName = typeof mc.name === 'string' ? mc.name : (mc.name[i18n.language] || mc.name.en || Object.values(mc.name)[0] || mc.id)
              const mcDesc = typeof mc.description === 'string' ? mc.description : (mc.description[i18n.language] || mc.description.en || Object.values(mc.description)[0] || '')
              return (
                <div key={mc.id} className="rounded-lg border border-border bg-card p-3.5 flex flex-col gap-2">
                  <div className="flex items-start gap-2.5">
                    <div className="w-9 h-9 rounded-lg bg-muted text-foreground flex items-center justify-center shrink-0">
                      <McIcon className="w-4 h-4" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-foreground truncate">{mcName}</span>
                        {isInstalled && <Check className="w-3.5 h-3.5 text-success shrink-0" />}
                      </div>
                      <p className="text-xs text-muted-foreground">{t('componentLibrary.version')}: {mc.version}{mc.author ? ` · ${mc.author}` : ''}</p>
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground line-clamp-2 flex-1 min-h-0">{mcDesc}</p>
                  <Button
                    variant={isInstalled ? 'ghost' : 'outline'}
                    size="sm"
                    className="w-full h-7 text-xs"
                    disabled={installingId === mc.id}
                    onClick={async () => {
                      onSetInstalling(mc.id)
                      try {
                        if (isInstalled) {
                          await onUninstall(mc.id)
                          notifySuccess(t('componentLibrary.uninstallSuccess'))
                        } else {
                          await onInstall(mc.id)
                          notifySuccess(t('componentLibrary.installSuccess'))
                          onLibraryTabChange('components')
                          setHighlightedId(mc.id)
                        }
                      } catch {
                        notifyError(t('componentLibrary.installError'))
                      } finally {
                        onSetInstalling(null)
                      }
                    }}
                  >
                    {installingId === mc.id ? (
                      <><Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />{isInstalled ? t('componentLibrary.uninstall') : t('componentLibrary.install')}</>
                    ) : isInstalled ? (
                      <><Trash2 className="w-3.5 h-3.5 mr-1" />{t('componentLibrary.uninstall')}</>
                    ) : (
                      <><Download className="w-3.5 h-3.5 mr-1" />{t('componentLibrary.install')}</>
                    )}
                  </Button>
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )

  return (
    <>
      <FullScreenDialog open={open} onOpenChange={(newOpen: boolean) => {
        onOpenChange(newOpen)
        if (!newOpen) { onLibrarySearchChange(''); onLibraryTabChange('components') }
      }}>
        <FullScreenDialogHeader
          title={t('visualDashboard.componentLibrary')}
          onClose={resetOnClose}
        />

        <FullScreenDialogContent>
          <style>{`
            @keyframes fadeHighlight {
              0% { box-shadow: 0 0 0 3px oklch(var(--primary) / 0.25); }
              70% { box-shadow: 0 0 0 3px oklch(var(--primary) / 0.15); }
              100% { box-shadow: 0 0 0 0px oklch(var(--primary) / 0); }
            }
          `}</style>

          {isMobile ? (
            /* Mobile: no room for a rail — compact tab strip + inline
               search above the shared scroll content. */
            <div className="flex-1 flex flex-col overflow-hidden">
              <div className="px-4 pt-3 pb-3 shrink-0 space-y-3">
                <div className="flex items-center gap-3">
                  <Tabs value={libraryTab} onValueChange={(v) => onLibraryTabChange(v as 'components' | 'marketplace')}>
                    <TabsList className="h-9">
                      <TabsTrigger value="components" className="gap-1.5 text-xs px-3">
                        <LayoutGrid className="w-3.5 h-3.5" />
                        {t('componentLibrary.tabComponents')}
                      </TabsTrigger>
                      <TabsTrigger value="marketplace" className="gap-1.5 text-xs px-3">
                        <StoreIcon className="w-3.5 h-3.5" />
                        {t('componentLibrary.tabMarketplace')}
                      </TabsTrigger>
                    </TabsList>
                  </Tabs>
                  {libraryTab === 'marketplace' && (
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-9 gap-1.5 text-xs"
                      onClick={() => onImportDialogOpenChange(true)}
                    >
                      <PackagePlus className="w-3.5 h-3.5" />
                      {t('componentLibrary.importComponent')}
                    </Button>
                  )}
                </div>
                {libraryTab === 'components' && searchInput}
              </div>
              {libraryTab === 'components' ? componentsPane : marketplacePane}
            </div>
          ) : (
            /* Desktop split layout: the rail owns navigation (source
               switch + category filter / import entry) so the content
               pane is purely results — search sits directly above the
               grid it filters instead of floating next to tabs. */
            <div className="flex-1 flex overflow-hidden">
              <FullScreenDialogSidebar className="flex flex-col overflow-hidden p-3 gap-1">
                <button
                  type="button"
                  onClick={() => onLibraryTabChange('components')}
                  className={cn(
                    'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-sm font-medium transition-colors',
                    libraryTab === 'components'
                      ? 'bg-muted text-foreground'
                      : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                  )}
                >
                  <LayoutGrid className="h-4 w-4 shrink-0" />
                  {t('componentLibrary.tabComponents')}
                </button>
                <button
                  type="button"
                  onClick={() => onLibraryTabChange('marketplace')}
                  className={cn(
                    'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-sm font-medium transition-colors',
                    libraryTab === 'marketplace'
                      ? 'bg-muted text-foreground'
                      : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                  )}
                >
                  <StoreIcon className="h-4 w-4 shrink-0" />
                  {t('componentLibrary.tabMarketplace')}
                </button>

                {libraryTab === 'components' ? (
                  <>
                    <div className="my-2 border-t border-border" />
                    <nav className="flex-1 overflow-y-auto space-y-0.5">
                      <button
                        type="button"
                        onClick={() => setSelectedCategory('all')}
                        className={cn(
                          'flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-sm transition-colors',
                          selectedCategory === 'all'
                            ? 'bg-muted text-foreground'
                            : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                        )}
                      >
                        <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-muted text-muted-foreground">
                          <Boxes className="h-3 w-3" />
                        </span>
                        <span className="truncate">{t('componentLibrary.all')}</span>
                        <span className="ml-auto text-xs tabular-nums text-muted-foreground">{totalComponentCount}</span>
                      </button>
                      {filteredLibrary.map((category) => (
                        <button
                          type="button"
                          key={category.category}
                          onClick={() => setSelectedCategory(category.category)}
                          disabled={category.items.length === 0}
                          className={cn(
                            'flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-sm transition-colors',
                            selectedCategory === category.category
                              ? 'bg-muted text-foreground'
                              : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                            category.items.length === 0 && 'opacity-50 cursor-default',
                          )}
                        >
                          <span className={`flex h-5 w-5 shrink-0 items-center justify-center rounded ${category.categoryColor}`}>
                            <category.categoryIcon className="h-3 w-3" />
                          </span>
                          <span className="truncate">{category.categoryLabel}</span>
                          <span className="ml-auto text-xs tabular-nums text-muted-foreground">{category.items.length}</span>
                        </button>
                      ))}
                    </nav>
                  </>
                ) : (
                  <div className="pt-1">
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-9 w-full justify-start gap-2 text-sm font-medium"
                      onClick={() => onImportDialogOpenChange(true)}
                    >
                      <PackagePlus className="h-4 w-4" />
                      {t('componentLibrary.importComponent')}
                    </Button>
                  </div>
                )}
              </FullScreenDialogSidebar>

              <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                {libraryTab === 'components' && (
                  <div className="shrink-0 px-4 md:px-6 pt-4 pb-3">
                    <div className="mx-auto w-full max-w-5xl">
                      {searchInput}
                    </div>
                  </div>
                )}
                {libraryTab === 'components' ? componentsPane : marketplacePane}
              </div>
            </div>
          )}
        </FullScreenDialogContent>
      </FullScreenDialog>

      <InstallComponentDialog open={importDialogOpen} onOpenChange={onImportDialogOpenChange} />
    </>
  )
})
