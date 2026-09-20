// Selected-resource card with metric/command toggles — split from AgentEditorFullScreen.tsx

import React, { useState } from 'react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import { Target, X, Puzzle, ChevronRight } from 'lucide-react'
import type { SelectedResource } from './types'
import { inlineLinkBtn } from './constants'

export interface SelectedResourceItemProps {
  resource: SelectedResource
  setSelectedResources: React.Dispatch<React.SetStateAction<SelectedResource[]>>
  onRemove: () => void
  onToggleMetric: (resourceId: string, metricName: string) => void
  onToggleCommand: (resourceId: string, commandName: string) => void
  isMobile?: boolean
}

export function SelectedResourceItem({ resource, setSelectedResources, onRemove, onToggleMetric, onToggleCommand, isMobile = false }: SelectedResourceItemProps) {
  const [expanded, setExpanded] = useState(false)
  const [showAllMetrics, setShowAllMetrics] = useState(false)
  const [showAllCommands, setShowAllCommands] = useState(false)
  const selectedMetricCount = resource.selectedMetrics.size
  const selectedCommandCount = resource.selectedCommands.size
  const allMetricCount = resource.allMetrics.length
  const allCommandCount = resource.allCommands.length

  const hasMetrics = resource.allMetrics.length > 0
  const hasCommands = resource.allCommands.length > 0

  return (
    <div className={cn("rounded-lg border group", isMobile ? "px-4 py-3" : "px-3 py-2")}>
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
          {/* Metrics */}
          {hasMetrics && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Metrics ({selectedMetricCount}/{allMetricCount})</span>
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
                  {selectedMetricCount === allMetricCount ? 'Deselect All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {(showAllMetrics || selectedMetricCount === 0 ? resource.allMetrics : resource.allMetrics.filter(m => resource.selectedMetrics.has(m.name)))
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
              {!showAllMetrics && selectedMetricCount < allMetricCount && (
                <button type="button" onClick={() => setShowAllMetrics(true)} className={inlineLinkBtn}>
                  Show All ({allMetricCount})
                </button>
              )}
              {showAllMetrics && selectedMetricCount < allMetricCount && (
                <button type="button" onClick={() => setShowAllMetrics(false)} className={inlineLinkBtn}>
                  Show Selected Only
                </button>
              )}
            </div>
          )}

          {/* Commands */}
          {hasCommands && (
            <div className="space-y-1">
              <div className={cn("text-muted-foreground flex items-center justify-between", isMobile ? "text-sm" : "text-xs")}>
                <span>Commands ({selectedCommandCount}/{allCommandCount})</span>
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
                  {selectedCommandCount === allCommandCount ? 'Deselect All' : 'Select All'}
                </button>
              </div>
              <div className={cn("gap-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                {(showAllCommands || selectedCommandCount === 0 ? resource.allCommands : resource.allCommands.filter(c => resource.selectedCommands.has(c.name)))
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
              {!showAllCommands && selectedCommandCount < allCommandCount && (
                <button type="button" onClick={() => setShowAllCommands(true)} className={inlineLinkBtn}>
                  Show All ({allCommandCount})
                </button>
              )}
              {showAllCommands && selectedCommandCount < allCommandCount && (
                <button type="button" onClick={() => setShowAllCommands(false)} className={inlineLinkBtn}>
                  Show Selected Only
                </button>
              )}
            </div>
          )}

          {!hasMetrics && !hasCommands && (
            <p className={cn("text-muted-foreground italic", isMobile ? "text-xs" : "text-xs")}>No metrics or commands available</p>
          )}

        </div>
      )}
    </div>
  )
}

// Export as default
