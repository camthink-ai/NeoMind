// Selected-resource card with metric/command toggles — split from AgentEditorFullScreen.tsx

import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Target, X, Puzzle, ChevronRight } from 'lucide-react'
import type { SelectedResource } from './types'

export interface SelectedResourceItemProps {
  resource: SelectedResource
  setSelectedResources: React.Dispatch<React.SetStateAction<SelectedResource[]>>
  onRemove: () => void
  onToggleMetric: (resourceId: string, metricName: string) => void
  onToggleCommand: (resourceId: string, commandName: string) => void
  isMobile?: boolean
  /** Inside the selection dialog: the pane already provides the chrome, so the
   *  item drops its own border instead of card-in-card-in-pane. */
  embedded?: boolean
}

export function SelectedResourceItem({ resource, setSelectedResources, onRemove, onToggleMetric, onToggleCommand, isMobile = false, embedded = false }: SelectedResourceItemProps) {
  const { t: tAgent } = useTranslation('agents')
  const [expanded, setExpanded] = useState(false)
  const selectedMetricCount = resource.selectedMetrics.size
  const selectedCommandCount = resource.selectedCommands.size
  const allMetricCount = resource.allMetrics.length
  const allCommandCount = resource.allCommands.length

  const hasMetrics = resource.allMetrics.length > 0
  const hasCommands = resource.allCommands.length > 0

  // How far back collection reaches for this source. A device that reports
  // every few hours is invisible to the 60-minute default, and the agent then
  // has nothing to read — which looks like a broken agent rather than a window
  // set too narrow.
  const lookbackMinutes = resource.config?.data_collection?.time_range_minutes ?? 60
  const setLookbackMinutes = (minutes: number) => {
    if (!Number.isFinite(minutes) || minutes < 1) return
    setSelectedResources((prev: SelectedResource[]) =>
      prev.map(r =>
        r.id === resource.id
          ? {
              ...r,
              config: {
                ...r.config,
                data_collection: {
                  include_history: false,
                  include_trend: false,
                  include_baseline: false,
                  ...(r.config?.data_collection ?? {}),
                  time_range_minutes: minutes,
                },
              },
            }
          : r
      )
    )
  }

  return (
    <div
      className={cn(
        "group",
        embedded
          ? isMobile ? "py-2.5" : "py-2"
          : cn("rounded-lg border", isMobile ? "px-4 py-3" : "px-3 py-2"),
      )}
    >
      {/* Header */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          aria-expanded={expanded}
          aria-label={resource.name}
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-2 min-w-0 flex-1"
        >
          <ChevronRight
            className={cn(
              "text-muted-foreground transition-transform",
              isMobile ? "h-5 w-5" : "h-4 w-4",
              expanded && "rotate-90"
            )}
          />
          <div className={cn(
            "rounded",
            isMobile ? "p-2" : "p-1",
            resource.type === 'extension' ? "bg-accent-purple-light text-accent-purple" : "bg-info-light text-info"
          )}>
            {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />}
          </div>
          <span className={cn("font-medium truncate", isMobile ? "text-base" : "text-sm")}>{resource.name}</span>
          {(hasMetrics || hasCommands) && (
            <Badge variant="secondary" className={cn(isMobile ? "text-xs" : "text-xs")}>
              {selectedMetricCount}/{allMetricCount} • {selectedCommandCount}/{allCommandCount}
            </Badge>
          )}
        </button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={`remove ${resource.name}`}
          className={cn("transition-opacity", isMobile ? "h-9 w-9" : "h-6 w-6", isMobile ? "opacity-100" : "opacity-0 group-hover:opacity-100")}
          onClick={onRemove}
        >
          <X className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />
        </Button>
      </div>

      {/* Collapsed state summary */}
      {!expanded && (selectedMetricCount > 0 || selectedCommandCount > 0) && (
        <div className={cn("mt-1", isMobile ? "pl-7" : "pl-6")}>
          <p className="text-xs text-muted-foreground truncate">
            {resource.allMetrics
              .filter(m => resource.selectedMetrics.has(m.name))
              .map(m => m.display_name)
              .slice(0, 3)
              .join(', ')}
            {selectedMetricCount > 3 && ` +${selectedMetricCount - 3} more`}
            {selectedMetricCount > 0 && selectedCommandCount > 0 && ' \u00B7 '}
            {selectedCommandCount > 0 && `${selectedCommandCount} cmd${selectedCommandCount > 1 ? 's' : ''}`}
          </p>
        </div>
      )}

      {/* Expandable Metrics/Commands — partition folding: show only selected by default */}
      {expanded && (hasMetrics || hasCommands) && (
        <div className={cn("space-y-2", isMobile ? "mt-3 pl-7" : "mt-2 pl-6")}>
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <label htmlFor={`lookback-${resource.id}`}>
              {tAgent('creator.resources.lookback')}
            </label>
            <Input
              id={`lookback-${resource.id}`}
              type="number"
              min={1}
              value={lookbackMinutes}
              onChange={e => setLookbackMinutes(Number(e.target.value))}
              className="h-7 w-20"
            />
            <span>{tAgent('creator.resources.lookbackUnit')}</span>
          </div>
          {/* Metrics */}
          {hasMetrics && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>
                  {tAgent('creator.resources.metrics')} ({selectedMetricCount}/{allMetricCount})
                </span>
                <button
                  type="button"
                  onClick={() => {
                    const selectAll = selectedMetricCount < allMetricCount
                    setSelectedResources((prev: SelectedResource[]) =>
                      prev.map(r =>
                        r.id === resource.id
                          ? {
                              ...r,
                              selectedMetrics: selectAll
                                ? new Set(resource.allMetrics.map(m => m.name))
                                : new Set(),
                            }
                          : r
                      )
                    )
                  }}
                  className="text-primary hover:underline"
                >
                  {selectedMetricCount === allMetricCount
                    ? tAgent('creator.resources.deselectAll')
                    : tAgent('creator.resources.selectAll')}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {/* Every option stays visible. Hiding the unselected ones once
                    a choice was made left no way to make a second one without
                    first finding the "show all" link. */}
                {resource.allMetrics
                  .map((metric) => (
                  <div key={metric.name} className="contents">
                    <div
                      className={cn(
                        "flex items-center justify-between rounded transition-colors",
                        isMobile
                          ? "px-2 py-1 text-sm"
                          : "px-1.5 py-0.5 text-xs",
                        resource.selectedMetrics.has(metric.name)
                          ? "bg-muted text-primary"
                          : "hover:bg-muted-30"
                      )}
                    >
                      <div
                        className="flex items-center gap-2 cursor-pointer min-w-0"
                        onClick={() => onToggleMetric(resource.id, metric.name)}
                      >
                        <Checkbox
                          checked={resource.selectedMetrics.has(metric.name)}
                          className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")}
                        />
                        <span className="truncate">{metric.display_name}</span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Commands */}
          {hasCommands && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>
                  {tAgent('creator.resources.commands')} ({selectedCommandCount}/{allCommandCount})
                </span>
                <button
                  type="button"
                  onClick={() => {
                    const selectAll = selectedCommandCount < allCommandCount
                    setSelectedResources((prev: SelectedResource[]) =>
                      prev.map(r =>
                        r.id === resource.id
                          ? {
                              ...r,
                              selectedCommands: selectAll
                                ? new Set(resource.allCommands.map(c => c.name))
                                : new Set(),
                            }
                          : r
                      )
                    )
                  }}
                  className="text-primary hover:underline"
                >
                  {selectedCommandCount === allCommandCount
                    ? tAgent('creator.resources.deselectAll')
                    : tAgent('creator.resources.selectAll')}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {resource.allCommands
                  .map((command) => (
                  <div
                    key={command.name}
                    className={cn(
                      "flex items-center gap-2 rounded cursor-pointer transition-colors",
                      isMobile
                        ? "px-2 py-1 text-sm"
                        : "px-1.5 py-0.5 text-xs",
                      resource.selectedCommands.has(command.name)
                        ? "bg-muted text-primary"
                        : "hover:bg-muted-30"
                    )}
                    onClick={() => onToggleCommand(resource.id, command.name)}
                  >
                    <Checkbox
                      checked={resource.selectedCommands.has(command.name)}
                      className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")}
                    />
                    <span className="truncate">{command.display_name}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {!hasMetrics && !hasCommands && (
            <p className={cn("text-muted-foreground italic", isMobile ? "text-xs" : "text-xs")}>
              {tAgent('creator.resources.noneAvailable')}
            </p>
          )}

        </div>
      )}
    </div>
  )
}

// Export as default
