// AI-recommendation card — split from AgentEditorFullScreen.tsx

import { cn } from '@/lib/utils'
import { Check, Target, Puzzle } from 'lucide-react'
import type { ResourceRecommendation } from './types'

export interface RecommendationCardProps {
  recommendation: ResourceRecommendation
  selected: boolean
  onClick: () => void
}

export function RecommendationCard({ recommendation, selected, onClick }: RecommendationCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 px-3 py-2 rounded-lg border text-left whitespace-nowrap transition-colors min-w-0",
        selected ? "border-primary bg-muted" : "border-border hover:bg-muted-30"
      )}
    >
      <div className={cn(
        "p-1 rounded",
        selected ? "bg-primary text-primary-foreground" : "bg-muted"
      )}>
        {recommendation.type === 'extension' ? <Puzzle className="h-4 w-4" /> : <Target className="h-4 w-4" />}
      </div>
      <div className="min-w-0">
        <div className="text-sm font-medium truncate">{recommendation.name}</div>
        <div className="text-xs text-muted-foreground truncate">{recommendation.reason}</div>
      </div>
      {selected && <Check className="h-4 w-4 text-primary ml-1 shrink-0" />}
    </button>
  )
}

