import { useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

interface MobileTabStripProps<T extends string> {
  items: ReadonlyArray<{ value: T; label: string }>
  value: T
  onValueChange: (value: T) => void
  className?: string
  ariaLabel?: string
}

/** Shared mobile section navigation. Keeps the active item visible on narrow
 * screens and uses the same underline treatment across full-screen surfaces. */
export function MobileTabStrip<T extends string>({
  items,
  value,
  onValueChange,
  className,
  ariaLabel,
}: MobileTabStripProps<T>) {
  const activeRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    activeRef.current?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
  }, [value])

  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      className={cn(
        'flex shrink-0 gap-1 overflow-x-auto border-b border-border px-3 scrollbar-none',
        className,
      )}
    >
      {items.map((item) => {
        const active = item.value === value
        return (
          <button
            key={item.value}
            ref={active ? activeRef : undefined}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onValueChange(item.value)}
            className={cn(
              'relative h-11 shrink-0 px-3 text-sm font-medium transition-colors',
              'after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full',
              active
                ? 'text-foreground after:bg-foreground'
                : 'text-muted-foreground after:bg-transparent active:text-foreground',
            )}
          >
            {item.label}
          </button>
        )
      })}
    </div>
  )
}
