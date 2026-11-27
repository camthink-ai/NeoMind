import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

export interface SegmentedOption<T extends string> {
  value: T
  label: ReactNode
  title?: string
  ariaLabel?: string
}

export interface SegmentedControlProps<T extends string> {
  options: SegmentedOption<T>[]
  value: T
  onChange: (value: T) => void
  ariaLabel?: string
  size?: "icon" | "sm"
  className?: string
}

/**
 * Compact monochrome segmented control for view switches and inline tab strips.
 */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  size = "sm",
  className,
}: SegmentedControlProps<T>) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className={cn(
        "inline-flex items-center rounded-lg border border-border bg-muted-30 p-0.5 shrink-0",
        className
      )}
    >
      {options.map((opt) => {
        const active = opt.value === value
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => onChange(opt.value)}
            aria-pressed={active}
            aria-label={opt.ariaLabel}
            title={opt.title}
            className={cn(
              "inline-flex items-center justify-center rounded-md transition-colors",
              size === "icon" ? "h-7 w-7" : "h-7 px-2.5 text-xs font-medium gap-1.5",
              active
                ? "bg-card text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}
