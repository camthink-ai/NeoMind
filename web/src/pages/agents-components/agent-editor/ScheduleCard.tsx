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
    // Icon beside stacked text, not above it: the old column centred an icon
    // in its own padded box and let the description wrap under it, which cost
    // roughly twice the height for the same information.
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex min-w-0 items-center rounded-lg border text-left transition-colors",
        isMobile ? "gap-2.5 p-3" : "gap-2 px-3 py-2.5",
        active
          ? "border-primary bg-muted"
          : "border-border hover:border-muted-foreground"
      )}
    >
      <span className="shrink-0">{icon}</span>
      <span className="min-w-0">
        <span className={cn("block truncate font-medium", active ? "text-foreground" : "text-muted-foreground", isMobile ? "text-sm" : "text-xs")}>{label}</span>
        <span className={cn("block truncate text-muted-foreground", isMobile ? "text-xs" : textNano)}>{description}</span>
      </span>
    </button>
  )
}

