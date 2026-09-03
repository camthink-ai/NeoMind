/**
 * Component Library Sidebar
 *
 * Full-screen dialog split into a left navigation rail (source switch:
 * built-in Components / Marketplace / Custom imports, plus category
 * filtering for the components source) and a right content pane whose
 * toolbar carries the per-source action — search above the grid it
 * filters, market refresh, or the import entry for custom components.
 * Mobile keeps a compact tab strip instead of the rail.
 */

import { memo, useState, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { dynamicIconMap } from '@/lib/dynamicIcons'
import {
  LayoutGrid, Store as StoreIcon, Search, Boxes, PackagePlus, Plus, Puzzle, RefreshCw,
  Box, Check, Trash2, Loader2, ArrowDownCircle,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import {
  FullScreenDialog, FullScreenDialogHeader, FullScreenDialogContent,
  FullScreenDialogSidebar,
} from '@/components/automation/dialog'
import { EmptyState } from '@/components/shared/EmptyState'
import { notifySuccess, notifyError } from '@/lib/notify'
import { useIsMobile } from '@/hooks/useMobile'
import type { MarketComponentEntry, FrontendComponentMeta } from '@/types/frontend-component'
import type { ComponentCategory } from './componentLibraryUtils'
import { InstallComponentDialog } from './InstallComponentDialog'

export type ComponentLibraryTab = 'components' | 'extensions' | 'marketplace' | 'custom'

export interface ComponentLibrarySidebarProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  libraryTab: ComponentLibraryTab
  onLibraryTabChange: (tab: ComponentLibraryTab) => void
  librarySearch: string
  onLibrarySearchChange: (search: string) => void
  filteredLibrary: ComponentCategory[]
  onAddComponent: (componentType: string) => void

  // Marketplace
  marketComponents: MarketComponentEntry[]
  marketLoading: boolean
  installedComponents: FrontendComponentMeta[]
  /** Re-fetch the marketplace index (toolbar refresh on the marketplace source). */
  onRefreshMarket: () => Promise<void>
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

/** Resolve a possibly localized manifest string field. */
function localized(value: string | Record<string, string> | undefined, language: string, fallback: string): string {
  if (typeof value === 'string') return value
  if (value && typeof value === 'object') {
    return value[language] || value.en || Object.values(value)[0] || fallback
  }
  return fallback
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
  onRefreshMarket,
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

  // Source-of-origin split: Components = built-ins only, Extensions =
  // everything registered by extensions (their self-declared categories,
  // 'custom' as canonical fallback), Custom = manual imports. An
  // extension may declare a built-in category ("charts") — so the split
  // is per ITEM, then empty groups are dropped.
  const extensionTypes = useMemo(
    () => new Set(dynamicRegistry.getAllMetas().map((d) => d.type)),
    []
  )
  const nonCommunity = useMemo(
    () => filteredLibrary.filter((c) => c.category !== 'local' && c.category !== 'marketplace'),
    [filteredLibrary]
  )
  const builtinCategories = useMemo(
    () =>
      nonCommunity
        .map((c) => ({ ...c, items: c.items.filter((i) => !extensionTypes.has(i.id)) }))
        .filter((c) => c.items.length > 0),
    [nonCommunity, extensionTypes]
  )
  const extensionCategories = useMemo(
    () =>
      nonCommunity
        .map((c) => ({ ...c, items: c.items.filter((i) => extensionTypes.has(i.id)) }))
        .filter((c) => c.items.length > 0),
    [nonCommunity, extensionTypes]
  )

  // Search always matches across every category; otherwise the rail's
  // selection scopes the grid (mobile has no rail, so it shows all).
  const searching = librarySearch.trim().length > 0
  const showAllCategories = searching || selectedCategory === 'all' || isMobile
  const visibleCategories = showAllCategories
    ? builtinCategories
    : builtinCategories.filter((c) => c.category === selectedCategory)
  const totalComponentCount = builtinCategories.reduce((n, c) => n + c.items.length, 0)

  const resetOnClose = () => {
    onOpenChange(false)
    onLibrarySearchChange('')
    onLibraryTabChange('components')
  }

  const searchInput = (
    <div className="relative w-full">
      <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
      <Input
        value={librarySearch}
        onChange={(e) => onLibrarySearchChange(e.target.value)}
        placeholder={t('componentLibrary.searchPlaceholder')}
        className="h-9 pl-8"
      />
    </div>
  )

  const importButton = (fullWidth = false) => (
    <Button
      variant="outline"
      size="sm"
      className={cn('h-9 gap-1.5 text-xs', fullWidth && 'w-full justify-start')}
      onClick={() => onImportDialogOpenChange(true)}
    >
      <PackagePlus className="w-3.5 h-3.5" />
      {t('componentLibrary.importComponent')}
    </Button>
  )

  const handleUninstall = async (id: string) => {
    onSetInstalling(id)
    try {
      await onUninstall(id)
      notifySuccess(t('componentLibrary.uninstallSuccess'))
    } catch {
      notifyError(t('componentLibrary.installError'))
    } finally {
      onSetInstalling(null)
    }
  }

  const handleRefresh = async (id: string) => {
    onSetInstalling(id)
    try {
      await onRefreshComponent(id)
      notifySuccess(t('componentLibrary.reinstallSuccess'))
    } catch {
      notifyError(t('componentLibrary.installError'))
    } finally {
      onSetInstalling(null)
    }
  }

  const renderCategorySections = (categories: ComponentCategory[], showHeaders: boolean) =>
    categories.map((category) => (
      <section key={category.category} className="pt-5 first:pt-1">
        {showHeaders && (
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
            const isHighlighted = highlightedId === item.id
            return (
              <button
                key={item.id}
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
            )
          })}
        </div>
      </section>
    ))

  const noResultsPane = (
    <div className="flex h-full min-h-[320px]">
      <EmptyState
        icon="search"
        title={t('componentLibrary.noResults')}
        description={t('componentLibrary.noResultsHint')}
      />
    </div>
  )

  const componentsPane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl">
        {visibleCategories.length === 0
          ? noResultsPane
          : renderCategorySections(visibleCategories, showAllCategories)}
      </div>
    </div>
  )

  const extensionsPane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl">
        {extensionCategories.length === 0 ? (
          <div className="flex h-full min-h-[320px]">
            <EmptyState
              icon={<Puzzle className="h-12 w-12" />}
              title={t('componentLibrary.extensionsEmptyTitle')}
              description={t('componentLibrary.extensionsEmptyDesc')}
            />
          </div>
        ) : (
          renderCategorySections(extensionCategories, true)
        )}
      </div>
    </div>
  )

  const marketplacePane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl">
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
              const mcName = localized(mc.name, i18n.language, mc.id)
              const mcDesc = localized(mc.description, i18n.language, '')
              const isHighlighted = highlightedId === mc.id
              return (
                <div
                  key={mc.id}
                  ref={isHighlighted ? highlightedRef : undefined}
                  className={`relative group rounded-lg border bg-card p-3.5 flex flex-col gap-2 ${isHighlighted ? 'border-primary shadow-sm ring-2 ring-primary animate-[fadeHighlight_2s_ease-out_forwards]' : 'border-border'}`}
                >
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
                  {isInstalled ? (
                    /* Installed: add to dashboard (primary) + uninstall (icon). */
                    <div className="flex items-center gap-1.5">
                      <Button
                        variant="outline"
                        size="sm"
                        className="flex-1 h-7 text-xs"
                        onClick={() => onAddComponent(mc.id)}
                      >
                        <Plus className="w-3.5 h-3.5" />
                        {t('visualDashboard.addComponent')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-muted-foreground hover:text-error shrink-0"
                        disabled={installingId === mc.id}
                        aria-label={t('componentLibrary.uninstall')}
                        onClick={async () => {
                          onSetInstalling(mc.id)
                          try {
                            await onUninstall(mc.id)
                            notifySuccess(t('componentLibrary.uninstallSuccess'))
                          } catch {
                            notifyError(t('componentLibrary.installError'))
                          } finally {
                            onSetInstalling(null)
                          }
                        }}
                      >
                        {installingId === mc.id
                          ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          : <Trash2 className="h-3.5 w-3.5" />}
                      </Button>
                    </div>
                  ) : (
                    <Button
                      variant="outline"
                      size="sm"
                      className="w-full h-7 text-xs"
                      disabled={installingId === mc.id}
                      onClick={async () => {
                        onSetInstalling(mc.id)
                        try {
                          await onInstall(mc.id)
                          notifySuccess(t('componentLibrary.installSuccess'))
                          // Stay here: the card flips to its installed state
                          // (add + uninstall) and pulses, so the next action
                          // is right where the user is looking.
                          setHighlightedId(mc.id)
                        } catch {
                          notifyError(t('componentLibrary.installError'))
                        } finally {
                          onSetInstalling(null)
                        }
                      }}
                    >
                      {installingId === mc.id ? (
                        <><Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />{t('componentLibrary.install')}</>
                      ) : (
                        <><PackagePlus className="w-3.5 h-3.5 mr-1" />{t('componentLibrary.install')}</>
                      )}
                    </Button>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )

  // Custom holds MANUAL imports only (zip upload / server path).
  // Marketplace-installed components stay managed on the Marketplace
  // source — they are third-party published, not "custom".
  const manualImports = installedComponents.filter((c) => c.source !== 'marketplace')

  const customPane = (
    <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-8">
      <div className="mx-auto w-full max-w-5xl">
        {manualImports.length === 0 ? (
          <div className="flex h-full min-h-[320px]">
            <EmptyState
              icon={<PackagePlus className="h-12 w-12" />}
              title={t('componentLibrary.customEmptyTitle')}
              description={t('componentLibrary.customEmptyDesc')}
              action={{
                label: t('componentLibrary.importComponent'),
                onClick: () => onImportDialogOpenChange(true),
              }}
            />
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3 pt-1">
            {manualImports.map((c) => {
              const Icon = dynamicIconMap[c.icon || 'Box'] || Box
              const name = localized(c.name, i18n.language, c.id)
              const desc = localized(c.description, i18n.language, '')
              const update = updatesAvailable[c.id]
              const isHighlighted = highlightedId === c.id
              return (
                <div
                  key={c.id}
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
                    onClick={() => onAddComponent(c.id)}
                    className={`w-full h-[88px] flex items-center gap-3 py-2 px-3 rounded-lg border bg-card hover:shadow-sm transition-all duration-normal cursor-pointer active:scale-[0.98] text-left ${isHighlighted ? 'border-primary shadow-sm ring-2 ring-primary animate-[fadeHighlight_2s_ease-out_forwards]' : 'border-border'}`}
                  >
                    <span className="w-9 h-9 rounded-lg bg-muted text-foreground flex items-center justify-center shrink-0">
                      <Icon className="h-4 w-4 shrink-0" />
                    </span>
                    <span className="flex-1 min-w-0">
                      <span className="text-sm font-medium text-foreground truncate block">{name}</span>
                      <span className="block text-xs text-muted-foreground mt-0.5">
                        {t('componentLibrary.version')}: {c.version}
                      </span>
                      <span className="block text-xs text-muted-foreground mt-0.5 line-clamp-1 leading-snug">{desc}</span>
                    </span>
                  </button>
                  <div className="absolute top-1 right-1 flex gap-0.5 rounded-md bg-background/90 backdrop-blur-sm p-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                    {update && (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 text-info"
                        disabled={installingId === c.id}
                        title={t('componentLibrary.updateAvailable', { version: update.latest })}
                        aria-label={t('componentLibrary.updateAvailable', { version: update.latest })}
                        onClick={(e) => {
                          e.stopPropagation()
                          handleRefresh(c.id)
                        }}
                      >
                        {installingId === c.id
                          ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          : <ArrowDownCircle className="h-3.5 w-3.5" />}
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 text-muted-foreground hover:text-error"
                      disabled={installingId === c.id}
                      aria-label={t('componentLibrary.uninstall')}
                      onClick={(e) => {
                        e.stopPropagation()
                        handleUninstall(c.id)
                      }}
                    >
                      {installingId === c.id
                        ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        : <Trash2 className="h-3.5 w-3.5" />}
                    </Button>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )

  const sourceButtons = [
    { id: 'components' as const, icon: LayoutGrid, label: t('componentLibrary.tabComponents') },
    { id: 'extensions' as const, icon: Puzzle, label: t('componentLibrary.tabExtensions') },
    { id: 'marketplace' as const, icon: StoreIcon, label: t('componentLibrary.tabMarketplace') },
    { id: 'custom' as const, icon: PackagePlus, label: t('componentLibrary.tabCustom') },
  ]

  const paneFor = (tab: ComponentLibraryTab) =>
    tab === 'components' ? componentsPane
      : tab === 'extensions' ? extensionsPane
        : tab === 'marketplace' ? marketplacePane
          : customPane

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
            /* Mobile: no room for a rail — compact tab strip + contextual
               action + inline search above the shared scroll content. */
            <div className="flex-1 flex flex-col overflow-hidden">
              <div className="px-4 pt-3 pb-3 shrink-0 space-y-3">
                <div className="flex items-center gap-3">
                  <Tabs value={libraryTab} onValueChange={(v) => onLibraryTabChange(v as ComponentLibraryTab)}>
                    <TabsList className="h-9">
                      {sourceButtons.map(({ id, icon: Icon, label }) => (
                        <TabsTrigger key={id} value={id} className="gap-1.5 text-xs px-3">
                          <Icon className="w-3.5 h-3.5" />
                          {label}
                        </TabsTrigger>
                      ))}
                    </TabsList>
                  </Tabs>
                  {libraryTab === 'custom' && importButton()}
                </div>
                {libraryTab === 'components' && searchInput}
              </div>
              {paneFor(libraryTab)}
            </div>
          ) : (
            /* Desktop split layout: the rail owns navigation (source
               switch + category filter); the content pane owns actions
               and results. */
            <div className="flex-1 flex overflow-hidden">
              <FullScreenDialogSidebar className="flex flex-col overflow-hidden p-3 gap-1">
                {sourceButtons.map(({ id, icon: Icon, label }) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() => {
                      onLibraryTabChange(id)
                      // A category selection from another Components
                      // session may reference a category that vanished
                      // (e.g. its extension was uninstalled) — start
                      // every source visit from the full list.
                      setSelectedCategory('all')
                    }}
                    className={cn(
                      'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-sm font-medium transition-colors',
                      libraryTab === id
                        ? 'bg-muted text-foreground'
                        : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                    )}
                  >
                    <Icon className="h-4 w-4 shrink-0" />
                    {label}
                  </button>
                ))}

                {libraryTab === 'components' && (
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
                      {builtinCategories.map((category) => (
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
                )}
              </FullScreenDialogSidebar>

              <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                {/* Content toolbar — same row geometry for every source so
                    switching never jumps. */}
                <div className="shrink-0 px-4 md:px-6 pt-4 pb-3">
                  <div className="mx-auto w-full max-w-5xl">
                    {libraryTab === 'components' || libraryTab === 'extensions' ? (
                      searchInput
                    ) : libraryTab === 'marketplace' ? (
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-9 gap-1.5 text-xs"
                        disabled={marketLoading}
                        onClick={() => onRefreshMarket()}
                      >
                        {marketLoading
                          ? <Loader2 className="w-3.5 h-3.5 animate-spin" />
                          : <RefreshCw className="w-3.5 h-3.5" />}
                        {t('componentLibrary.refreshMarket')}
                      </Button>
                    ) : (
                      importButton()
                    )}
                  </div>
                </div>
                {paneFor(libraryTab)}
              </div>
            </div>
          )}
        </FullScreenDialogContent>
      </FullScreenDialog>

      <InstallComponentDialog open={importDialogOpen} onOpenChange={onImportDialogOpenChange} />
    </>
  )
})
