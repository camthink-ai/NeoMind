/**
 * Component Library Sidebar
 *
 * Full-screen dialog containing the component library with tabs for
 * built-in components and marketplace. Extracted from VisualDashboard
 * to reduce its file size and improve maintainability.
 */

import { memo, useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { dynamicIconMap } from '@/lib/dynamicIcons'
import {
  LayoutGrid, Store as StoreIcon, Search,
  Box, Check, Trash2, Download, Loader2, Upload, PackagePlus, RefreshCw, ArrowDownCircle,
} from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  FullScreenDialog, FullScreenDialogHeader, FullScreenDialogContent,
} from '@/components/automation/dialog'
import { notifySuccess, notifyError } from '@/lib/notify'
import { textNano } from '@/design-system/tokens/typography'
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

  // Highlight newly installed community component
  const [highlightedId, setHighlightedId] = useState<string | null>(null)
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

  return (
    <>
      <FullScreenDialog open={open} onOpenChange={(newOpen: boolean) => {
        onOpenChange(newOpen)
        if (!newOpen) { onLibrarySearchChange(''); onLibraryTabChange('components') }
      }}>
        <FullScreenDialogHeader
          icon={<LayoutGrid className="h-5 w-5" />}
          iconBg="bg-info-light"
          iconColor="text-info"
          title={t('visualDashboard.componentLibrary')}
          onClose={() => {
            onOpenChange(false)
            onLibrarySearchChange('')
            onLibraryTabChange('components')
          }}
        />

        <FullScreenDialogContent>
          <style>{`
            @keyframes fadeHighlight {
              0% { box-shadow: 0 0 0 3px oklch(var(--primary) / 0.25); }
              70% { box-shadow: 0 0 0 3px oklch(var(--primary) / 0.15); }
              100% { box-shadow: 0 0 0 0px oklch(var(--primary) / 0); }
            }
          `}</style>
          <div className="flex-1 overflow-hidden flex flex-col">
            {/* Tabs */}
            <div className="px-4 md:px-6 pt-4 pb-2 shrink-0 space-y-3">
              <div className="flex items-center gap-3">
                <Tabs value={libraryTab} onValueChange={(v) => onLibraryTabChange(v as 'components' | 'marketplace')} className="flex-1">
                  <TabsList className="h-8">
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
                    className="h-8 gap-1.5 text-xs"
                    onClick={() => onImportDialogOpenChange(true)}
                  >
                    <PackagePlus className="w-3.5 h-3.5" />
                    {t('componentLibrary.importComponent')}
                  </Button>
                )}
              </div>

              {/* Search (only in components tab) */}
              {libraryTab === 'components' && (
                <div className="relative">
                  <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={librarySearch}
                    onChange={(e) => onLibrarySearchChange(e.target.value)}
                    placeholder={t('componentLibrary.searchPlaceholder')}
                    className="h-9 pl-8"
                  />
                </div>
              )}
            </div>

            {/* Tab Content */}
            {libraryTab === 'components' ? (
              <div className="flex-1 overflow-y-auto px-4 md:px-6 pb-6 space-y-1">
                {filteredLibrary.length === 0 ? (
                  <div className="text-center py-12 text-muted-foreground">
                    <p className="text-sm">{t('componentLibrary.noResults')}</p>
                    <p className="text-xs mt-1">{t('componentLibrary.noResultsHint')}</p>
                  </div>
                ) : (
                  filteredLibrary.map((category) => (
                    <div key={category.category}>
                      <div className="flex items-center gap-2 pt-3 pb-2 px-1">
                        <category.categoryIcon className="h-4 w-4 text-muted-foreground" />
                        <span className="text-sm font-medium text-foreground">{category.categoryLabel}</span>
                        <span className="text-xs text-muted-foreground bg-muted rounded-full px-1.5 py-0.5 min-w-[24px] text-center">
                          {category.items.length}
                        </span>
                      </div>
                      <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3 pb-2 px-1">
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
                                  className="absolute top-1.5 right-1.5 z-10 h-2 w-2 rounded-full bg-info ring-2 ring-background"
                                  title={t('componentLibrary.updateAvailable', { version: update.latest })}
                                />
                              )}
                              <button
                                type="button"
                                onClick={() => onAddComponent(item.id)}
                                className={`w-full h-[72px] flex items-center gap-3 py-2 px-3 rounded-md border hover:shadow-sm transition-all duration-200 cursor-pointer active:scale-[0.98] text-left ${isHighlighted ? 'border-primary shadow-sm ring-2 ring-primary animate-[fadeHighlight_2s_ease-out_forwards]' : 'border-border'}`}
                              >
                                <span className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 ${category.categoryColor}`}>
                                  <Icon className="h-4 w-4 shrink-0" />
                                </span>
                                <div className="flex-1 min-w-0">
                                  <span className="text-xs font-medium block truncate">{item.name}</span>
                                  <p className={`${textNano} text-muted-foreground mt-0.5 line-clamp-2 leading-snug`}>{item.description}</p>
                                </div>
                              </button>
                              {isCommunity && (
                                <div className="absolute top-1 right-1 flex gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className={`h-5 w-5 ${update ? 'text-info' : 'text-muted-foreground hover:text-info'}`}
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
                                      ? <Loader2 className="h-3 w-3 animate-spin" />
                                      : update
                                        ? <ArrowDownCircle className="h-3 w-3" />
                                        : <RefreshCw className="h-3 w-3" />}
                                  </Button>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-5 w-5 text-muted-foreground hover:text-error"
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
                                      ? <Loader2 className="h-3 w-3 animate-spin" />
                                      : <Trash2 className="h-3 w-3" />}
                                  </Button>
                                </div>
                              )}
                            </div>
                          )
                        })}
                      </div>
                    </div>
                  ))
                )}
              </div>
            ) : (
              /* Marketplace tab */
              <div className="flex-1 overflow-y-auto px-4 md:px-6 pt-4 pb-6">
                {marketLoading ? (
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
                    {Array.from({ length: 6 }).map((_, i) => (
                      <div key={i} className="rounded-lg border border-border p-4 space-y-3">
                        <div className="w-10 h-10 rounded-lg bg-muted animate-pulse" />
                        <div className="h-4 bg-muted rounded w-3/4 animate-pulse" />
                        <div className="h-3 bg-muted rounded w-full animate-pulse" />
                      </div>
                    ))}
                  </div>
                ) : marketComponents.length === 0 ? (
                  <div className="flex flex-col items-center justify-center py-16 text-center">
                    <Upload className="h-10 w-10 text-muted-foreground mb-3" />
                    <p className="text-sm text-muted-foreground">{t('componentLibrary.marketplaceEmpty')}</p>
                  </div>
                ) : (
                  <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
                    {marketComponents.map((mc: MarketComponentEntry) => {
                      const isInstalled = installedComponents.some(c => c.id === mc.id)
                      const McIcon = dynamicIconMap[mc.icon || 'Box'] || Box
                      const mcName = typeof mc.name === 'string' ? mc.name : (mc.name[i18n.language] || mc.name.en || Object.values(mc.name)[0] || mc.id)
                      const mcDesc = typeof mc.description === 'string' ? mc.description : (mc.description[i18n.language] || mc.description.en || Object.values(mc.description)[0] || '')
                      return (
                        <div key={mc.id} className="rounded-md border border-border bg-card p-3.5 flex flex-col gap-2">
                          <div className="flex items-start gap-2.5">
                            <div className="w-9 h-9 rounded-lg bg-success-light flex items-center justify-center shrink-0">
                              <McIcon className="w-4 h-4 text-success" />
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
            )}
          </div>
        </FullScreenDialogContent>
      </FullScreenDialog>

      <InstallComponentDialog open={importDialogOpen} onOpenChange={onImportDialogOpenChange} />
    </>
  )
})
