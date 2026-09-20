// Available-resource list row — split from AgentEditorFullScreen.tsx

import { cn } from '@/lib/utils'
import { Check, Target, Puzzle } from 'lucide-react'
import type { AvailableResource } from './types'

export interface ResourceListItemProps {
  resource: AvailableResource
  selected: boolean
  onClick: () => void
  isMobile?: boolean
}

export function ResourceListItem({ resource, selected, onClick, isMobile = false }: ResourceListItemProps) {
  const metricCount = resource.metrics.length
  const commandCount = resource.commands.length

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "w-full rounded-lg text-left transition-colors flex items-center justify-between group",
        isMobile ? "px-4 py-3" : "px-3 py-2.5",
        selected ? "bg-muted border border-border" : "hover:bg-muted border border-transparent"
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <div className={cn(
          "rounded",
          isMobile ? "p-2" : "p-1.5",
          selected ? "bg-primary text-primary-foreground" : "bg-muted"
        )}>
          {resource.type === 'extension' ? <Puzzle className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} /> : <Target className={cn(isMobile ? "h-4 w-4" : "h-4 w-4")} />}
        </div>
        <div className="min-w-0">
          <div className={cn("font-medium truncate", isMobile ? "text-base" : "text-sm")}>{resource.name}</div>
          <div className={cn("text-muted-foreground", isMobile ? "text-xs" : "text-xs")}>
            {metricCount > 0 && `${metricCount} metric${metricCount > 1 ? 's' : ''}`}
            {metricCount > 0 && commandCount > 0 && ' • '}
            {commandCount > 0 && `${commandCount} command${commandCount > 1 ? 's' : ''}`}
          </div>
        </div>
      </div>
      {selected && <Check className={cn("text-primary shrink-0", isMobile ? "h-5 w-5" : "h-4 w-4")} />}
    </button>
  )
}

