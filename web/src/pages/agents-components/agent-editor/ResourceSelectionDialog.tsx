// Resource selection dialog (desktop Dialog + mobile portal) — split from AgentEditorFullScreen.tsx

import { getPortalRoot } from '@/lib/portal'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { createPortal } from 'react-dom'
import { useIsMobile, useSafeAreaInsets } from '@/hooks/useMobile'
import { useBodyScrollLock } from '@/hooks/useBodyScrollLock'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Search, Loader2, Target, X, Sparkles } from 'lucide-react'
import type { SelectedResource, ResourceRecommendation, ScheduleType, AvailableResource } from './types'
import { RecommendationCard } from './RecommendationCard'
import { ResourceListItem } from './ResourceListItem'
import { SelectedResourceItem } from './SelectedResourceItem'

export interface ResourceSelectionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  availableResources: AvailableResource[]
  selectedResources: SelectedResource[]
  setSelectedResources: React.Dispatch<React.SetStateAction<SelectedResource[]>>
  recommendations: ResourceRecommendation[]
  generatingRecommendations: boolean
  searchQuery: string
  setSearchQuery: (v: string) => void
  toggleResource: (r: AvailableResource) => void
  toggleRecommendation: (r: ResourceRecommendation) => void
  scheduleType: ScheduleType
}

export function ResourceSelectionDialog({
  open,
  onOpenChange,
  availableResources,
  selectedResources,
  setSelectedResources,
  recommendations,
  generatingRecommendations,
  searchQuery,
  setSearchQuery,
  toggleResource,
  toggleRecommendation,
  scheduleType: _scheduleType,
}: ResourceSelectionDialogProps) {
  const { t: tAgent } = useTranslation('agents')
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Lock body scroll when dialog is open (mobile only)
  useBodyScrollLock(open, { mobileOnly: true })

  const isSelected = (id: string) => selectedResources.some(r => r.id === id)

  // Mobile full-screen portal
  if (isMobile) {
    return createPortal(
      <div
        className={cn(
          "fixed inset-0 z-[100] bg-background flex flex-col",
          !open && "hidden"
        )}
        style={{
          paddingTop: `${insets.top}px`,
          paddingBottom: `${insets.bottom}px`,
        }}
      >
        {/* Mobile Header */}
        <div className="flex items-center justify-between px-4 py-4 border-b shrink-0">
          <h2 className="text-base font-semibold">{tAgent('creator.resources.dialog.title')}</h2>
          <Button
            variant="ghost"
            size="icon"
            className="h-10 w-10"
            onClick={() => onOpenChange(false)}
          >
            <X className="h-5 w-5" />
          </Button>
        </div>

        {/* Mobile Content */}
        <div className="flex-1 overflow-y-auto">
          {/* Recommendations */}
          {recommendations.length > 0 && (
            <div className="px-4 py-3 border-b shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />}
              </div>
              <div className="flex gap-2 overflow-x-auto pb-1">
                {recommendations.map((rec) => (
                  <RecommendationCard
                    key={rec.id}
                    recommendation={rec}
                    selected={isSelected(rec.id)}
                    onClick={() => toggleRecommendation(rec)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Available Resources Section */}
          <div className="px-4 py-3 border-b">
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm font-medium">{tAgent('creator.resources.dialog.available')}</span>
              <Badge variant="secondary">{availableResources.length}</Badge>
            </div>
            <div className="relative mb-3">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={tAgent('creator.resources.dialog.searchPlaceholder')}
                className="h-11 text-base pl-10"
              />
            </div>
            <div className="space-y-2">
              {availableResources.map((resource) => {
                const selected = isSelected(resource.id)
                return (
                  <ResourceListItem
                    key={resource.id}
                    resource={resource}
                    selected={selected}
                    onClick={() => toggleResource(resource)}
                    isMobile={true}
                  />
                )
              })}
              {availableResources.length === 0 && (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  {tAgent('creator.resources.noResourcesFound')}
                </div>
              )}
            </div>
          </div>

          {/* Selected Resources Section */}
          <div className="px-4 py-3">
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm font-medium">{tAgent('creator.resources.dialog.selected')}</span>
              <Badge variant={selectedResources.length === 0 ? "secondary" : "default"}>
                {selectedResources.length}
              </Badge>
            </div>
            {selectedResources.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-center py-8">
                <Target className="h-8 w-8 text-muted-foreground mb-2" />
                <p className="text-sm text-muted-foreground">
                  {tAgent('creator.resources.dialog.noResourcesHint')}
                </p>
              </div>
            ) : (
              <div className="divide-y divide-border">
                {selectedResources.map((resource) => (
                  <SelectedResourceItem
                    key={resource.id}
                    embedded
                    resource={resource}
                    setSelectedResources={setSelectedResources}
                    onRemove={() => {
                      setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
                    }}
                    onToggleMetric={(resourceId, metricName) => {
                      setSelectedResources((prev) =>
                        prev.map(r =>
                          r.id === resourceId
                            ? {
                                ...r,
                                selectedMetrics: new Set(
                                  r.selectedMetrics.has(metricName)
                                    ? Array.from(r.selectedMetrics).filter(n => n !== metricName)
                                    : [...r.selectedMetrics, metricName]
                                ),
                              }
                            : r
                        )
                      )
                    }}
                    onToggleCommand={(resourceId, commandName) => {
                      setSelectedResources((prev) =>
                        prev.map(r =>
                          r.id === resourceId
                            ? {
                                ...r,
                                selectedCommands: new Set(
                                  r.selectedCommands.has(commandName)
                                    ? Array.from(r.selectedCommands).filter(n => n !== commandName)
                                    : [...r.selectedCommands, commandName]
                                ),
                              }
                            : r
                        )
                      )
                    }}
                    isMobile={true}
                                      />
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Mobile Footer */}
        <div className="px-4 py-4 border-t flex justify-between items-center shrink-0">
          <p className="text-sm text-muted-foreground">
            {tAgent('creator.resources.dialog.selectedCount', { count: selectedResources.length })}
          </p>
          <Button className="min-w-[100px] h-12" onClick={() => onOpenChange(false)}>
            {tAgent('creator.resources.dialog.done')}
          </Button>
        </div>
      </div>, getPortalRoot()
    )
  }

  // Desktop Dialog
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="z-[110] sm:max-w-3xl sm:max-h-[80vh] flex flex-col p-0 sm:p-0 gap-0 m-0">
        <DialogHeader className="px-5 py-3 border-b">
          <DialogTitle>{tAgent('creator.resources.dialog.title')}</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-hidden flex flex-col min-h-0">
          {/* Recommendations */}
          {recommendations.length > 0 && (
            <div className="px-5 py-3 border-b shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <span className="text-sm font-medium">{tAgent('creator.resources.dialog.recommended')}</span>
                {generatingRecommendations && <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />}
              </div>
              <div className="flex gap-2 overflow-x-auto pb-1">
                {recommendations.map((rec) => (
                  <RecommendationCard
                    key={rec.id}
                    recommendation={rec}
                    selected={isSelected(rec.id)}
                    onClick={() => toggleRecommendation(rec)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* Dual-pane layout */}
          <div className="flex-1 flex gap-4 min-h-0 p-4 overflow-hidden">
            {/* Available Resources */}
            <div className="flex-1 flex flex-col overflow-hidden rounded-lg border">
              <div className="border-b bg-muted-30 p-3">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium">{tAgent('creator.resources.dialog.available')}</span>
                  <Badge variant="secondary">{availableResources.length}</Badge>
                </div>
                <div className="relative">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={tAgent('creator.resources.dialog.searchPlaceholder')}
                    className="h-8 text-sm pl-9"
                  />
                </div>
              </div>
              <ScrollArea className="flex-1">
                <div className="p-2 space-y-1">
                  {availableResources.map((resource) => {
                    const selected = isSelected(resource.id)
                    return (
                      <ResourceListItem
                        key={resource.id}
                        resource={resource}
                        selected={selected}
                        onClick={() => toggleResource(resource)}
                      />
                    )
                  })}
                  {availableResources.length === 0 && (
                    <div className="text-center py-8 text-muted-foreground text-sm">
                      {tAgent('creator.resources.noResourcesFound')}
                    </div>
                  )}
                </div>
              </ScrollArea>
            </div>

            {/* Selected Resources */}
            <div className="flex-1 flex flex-col overflow-hidden rounded-lg border">
              <div className="border-b bg-muted-30 p-3">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">{tAgent('creator.resources.dialog.selected')}</span>
                  <Badge variant={selectedResources.length === 0 ? "secondary" : "default"}>
                    {selectedResources.length}
                  </Badge>
                </div>
              </div>
              <ScrollArea className="flex-1">
                <div className="px-3 py-1">
                  {selectedResources.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-center py-8">
                      <Target className="h-8 w-8 text-muted-foreground mb-2" />
                      <p className="text-sm text-muted-foreground">
                        {tAgent('creator.resources.dialog.noResourcesHint')}
                      </p>
                    </div>
                  ) : (
                    <div className="divide-y divide-border">
                    {selectedResources.map((resource) => (
                      <SelectedResourceItem
                        key={resource.id}
                        embedded
                        resource={resource}
                        setSelectedResources={setSelectedResources}
                        onRemove={() => {
                          setSelectedResources(prev => prev.filter(r => r.id !== resource.id))
                        }}
                        onToggleMetric={(resourceId, metricName) => {
                          setSelectedResources((prev) =>
                            prev.map(r =>
                              r.id === resourceId
                                ? {
                                    ...r,
                                    selectedMetrics: new Set(
                                      r.selectedMetrics.has(metricName)
                                        ? Array.from(r.selectedMetrics).filter(n => n !== metricName)
                                        : [...r.selectedMetrics, metricName]
                                    ),
                                  }
                                : r
                            )
                          )
                        }}
                        onToggleCommand={(resourceId, commandName) => {
                          setSelectedResources((prev) =>
                            prev.map(r =>
                              r.id === resourceId
                                ? {
                                    ...r,
                                    selectedCommands: new Set(
                                      r.selectedCommands.has(commandName)
                                        ? Array.from(r.selectedCommands).filter(n => n !== commandName)
                                        : [...r.selectedCommands, commandName]
                                    ),
                                  }
                                : r
                            )
                          )
                        }}
                                              />
                    ))}
                    </div>
                  )}
                </div>
              </ScrollArea>
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-between border-t px-5 py-3">
          <p className="text-sm text-muted-foreground">
            {tAgent('creator.resources.dialog.selectedCount', { count: selectedResources.length })}
          </p>
          <Button onClick={() => onOpenChange(false)}>
            {selectedResources.length > 0
              ? tAgent('creator.resources.dialog.doneWithCount', { count: selectedResources.length })
              : tAgent('creator.resources.dialog.done')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// Sub-Components
// ============================================================================

