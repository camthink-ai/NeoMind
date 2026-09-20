// Schedule type card — split from AgentEditorFullScreen.tsx

import React from 'react'
import { cn } from '@/lib/utils'
import { textNano } from '@/design-system/tokens/typography'

export interface ScheduleCardProps {
  icon: React.ReactNode
  label: string
  description: string
  active: boolean
  onClick: () => void
  isMobile?: boolean
}

export function ScheduleCard({ icon, label, description, active, onClick, isMobile = false }: ScheduleCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex flex-col items-center rounded-lg border transition-colors",
        isMobile ? "gap-3 p-4" : "gap-2 p-3",
        active
          ? "border-primary bg-muted"
          : "border-transparent hover:border-border"
      )}
    >
      <div className={cn("rounded-lg", active ? "bg-muted" : "", isMobile ? "p-2" : "p-1.5")}>
        {icon}
      </div>
      <div className="text-center">
        <div className={cn("font-medium", active ? "text-foreground" : "text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{label}</div>
        <div className={cn("text-muted-foreground", isMobile ? "text-xs" : textNano)}>{description}</div>
      </div>
    </button>
  )
}

