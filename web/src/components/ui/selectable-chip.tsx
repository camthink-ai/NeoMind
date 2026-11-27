import type { ReactNode } from "react"
import { Check } from "lucide-react"
import { cn } from "@/lib/utils"

export interface SelectableChipProps {
  selected: boolean
  onToggle: () => void
  children: ReactNode
  className?: string
}

/**
 * Multi-select bordered toggle chip used in filter and option grids.
 */
export function SelectableChip({
  selected,
  onToggle,
  children,
  className,
}: SelectableChipProps) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onToggle}
      className={cn(
        "flex items-center justify-between gap-2 rounded-lg border px-3 py-2 text-xs font-medium transition-colors text-left",
        selected
          ? "border-foreground bg-muted-30 text-foreground"
          : "border-border bg-card text-muted-foreground hover:bg-muted-20 hover:text-foreground",
        className
      )}
    >
      <span className="truncate">{children}</span>
      <span
        className={cn(
          "flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors",
          selected
            ? "border-foreground bg-foreground text-background"
            : "border-border bg-background"
        )}
      >
        {selected && <Check className="h-2.5 w-2.5" strokeWidth={2.5} />}
      </span>
    </button>
  )
}
